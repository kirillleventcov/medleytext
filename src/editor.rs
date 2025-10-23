//! Text editor component with markdown syntax highlighting.
//!
//! This module provides the core `TextEditor` struct and its associated
//! functionality including cursor management, text selection, clipboard operations,
//! scrolling, and rendering with real-time markdown syntax highlighting.

use gpui::{
    App, ClipboardItem, Context, FocusHandle, Focusable, KeyDownEvent, MouseDownEvent, Render,
    ScrollWheelEvent, Window, actions, div, prelude::*, px, rgb,
};

use crate::autocomplete::Autocomplete;
use crate::markdown::MarkdownHighlighter;
use crate::palette::Palette;

/// Define GPUI actions for keyboard shortcuts and user commands.
/// These actions are bound to keys in main.rs and handled by the TextEditor.
actions!(
    editor,
    [
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        Backspace,
        Enter,
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
        TogglePalette,
    ]
);

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
///
/// # Future Improvements
///
/// - Replace `String` with rope data structure for better performance on large files
/// - Add undo/redo stack
/// - Implement multi-cursor support
/// - Add line numbers in gutter
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
    pub fn with_file(file_path: Option<String>, cx: &mut Context<Self>) -> Self {
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
                String::from("Welcome to MedleyText!\n\nStart typing..."),
                None,
            )
        };

        let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

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
        }
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
        self.delete_selection();
        self.content.insert(self.cursor_position, c);
        self.cursor_position += 1;
        self.is_dirty = true;

        // Check if this character should trigger autocomplete
        let trigger = c.to_string();
        let triggers = ["#", "-", "`", ">", "[", "*"];

        if triggers.contains(&trigger.as_str()) {
            let line_content = self.get_current_line_content();
            self.autocomplete = Autocomplete::new(&trigger, &line_content);
        } else if c == ' ' || c == '\n' {
            // Close autocomplete on space or newline
            self.autocomplete = None;
        }

        cx.notify();
    }

    /// Handles backspace key press.
    ///
    /// Behavior:
    /// - If selection exists: delete selected text
    /// - Otherwise: delete character before cursor
    /// - Does nothing if cursor is at document start
    fn handle_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        // Close autocomplete on backspace
        self.autocomplete = None;

        if !self.delete_selection() {
            if self.cursor_position > 0 {
                self.cursor_position -= 1;
                self.content.remove(self.cursor_position);
                self.is_dirty = true;
            }
        } else {
            self.is_dirty = true;
        }
        cx.notify();
    }

    /// Handles Enter key press by inserting a newline at cursor position.
    /// If autocomplete is active, accepts the selected suggestion instead.
    fn handle_enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
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
            }
            self.autocomplete = None;
            cx.notify();
            return;
        }

        self.content.insert(self.cursor_position, '\n');
        self.cursor_position += 1;
        self.is_dirty = true;
        cx.notify();
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
                self.delete_selection();
                self.content.insert_str(self.cursor_position, &text);
                self.cursor_position += text.len();
                self.is_dirty = true;
                cx.notify();
            }
        }
    }

    /// Handles Ctrl+X (Cut) action.
    /// Copies selected text to clipboard and deletes it. Does nothing if no selection.
    fn handle_cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.get_selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.delete_selection();
            self.is_dirty = true;
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
            // Open palette and transfer focus to it
            let palette_entity = cx.new(|cx| Palette::new(self.working_dir.clone(), cx));
            window.focus(&palette_entity.read(cx).focus_handle(cx));
            self.palette = Some(palette_entity);
        }
        cx.notify();
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
                self.current_file = Some(path.to_string_lossy().to_string());
                self.is_dirty = false;
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
    /// # Magic Numbers
    ///
    /// Hardcoded layout constants should be extracted to `TextEditor` constants:
    /// - `char_width`: 7.2px (assumes monospace font)
    /// - `line_height`: 22px
    /// - `header_height`: 30px (status bar)
    /// - `padding`: 16px
    fn handle_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.clear_selection();

        let char_width = px(7.2);
        let line_height = px(22.0);
        let header_height = px(30.0);
        let padding = px(16.0);

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
        cx.notify();
    }

    /// Handles mouse scroll wheel events for vertical scrolling.
    ///
    /// Supports both pixel-based and line-based scroll deltas.
    /// Clamps scroll offset to valid range [0, max_content_height - viewport_height].
    ///
    /// # Magic Numbers
    ///
    /// - `line_height`: 22.0px (should match rendering constant)
    /// - `viewport_height`: 538.0px (derived from window height - header - padding)
    fn handle_scroll_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let line_height = 22.0;

        let scroll_amount = match event.delta {
            gpui::ScrollDelta::Pixels(delta) => delta.y.into(),
            gpui::ScrollDelta::Lines(delta) => delta.y * line_height,
        };

        self.scroll_offset -= scroll_amount;

        let lines: Vec<&str> = self.content.split('\n').collect();
        let total_content_height = lines.len() as f32 * line_height;

        let viewport_height = 538.0;
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
        let lines: Vec<&str> = self.content.split('\n').collect();
        let mut current_pos = 0;
        let mut current_line = 0;
        let mut col_in_line = 0;

        for (line_idx, line) in lines.iter().enumerate() {
            if current_pos + line.len() >= self.cursor_position {
                current_line = line_idx;
                col_in_line = self.cursor_position - current_pos;
                break;
            }
            current_pos += line.len() + 1;
        }

        if current_line > 0 {
            let prev_line_len = lines[current_line - 1].len();
            let new_col = col_in_line.min(prev_line_len);
            let mut new_pos = 0;
            for (i, line) in lines.iter().enumerate() {
                if i == current_line - 1 {
                    new_pos += new_col;
                    break;
                }
                new_pos += line.len() + 1;
            }
            self.cursor_position = new_pos;
        }
    }

    /// Internal helper for moving cursor down one line while preserving column position.
    ///
    /// Algorithm mirrors `move_up_internal` but moves to the next line instead.
    /// Handles edge cases like moving from long line to short line gracefully.
    fn move_down_internal(&mut self) {
        let lines: Vec<&str> = self.content.split('\n').collect();
        let mut current_pos = 0;
        let mut current_line = 0;
        let mut col_in_line = 0;

        for (line_idx, line) in lines.iter().enumerate() {
            if current_pos + line.len() >= self.cursor_position {
                current_line = line_idx;
                col_in_line = self.cursor_position - current_pos;
                break;
            }
            current_pos += line.len() + 1;
        }

        if current_line < lines.len() - 1 {
            let next_line_len = lines[current_line + 1].len();
            let new_col = col_in_line.min(next_line_len);
            let mut new_pos = 0;
            for (i, line) in lines.iter().enumerate() {
                if i == current_line + 1 {
                    new_pos += new_col;
                    break;
                }
                new_pos += line.len() + 1;
            }
            self.cursor_position = new_pos;
        }
    }
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
        // Check if palette wants to open a file or close
        if let Some(palette_entity) = &self.palette {
            let palette = palette_entity.read(cx);
            if palette.should_open {
                let selected_file = palette.get_selected_file();
                drop(palette); // Release the read lock
                if let Some(file_to_load) = selected_file {
                    self.palette = None;
                    window.focus(&self.focus_handle);
                    self.load_file(file_to_load, cx);
                }
            } else if palette.should_close {
                drop(palette); // Release the read lock
                self.palette = None;
                window.focus(&self.focus_handle);
                cx.notify();
            }
        }

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
            .on_action(cx.listener(Self::handle_backspace))
            .on_action(cx.listener(Self::handle_enter))
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
            .on_action(cx.listener(Self::handle_toggle_palette))
            .on_key_down(cx.listener(|editor, event: &KeyDownEvent, _, cx| {
                // Handle Escape to close autocomplete
                if event.keystroke.key == "escape" && editor.autocomplete.is_some() {
                    editor.autocomplete = None;
                    cx.notify();
                    return;
                }

                // Regular character input (only when palette is closed)
                if editor.palette.is_none() {
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
            .bg(rgb(0x2d2d2d))
            .border_1()
            .border_color(rgb(0x454545))
            .rounded_md()
            .shadow_lg()
            .text_color(rgb(0xd4d4d4))
            .p_4()
            .font_family("monospace")
            .text_sm()
            .child(div().mb_2().text_color(rgb(0x808080)).child(format!(
                        "MedleyText - {} | Ctrl+P: files | Ctrl+S: save | Ctrl+Q: quit",
                        self.current_file
                            .as_ref()
                            .map(|p| p.as_str())
                            .unwrap_or("[unsaved]")
                    )))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_1()
                    .overflow_hidden()
                    .child(div().flex().flex_col().mt(px(-self.scroll_offset)).child({
                        let lines: Vec<&str> = self.content.split('\n').collect();
                        let mut current_pos = 0;
                        let mut result = div().flex().flex_col();
                        let selection_range = self.get_selection_range();

                        for line in lines {
                            let line_start = current_pos;
                            let line_end = current_pos + line.len();
                            let cursor_on_line = self.cursor_position >= line_start
                                && self.cursor_position <= line_end;

                            let tokens = MarkdownHighlighter::tokenize_line(line);

                            let mut line_div = div().flex().flex_row();
                            let mut char_count = 0;

                            for (text, token_type) in tokens {
                                let token_color = MarkdownHighlighter::get_color(&token_type);
                                let token_start = line_start + char_count;
                                let token_end = token_start + text.len();

                                let cursor_in_token = cursor_on_line
                                    && self.cursor_position >= token_start
                                    && self.cursor_position < token_end;

                                if let Some((sel_start, sel_end)) = selection_range {
                                    if token_end > sel_start && token_start < sel_end {
                                        let overlap_start =
                                            sel_start.max(token_start) - token_start;
                                        let overlap_end = sel_end.min(token_end) - token_start;

                                        let before_sel = &text[..overlap_start];
                                        let selected = &text[overlap_start..overlap_end];
                                        let after_sel = &text[overlap_end..];

                                        if !before_sel.is_empty() {
                                            line_div = line_div.child(
                                                div()
                                                    .text_color(token_color)
                                                    .child(before_sel.to_string()),
                                            );
                                        }
                                        if !selected.is_empty() {
                                            line_div = line_div.child(
                                                div()
                                                    .bg(rgb(0x264F78))
                                                    .text_color(rgb(0xffffff))
                                                    .child(selected.to_string()),
                                            );
                                        }
                                        if !after_sel.is_empty() {
                                            line_div = line_div.child(
                                                div()
                                                    .text_color(token_color)
                                                    .child(after_sel.to_string()),
                                            );
                                        }
                                    } else if cursor_in_token {
                                        let cursor_offset = self.cursor_position - token_start;
                                        let before = &text[..cursor_offset];
                                        let after = &text[cursor_offset..];

                                        if !before.is_empty() {
                                            line_div = line_div.child(
                                                div()
                                                    .text_color(token_color)
                                                    .child(before.to_string()),
                                            );
                                        }
                                        line_div = line_div
                                            .child(div().w(px(4.0)).h(px(18.0)).bg(rgb(0xcccccc)));
                                        if !after.is_empty() {
                                            line_div = line_div.child(
                                                div()
                                                    .text_color(token_color)
                                                    .child(after.to_string()),
                                            );
                                        }
                                    } else {
                                        line_div = line_div.child(
                                            div().text_color(token_color).child(text.clone()),
                                        );
                                    }
                                } else if cursor_in_token {
                                    let cursor_offset = self.cursor_position - token_start;
                                    let before = &text[..cursor_offset];
                                    let after = &text[cursor_offset..];

                                    if !before.is_empty() {
                                        line_div = line_div.child(
                                            div().text_color(token_color).child(before.to_string()),
                                        );
                                    }
                                    line_div = line_div
                                        .child(div().w(px(4.0)).h(px(18.0)).bg(rgb(0xcccccc)));
                                    if !after.is_empty() {
                                        line_div = line_div.child(
                                            div().text_color(token_color).child(after.to_string()),
                                        );
                                    }
                                } else {
                                    line_div = line_div
                                        .child(div().text_color(token_color).child(text.clone()));
                                }

                                char_count += text.len();
                            }

                            if cursor_on_line {
                                let cursor_col = self.cursor_position - line_start;
                                if cursor_col == line.len() {
                                    line_div = line_div
                                        .child(div().w(px(4.0)).h(px(18.0)).bg(rgb(0xcccccc)));
                                }
                            }

                            result = result.child(line_div);
                            current_pos = line_end + 1;
                        }

                        result
                    })),
            )
            .child(
                div()
                    .mt_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(0x454545))
                    .flex()
                    .flex_row()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(0x808080))
                    .child(div().child(format!("Line {}", self.get_current_line_number())))
                    .child(div().child(if self.is_dirty {
                        "● unsaved"
                    } else {
                        "✓ saved"
                    })),
            );

        // Wrap in a container and add overlays (autocomplete and/or palette)
        let mut container = div().size_full().child(editor_content);

        // Add autocomplete overlay if active
        if let Some(autocomplete) = &self.autocomplete {
            let suggestions = autocomplete.get_suggestions_display();

            // Calculate cursor position for positioning the dropdown
            let line_height = 22.0;
            let header_height = 30.0;
            let padding = 16.0;
            let current_line = self.get_current_line_number() as f32 - 1.0;
            let top = padding + header_height + (current_line * line_height) + line_height
                - self.scroll_offset;

            let autocomplete_menu = div()
                .absolute()
                .top(px(top))
                .left(px(padding))
                .w(px(400.0))
                .bg(rgb(0x2d2d2d))
                .border_1()
                .border_color(rgb(0x454545))
                .rounded_md()
                .shadow_lg()
                .flex()
                .flex_col()
                .overflow_hidden()
                .children(suggestions.iter().map(|(is_selected, suggestion)| {
                    div()
                        .p_2()
                        .pl_3()
                        .bg(crate::autocomplete::Autocomplete::item_bg_color(
                            *is_selected,
                        ))
                        .flex()
                        .flex_row()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_family("monospace")
                                .text_color(crate::autocomplete::Autocomplete::item_text_color(
                                    *is_selected,
                                ))
                                .child(suggestion.insert_text.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x808080))
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
