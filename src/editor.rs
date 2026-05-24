//! Text editor component with Brief- and Markdown-aware syntax highlighting.
//!
//! This module provides the core `TextEditor` struct and its associated
//! functionality including cursor management, text selection, clipboard operations,
//! scrolling, and rendering. The highlighter is chosen per file from the
//! extension (`.md` → Markdown, everything else → Brief).

use gpui::{
    App, Bounds, ClipboardItem, Context, FocusHandle, Focusable, KeyDownEvent, MouseDownEvent,
    PathPromptOptions, Pixels, Point, PromptLevel, Render, Rgba, ScrollWheelEvent, Timer, Window,
    WindowBounds, WindowOptions, actions, canvas, div, prelude::*, px, size,
};

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::autocomplete::Autocomplete;
use crate::brief::BriefHighlighter;
use crate::compile::{self, CompileDiagnostic};
use crate::config::{EditorConfig, SyntaxTheme, Theme};
use crate::find::{ActiveInput, FindPanelState, SearchMatch};
use crate::markdown::MarkdownHighlighter;
use crate::palette::Palette;

/// Markup language used to highlight, autocomplete, and continue lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Brief,
    Markdown,
}

impl Language {
    /// Returns the language for a file path, defaulting to Brief for any
    /// extension that isn't `.md`/`.markdown` (including no extension).
    pub fn from_path(path: &str) -> Self {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());
        match ext.as_deref() {
            Some("md") | Some("markdown") | Some("mdown") | Some("mkd") => Language::Markdown,
            _ => Language::Brief,
        }
    }
}

// Define GPUI actions for keyboard shortcuts and user commands.
// These actions are bound to keys in main.rs and handled by the TextEditor.
actions!(
    editor,
    [
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveWordLeft,
        MoveWordRight,
        MoveHome,
        MoveEnd,
        Backspace,
        Delete,
        Enter,
        Tab,
        ShiftTab,
        Save,
        Quit,
        Copy,
        Paste,
        Cut,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        ToggleFind,
        FindNext,
        FindPrevious,
        ToggleGoToLine,
        TogglePalette,
        OpenFolder,
        Undo,
        Redo,
        ExportHtml,
        ToggleDiagnostics,
        NextDiagnostic,
    ]
);

/// Diagnostic gutter / underline colors. Hardcoded (not themed) so the
/// editor stays usable without expanding the config surface.
const DIAG_ERROR_COLOR: u32 = 0xe06c75;
const DIAG_WARNING_COLOR: u32 = 0xe5c07b;

/// Maximum number of undo records retained. Older edits fall off the bottom so
/// memory can't grow without bound on a long editing session.
const MAX_UNDO_HISTORY: usize = 1000;

/// A single, compact edit for undo/redo.
///
/// Instead of snapshotting the whole document twice per keystroke (which made
/// memory grow with `document_size × edits`), each record stores only the
/// changed span: at byte `start`, the text `removed` was replaced by `inserted`.
/// Undo splices `removed` back in; redo re-applies `inserted`.
#[derive(Clone, Debug)]
struct EditOperation {
    /// Byte offset where the change begins (a char boundary in both versions).
    start: usize,
    /// Text that occupied `start..start+removed.len()` before the edit.
    removed: String,
    /// Text that occupies `start..start+inserted.len()` after the edit.
    inserted: String,
    /// Cursor position before the edit.
    old_cursor: usize,
    /// Cursor position after the edit.
    new_cursor: usize,
    /// Selection start before the edit.
    old_selection: Option<usize>,
    /// Selection start after the edit.
    new_selection: Option<usize>,
}

/// Computes the minimal changed span between two document versions as
/// `(start, removed, inserted)`, trimming the common prefix and suffix. All
/// three boundaries are clamped to UTF-8 char boundaries so splicing the
/// span back is always valid.
fn compute_diff(old: &str, new: &str) -> (usize, String, String) {
    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();

    // Longest common prefix, backed off to a shared char boundary.
    let max_prefix = old_bytes.len().min(new_bytes.len());
    let mut prefix = 0;
    while prefix < max_prefix && old_bytes[prefix] == new_bytes[prefix] {
        prefix += 1;
    }
    while prefix > 0 && (!old.is_char_boundary(prefix) || !new.is_char_boundary(prefix)) {
        prefix -= 1;
    }

    // Longest common suffix that doesn't overlap the prefix, on a boundary.
    let max_suffix = (old_bytes.len() - prefix).min(new_bytes.len() - prefix);
    let mut suffix = 0;
    while suffix < max_suffix
        && old_bytes[old_bytes.len() - 1 - suffix] == new_bytes[new_bytes.len() - 1 - suffix]
    {
        suffix += 1;
    }
    while suffix > 0
        && (!old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix))
    {
        suffix -= 1;
    }

    let removed = old[prefix..old.len() - suffix].to_string();
    let inserted = new[prefix..new.len() - suffix].to_string();
    (prefix, removed, inserted)
}

/// Core text editor component.
///
/// Manages document state, cursor position, text selection, file I/O, and rendering.
/// All text is stored as UTF-8 in a single `String`, with positions tracked as byte offsets.
///
/// # Architecture Notes
///
/// - **Cursor Position**: Byte offset into `content` string (not character index)
/// - **Selection Model**: Anchor-based selection with `selection_start` and `cursor_position` endpoints
/// - **Scrolling**: Pixel-based vertical scroll offset, clamped to content bounds
/// - **Rendering**: Token-based rendering with per-token color application from markdown highlighter
/// - **Undo/Redo**: Stack-based undo/redo with full state capture per operation
///
/// # Future Improvements
///
/// - Replace `String` with rope data structure for better performance on large files
/// - Implement multi-cursor support
/// - Consider caching tokenized lines for better rendering performance
pub struct TextEditor {
    /// Full document content as UTF-8 string. Consider rope data structure for large files.
    content: String,

    /// Byte offset of cursor position in `content`. Use byte index, not char index.
    cursor_position: usize,

    /// Anchor point for text selection. When `Some`, a selection exists between this and `cursor_position`.
    selection_start: Option<usize>,

    /// GPUI focus handle for keyboard event routing.
    focus_handle: FocusHandle,

    /// Path to currently opened file. `None` indicates unsaved buffer.
    current_file: Option<String>,

    /// Vertical scroll position in pixels. Clamped to [0, max_content_height - viewport_height].
    scroll_offset: f32,

    /// Command palette for fuzzy file finding. `None` when closed.
    palette: Option<gpui::Entity<Palette>>,

    /// Working directory for palette fuzzy-find. `None` in draft mode — the
    /// editor avoids scanning the filesystem until a folder is explicitly
    /// opened.
    working_dir: Option<PathBuf>,

    /// Tracks if buffer has unsaved changes.
    is_dirty: bool,

    /// Autocomplete suggestion menu. `None` when not active.
    autocomplete: Option<Autocomplete>,

    /// Find/replace panel state. `None` when closed.
    find_panel: Option<FindPanelState>,

    /// Guards against the editor handling Enter after the find panel consumed it.
    suppress_next_enter: bool,

    /// Editor configuration loaded from disk.
    config: EditorConfig,

    /// Undo stack for reverting changes.
    undo_stack: Vec<EditOperation>,

    /// Redo stack for reapplying undone changes.
    redo_stack: Vec<EditOperation>,

    /// Go-to-line panel input. None when closed.
    goto_panel: Option<String>,

    /// Tracks whether the mouse is being dragged for selection.
    is_dragging: bool,

    /// Starting cursor position for a mouse drag selection.
    drag_start_position: usize,

    /// Cached window width for word-wrap calculations.
    window_width: f32,

    /// Cached window height, used as a fallback for viewport sizing before the
    /// scroll viewport has been measured during paint.
    window_height: f32,

    /// Markup language driving highlighter, autocomplete, and smart-list
    /// continuation. Re-derived from the file extension on open/load.
    language: Language,

    /// Whether the caret is visible during the blink cycle.
    cursor_blink_visible: bool,

    /// Last cursor position used to restart blinking after movement.
    cursor_blink_reset_position: usize,

    /// Window bounds of the scroll viewport, updated each frame during paint.
    scroll_viewport_bounds: Option<Bounds<Pixels>>,

    /// Brief compiler diagnostics for the current buffer (empty for Markdown).
    diagnostics: Vec<CompileDiagnostic>,

    /// Set when the buffer changed and diagnostics need recomputing. Render
    /// kicks off a debounced recompute when this is true.
    diag_dirty: bool,

    /// Monotonic token used to debounce/cancel stale diagnostic recomputes.
    diag_generation: u64,

    /// Whether the diagnostics list panel is visible.
    show_diagnostics: bool,
}

#[derive(Clone)]
struct RenderRun {
    text: String,
    text_color: Rgba,
    background: Option<Rgba>,
}

enum SegmentPiece {
    Text(RenderRun),
    Cursor,
}

#[derive(Clone, Copy)]
struct HighlightSlice {
    start: usize,
    end: usize,
    kind: HighlightKind,
}

#[derive(Clone, Copy)]
enum HighlightKind {
    Selection,
    SearchActive,
    SearchMatch,
}

/// How a content line should be highlighted, accounting for multi-line context
/// (fenced code blocks and Brief block comments) the per-line tokenizers miss.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LineMode {
    Normal,
    Code,
    Comment,
}

impl HighlightKind {
    fn priority(&self) -> u8 {
        match self {
            HighlightKind::Selection => 3,
            HighlightKind::SearchActive => 2,
            HighlightKind::SearchMatch => 1,
        }
    }

    fn background(&self, theme: &Theme) -> Rgba {
        match self {
            HighlightKind::Selection => theme.highlight.selection_bg,
            HighlightKind::SearchActive => theme.highlight.search_active_bg,
            HighlightKind::SearchMatch => theme.highlight.search_match_bg,
        }
    }

    fn text_color(&self, _fallback: Rgba, theme: &Theme) -> Rgba {
        match self {
            HighlightKind::Selection => theme.highlight.selection_fg,
            HighlightKind::SearchActive => theme.highlight.search_active_fg,
            HighlightKind::SearchMatch => theme.highlight.search_match_fg,
        }
    }
}

impl TextEditor {
    /// Creates a new TextEditor instance.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Optional path to a file to load. When `None`, the editor
    ///   starts as an empty draft buffer (Notepad++-style).
    /// * `working_dir` - Optional folder to scope palette fuzzy-find. When
    ///   `None`, the editor will not scan the filesystem; Ctrl+P/Ctrl+O fall
    ///   back to a native file picker instead.
    /// * `config` - Loaded editor configuration.
    /// * `cx` - GPUI context for initialization.
    ///
    /// # Behavior
    ///
    /// - If `file_path` exists: loads content and stores path.
    /// - If `file_path` is provided but missing: starts with an empty buffer
    ///   and remembers the target path; nothing is written to disk until the
    ///   user saves.
    /// - If no `file_path`: starts as an empty unsaved draft.
    pub fn with_file(
        file_path: Option<String>,
        working_dir: Option<PathBuf>,
        config: EditorConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let (content, current_file) = if let Some(path) = file_path {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    println!("Loaded file: {}", path);
                    (content, Some(path))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), Some(path)),
                Err(e) => {
                    eprintln!("Failed to open file: {}", e);
                    (String::new(), Some(path))
                }
            }
        } else {
            (String::new(), None)
        };

        let language = current_file
            .as_deref()
            .map(Language::from_path)
            .unwrap_or(Language::Brief);

        let editor = Self {
            content,
            cursor_position: 0,
            selection_start: None,
            focus_handle: cx.focus_handle(),
            current_file,
            scroll_offset: 0.0,
            palette: None,
            working_dir,
            is_dirty: false,
            autocomplete: None,
            find_panel: None,
            suppress_next_enter: false,
            config,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            goto_panel: None,
            is_dragging: false,
            drag_start_position: 0,
            window_width: 800.0,
            window_height: 600.0,
            language,
            cursor_blink_visible: true,
            cursor_blink_reset_position: 0,
            scroll_viewport_bounds: None,
            diagnostics: Vec::new(),
            diag_dirty: true,
            diag_generation: 0,
            show_diagnostics: false,
        };

        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(530)).await;
                let _ = this.update(cx, |editor, cx| {
                    editor.cursor_blink_visible = !editor.cursor_blink_visible;
                    cx.notify();
                });
            }
        })
        .detach();

        editor
    }

    /// Produces colored runs for one source line by dispatching to the
    /// active language highlighter. Concatenation of run texts equals the
    /// input line (callers rely on byte alignment).
    fn line_runs(&self, line: &str, syntax: &SyntaxTheme) -> Vec<(String, Rgba)> {
        match self.language {
            Language::Brief => BriefHighlighter::tokenize_line(line)
                .into_iter()
                .map(|(text, kind)| (text, BriefHighlighter::get_color(&kind, syntax)))
                .collect(),
            Language::Markdown => MarkdownHighlighter::tokenize_line(line)
                .into_iter()
                .map(|(text, kind)| (text, MarkdownHighlighter::get_color(&kind, syntax)))
                .collect(),
        }
    }

    /// Produces colored runs for one source line, honoring multi-line context.
    ///
    /// The per-line highlighters are stateless, so without this the *interior*
    /// of fenced code blocks (and Brief `/* */` block comments) would be
    /// tokenized as ordinary markup. `mode` is precomputed by
    /// [`Self::compute_line_modes`] so interior lines render verbatim.
    fn runs_for_line(
        &self,
        line: &str,
        mode: LineMode,
        syntax: &SyntaxTheme,
    ) -> Vec<(String, Rgba)> {
        match mode {
            LineMode::Normal => self.line_runs(line, syntax),
            LineMode::Code => vec![(line.to_string(), syntax.code_block)],
            LineMode::Comment => vec![(line.to_string(), syntax.comment)],
        }
    }

    /// Classifies every content line as normal markup, fenced-code interior, or
    /// block-comment interior. Fence (```` ``` ````) handling applies to both
    /// languages; `/* */` block comments are Brief-only. Marker lines stay
    /// `Normal` so the per-line highlighter colors the fence/comment markers.
    fn compute_line_modes(&self) -> Vec<LineMode> {
        let brief = self.language == Language::Brief;
        let mut modes = Vec::new();
        let mut in_fence = false;
        let mut in_comment = false;

        for line in self.content.split('\n') {
            let trimmed = line.trim_start_matches(' ');

            if in_comment {
                modes.push(LineMode::Comment);
                if trimmed.ends_with("*/") {
                    in_comment = false;
                }
                continue;
            }

            if in_fence {
                if trimmed.starts_with("```") {
                    in_fence = false;
                    modes.push(LineMode::Normal); // closing fence marker
                } else {
                    modes.push(LineMode::Code);
                }
                continue;
            }

            if trimmed.starts_with("```") {
                in_fence = true;
                modes.push(LineMode::Normal); // opening fence marker
            } else if brief && trimmed.starts_with("/*") && !trimmed[2..].contains("*/") {
                in_comment = true;
                modes.push(LineMode::Normal); // opening comment marker
            } else {
                modes.push(LineMode::Normal);
            }
        }

        modes
    }

    fn font_size(&self) -> f32 {
        self.config.font_size()
    }

    fn font_scale(&self) -> f32 {
        (self.font_size() / EditorConfig::DEFAULT_FONT_SIZE).max(0.5)
    }

    fn line_height(&self) -> f32 {
        20.0 * self.font_scale()
    }

    fn cursor_height(&self) -> f32 {
        (self.line_height() - 2.0 * self.font_scale()).max(8.0)
    }

    fn char_width(&self) -> f32 {
        8.0 * self.font_scale()
    }

    fn header_height(&self) -> f32 {
        self.line_height() + 8.0 * self.font_scale()
    }

    fn padding(&self) -> f32 {
        16.0
    }

    fn viewport_height(&self) -> f32 {
        // Prefer the viewport height measured during paint (tracks window
        // resizes exactly). Fall back to deriving it from the cached window
        // height before the first paint.
        if let Some(bounds) = &self.scroll_viewport_bounds {
            let measured: f32 = bounds.size.height.into();
            if measured > 0.0 {
                return measured;
            }
        }
        let chrome = self.padding() * 2.0 + self.header_height() + self.line_height() + 16.0;
        (self.window_height - chrome).max(self.line_height())
    }

    fn gutter_width(&self) -> f32 {
        let total_lines = self.content.split('\n').count();
        (total_lines.to_string().len() as f32 * self.char_width()) + 16.0
    }

    fn text_area_width(&self) -> f32 {
        (self.window_width - self.padding() * 2.0 - self.gutter_width() - 8.0).max(100.0)
    }

    fn chars_per_line(&self) -> usize {
        let width = self.text_area_width();
        let cw = self.char_width();
        if cw <= 0.0 {
            80
        } else {
            (width / cw).max(10.0) as usize
        }
    }

    fn should_show_cursor(&self) -> bool {
        self.cursor_blink_visible || self.is_dragging
    }

    /// Maps a display column (monospace character index) to a byte offset within `segment`.
    fn char_col_to_byte_offset(segment: &str, char_col: usize) -> usize {
        if segment.is_empty() {
            return 0;
        }

        let mut chars_seen = 0;
        for (byte_idx, _ch) in segment.char_indices() {
            if chars_seen >= char_col {
                return byte_idx;
            }
            chars_seen += 1;
        }

        segment.len()
    }

    /// Converts a window-space click position to a document byte offset.
    ///
    /// Uses the same visual-line model as rendering and keyboard navigation.
    /// Y is mapped via the measured scroll viewport bounds when available.
    fn byte_offset_at_pixel(&self, position: Point<Pixels>) -> usize {
        let char_width = self.char_width();
        let line_height = self.line_height();
        let padding = self.padding();
        let gutter = self.gutter_width();

        // X: window position minus chrome before the text column (validated layout).
        let click_x: f32 = (position.x - px(padding) - px(gutter)).into();

        // Y: use measured viewport top so we don't rely on estimated header height.
        let click_y: f32 = if let Some(bounds) = &self.scroll_viewport_bounds {
            (position.y - bounds.top() + px(self.scroll_offset)).into()
        } else {
            (position.y - px(padding) - px(self.header_height()) + px(self.scroll_offset)).into()
        };

        let visual_lines = self.build_visual_lines();
        if visual_lines.is_empty() {
            return 0;
        }

        let visual_line_idx = (click_y / line_height).max(0.0).floor() as usize;
        let visual_line_idx = visual_line_idx.min(visual_lines.len() - 1);
        let vl = &visual_lines[visual_line_idx];

        let char_col = if click_x <= 0.0 {
            0
        } else {
            (click_x / char_width).round() as usize
        };

        let segment = &self.content[vl.start_byte_in_content..vl.end_byte_in_content];
        let byte_in_segment = Self::char_col_to_byte_offset(segment, char_col);

        (vl.start_byte_in_content + byte_in_segment).min(self.content.len())
    }

    /// Records an edit on the undo stack as a compact diff and clears redo.
    ///
    /// Consecutive single-character typing is coalesced into one record (so one
    /// Ctrl+Z undoes a whole word rather than a letter), breaking at whitespace
    /// so the grouping feels natural. History is capped at
    /// [`MAX_UNDO_HISTORY`] records.
    fn push_edit(&mut self, old_content: String, old_cursor: usize, old_selection: Option<usize>) {
        let (start, removed, inserted) = compute_diff(&old_content, &self.content);

        // Nothing actually changed — don't record an empty operation.
        if removed.is_empty() && inserted.is_empty() {
            return;
        }

        // Try to merge a pure single-char insertion onto the previous record
        // when it continues an uninterrupted typing run.
        let is_pure_single_insert = removed.is_empty() && inserted.chars().count() == 1;
        if is_pure_single_insert {
            if let Some(prev) = self.undo_stack.last_mut() {
                let prev_was_insert = prev.removed.is_empty();
                let contiguous = prev.start + prev.inserted.len() == start;
                let break_on_ws = inserted.starts_with(char::is_whitespace)
                    || prev.inserted.ends_with(char::is_whitespace);
                if prev_was_insert && contiguous && !break_on_ws {
                    prev.inserted.push_str(&inserted);
                    prev.new_cursor = self.cursor_position;
                    prev.new_selection = self.selection_start;
                    self.redo_stack.clear();
                    return;
                }
            }
        }

        self.undo_stack.push(EditOperation {
            start,
            removed,
            inserted,
            old_cursor,
            new_cursor: self.cursor_position,
            old_selection,
            new_selection: self.selection_start,
        });
        if self.undo_stack.len() > MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// Finds the position of the previous word boundary from the current cursor position.
    ///
    /// Word boundaries are defined as transitions between alphanumeric and non-alphanumeric characters.
    fn find_prev_word_boundary(&self) -> usize {
        if self.cursor_position == 0 {
            return 0;
        }

        let chars: Vec<char> = self.content.chars().collect();

        // Convert byte offset to char index
        let mut char_pos = 0;
        for (i, _) in self.content.char_indices() {
            if i >= self.cursor_position {
                break;
            }
            char_pos += 1;
        }

        // Skip trailing whitespace
        while char_pos > 0 && chars[char_pos - 1].is_whitespace() {
            char_pos -= 1;
        }

        if char_pos == 0 {
            return 0;
        }

        // Determine the type of the current character
        let is_alphanum = chars[char_pos - 1].is_alphanumeric() || chars[char_pos - 1] == '_';

        // Move back to the start of the word
        while char_pos > 0 {
            let prev_is_alphanum =
                chars[char_pos - 1].is_alphanumeric() || chars[char_pos - 1] == '_';
            if prev_is_alphanum != is_alphanum {
                break;
            }
            char_pos -= 1;
        }

        // Convert char index back to byte offset
        self.content
            .char_indices()
            .nth(char_pos)
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// Finds the position of the next word boundary from the current cursor position.
    ///
    /// Word boundaries are defined as transitions between alphanumeric and non-alphanumeric characters.
    fn find_next_word_boundary(&self) -> usize {
        let len = self.content.len();
        if self.cursor_position >= len {
            return len;
        }

        let chars: Vec<char> = self.content.chars().collect();

        // Convert byte offset to char index
        let mut char_pos = 0;
        for (i, _) in self.content.char_indices() {
            if i >= self.cursor_position {
                break;
            }
            char_pos += 1;
        }

        if char_pos >= chars.len() {
            return len;
        }

        // Determine the type of the current character
        let is_alphanum = chars[char_pos].is_alphanumeric() || chars[char_pos] == '_';

        // Move forward to the end of the word
        while char_pos < chars.len() {
            let curr_is_alphanum = chars[char_pos].is_alphanumeric() || chars[char_pos] == '_';
            if curr_is_alphanum != is_alphanum {
                break;
            }
            char_pos += 1;
        }

        // Skip leading whitespace of the next word
        while char_pos < chars.len() && chars[char_pos].is_whitespace() {
            char_pos += 1;
        }

        // Convert char index back to byte offset
        self.content
            .char_indices()
            .nth(char_pos)
            .map(|(i, _)| i)
            .unwrap_or(len)
    }

    /// Byte offset of the char boundary immediately before `pos` (UTF-8 safe).
    fn prev_char_boundary(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        let mut p = pos - 1;
        while p > 0 && !self.content.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    /// Byte offset of the char boundary immediately after `pos` (UTF-8 safe).
    fn next_char_boundary(&self, pos: usize) -> usize {
        let len = self.content.len();
        if pos >= len {
            return len;
        }
        let mut p = pos + 1;
        while p < len && !self.content.is_char_boundary(p) {
            p += 1;
        }
        p
    }

    /// Finds the start of the current line (byte offset).
    fn find_line_start(&self) -> usize {
        self.content[..self.cursor_position]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0)
    }

    /// Finds the end of the current line (byte offset).
    fn find_line_end(&self) -> usize {
        self.content[self.cursor_position..]
            .find('\n')
            .map(|pos| self.cursor_position + pos)
            .unwrap_or(self.content.len())
    }

    /// Calculates the current line number (1-indexed) based on cursor position.
    ///
    /// Counts newlines before the cursor to determine which line we're on.
    fn get_current_line_number(&self) -> usize {
        self.content[..self.cursor_position]
            .chars()
            .filter(|&c| c == '\n')
            .count()
            + 1
    }

    /// Gets the content of the current line up to the cursor position.
    ///
    /// Used for autocomplete trigger detection.
    fn get_current_line_content(&self) -> String {
        let start = self.content[..self.cursor_position]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        self.content[start..self.cursor_position].to_string()
    }

    /// Returns the normalized selection range as (start, end) byte offsets.
    ///
    /// Selection is always normalized so that start < end, regardless of
    /// the direction the selection was made (forward or backward).
    ///
    /// Returns `None` if no selection is active.
    fn get_selection_range(&self) -> Option<(usize, usize)> {
        self.selection_start.map(|start| {
            if start < self.cursor_position {
                (start, self.cursor_position)
            } else {
                (self.cursor_position, start)
            }
        })
    }

    /// Extracts the currently selected text as a string.
    ///
    /// Returns `None` if no selection is active.
    /// Used for copy and cut operations.
    fn get_selected_text(&self) -> Option<String> {
        self.get_selection_range()
            .map(|(start, end)| self.content[start..end].to_string())
    }

    /// Recomputes matches when content or query changes.
    ///
    /// This is the single hook every content mutation funnels through, so it
    /// also flags diagnostics as stale; `render` debounces the recompute.
    fn refresh_search_matches(&mut self) {
        self.diag_dirty = true;
        let has_panel = self.find_panel.is_some();
        if let Some(find) = self.find_panel.as_mut() {
            find.recompute_matches(&self.content);
        }
        if has_panel {
            if !self.focus_current_search_match() {
                self.selection_start = None;
            }
        }
    }

    /// Kicks off a debounced Brief diagnostics recompute.
    ///
    /// Markdown buffers have no Brief diagnostics, so they clear immediately.
    /// For Brief, a generation token cancels stale recomputes: only the most
    /// recent edit's analysis is stored, keeping typing responsive on large
    /// files (compilation runs at most once per ~200ms quiet window).
    fn schedule_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.diag_dirty = false;

        if self.language != Language::Brief {
            if !self.diagnostics.is_empty() {
                self.diagnostics.clear();
            }
            return;
        }

        self.diag_generation = self.diag_generation.wrapping_add(1);
        let generation = self.diag_generation;
        let content = self.content.clone();
        let path = self
            .current_file
            .clone()
            .unwrap_or_else(|| "draft.brf".to_string());

        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(200)).await;
            let _ = this.update(cx, |editor, cx| {
                // Drop this result if a newer edit superseded it.
                if editor.diag_generation != generation {
                    return;
                }
                let analysis = compile::analyze(&path, &content);
                editor.diagnostics = analysis.diagnostics;
                cx.notify();
            });
        })
        .detach();
    }

    /// Counts errors / warnings in the current diagnostics.
    fn diagnostic_counts(&self) -> (usize, usize) {
        let errors = self.diagnostics.iter().filter(|d| d.is_error()).count();
        (errors, self.diagnostics.len() - errors)
    }

    /// Highest-severity diagnostic kind on a given 1-indexed line, if any.
    /// `Some(true)` = an error is present, `Some(false)` = only warnings.
    fn line_diagnostic(&self, line: usize) -> Option<bool> {
        let mut found_warning = false;
        for diag in self.diagnostics.iter().filter(|d| d.line == line) {
            if diag.is_error() {
                return Some(true);
            }
            found_warning = true;
        }
        found_warning.then_some(false)
    }

    /// Moves the cursor to the next diagnostic after the current line, wrapping
    /// around. No-op when there are no diagnostics.
    fn goto_next_diagnostic(&mut self) {
        if self.diagnostics.is_empty() {
            return;
        }
        let current_line = self.get_current_line_number();
        let mut lines: Vec<usize> = self.diagnostics.iter().map(|d| d.line).collect();
        lines.sort_unstable();
        lines.dedup();
        let target = lines
            .iter()
            .copied()
            .find(|&l| l > current_line)
            .unwrap_or(lines[0]);

        // Convert the 1-indexed line to a byte offset at the line start.
        let mut byte = 0usize;
        for (idx, line) in self.content.split('\n').enumerate() {
            if idx + 1 == target {
                self.cursor_position = byte;
                self.selection_start = None;
                self.ensure_position_visible(byte);
                break;
            }
            byte += line.len() + 1;
        }
    }

    /// Opens the find panel, seeding it from the current selection when possible.
    fn open_find_panel(&mut self) {
        let initial = self
            .get_selected_text()
            .filter(|text| !text.trim().is_empty() && !text.contains('\n'));
        let mut panel = FindPanelState::new(initial);
        panel.recompute_matches(&self.content);
        self.find_panel = Some(panel);
    }

    /// Closes the panel and clears highlights.
    fn close_find_panel(&mut self) {
        self.find_panel = None;
    }

    /// Ensures the byte offset is visible inside the viewport.
    fn ensure_position_visible(&mut self, byte_offset: usize) {
        let line_height = self.line_height();
        let viewport_height = self.viewport_height();
        let visual_lines = self.build_visual_lines();

        for (idx, vl) in visual_lines.iter().enumerate() {
            if byte_offset >= vl.start_byte_in_content && byte_offset <= vl.end_byte_in_content {
                let top = idx as f32 * line_height;
                let bottom = top + line_height;
                let viewport_top = self.scroll_offset;
                let viewport_bottom = viewport_top + viewport_height;

                if top < viewport_top {
                    self.scroll_offset = top.max(0.0);
                } else if bottom > viewport_bottom {
                    self.scroll_offset = (bottom - viewport_height).max(0.0);
                }
                break;
            }
        }
    }

    /// Applies selection and caret to the provided match range.
    fn focus_match(&mut self, range: SearchMatch) {
        self.selection_start = Some(range.start);
        self.cursor_position = range.end;
        self.ensure_position_visible(range.start);
    }

    fn focus_current_search_match(&mut self) -> bool {
        if let Some(panel) = &self.find_panel {
            if let Some(range) = panel.current_match() {
                self.focus_match(range);
                return true;
            }
        }
        false
    }

    /// Advances search selection by direction and updates view.
    fn advance_search(&mut self, direction: isize) -> Option<SearchMatch> {
        if let Some(panel) = self.find_panel.as_mut() {
            if !panel.has_matches() {
                return None;
            }
            let range = panel.cycle(direction)?;
            panel.refresh_anchor();
            Some(range)
        } else {
            None
        }
    }

    /// Handles backspace when the find panel is active.
    fn handle_find_backspace(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(panel) = self.find_panel.as_mut() {
            panel.backspace(&self.content);
            if panel.has_matches() {
                panel.refresh_anchor();
                self.focus_current_search_match();
            } else {
                self.selection_start = None;
            }
            cx.notify();
            return true;
        }
        false
    }

    /// Replaces the current match with the replacement text.
    fn replace_current_match(&mut self) -> bool {
        let (range, replacement) = {
            let panel = match self.find_panel.as_ref() {
                Some(panel) if panel.has_matches() && !panel.query.is_empty() => panel,
                _ => return false,
            };
            // Only allow replacements when the UI exposes the intent.
            if !panel.show_replace {
                return false;
            }
            let replace_value = panel.replace.clone();
            let range = panel.current_match().unwrap();
            (range, replace_value)
        };

        self.content
            .replace_range(range.start..range.end, &replacement);
        self.cursor_position = range.start + replacement.len();
        self.selection_start = Some(range.start);
        self.is_dirty = true;

        self.refresh_search_matches();
        if let Some(panel) = self.find_panel.as_mut() {
            panel.refresh_anchor();
        }
        true
    }

    /// Replaces all matches, returning how many edits were made.
    fn replace_all_matches(&mut self) -> usize {
        let (needle, replacement) = {
            let panel = match self.find_panel.as_ref() {
                Some(panel) if panel.has_query() && panel.show_replace => panel,
                _ => return 0,
            };
            (panel.query.clone(), panel.replace.clone())
        };

        if needle.is_empty() {
            return 0;
        }

        let mut replaced = 0;
        let mut search_index = 0;

        while search_index <= self.content.len() {
            let tail = &self.content[search_index..];
            if let Some(found) = tail.find(&needle) {
                let start = search_index + found;
                let end = start + needle.len();
                self.content.replace_range(start..end, &replacement);
                search_index = start + replacement.len();
                replaced += 1;
            } else {
                break;
            }
        }

        if replaced > 0 {
            self.cursor_position = self.cursor_position.min(self.content.len());
            self.selection_start = None;
            self.is_dirty = true;
            self.refresh_search_matches();
            if let Some(panel) = self.find_panel.as_mut() {
                panel.refresh_anchor();
            }
        }

        replaced
    }

    fn build_segments_for_token(
        &self,
        text: &str,
        token_color: Rgba,
        token_start: usize,
        selection_range: Option<(usize, usize)>,
        cursor_position: Option<usize>,
        search_panel: Option<&FindPanelState>,
        theme: &Theme,
    ) -> Vec<SegmentPiece> {
        let token_len = text.len();
        if token_len == 0 {
            return Vec::new();
        }

        let token_end = token_start + token_len;
        let mut slices = Vec::new();

        if let Some((sel_start, sel_end)) = selection_range {
            if sel_end > token_start && sel_start < token_end {
                slices.push(HighlightSlice {
                    start: sel_start.max(token_start) - token_start,
                    end: sel_end.min(token_end) - token_start,
                    kind: HighlightKind::Selection,
                });
            }
        }

        if let Some(panel) = search_panel {
            if panel.has_query() {
                let active_index = panel.current_index();
                for (idx, search_match) in panel.matches.iter().enumerate() {
                    if search_match.end <= token_start {
                        continue;
                    }
                    if search_match.start >= token_end {
                        break;
                    }
                    let kind = if Some(idx) == active_index {
                        HighlightKind::SearchActive
                    } else {
                        HighlightKind::SearchMatch
                    };
                    slices.push(HighlightSlice {
                        start: search_match.start.max(token_start) - token_start,
                        end: search_match.end.min(token_end) - token_start,
                        kind,
                    });
                }
            }
        }

        let mut boundaries = vec![0, token_len];
        for slice in &slices {
            boundaries.push(slice.start);
            boundaries.push(slice.end);
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        let mut segments = Vec::new();
        for range in boundaries.windows(2) {
            let start = range[0];
            let end = range[1];
            if start == end {
                continue;
            }

            let mut run = RenderRun {
                text: text[start..end].to_string(),
                text_color: token_color,
                background: None,
            };

            if let Some(active_slice) = slices
                .iter()
                .filter(|slice| slice.start < end && slice.end > start)
                .max_by_key(|slice| slice.kind.priority())
            {
                run.background = Some(active_slice.kind.background(theme));
                run.text_color = active_slice.kind.text_color(token_color, theme);
            }

            segments.push(SegmentPiece::Text(run));
        }

        if segments.is_empty() {
            segments.push(SegmentPiece::Text(RenderRun {
                text: text.to_string(),
                text_color: token_color,
                background: None,
            }));
        }

        if let Some(cursor_abs) = cursor_position {
            let overlaps_selection = selection_range
                .map(|(sel_start, sel_end)| sel_end > token_start && sel_start < token_end)
                .unwrap_or(false);

            if !overlaps_selection && cursor_abs >= token_start && cursor_abs < token_end {
                let cursor_offset = cursor_abs - token_start;
                return Self::insert_cursor_segment(segments, cursor_offset);
            }
        }

        segments
    }

    fn insert_cursor_segment(
        segments: Vec<SegmentPiece>,
        cursor_offset: usize,
    ) -> Vec<SegmentPiece> {
        let mut consumed = 0;
        let mut result = Vec::new();
        let mut inserted = false;

        for segment in segments {
            match segment {
                SegmentPiece::Text(run) => {
                    let seg_len = run.text.len();

                    if !inserted && cursor_offset >= consumed && cursor_offset <= consumed + seg_len
                    {
                        let local = cursor_offset - consumed;
                        if local == 0 {
                            result.push(SegmentPiece::Cursor);
                            result.push(SegmentPiece::Text(run));
                        } else if local == seg_len {
                            result.push(SegmentPiece::Text(run));
                            result.push(SegmentPiece::Cursor);
                        } else {
                            let text = run.text;
                            let text_color = run.text_color;
                            let background = run.background;
                            let left_text = text[..local].to_string();
                            let right_text = text[local..].to_string();

                            result.push(SegmentPiece::Text(RenderRun {
                                text: left_text,
                                text_color,
                                background,
                            }));
                            result.push(SegmentPiece::Cursor);
                            result.push(SegmentPiece::Text(RenderRun {
                                text: right_text,
                                text_color,
                                background,
                            }));
                        }
                        inserted = true;
                    } else {
                        result.push(SegmentPiece::Text(run));
                    }

                    consumed += seg_len;
                }
                SegmentPiece::Cursor => result.push(SegmentPiece::Cursor),
            }
        }

        if !inserted {
            result.push(SegmentPiece::Cursor);
        }

        result
    }

    /// Routes key events to the find panel when it is open.
    fn handle_find_key_event(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.find_panel.is_none() {
            return false;
        }

        // Esc closes the panel.
        if event.keystroke.key == "escape" {
            self.close_find_panel();
            cx.notify();
            return true;
        }

        // Tab cycles between query/replace when both are visible.
        if event.keystroke.key == "tab" {
            if let Some(panel) = self.find_panel.as_mut() {
                if panel.show_replace {
                    let next = match panel.active_input {
                        ActiveInput::Query => ActiveInput::Replace,
                        ActiveInput::Replace => ActiveInput::Query,
                    };
                    panel.set_active_input(next);
                    cx.notify();
                    return true;
                }
            }
        }

        // Ctrl+H toggles replace visibility.
        if event.keystroke.key == "h"
            && event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform
        {
            if let Some(panel) = self.find_panel.as_mut() {
                panel.toggle_replace();
                cx.notify();
            }
            return true;
        }

        // Ctrl+R / Ctrl+Shift+R handle replace actions.
        if event.keystroke.key == "r"
            && event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform
        {
            if event.keystroke.modifiers.shift {
                if self.replace_all_matches() > 0 {
                    cx.notify();
                }
            } else if self.replace_current_match() {
                cx.notify();
            }
            return true;
        }

        // Enter navigates matches while the panel owns focus.
        if event.keystroke.key == "enter" {
            if let Some(range) = self.advance_search(if event.keystroke.modifiers.shift {
                -1
            } else {
                1
            }) {
                self.focus_match(range);
                cx.notify();
            }
            self.suppress_next_enter = true;
            return true;
        }

        // Regular character input updates the active field.
        if let Some(ref key_char) = event.keystroke.key_char {
            if key_char.len() == 1
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
            {
                if let Some(c) = key_char.chars().next() {
                    if let Some(panel) = self.find_panel.as_mut() {
                        panel.push_char(c, &self.content);
                        if panel.has_matches() {
                            panel.refresh_anchor();
                            self.focus_current_search_match();
                        }
                    }
                    cx.notify();
                    return true;
                }
            }
        }

        false
    }

    /// Clears the active selection without modifying content.
    ///
    /// Called after cursor movements that should deselect (arrow keys without shift).
    fn clear_selection(&mut self) {
        self.selection_start = None;
    }

    /// Deletes the selected text and clears the selection.
    ///
    /// # Returns
    ///
    /// `true` if text was deleted, `false` if no selection was active.
    ///
    /// # Side Effects
    ///
    /// - Removes selected bytes from `content`
    /// - Moves cursor to start of deleted range
    /// - Clears selection state
    fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.get_selection_range() {
            self.content.drain(start..end);
            self.cursor_position = start;
            self.clear_selection();
            true
        } else {
            false
        }
    }

    /// Inserts a single character at the cursor position.
    ///
    /// If a selection is active, it's deleted first (standard text editor behavior).
    /// Advances cursor position by the UTF-8 byte length of the character.
    ///
    /// # Arguments
    ///
    /// * `c` - Character to insert
    /// * `cx` - Context for triggering UI refresh via `notify()`
    fn insert_char(&mut self, c: char, cx: &mut Context<Self>) {
        let old_content = self.content.clone();
        let old_cursor = self.cursor_position;
        let old_selection = self.selection_start;

        self.delete_selection();
        self.content.insert(self.cursor_position, c);
        self.cursor_position += c.len_utf8();
        self.is_dirty = true;

        self.push_edit(old_content, old_cursor, old_selection);

        // Check if this character should trigger autocomplete. Brief has
        // an extra trigger (`@` for shortcodes) and excludes `*` because
        // Brief uses single `*` for bold (no `**bold**`).
        let trigger = c.to_string();
        let triggers: &[&str] = match self.language {
            Language::Brief => &["#", "-", "`", ">", "[", "@"],
            Language::Markdown => &["#", "-", "`", ">", "[", "*"],
        };

        if triggers.contains(&trigger.as_str()) {
            let line_content = self.get_current_line_content();
            self.autocomplete = Autocomplete::new(&trigger, &line_content, self.language);
        } else if c == ' ' || c == '\n' {
            // Close autocomplete on space or newline
            self.autocomplete = None;
        }

        self.refresh_search_matches();
        cx.notify();
    }

    /// Handles backspace key press.
    ///
    /// Behavior:
    /// - If selection exists: delete selected text
    /// - Otherwise: delete character before cursor
    /// - Does nothing if cursor is at document start
    fn handle_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.handle_find_backspace(cx) {
            return;
        }

        // Close autocomplete on backspace
        self.autocomplete = None;

        let old_content = self.content.clone();
        let old_cursor = self.cursor_position;
        let old_selection = self.selection_start;

        if !self.delete_selection() {
            if self.cursor_position > 0 {
                // Remove the whole previous char (UTF-8 safe).
                self.cursor_position = self.prev_char_boundary(self.cursor_position);
                self.content.remove(self.cursor_position);
                self.is_dirty = true;
            } else {
                return; // Nothing to delete, don't record
            }
        } else {
            self.is_dirty = true;
        }

        self.push_edit(old_content, old_cursor, old_selection);
        self.refresh_search_matches();
        cx.notify();
    }

    /// Handles Delete key press (forward delete).
    ///
    /// Behavior:
    /// - If selection exists: delete selected text
    /// - Otherwise: delete character after cursor
    /// - Does nothing if cursor is at document end
    fn handle_delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;

        let old_content = self.content.clone();
        let old_cursor = self.cursor_position;
        let old_selection = self.selection_start;

        if !self.delete_selection() {
            if self.cursor_position < self.content.len() {
                self.content.remove(self.cursor_position);
                self.is_dirty = true;
            } else {
                return; // Nothing to delete, don't record
            }
        } else {
            self.is_dirty = true;
        }

        self.push_edit(old_content, old_cursor, old_selection);
        self.refresh_search_matches();
        cx.notify();
    }

    /// Handles Enter key press by inserting a newline at cursor position.
    /// If autocomplete is active, accepts the selected suggestion instead.
    /// Implements smart list continuation for markdown lists.
    fn handle_enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.suppress_next_enter {
            self.suppress_next_enter = false;
            return;
        }

        let old_content = self.content.clone();
        let old_cursor = self.cursor_position;
        let old_selection = self.selection_start;

        // If autocomplete is active, accept the selected suggestion
        if let Some(autocomplete) = &self.autocomplete {
            if let Some(suggestion) = autocomplete.get_selected() {
                // Get the line start position
                let line_start = self.content[..self.cursor_position]
                    .rfind('\n')
                    .map(|pos| pos + 1)
                    .unwrap_or(0);

                // Replace from line start to cursor with the suggestion
                self.content.drain(line_start..self.cursor_position);
                self.content.insert_str(line_start, &suggestion.insert_text);
                self.cursor_position = line_start + suggestion.insert_text.len();
                self.is_dirty = true;
                self.push_edit(old_content, old_cursor, old_selection);
            }
            self.autocomplete = None;
            self.refresh_search_matches();
            cx.notify();
            return;
        }

        // Smart list continuation
        let line_start = self.find_line_start();
        let line_content = &self.content[line_start..self.cursor_position];

        // `*` and `+` bullets are Markdown-only — Brief rejects them.
        let allow_md_bullets = matches!(self.language, Language::Markdown);

        // Check for different list patterns
        let list_marker = if let Some(rest) = line_content.strip_prefix("- ") {
            if rest.trim().is_empty() {
                // Empty list item, remove the marker
                self.content.drain(line_start..self.cursor_position);
                self.cursor_position = line_start;
                self.is_dirty = true;
                self.push_edit(old_content, old_cursor, old_selection);
                self.refresh_search_matches();
                cx.notify();
                return;
            }
            Some(String::from("- "))
        } else if allow_md_bullets && let Some(rest) = line_content.strip_prefix("* ") {
            if rest.trim().is_empty() {
                self.content.drain(line_start..self.cursor_position);
                self.cursor_position = line_start;
                self.is_dirty = true;
                self.push_edit(old_content, old_cursor, old_selection);
                self.refresh_search_matches();
                cx.notify();
                return;
            }
            Some(String::from("* "))
        } else if allow_md_bullets && let Some(rest) = line_content.strip_prefix("+ ") {
            if rest.trim().is_empty() {
                self.content.drain(line_start..self.cursor_position);
                self.cursor_position = line_start;
                self.is_dirty = true;
                self.push_edit(old_content, old_cursor, old_selection);
                self.refresh_search_matches();
                cx.notify();
                return;
            }
            Some(String::from("+ "))
        } else if let Some(rest) = line_content.strip_prefix("- [ ] ") {
            if rest.trim().is_empty() {
                self.content.drain(line_start..self.cursor_position);
                self.cursor_position = line_start;
                self.is_dirty = true;
                self.push_edit(old_content, old_cursor, old_selection);
                self.refresh_search_matches();
                cx.notify();
                return;
            }
            Some(String::from("- [ ] "))
        } else if let Some(rest) = line_content.strip_prefix("- [x] ") {
            if rest.trim().is_empty() {
                self.content.drain(line_start..self.cursor_position);
                self.cursor_position = line_start;
                self.is_dirty = true;
                self.push_edit(old_content, old_cursor, old_selection);
                self.refresh_search_matches();
                cx.notify();
                return;
            }
            // Continue with unchecked checkbox
            Some(String::from("- [ ] "))
        } else {
            // Check for numbered lists (e.g., "1. ", "42. ")
            let trimmed = line_content.trim_start();
            if let Some(dot_pos) = trimmed.find(". ") {
                let num_part = &trimmed[..dot_pos];
                if num_part.chars().all(|c| c.is_ascii_digit()) {
                    let rest = &trimmed[dot_pos + 2..];
                    if rest.trim().is_empty() {
                        // Empty numbered item, remove it
                        self.content.drain(line_start..self.cursor_position);
                        self.cursor_position = line_start;
                        self.is_dirty = true;
                        self.push_edit(old_content, old_cursor, old_selection);
                        self.refresh_search_matches();
                        cx.notify();
                        return;
                    }
                    // Continue with next number
                    if let Ok(num) = num_part.parse::<usize>() {
                        Some(format!("{}. ", num + 1))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(marker) = list_marker {
            self.content.insert(self.cursor_position, '\n');
            self.cursor_position += 1;
            self.content.insert_str(self.cursor_position, &marker);
            self.cursor_position += marker.len();
        } else {
            self.content.insert(self.cursor_position, '\n');
            self.cursor_position += 1;
        }

        self.is_dirty = true;
        self.push_edit(old_content, old_cursor, old_selection);
        self.refresh_search_matches();
        cx.notify();
    }

    /// Handles Tab key press.
    ///
    /// Behavior:
    /// - If selection spans multiple lines: indent each selected line by 2 spaces
    /// - Otherwise: insert 2 spaces at cursor
    fn handle_tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;

        let old_content = self.content.clone();
        let old_cursor = self.cursor_position;
        let old_selection = self.selection_start;

        if let Some((sel_start, sel_end)) = self.get_selection_range() {
            // Multi-line indent: find line starts in selection and indent each
            let start_line = self.content[..sel_start]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);
            let _end_line = self.content[..sel_end]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);

            let mut insertions = Vec::new();
            let mut pos = start_line;
            while pos <= sel_end {
                insertions.push(pos);
                if let Some(next) = self.content[pos..].find('\n') {
                    pos += next + 1;
                    if pos > sel_end && pos > self.content.len() {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Apply insertions in reverse to preserve offsets
            let mut shift = 0;
            for &insert_pos in &insertions {
                self.content.insert_str(insert_pos + shift, "  ");
                shift += 2;
            }

            self.cursor_position = old_cursor + if old_cursor >= start_line { 2 } else { 0 };
            if let Some(ref mut sel) = self.selection_start {
                *sel = old_selection.unwrap()
                    + if old_selection.unwrap() >= start_line {
                        2
                    } else {
                        0
                    };
            }
            // Adjust selection end
            let sel_end_new = sel_end + shift;
            self.selection_start = Some(sel_end_new);
            self.cursor_position = sel_end_new;
            self.is_dirty = true;
            self.push_edit(old_content, old_cursor, old_selection);
            self.refresh_search_matches();
            cx.notify();
            return;
        }

        // Single-line: insert 2 spaces
        self.content.insert_str(self.cursor_position, "  ");
        self.cursor_position += 2;
        self.is_dirty = true;
        self.push_edit(old_content, old_cursor, old_selection);
        self.refresh_search_matches();
        cx.notify();
    }

    /// Handles Shift+Tab key press (unindent).
    ///
    /// Behavior:
    /// - If selection spans multiple lines: unindent each selected line by up to 2 spaces
    /// - Otherwise: remove up to 2 spaces before cursor
    fn handle_shift_tab(&mut self, _: &ShiftTab, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;

        let old_content = self.content.clone();
        let old_cursor = self.cursor_position;
        let old_selection = self.selection_start;

        if let Some((sel_start, sel_end)) = self.get_selection_range() {
            let start_line = self.content[..sel_start]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);

            let mut removals = Vec::new();
            let mut pos = start_line;
            while pos <= sel_end && pos < self.content.len() {
                let line_end = self.content[pos..]
                    .find('\n')
                    .map(|p| pos + p)
                    .unwrap_or(self.content.len());
                let line_text = &self.content[pos..line_end];
                let spaces = line_text.chars().take_while(|c| *c == ' ').count();
                let remove = spaces.min(2);
                if remove > 0 {
                    removals.push((pos, remove));
                }
                pos = line_end + 1;
            }

            let mut shift = 0;
            for &(remove_pos, remove_count) in removals.iter().rev() {
                self.content
                    .drain(remove_pos + shift..remove_pos + shift + remove_count);
                shift -= remove_count;
            }

            self.cursor_position =
                old_cursor.saturating_sub(if old_cursor > start_line { 2 } else { 0 });
            if let Some(ref mut sel) = self.selection_start {
                let old = old_selection.unwrap();
                *sel = old.saturating_sub(if old > start_line { 2 } else { 0 });
            }
            self.is_dirty = true;
            self.push_edit(old_content, old_cursor, old_selection);
            self.refresh_search_matches();
            cx.notify();
            return;
        }

        // Single-line: remove up to 2 spaces before cursor
        let line_start = self.find_line_start();
        let spaces_before = self.content[line_start..self.cursor_position]
            .chars()
            .rev()
            .take_while(|c| *c == ' ')
            .count()
            .min(2);
        if spaces_before > 0 {
            self.cursor_position -= spaces_before;
            self.content
                .drain(self.cursor_position..self.cursor_position + spaces_before);
            self.is_dirty = true;
            self.push_edit(old_content, old_cursor, old_selection);
            self.refresh_search_matches();
            cx.notify();
        }
    }

    /// Moves cursor left by one character.
    /// Clears any active selection (standard non-shift arrow key behavior).
    fn handle_move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        self.clear_selection();
        if self.cursor_position > 0 {
            self.cursor_position = self.prev_char_boundary(self.cursor_position);
            cx.notify();
        }
    }

    /// Moves cursor right by one character.
    /// Clears any active selection (standard non-shift arrow key behavior).
    fn handle_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        self.clear_selection();
        if self.cursor_position < self.content.len() {
            self.cursor_position = self.next_char_boundary(self.cursor_position);
            cx.notify();
        }
    }

    /// Moves cursor up one line, maintaining horizontal column position when possible.
    /// Clears any active selection.
    /// If autocomplete is active, navigates suggestions instead.
    fn handle_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        // If autocomplete is active, navigate suggestions
        if let Some(ref mut autocomplete) = self.autocomplete {
            autocomplete.move_up();
            cx.notify();
            return;
        }

        self.clear_selection();
        self.move_up_internal();
        cx.notify();
    }

    /// Moves cursor down one line, maintaining horizontal column position when possible.
    /// Clears any active selection.
    /// If autocomplete is active, navigates suggestions instead.
    fn handle_move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        // If autocomplete is active, navigate suggestions
        if let Some(ref mut autocomplete) = self.autocomplete {
            autocomplete.move_down();
            cx.notify();
            return;
        }

        self.clear_selection();
        self.move_down_internal();
        cx.notify();
    }

    /// Moves cursor to the previous word boundary.
    /// Clears any active selection.
    fn handle_move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        self.clear_selection();
        self.cursor_position = self.find_prev_word_boundary();
        cx.notify();
    }

    /// Moves cursor to the next word boundary.
    /// Clears any active selection.
    fn handle_move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.autocomplete = None;
        self.clear_selection();
        self.cursor_position = self.find_next_word_boundary();
        cx.notify();
    }

    /// Moves cursor to the start of the current line.
    /// Clears any active selection.
    fn handle_move_home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        self.clear_selection();
        self.cursor_position = self.find_line_start();
        cx.notify();
    }

    /// Moves cursor to the end of the current line.
    /// Clears any active selection.
    fn handle_move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        self.clear_selection();
        self.cursor_position = self.find_line_end();
        cx.notify();
    }

    /// Handles Ctrl+S (Save) action.
    ///
    /// - If a `current_file` is set, writes directly to that path.
    /// - Otherwise (draft buffer), opens a native Save-As dialog. Once the
    ///   user picks a destination, the file is written and the editor adopts
    ///   that path (and its parent as `working_dir` for fuzzy find).
    fn handle_save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.current_file.clone() {
            if let Err(e) = std::fs::write(&path, &self.content) {
                eprintln!("Failed to save file: {}", e);
            } else {
                self.is_dirty = false;
                println!("File saved to: {}", path);
                cx.notify();
            }
            return;
        }

        let directory = self
            .working_dir
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let suggested = match self.language {
            Language::Brief => "untitled.brf",
            Language::Markdown => "untitled.md",
        };
        let rx = cx.prompt_for_new_path(&directory, Some(suggested));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let _ = this.update(cx, |editor, cx| {
                if let Err(e) = std::fs::write(&path, &editor.content) {
                    eprintln!("Failed to save file: {}", e);
                    return;
                }
                let path_str = path.to_string_lossy().to_string();
                editor.language = Language::from_path(&path_str);
                editor.current_file = Some(path_str.clone());
                editor.is_dirty = false;
                editor.diag_dirty = true;
                if editor.working_dir.is_none() {
                    editor.working_dir = path.parent().map(|p| p.to_path_buf());
                }
                println!("File saved to: {}", path_str);
                cx.notify();
            });
        })
        .detach();
    }

    /// Handles Ctrl+Q (Quit) action.
    ///
    /// Quits immediately for a clean buffer; otherwise shows a confirmation
    /// dialog so unsaved work isn't discarded silently.
    fn handle_quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_dirty {
            cx.quit();
            return;
        }

        let answer = window.prompt(
            PromptLevel::Warning,
            "You have unsaved changes.",
            Some("Quit without saving? Your changes will be lost."),
            &["Quit without saving", "Cancel"],
            cx,
        );
        cx.spawn(async move |_this, cx| {
            // Answer index 0 is "Quit without saving".
            if let Ok(0) = answer.await {
                let _ = cx.update(|cx| cx.quit());
            }
        })
        .detach();
    }

    /// Handles Ctrl+C (Copy) action.
    /// Copies selected text to system clipboard. Does nothing if no selection.
    fn handle_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.get_selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Handles Ctrl+V (Paste) action.
    ///
    /// Behavior:
    /// - If selection exists: replace selected text with clipboard content
    /// - Otherwise: insert clipboard content at cursor
    /// - Advances cursor to end of pasted text
    fn handle_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(clipboard_item) = cx.read_from_clipboard() {
            if let Some(text) = clipboard_item.text().map(|s| s.to_string()) {
                let old_content = self.content.clone();
                let old_cursor = self.cursor_position;
                let old_selection = self.selection_start;

                self.delete_selection();
                self.content.insert_str(self.cursor_position, &text);
                self.cursor_position += text.len();
                self.is_dirty = true;
                self.push_edit(old_content, old_cursor, old_selection);
                self.refresh_search_matches();
                cx.notify();
            }
        }
    }

    /// Handles Ctrl+X (Cut) action.
    /// Copies selected text to clipboard and deletes it. Does nothing if no selection.
    fn handle_cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.get_selected_text() {
            let old_content = self.content.clone();
            let old_cursor = self.cursor_position;
            let old_selection = self.selection_start;

            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.delete_selection();
            self.is_dirty = true;
            self.push_edit(old_content, old_cursor, old_selection);
            self.refresh_search_matches();
            cx.notify();
        }
    }

    /// Handles Shift+Left (Select Left) action.
    /// Extends or initiates selection while moving cursor left.
    fn handle_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_position);
        }
        if self.cursor_position > 0 {
            self.cursor_position = self.prev_char_boundary(self.cursor_position);
            cx.notify();
        }
    }

    /// Handles Shift+Right (Select Right) action.
    /// Extends or initiates selection while moving cursor right.
    fn handle_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_position);
        }
        if self.cursor_position < self.content.len() {
            self.cursor_position = self.next_char_boundary(self.cursor_position);
            cx.notify();
        }
    }

    /// Handles Shift+Up (Select Up) action.
    /// Extends or initiates selection while moving cursor up one line.
    fn handle_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_position);
        }
        self.move_up_internal();
        cx.notify();
    }

    /// Handles Shift+Down (Select Down) action.
    /// Extends or initiates selection while moving cursor down one line.
    fn handle_select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_position);
        }
        self.move_down_internal();
        cx.notify();
    }

    /// Handles Ctrl+A (Select All) action.
    /// Selects entire document content.
    fn handle_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selection_start = Some(0);
        self.cursor_position = self.content.len();
        cx.notify();
    }

    fn handle_toggle_find(&mut self, _: &ToggleFind, _: &mut Window, cx: &mut Context<Self>) {
        if self.find_panel.is_some() {
            self.close_find_panel();
        } else {
            self.open_find_panel();
            self.focus_current_search_match();
        }
        cx.notify();
    }

    fn handle_find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        if self.find_panel.is_none() {
            self.open_find_panel();
            if self.focus_current_search_match() {
                cx.notify();
            }
            return;
        }

        if let Some(range) = self.advance_search(1) {
            self.focus_match(range);
            cx.notify();
        }
    }

    fn handle_find_previous(&mut self, _: &FindPrevious, _: &mut Window, cx: &mut Context<Self>) {
        if self.find_panel.is_none() {
            self.open_find_panel();
            if self.focus_current_search_match() {
                cx.notify();
            }
            return;
        }

        if let Some(range) = self.advance_search(-1) {
            self.focus_match(range);
            cx.notify();
        }
    }

    /// Handles Ctrl+P / Ctrl+O (Toggle Palette) action.
    ///
    /// - If a `working_dir` is set, opens the fuzzy-find palette over that
    ///   folder (existing behavior).
    /// - In draft mode (no `working_dir`), opens the native OS file picker
    ///   instead, so the editor never scans the filesystem on its own.
    fn handle_toggle_palette(
        &mut self,
        _: &TogglePalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            self.palette = None;
            window.focus(&self.focus_handle);
            cx.notify();
            return;
        }

        self.close_find_panel();

        if let Some(working_dir) = self.working_dir.clone() {
            let palette_theme = self.config.theme().palette.clone();
            let palette_entity =
                cx.new(move |cx| Palette::new(working_dir.clone(), palette_theme.clone(), cx));
            window.focus(&palette_entity.read(cx).focus_handle(cx));
            self.palette = Some(palette_entity);
            cx.notify();
            return;
        }

        // Draft mode: bring up the native file dialog and load whatever the
        // user picks. If the current draft is dirty, the file opens in a new
        // window so unsaved work isn't clobbered.
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let config = self.config.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |editor, cx| {
                if editor.is_dirty {
                    let path_str = path.to_string_lossy().to_string();
                    let parent = path.parent().map(|p| p.to_path_buf());
                    open_editor_window(Some(path_str), parent, config.clone(), cx);
                } else {
                    editor.load_file(path, cx);
                }
            });
        })
        .detach();
    }

    /// Handles Ctrl+Shift+O (Open Folder).
    ///
    /// Always opens a native folder picker; the selected folder is opened in
    /// a brand-new window with a fresh draft buffer, leaving the current
    /// window untouched.
    fn handle_open_folder(&mut self, _: &OpenFolder, _: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        let config = self.config.clone();
        cx.spawn(async move |_this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
                return;
            };
            let _ = cx.update(|cx| {
                open_editor_window(None, Some(folder), config.clone(), cx);
            });
        })
        .detach();
    }

    /// Handles Ctrl+Z (Undo) action.
    /// Reverts the last edit by splicing its removed text back in.
    fn handle_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(operation) = self.undo_stack.pop() {
            let end = operation.start + operation.inserted.len();
            self.content
                .replace_range(operation.start..end, &operation.removed);
            self.cursor_position = operation.old_cursor.min(self.content.len());
            self.selection_start = operation.old_selection;
            self.redo_stack.push(operation);
            self.is_dirty = true;
            self.refresh_search_matches();
            cx.notify();
        }
    }

    /// Handles Ctrl+Shift+Z or Ctrl+Y (Redo) action.
    /// Re-applies an undone edit by splicing its inserted text back in.
    fn handle_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(operation) = self.redo_stack.pop() {
            let end = operation.start + operation.removed.len();
            self.content
                .replace_range(operation.start..end, &operation.inserted);
            self.cursor_position = operation.new_cursor.min(self.content.len());
            self.selection_start = operation.new_selection;
            self.undo_stack.push(operation);
            self.is_dirty = true;
            self.refresh_search_matches();
            cx.notify();
        }
    }

    /// Toggles the diagnostics list panel (Brief buffers only).
    fn handle_toggle_diagnostics(
        &mut self,
        _: &ToggleDiagnostics,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_diagnostics = !self.show_diagnostics;
        cx.notify();
    }

    /// Jumps the cursor to the next diagnostic (F8), wrapping around.
    fn handle_next_diagnostic(
        &mut self,
        _: &NextDiagnostic,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.goto_next_diagnostic();
        cx.notify();
    }

    /// Compiles the current Brief buffer to a standalone HTML file and opens it
    /// in the system browser. Markdown buffers are skipped (no Brief compiler).
    fn handle_export_html(&mut self, _: &ExportHtml, _: &mut Window, cx: &mut Context<Self>) {
        if self.language != Language::Brief {
            eprintln!("Export: HTML preview is only available for Brief buffers");
            return;
        }

        let source_path = self
            .current_file
            .clone()
            .unwrap_or_else(|| "draft.brf".to_string());
        let title = Path::new(&source_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("MedleyText")
            .to_string();

        match compile::render_html(&source_path, &self.content, &title) {
            Ok(html) => {
                let out_path = html_output_path(&source_path);
                if let Err(e) = std::fs::write(&out_path, html) {
                    eprintln!("Export: failed to write {}: {}", out_path.display(), e);
                    return;
                }
                println!("Exported HTML to {}", out_path.display());
                open_in_browser(&out_path);
            }
            Err(analysis) => {
                eprintln!(
                    "Export: document has {} error(s) and {} warning(s); fix the errors before exporting",
                    analysis.error_count, analysis.warning_count
                );
                // Surface the errors in-editor too.
                self.diagnostics = analysis.diagnostics;
                self.show_diagnostics = true;
                cx.notify();
            }
        }
    }

    /// Toggles the go-to-line panel.
    fn handle_toggle_goto_line(
        &mut self,
        _: &ToggleGoToLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.goto_panel.is_some() {
            self.goto_panel = None;
        } else {
            self.goto_panel = Some(String::new());
        }
        cx.notify();
    }

    /// Commits a go-to-line jump.
    fn goto_line_commit(&mut self) {
        if let Some(ref input) = self.goto_panel {
            if let Ok(line_num) = input.parse::<usize>() {
                let target = line_num.saturating_sub(1);
                let lines: Vec<&str> = self.content.split('\n').collect();
                if target < lines.len() {
                    let mut byte_pos = 0;
                    for (idx, line) in lines.iter().enumerate() {
                        if idx == target {
                            self.cursor_position = byte_pos;
                            self.selection_start = None;
                            self.ensure_position_visible(byte_pos);
                            break;
                        }
                        byte_pos += line.len() + 1;
                    }
                }
            }
        }
        self.goto_panel = None;
    }

    /// Loads a file into the editor.
    ///
    /// This method reads the file content and updates the editor state.
    /// Called when a file is selected from the palette or the native picker.
    /// When the editor has no `working_dir` yet (draft mode), it adopts the
    /// file's parent directory so subsequent Ctrl+P uses fuzzy find.
    fn load_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.content = content;
                self.cursor_position = 0;
                self.selection_start = None;
                self.scroll_offset = 0.0;
                let path_str = path.to_string_lossy().to_string();
                self.language = Language::from_path(&path_str);
                if self.working_dir.is_none() {
                    self.working_dir = path.parent().map(|p| p.to_path_buf());
                }
                self.current_file = Some(path_str);
                self.is_dirty = false;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.diagnostics.clear();
                self.diag_dirty = true;
                println!("Loaded file: {}", path.display());
                cx.notify();
            }
            Err(e) => {
                eprintln!("Failed to load file: {}", e);
            }
        }
    }

    /// Handles mouse click events for cursor positioning.
    ///
    /// Converts a mouse click position to a document byte offset.
    ///
    /// Hit-testing uses the same visual-line word-wrap model as rendering and
    /// keyboard navigation, with gutter width and scroll offset accounted for.
    fn handle_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.clear_selection();

        let byte_position = self.byte_offset_at_pixel(event.position);

        self.cursor_position = byte_position;
        self.cursor_blink_visible = true;
        self.is_dragging = true;
        self.drag_start_position = byte_position;
        cx.notify();
    }

    /// Handles mouse move events for drag-to-select.
    fn handle_mouse_move(&mut self, event: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.is_dragging {
            return;
        }

        let byte_position = self.byte_offset_at_pixel(event.position);

        self.cursor_position = byte_position;
        self.cursor_blink_visible = true;
        self.selection_start = Some(self.drag_start_position);
        cx.notify();
    }

    /// Handles mouse up events to end drag selection.
    fn handle_mouse_up(&mut self, _event: &gpui::MouseUpEvent, _cx: &mut Context<Self>) {
        self.is_dragging = false;
    }

    /// Handles mouse scroll wheel events for vertical scrolling.
    ///
    /// Supports both pixel-based and line-based scroll deltas.
    /// Clamps scroll offset to valid range [0, max_content_height - viewport_height].
    ///
    /// # Layout Metrics
    ///
    /// Derived from the configured font metrics. At the default font size (14px):
    /// - `line_height` ≈ 20px
    /// - `viewport_height` ≈ 538px (window height minus padding/header)
    fn handle_scroll_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let line_height = self.line_height();

        let scroll_amount = match event.delta {
            gpui::ScrollDelta::Pixels(delta) => delta.y.into(),
            gpui::ScrollDelta::Lines(delta) => delta.y * line_height,
        };

        self.scroll_offset -= scroll_amount;

        let visual_lines = self.build_visual_lines();
        let total_content_height = visual_lines.len() as f32 * line_height;

        let viewport_height = self.viewport_height();
        let max_scroll = (total_content_height - viewport_height).max(0.0);

        self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll);

        cx.notify();
    }

    /// Internal helper for moving cursor up one line while preserving column position.
    ///
    /// Algorithm:
    /// 1. Find current line and column position
    /// 2. Move to previous line
    /// 3. Clamp column to line length (handles lines of different lengths)
    /// 4. Convert (line, column) back to byte offset
    ///
    /// This logic is shared by `handle_move_up` and `handle_select_up`.
    fn move_up_internal(&mut self) {
        self.move_up_internal_wrapped();
    }

    /// Internal helper for moving cursor down one line while preserving column position.
    ///
    /// Algorithm mirrors `move_up_internal` but moves to the next line instead.
    /// Handles edge cases like moving from long line to short line gracefully.
    fn move_down_internal(&mut self) {
        self.move_down_internal_wrapped();
    }

    /// Builds visual lines for word-wrapped rendering.
    fn build_visual_lines(&self) -> Vec<VisualLine> {
        let max_chars = self.chars_per_line();
        let lines: Vec<&str> = self.content.split('\n').collect();
        let mut result = Vec::new();
        let mut content_byte = 0usize;

        for (line_idx, line) in lines.iter().enumerate() {
            let mut char_count = 0usize;
            let mut start_byte = 0usize;
            let mut is_first = true;

            for (byte_idx, _ch) in line.char_indices() {
                if char_count >= max_chars && char_count > 0 {
                    result.push(VisualLine {
                        content_line: line_idx,
                        start_byte_in_content: content_byte + start_byte,
                        end_byte_in_content: content_byte + byte_idx,
                        is_first,
                    });
                    start_byte = byte_idx;
                    char_count = 0;
                    is_first = false;
                }
                char_count += 1;
            }

            result.push(VisualLine {
                content_line: line_idx,
                start_byte_in_content: content_byte + start_byte,
                end_byte_in_content: content_byte + line.len(),
                is_first,
            });

            content_byte += line.len() + 1;
        }

        result
    }

    /// Finds which visual line contains the given byte offset.
    fn byte_offset_to_visual_line(
        &self,
        byte_offset: usize,
        visual_lines: &[VisualLine],
    ) -> (usize, usize) {
        for (idx, vl) in visual_lines.iter().enumerate() {
            if byte_offset >= vl.start_byte_in_content && byte_offset <= vl.end_byte_in_content {
                let col = byte_offset - vl.start_byte_in_content;
                return (idx, col);
            }
        }
        (visual_lines.len().saturating_sub(1), 0)
    }

    /// Moves cursor up one visual line (word-wrap aware).
    fn move_up_internal_wrapped(&mut self) {
        let visual_lines = self.build_visual_lines();
        let (current_vl_idx, current_col) =
            self.byte_offset_to_visual_line(self.cursor_position, &visual_lines);
        if current_vl_idx == 0 {
            return;
        }
        let prev_vl = &visual_lines[current_vl_idx - 1];
        let vl_len = prev_vl.end_byte_in_content - prev_vl.start_byte_in_content;
        let new_col = current_col.min(vl_len);
        self.cursor_position = prev_vl.start_byte_in_content + new_col;
    }

    /// Moves cursor down one visual line (word-wrap aware).
    fn move_down_internal_wrapped(&mut self) {
        let visual_lines = self.build_visual_lines();
        let (current_vl_idx, current_col) =
            self.byte_offset_to_visual_line(self.cursor_position, &visual_lines);
        if current_vl_idx + 1 >= visual_lines.len() {
            return;
        }
        let next_vl = &visual_lines[current_vl_idx + 1];
        let vl_len = next_vl.end_byte_in_content - next_vl.start_byte_in_content;
        let new_col = current_col.min(vl_len);
        self.cursor_position = next_vl.start_byte_in_content + new_col;
    }
}

/// Represents a single visual line after word wrapping.
#[derive(Clone, Copy)]
struct VisualLine {
    content_line: usize,
    start_byte_in_content: usize,
    end_byte_in_content: usize,
    is_first: bool,
}

/// GPUI Focusable trait implementation for keyboard event routing.
impl Focusable for TextEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// GPUI Render trait implementation for UI rendering.
///
/// This is the core rendering logic that:
/// 1. Splits content into lines
/// 2. Tokenizes each line for markdown syntax
/// 3. Applies colors per token type
/// 4. Renders cursor and selection overlays
/// 5. Handles scrolling via transform offset
///
/// # Performance Considerations
///
/// - Tokenizes all visible lines on every render
/// - Consider caching tokenized lines if performance becomes an issue
/// - Selection rendering splits tokens that cross selection boundaries
///
/// # Rendering Architecture
///
/// - Uses GPUI's flexbox-based layout system
/// - Cursor is rendered as a 4px wide colored div
/// - Selection uses background color overlay
/// - Text is rendered in monospace font for consistent character width
impl Render for TextEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Cache window dimensions for word-wrap and viewport sizing.
        let window_bounds = window.bounds();
        self.window_width = window_bounds.size.width.into();
        self.window_height = window_bounds.size.height.into();

        // Recompute Brief diagnostics (debounced) when the buffer changed.
        if self.diag_dirty {
            self.schedule_diagnostics(cx);
        }

        // Check if palette wants to open a file or close
        if let Some(palette_entity) = &self.palette {
            let palette = palette_entity.read(cx);
            if palette.should_open {
                let selected_file = palette.get_selected_file();
                let _ = palette;
                if let Some(file_to_load) = selected_file {
                    self.palette = None;
                    window.focus(&self.focus_handle);
                    self.load_file(file_to_load, cx);
                }
            } else if palette.should_close {
                let _ = palette;
                self.palette = None;
                window.focus(&self.focus_handle);
                cx.notify();
            }
        }

        let theme = self.config.theme().clone();
        let font_family = "monospace";

        if self.cursor_position != self.cursor_blink_reset_position {
            self.cursor_blink_visible = true;
            self.cursor_blink_reset_position = self.cursor_position;
        }

        let show_cursor = self.should_show_cursor();
        let entity = cx.entity().downgrade();

        let editor_content = div()
            .track_focus(&self.focus_handle(cx))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                    editor.handle_mouse_down(event, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|editor, event: &ScrollWheelEvent, _, cx| {
                editor.handle_scroll_wheel(event, cx);
            }))
            .on_action(cx.listener(Self::handle_move_left))
            .on_action(cx.listener(Self::handle_move_right))
            .on_action(cx.listener(Self::handle_move_up))
            .on_action(cx.listener(Self::handle_move_down))
            .on_action(cx.listener(Self::handle_move_word_left))
            .on_action(cx.listener(Self::handle_move_word_right))
            .on_action(cx.listener(Self::handle_move_home))
            .on_action(cx.listener(Self::handle_move_end))
            .on_action(cx.listener(Self::handle_backspace))
            .on_action(cx.listener(Self::handle_delete))
            .on_action(cx.listener(Self::handle_enter))
            .on_action(cx.listener(Self::handle_tab))
            .on_action(cx.listener(Self::handle_shift_tab))
            .on_action(cx.listener(Self::handle_save))
            .on_action(cx.listener(Self::handle_quit))
            .on_action(cx.listener(Self::handle_copy))
            .on_action(cx.listener(Self::handle_paste))
            .on_action(cx.listener(Self::handle_cut))
            .on_action(cx.listener(Self::handle_select_left))
            .on_action(cx.listener(Self::handle_select_right))
            .on_action(cx.listener(Self::handle_select_up))
            .on_action(cx.listener(Self::handle_select_down))
            .on_action(cx.listener(Self::handle_select_all))
            .on_action(cx.listener(Self::handle_toggle_find))
            .on_action(cx.listener(Self::handle_find_next))
            .on_action(cx.listener(Self::handle_find_previous))
            .on_action(cx.listener(Self::handle_toggle_goto_line))
            .on_action(cx.listener(Self::handle_toggle_palette))
            .on_action(cx.listener(Self::handle_open_folder))
            .on_action(cx.listener(Self::handle_undo))
            .on_action(cx.listener(Self::handle_redo))
            .on_action(cx.listener(Self::handle_export_html))
            .on_action(cx.listener(Self::handle_toggle_diagnostics))
            .on_action(cx.listener(Self::handle_next_diagnostic))
            .on_mouse_move(cx.listener(|editor, event: &gpui::MouseMoveEvent, _, cx| {
                editor.handle_mouse_move(event, cx);
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|editor, event: &gpui::MouseUpEvent, _, _cx| {
                    editor.handle_mouse_up(event, _cx);
                }),
            )
            .on_key_down(cx.listener(|editor, event: &KeyDownEvent, _, cx| {
                if editor.handle_find_key_event(event, cx) {
                    return;
                }

                // Handle go-to-line panel input
                if let Some(ref mut input) = editor.goto_panel {
                    match event.keystroke.key.as_str() {
                        "escape" => {
                            editor.goto_panel = None;
                            cx.notify();
                            return;
                        }
                        "enter" => {
                            editor.goto_line_commit();
                            cx.notify();
                            return;
                        }
                        "backspace" => {
                            input.pop();
                            cx.notify();
                            return;
                        }
                        _ => {}
                    }
                    if let Some(ref key_char) = event.keystroke.key_char {
                        if key_char.len() == 1
                            && !event.keystroke.modifiers.control
                            && !event.keystroke.modifiers.alt
                            && !event.keystroke.modifiers.platform
                        {
                            if let Some(c) = key_char.chars().next() {
                                if c.is_ascii_digit() {
                                    input.push(c);
                                    cx.notify();
                                    return;
                                }
                            }
                        }
                    }
                    return;
                }

                // Handle Escape to close autocomplete
                if event.keystroke.key == "escape" && editor.autocomplete.is_some() {
                    editor.autocomplete = None;
                    cx.notify();
                    return;
                }

                // Regular character input (only when palette is closed).
                // Accept any printable text, including non-ASCII (accents, CJK,
                // emoji); only control chars and modifier combos are rejected.
                // IME commits may deliver several characters at once.
                if editor.palette.is_none() && editor.find_panel.is_none() {
                    if let Some(key_char) = &event.keystroke.key_char {
                        if !event.keystroke.modifiers.control
                            && !event.keystroke.modifiers.alt
                            && !event.keystroke.modifiers.platform
                            && !key_char.is_empty()
                            && !key_char.chars().any(|c| c.is_control())
                        {
                            let text = key_char.clone();
                            for c in text.chars() {
                                editor.insert_char(c, cx);
                            }
                        }
                    }
                }
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.editor.background)
            .border_1()
            .border_color(theme.editor.border)
            .rounded_md()
            .shadow_lg()
            .text_color(theme.editor.text)
            .p_4()
            .font_family(font_family)
            .text_size(px(self.font_size()))
            .line_height(px(self.line_height()))
            .child(
                div()
                    .mb_2()
                    .text_color(theme.editor.muted_text)
                    .child(format!(
                        "MedleyText - {} | Ctrl+P: open file | Ctrl+Shift+O: open folder | Ctrl+S: save | Ctrl+Q: quit",
                        self.current_file
                            .as_ref()
                            .map(|p| p.as_str())
                            .unwrap_or("[Draft]")
                    )),
            )
            .child({
                let entity = entity.clone();
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_hidden()
                    .relative()
                    .child(
                        canvas(
                            {
                                let entity = entity.clone();
                                move |bounds, _window, cx| {
                                    let _ = entity.update(cx, |editor, _| {
                                        editor.scroll_viewport_bounds = Some(bounds);
                                    });
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(div().size_full().overflow_hidden().child(
                        div().flex().flex_col().mt(px(-self.scroll_offset)).child({
                            let selection_range = self.get_selection_range();
                            let visual_lines = self.build_visual_lines();
                            let line_modes = self.compute_line_modes();
                            let mut result = div().flex().flex_col();
                            let gutter_width = self.gutter_width();
                            let row_height = self.line_height();
                            let cursor_bar_height = self.cursor_height();

                            // Viewport culling: only build elements for visual
                            // lines inside (or just outside) the visible window,
                            // bracketed by spacers that preserve scroll geometry.
                            // This keeps a 100k-line file rendering as cheaply as
                            // a one-screen file.
                            let total_lines = visual_lines.len();
                            let overscan = 4usize;
                            let first_visible = ((self.scroll_offset / row_height).floor() as usize)
                                .saturating_sub(overscan);
                            let viewport_rows =
                                (self.viewport_height() / row_height).ceil() as usize;
                            let last_visible =
                                (first_visible + viewport_rows + overscan * 2 + 1).min(total_lines);

                            if first_visible > 0 {
                                result = result
                                    .child(div().h(px(first_visible as f32 * row_height)));
                            }

                            for vl in visual_lines[first_visible..last_visible].iter() {
                                let mut line_wrapper = div().flex().flex_row().h(px(row_height));

                                // Line number gutter (only on first visual line of content line).
                                // Lines carrying a diagnostic are tinted red (error) or amber (warning).
                                if vl.is_first {
                                    let gutter_color = match self.line_diagnostic(vl.content_line + 1)
                                    {
                                        Some(true) => Rgba::from(gpui::rgb(DIAG_ERROR_COLOR)),
                                        Some(false) => Rgba::from(gpui::rgb(DIAG_WARNING_COLOR)),
                                        None => theme.editor.muted_text,
                                    };
                                    line_wrapper = line_wrapper.child(
                                        div()
                                            .w(px(gutter_width))
                                            .flex()
                                            .justify_end()
                                            .pr_2()
                                            .text_color(gutter_color)
                                            .child(format!("{}", vl.content_line + 1)),
                                    );
                                } else {
                                    line_wrapper = line_wrapper.child(div().w(px(gutter_width)));
                                }

                                let mut line_div = div().flex().flex_row().h(px(row_height));

                                // Find parent line boundaries for tokenization
                                let parent_line_start = self.content[..vl.start_byte_in_content]
                                    .rfind('\n')
                                    .map(|p| p + 1)
                                    .unwrap_or(0);
                                let parent_line_end = self.content[vl.start_byte_in_content..]
                                    .find('\n')
                                    .map(|p| vl.start_byte_in_content + p)
                                    .unwrap_or(self.content.len());
                                let parent_text = &self.content[parent_line_start..parent_line_end];
                                let line_mode = line_modes
                                    .get(vl.content_line)
                                    .copied()
                                    .unwrap_or(LineMode::Normal);
                                let runs =
                                    self.runs_for_line(parent_text, line_mode, &theme.syntax);

                                let mut token_byte = parent_line_start;
                                for (text, token_color) in runs {
                                    let token_start = token_byte;
                                    let token_end = token_byte + text.len();

                                    if token_end <= vl.start_byte_in_content
                                        || token_start >= vl.end_byte_in_content
                                    {
                                        token_byte += text.len();
                                        continue;
                                    }

                                    let overlap_start = token_start.max(vl.start_byte_in_content);
                                    let overlap_end = token_end.min(vl.end_byte_in_content);
                                    let overlap_text = &self.content[overlap_start..overlap_end];
                                    let cursor_pos = if self.cursor_position >= overlap_start
                                        && self.cursor_position <= overlap_end
                                    {
                                        Some(self.cursor_position)
                                    } else {
                                        None
                                    };

                                    let segments = self.build_segments_for_token(
                                        overlap_text,
                                        token_color,
                                        overlap_start,
                                        selection_range,
                                        cursor_pos,
                                        self.find_panel.as_ref(),
                                        &theme,
                                    );

                                    for segment in segments {
                                        match segment {
                                            SegmentPiece::Cursor => {
                                                if show_cursor {
                                                    line_div = line_div.child(
                                                        div()
                                                            .w(px(4.0))
                                                            .h(px(cursor_bar_height))
                                                            .bg(theme.editor.cursor),
                                                    );
                                                }
                                            }
                                            SegmentPiece::Text(run) => {
                                                if run.text.is_empty() {
                                                    continue;
                                                }
                                                let mut node = div().text_color(run.text_color);
                                                if let Some(bg) = run.background {
                                                    node = node.bg(bg);
                                                }
                                                line_div = line_div.child(node.child(run.text));
                                            }
                                        }
                                    }

                                    token_byte += text.len();
                                }

                                // Cursor at end of visual line
                                if show_cursor
                                    && self.cursor_position == vl.end_byte_in_content
                                    && self.cursor_position <= self.content.len()
                                {
                                    line_div = line_div.child(
                                        div()
                                            .w(px(4.0))
                                            .h(px(cursor_bar_height))
                                            .bg(theme.editor.cursor),
                                    );
                                }

                                line_wrapper = line_wrapper.child(line_div);
                                result = result.child(line_wrapper);
                            }

                            if last_visible < total_lines {
                                result = result.child(
                                    div().h(px((total_lines - last_visible) as f32 * row_height)),
                                );
                            }

                            result
                        }),
                    ))
            })
            .child(
                div()
                    .mt_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(theme.editor.border)
                    .flex()
                    .flex_row()
                    .justify_between()
                    .text_xs()
                    .text_color(theme.editor.muted_text)
                    .child(div().child(format!("Line {}", self.get_current_line_number())))
                    .child({
                        // Center: Brief diagnostics summary (Ctrl+Shift+D for the list).
                        let (errors, warnings) = self.diagnostic_counts();
                        if self.language != Language::Brief {
                            div().child("Markdown")
                        } else if errors == 0 && warnings == 0 {
                            div().text_color(theme.editor.muted_text).child("✓ no issues")
                        } else {
                            let color = if errors > 0 {
                                Rgba::from(gpui::rgb(DIAG_ERROR_COLOR))
                            } else {
                                Rgba::from(gpui::rgb(DIAG_WARNING_COLOR))
                            };
                            div().text_color(color).child(format!(
                                "✗ {} error{} · ⚠ {} warning{} (Ctrl+Shift+D)",
                                errors,
                                if errors == 1 { "" } else { "s" },
                                warnings,
                                if warnings == 1 { "" } else { "s" },
                            ))
                        }
                    })
                    .child(div().child(if self.is_dirty {
                        "● unsaved"
                    } else {
                        "✓ saved"
                    })),
            );

        // Wrap in a container and add overlays (autocomplete and/or palette)
        let mut container = div().size_full().child(editor_content);

        if let Some(find_panel) = &self.find_panel {
            let build_row = |label: &str, value: &str, placeholder: &str, active: bool| {
                let display = if value.is_empty() {
                    placeholder.to_string()
                } else {
                    value.to_string()
                };
                let text_color = if value.is_empty() {
                    theme.panel.placeholder_text
                } else {
                    theme.panel.value_text
                };

                div()
                    .px_3()
                    .py_2()
                    .bg(if active {
                        theme.panel.active_row_background
                    } else {
                        theme.panel.inactive_row_background
                    })
                    .rounded_sm()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.panel.label_text)
                            .child(label.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(self.font_size()))
                            .font_family(font_family)
                            .text_color(text_color)
                            .child(display),
                    )
            };

            let status_text = if !find_panel.has_query() {
                "Type to search".to_string()
            } else if !find_panel.has_matches() {
                "No matches".to_string()
            } else {
                let position = find_panel.current_index().unwrap_or(0) + 1;
                format!("{} / {} matches", position, find_panel.matches.len())
            };

            let find_overlay = div()
                    .absolute()
                    .top(px(self.padding()))
                    .right(px(self.padding()))
                .w(px(360.0))
                .bg(theme.panel.background)
                .border_1()
                .border_color(theme.panel.border)
                .rounded_md()
                .shadow_lg()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(build_row(
                    "Find",
                    &find_panel.query,
                    "Type to search...",
                    find_panel.active_input == ActiveInput::Query,
                ))
                .when(find_panel.show_replace, |view| {
                    view.child(build_row(
                        "Replace",
                        &find_panel.replace,
                        "Ctrl+H to show",
                        find_panel.active_input == ActiveInput::Replace,
                    ))
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.panel.status_text)
                        .child(status_text),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.panel.shortcut_text)
                        .child(
                            "Enter: next • Shift+Enter: prev • Ctrl+R: replace • Ctrl+Shift+R: replace all • Esc: close"
                                .to_string(),
                        ),
                );

            container = container.child(find_overlay);
        }

        // Add go-to-line overlay if active
        if let Some(ref goto_input) = self.goto_panel {
            let goto_overlay = div()
                .absolute()
                .top(px(self.padding()))
                .left(px(self.padding() + self.gutter_width()))
                .w(px(240.0))
                .bg(theme.panel.background)
                .border_1()
                .border_color(theme.panel.border)
                .rounded_md()
                .shadow_lg()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.panel.label_text)
                        .child("Go to line"),
                )
                .child(
                    div()
                        .text_size(px(self.font_size()))
                        .font_family(font_family)
                        .text_color(if goto_input.is_empty() {
                            theme.panel.placeholder_text
                        } else {
                            theme.panel.value_text
                        })
                        .child(if goto_input.is_empty() {
                            "Type line number...".to_string()
                        } else {
                            goto_input.clone()
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.panel.shortcut_text)
                        .child("Enter: jump • Esc: close"),
                );

            container = container.child(goto_overlay);
        }

        // Diagnostics list panel (toggle with Ctrl+Shift+D).
        if self.show_diagnostics && self.language == Language::Brief {
            let (errors, warnings) = self.diagnostic_counts();
            let header = if self.diagnostics.is_empty() {
                "No issues — document compiles cleanly".to_string()
            } else {
                format!("{} error(s), {} warning(s) · F8 to jump", errors, warnings)
            };

            let mut panel = div()
                .absolute()
                .bottom(px(self.padding() + self.line_height() + 16.0))
                .right(px(self.padding()))
                .w(px(440.0))
                .max_h(px(260.0))
                .bg(theme.panel.background)
                .border_1()
                .border_color(theme.panel.border)
                .rounded_md()
                .shadow_lg()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.panel.label_text)
                        .child(format!("Diagnostics — {}", header)),
                );

            for diag in self.diagnostics.iter().take(12) {
                let color = if diag.is_error() {
                    Rgba::from(gpui::rgb(DIAG_ERROR_COLOR))
                } else {
                    Rgba::from(gpui::rgb(DIAG_WARNING_COLOR))
                };
                panel = panel.child(
                    div()
                        .text_size(px(self.font_size() * 0.85))
                        .font_family(font_family)
                        .text_color(color)
                        .child(format!(
                            "{}:{}  [{}] {}",
                            diag.line, diag.col, diag.code, diag.message
                        )),
                );
            }

            container = container.child(panel);
        }

        // Add autocomplete overlay if active
        if let Some(autocomplete) = &self.autocomplete {
            let suggestions = autocomplete.get_suggestions_display();

            // Calculate cursor position for positioning the dropdown
            let line_height = self.line_height();
            let header_height = self.header_height();
            let padding = self.padding();
            let current_line = self.get_current_line_number() as f32 - 1.0;
            let top = padding + header_height + (current_line * line_height) + line_height
                - self.scroll_offset;

            let autocomplete_menu = div()
                .absolute()
                .top(px(top))
                .left(px(padding))
                .w(px(400.0))
                .bg(theme.autocomplete.background)
                .border_1()
                .border_color(theme.autocomplete.border)
                .rounded_md()
                .shadow_lg()
                .flex()
                .flex_col()
                .overflow_hidden()
                .children(suggestions.iter().map(|(is_selected, suggestion)| {
                    let item_bg = if *is_selected {
                        theme.autocomplete.item_selected_background
                    } else {
                        theme.autocomplete.item_background
                    };
                    let item_fg = if *is_selected {
                        theme.autocomplete.item_selected_text
                    } else {
                        theme.autocomplete.item_text
                    };

                    div()
                        .p_2()
                        .pl_3()
                        .bg(item_bg)
                        .flex()
                        .flex_row()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(self.font_size()))
                                .font_family(font_family)
                                .text_color(item_fg)
                                .child(suggestion.insert_text.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.autocomplete.label_text)
                                .child(suggestion.label.clone()),
                        )
                }));

            container = container.child(autocomplete_menu);
        }

        // Add palette overlay if open
        if let Some(palette_entity) = &self.palette {
            container = container.child(palette_entity.clone());
        }

        container
    }
}

/// Derives the HTML export path from a source path by swapping the extension
/// to `.html` (e.g. `notes.brf` → `notes.html`). Drafts without a real path
/// land next to the temp dir.
fn html_output_path(source_path: &str) -> PathBuf {
    let path = Path::new(source_path);
    if path.is_absolute()
        || path
            .parent()
            .map(|p| !p.as_os_str().is_empty())
            .unwrap_or(false)
    {
        path.with_extension("html")
    } else {
        std::env::temp_dir().join(path.with_extension("html").file_name().unwrap_or_default())
    }
}

/// Opens a path in the platform's default browser, best-effort.
fn open_in_browser(path: &Path) {
    let path = path.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    let cmd = ("open", vec![path]);
    #[cfg(target_os = "windows")]
    let cmd = (
        "cmd",
        vec!["/C".to_string(), "start".to_string(), String::new(), path],
    );
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = ("xdg-open", vec![path]);

    if let Err(e) = std::process::Command::new(cmd.0).args(cmd.1).spawn() {
        eprintln!(
            "Export: could not open browser ({}); file written to disk",
            e
        );
    }
}

/// Opens a fresh editor window. Used both at startup and when the user picks
/// "Open Folder" — opening a folder must spawn a new window so the current
/// draft is left untouched.
pub fn open_editor_window(
    file_path: Option<String>,
    working_dir: Option<PathBuf>,
    config: EditorConfig,
    cx: &mut App,
) {
    let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            is_movable: true,
            ..Default::default()
        },
        move |_window, cx| {
            cx.new(move |cx| TextEditor::with_file(file_path, working_dir, config, cx))
        },
    )
    .ok();
}

#[cfg(test)]
mod tests {
    use super::{EditOperation, compute_diff};

    /// Applies a diff record forward (redo direction) to a string.
    fn apply(content: &str, op: &EditOperation) -> String {
        let mut s = content.to_string();
        let end = op.start + op.removed.len();
        s.replace_range(op.start..end, &op.inserted);
        s
    }

    /// Reverts a diff record (undo direction) on a string.
    fn revert(content: &str, op: &EditOperation) -> String {
        let mut s = content.to_string();
        let end = op.start + op.inserted.len();
        s.replace_range(op.start..end, &op.removed);
        s
    }

    fn op_from(old: &str, new: &str) -> EditOperation {
        let (start, removed, inserted) = compute_diff(old, new);
        EditOperation {
            start,
            removed,
            inserted,
            old_cursor: 0,
            new_cursor: 0,
            old_selection: None,
            new_selection: None,
        }
    }

    #[test]
    fn diff_insertion_in_middle() {
        let (start, removed, inserted) = compute_diff("hello", "heLLO_llo");
        // common prefix "he", common suffix "llo".
        assert_eq!(start, 2);
        assert_eq!(removed, "");
        assert_eq!(inserted, "LLO_");
    }

    #[test]
    fn diff_deletion() {
        let (start, removed, inserted) = compute_diff("abcdef", "abef");
        assert_eq!(start, 2);
        assert_eq!(removed, "cd");
        assert_eq!(inserted, "");
    }

    #[test]
    fn diff_round_trips_through_undo_redo() {
        let cases = [
            ("", "a"),
            ("hello", "hello world"),
            ("hello world", "hello"),
            ("abc", "axc"),
            ("the quick fox", "the slow fox"),
            ("café", "cafe"), // multibyte removed
            ("cafe", "café"), // multibyte inserted
            ("naïve café", "naive cafe"),
            ("日本語", "日X語"), // multibyte both sides
            ("a😀b", "a🚀b"),    // emoji swap (4-byte)
        ];
        for (old, new) in cases {
            let op = op_from(old, new);
            assert_eq!(apply(old, &op), new, "redo failed for {old:?}->{new:?}");
            assert_eq!(revert(new, &op), old, "undo failed for {old:?}->{new:?}");
        }
    }

    #[test]
    fn diff_respects_char_boundaries() {
        // é (0xC3 0xA9) vs è (0xC3 0xA8) share a leading byte; the diff must
        // not split inside the codepoint.
        let (start, removed, inserted) = compute_diff("é", "è");
        assert!("é".is_char_boundary(start));
        assert_eq!(removed, "é");
        assert_eq!(inserted, "è");
    }

    #[test]
    fn diff_identical_is_empty() {
        let (_, removed, inserted) = compute_diff("same", "same");
        assert!(removed.is_empty() && inserted.is_empty());
    }
}
