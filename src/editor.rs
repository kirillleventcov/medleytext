//! Text editor component with Brief- and Markdown-aware syntax highlighting.
//!
//! This module provides the core `TextEditor` struct and its associated
//! functionality including cursor management, text selection, clipboard operations,
//! scrolling, and rendering. The highlighter is chosen per file from the
//! extension (`.md` → Markdown, everything else → Brief).

use gpui::{
    App, ClipboardItem, Context, FocusHandle, Focusable, KeyDownEvent, MouseDownEvent, Render,
    Rgba, ScrollWheelEvent, Window, actions, div, prelude::*, px,
};

use std::path::Path;

use crate::autocomplete::Autocomplete;
use crate::brief::BriefHighlighter;
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
        Undo,
        Redo,
    ]
);

/// Represents a single edit operation for undo/redo.
#[derive(Clone, Debug)]
struct EditOperation {
    /// Content before the edit
    old_content: String,
    /// Content after the edit
    new_content: String,
    /// Cursor position before the edit
    old_cursor: usize,
    /// Cursor position after the edit
    new_cursor: usize,
    /// Selection start before the edit
    old_selection: Option<usize>,
    /// Selection start after the edit
    new_selection: Option<usize>,
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

    /// Working directory for file operations and palette scanning.
    working_dir: std::path::PathBuf,

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

    /// Markup language driving highlighter, autocomplete, and smart-list
    /// continuation. Re-derived from the file extension on open/load.
    language: Language,
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
    /// Creates a new TextEditor instance, optionally loading content from a file.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Optional path to file to load. If `None`, starts with welcome message.
    /// * `cx` - GPUI context for initialization.
    ///
    /// # Behavior
    ///
    /// - If file exists: loads content and stores path
    /// - If file doesn't exist: creates empty file on disk and stores path
    /// - If no path provided: shows welcome message with no associated file
    ///
    /// # Error Handling
    ///
    /// File read errors are logged to stderr but don't prevent editor initialization.
    /// This allows creating new files or recovering from read permission issues.
    pub fn with_file(
        file_path: Option<String>,
        config: EditorConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let (content, current_file) = if let Some(path) = file_path {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    println!("Loaded file: {}", path);
                    (content, Some(path))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if let Err(create_err) = std::fs::write(&path, "") {
                        eprintln!("Failed to create file: {}", create_err);
                        (String::new(), Some(path))
                    } else {
                        println!("Created new file: {}", path);
                        (String::new(), Some(path))
                    }
                }
                Err(e) => {
                    eprintln!("Failed to open file: {}", e);
                    (String::new(), Some(path))
                }
            }
        } else {
            (
                String::from("Welcome to MedleyText!\n\nBrief-first editor. Start typing..."),
                None,
            )
        };

        let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

        let language = current_file
            .as_deref()
            .map(Language::from_path)
            .unwrap_or(Language::Brief);

        Self {
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
            language,
        }
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
        538.0 - 20.0 + self.line_height()
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

    /// Records an edit operation to the undo stack.
    ///
    /// This captures the state before and after an edit, allowing it to be undone later.
    /// When a new edit is recorded, the redo stack is cleared.
    fn push_edit(&mut self, old_content: String, old_cursor: usize, old_selection: Option<usize>) {
        let operation = EditOperation {
            old_content,
            new_content: self.content.clone(),
            old_cursor,
            new_cursor: self.cursor_position,
            old_selection,
            new_selection: self.selection_start,
        };
        self.undo_stack.push(operation);
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
    fn refresh_search_matches(&mut self) {
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
        self.cursor_position += 1;
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
                self.cursor_position -= 1;
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
            self.cursor_position -= 1;
            cx.notify();
        }
    }

    /// Moves cursor right by one character.
    /// Clears any active selection (standard non-shift arrow key behavior).
    fn handle_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        self.clear_selection();
        if self.cursor_position < self.content.len() {
            self.cursor_position += 1;
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
    /// Behavior:
    /// - If `current_file` is set: writes content to that path
    /// - Otherwise: prompts for file path via stdin (blocking)
    ///
    /// # Limitations
    ///
    /// - Stdin prompt is blocking and non-ideal for GUI application
    /// - Consider implementing modal dialog for file path input
    /// - No dirty flag tracking or save confirmation yet
    fn handle_save(&mut self, _: &Save, _: &mut Window, _cx: &mut Context<Self>) {
        use std::io::{self, Write};

        let path = if let Some(ref current) = self.current_file {
            current.clone()
        } else {
            print!("Enter file path to save: ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                input.trim().to_string()
            } else {
                eprintln!("Failed to read input");
                return;
            }
        };

        if path.is_empty() {
            eprintln!("No file path provided");
            return;
        }

        if let Err(e) = std::fs::write(&path, &self.content) {
            eprintln!("Failed to save file: {}", e);
        } else {
            self.current_file = Some(path.clone());
            self.is_dirty = false;
            println!("File saved to: {}", path);
        }
    }

    /// Handles Ctrl+Q (Quit) action by terminating the application.
    fn handle_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
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
            self.cursor_position -= 1;
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
            self.cursor_position += 1;
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

    /// Handles Ctrl+P (Toggle Palette) action.
    /// Opens or closes the command palette for fuzzy file finding.
    fn handle_toggle_palette(
        &mut self,
        _: &TogglePalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            // Close palette and restore focus to editor
            self.palette = None;
            window.focus(&self.focus_handle);
        } else {
            self.close_find_panel();
            // Open palette and transfer focus to it
            let working_dir = self.working_dir.clone();
            let palette_theme = self.config.theme().palette.clone();
            let palette_entity =
                cx.new(move |cx| Palette::new(working_dir.clone(), palette_theme.clone(), cx));
            window.focus(&palette_entity.read(cx).focus_handle(cx));
            self.palette = Some(palette_entity);
        }
        cx.notify();
    }

    /// Handles Ctrl+Z (Undo) action.
    /// Reverts the last edit operation and moves it to the redo stack.
    fn handle_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(operation) = self.undo_stack.pop() {
            self.content = operation.old_content.clone();
            self.cursor_position = operation.old_cursor;
            self.selection_start = operation.old_selection;
            self.redo_stack.push(operation);
            self.is_dirty = true;
            self.refresh_search_matches();
            cx.notify();
        }
    }

    /// Handles Ctrl+Shift+Z or Ctrl+Y (Redo) action.
    /// Reapplies an undone edit operation.
    fn handle_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(operation) = self.redo_stack.pop() {
            self.content = operation.new_content.clone();
            self.cursor_position = operation.new_cursor;
            self.selection_start = operation.new_selection;
            self.undo_stack.push(operation);
            self.is_dirty = true;
            self.refresh_search_matches();
            cx.notify();
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
    /// Called when a file is selected from the palette.
    fn load_file(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.content = content;
                self.cursor_position = 0;
                self.selection_start = None;
                self.scroll_offset = 0.0;
                let path_str = path.to_string_lossy().to_string();
                self.language = Language::from_path(&path_str);
                self.current_file = Some(path_str);
                self.is_dirty = false;
                self.undo_stack.clear();
                self.redo_stack.clear();
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
    /// Converts pixel coordinates to document position by:
    /// 1. Calculating clicked line from Y coordinate
    /// 2. Calculating column from X coordinate
    /// 3. Converting (line, column) to byte offset
    ///
    /// # Layout Metrics
    ///
    /// Values derive from the configured font size. Defaults (font-size 14px) yield:
    /// - `char_width` ≈ 8px (monospace width)
    /// - `line_height` ≈ 20px
    /// - `header_height` ≈ 28px (line height + header spacing)
    /// - `padding` = 16px (p_4)
    fn handle_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.clear_selection();

        let char_width = px(self.char_width());
        let line_height = px(self.line_height());
        let header_height = px(self.header_height());
        let padding = px(self.padding());

        let click_x = event.position.x - padding;
        let click_y = event.position.y - padding - header_height + px(self.scroll_offset);

        let clicked_line = ((click_y / line_height).max(0.0).floor() as usize).max(0);

        let clicked_col = ((click_x / char_width).max(0.0).round() as usize).max(0);

        let lines: Vec<&str> = self.content.split('\n').collect();

        let target_line = clicked_line.min(lines.len().saturating_sub(1));

        let mut byte_position = 0;
        for (idx, line) in lines.iter().enumerate() {
            if idx == target_line {
                let target_col = clicked_col.min(line.len());
                byte_position += target_col;
                break;
            }
            byte_position += line.len() + 1;
        }

        self.cursor_position = byte_position;
        self.is_dragging = true;
        self.drag_start_position = byte_position;
        cx.notify();
    }

    /// Handles mouse move events for drag-to-select.
    fn handle_mouse_move(&mut self, event: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.is_dragging {
            return;
        }

        let char_width = px(self.char_width());
        let line_height = px(self.line_height());
        let header_height = px(self.header_height());
        let padding = px(self.padding());

        let move_x = event.position.x - padding;
        let move_y = event.position.y - padding - header_height + px(self.scroll_offset);

        let moved_line = ((move_y / line_height).max(0.0).floor() as usize).max(0);
        let moved_col = ((move_x / char_width).max(0.0).round() as usize).max(0);

        let lines: Vec<&str> = self.content.split('\n').collect();
        let target_line = moved_line.min(lines.len().saturating_sub(1));

        let mut byte_position = 0;
        for (idx, line) in lines.iter().enumerate() {
            if idx == target_line {
                let target_col = moved_col.min(line.len());
                byte_position += target_col;
                break;
            }
            byte_position += line.len() + 1;
        }

        self.cursor_position = byte_position;
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
        // Cache window width for word-wrap calculations
        let window_bounds = window.bounds();
        self.window_width = window_bounds.size.width.into();

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
            .on_action(cx.listener(Self::handle_undo))
            .on_action(cx.listener(Self::handle_redo))
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

                // Regular character input (only when palette is closed)
                if editor.palette.is_none() && editor.find_panel.is_none() {
                    if let Some(key_char) = &event.keystroke.key_char {
                        if key_char.len() == 1
                            && !event.keystroke.modifiers.control
                            && !event.keystroke.modifiers.alt
                            && !event.keystroke.modifiers.platform
                        {
                            if let Some(c) = key_char.chars().next() {
                                if c.is_ascii_graphic() || c == ' ' {
                                    editor.insert_char(c, cx);
                                }
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
            .child(
                div()
                    .mb_2()
                    .text_color(theme.editor.muted_text)
                    .child(format!(
                        "MedleyText - {} | Ctrl+P: files | Ctrl+S: save | Ctrl+Q: quit",
                        self.current_file
                            .as_ref()
                            .map(|p| p.as_str())
                            .unwrap_or("[unsaved]")
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_1()
                    .overflow_hidden()
                    .child(div().flex().flex_col().mt(px(-self.scroll_offset)).child({
                        let selection_range = self.get_selection_range();
                        let visual_lines = self.build_visual_lines();
                        let mut result = div().flex().flex_col();
                        let gutter_width = self.gutter_width();
                        let content_line_height = self.cursor_height();

                        for vl in visual_lines.iter() {
                            let mut line_wrapper =
                                div().flex().flex_row().min_h(px(content_line_height));

                            // Line number gutter (only on first visual line of content line)
                            if vl.is_first {
                                line_wrapper = line_wrapper.child(
                                    div()
                                        .w(px(gutter_width))
                                        .flex()
                                        .justify_end()
                                        .pr_2()
                                        .text_color(theme.editor.muted_text)
                                        .child(format!("{}", vl.content_line + 1)),
                                );
                            } else {
                                line_wrapper = line_wrapper.child(div().w(px(gutter_width)));
                            }

                            let mut line_div =
                                div().flex().flex_row().min_h(px(content_line_height));

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
                            let runs = self.line_runs(parent_text, &theme.syntax);

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
                                            line_div = line_div.child(
                                                div()
                                                    .w(px(4.0))
                                                    .h(px(content_line_height))
                                                    .bg(theme.editor.cursor),
                                            );
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
                            if self.cursor_position == vl.end_byte_in_content
                                && self.cursor_position <= self.content.len()
                            {
                                line_div = line_div.child(
                                    div()
                                        .w(px(4.0))
                                        .h(px(content_line_height))
                                        .bg(theme.editor.cursor),
                                );
                            }

                            line_wrapper = line_wrapper.child(line_div);
                            result = result.child(line_wrapper);
                        }

                        result
                    })),
            )
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
