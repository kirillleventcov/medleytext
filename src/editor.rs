use gpui::{
    App, ClipboardItem, Context, FocusHandle, Focusable, KeyDownEvent, Render, Window, actions,
    div, prelude::*, px, rgb,
};

use crate::markdown::MarkdownHighlighter;

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
    ]
);

pub struct TextEditor {
    content: String,
    cursor_position: usize,
    selection_start: Option<usize>,
    focus_handle: FocusHandle,
    current_file: Option<String>,
}

impl TextEditor {
    pub fn with_file(file_path: Option<String>, cx: &mut Context<Self>) -> Self {
        let (content, current_file) = if let Some(path) = file_path {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    println!("Loaded file: {}", path);
                    (content, Some(path))
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

        Self {
            content,
            cursor_position: 0,
            selection_start: None,
            focus_handle: cx.focus_handle(),
            current_file,
        }
    }

    fn get_selection_range(&self) -> Option<(usize, usize)> {
        self.selection_start.map(|start| {
            if start < self.cursor_position {
                (start, self.cursor_position)
            } else {
                (self.cursor_position, start)
            }
        })
    }

    fn get_selected_text(&self) -> Option<String> {
        self.get_selection_range()
            .map(|(start, end)| self.content[start..end].to_string())
    }

    fn clear_selection(&mut self) {
        self.selection_start = None;
    }

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

    fn insert_char(&mut self, c: char, cx: &mut Context<Self>) {
        self.delete_selection();
        self.content.insert(self.cursor_position, c);
        self.cursor_position += 1;
        cx.notify();
    }

    fn handle_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if !self.delete_selection() {
            if self.cursor_position > 0 {
                self.cursor_position -= 1;
                self.content.remove(self.cursor_position);
            }
        }
        cx.notify();
    }

    fn handle_enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        self.content.insert(self.cursor_position, '\n');
        self.cursor_position += 1;
        cx.notify();
    }

    fn handle_move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_selection();
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            cx.notify();
        }
    }

    fn handle_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_selection();
        if self.cursor_position < self.content.len() {
            self.cursor_position += 1;
            cx.notify();
        }
    }

    fn handle_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_selection();
        self.move_up_internal();
        cx.notify();
    }

    fn handle_move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_selection();
        self.move_down_internal();
        cx.notify();
    }

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
            println!("File saved to: {}", path);
        }
    }

    fn handle_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn handle_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.get_selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn handle_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(clipboard_item) = cx.read_from_clipboard() {
            if let Some(text) = clipboard_item.text().map(|s| s.to_string()) {
                self.delete_selection();
                self.content.insert_str(self.cursor_position, &text);
                self.cursor_position += text.len();
                cx.notify();
            }
        }
    }

    fn handle_cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.get_selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.delete_selection();
            cx.notify();
        }
    }

    fn handle_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_position);
        }
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            cx.notify();
        }
    }

    fn handle_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_position);
        }
        if self.cursor_position < self.content.len() {
            self.cursor_position += 1;
            cx.notify();
        }
    }

    fn handle_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_position);
        }
        self.move_up_internal();
        cx.notify();
    }

    fn handle_select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_position);
        }
        self.move_down_internal();
        cx.notify();
    }

    fn handle_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selection_start = Some(0);
        self.cursor_position = self.content.len();
        cx.notify();
    }

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

impl Focusable for TextEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle(cx))
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
            .on_key_down(cx.listener(|editor, event: &KeyDownEvent, _, cx| {
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
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xd4d4d4))
            .p_4()
            .font_family("monospace")
            .text_sm()
            .child(div().mb_2().text_color(rgb(0x808080)).child(format!(
                        "MedleyText Editor - {} | Ctrl+S: save | Ctrl+Q: quit",
                        self.current_file
                            .as_ref()
                            .map(|p| p.as_str())
                            .unwrap_or("[unsaved]")
                    )))
            .child(div().flex().flex_col().gap_1().child({
                let lines: Vec<&str> = self.content.split('\n').collect();
                let mut current_pos = 0;
                let mut result = div().flex().flex_col();
                let selection_range = self.get_selection_range();

                for line in lines {
                    let line_start = current_pos;
                    let line_end = current_pos + line.len();
                    let cursor_on_line =
                        self.cursor_position >= line_start && self.cursor_position <= line_end;

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
                                let overlap_start = sel_start.max(token_start) - token_start;
                                let overlap_end = sel_end.min(token_end) - token_start;

                                let before_sel = &text[..overlap_start];
                                let selected = &text[overlap_start..overlap_end];
                                let after_sel = &text[overlap_end..];

                                if !before_sel.is_empty() {
                                    line_div = line_div.child(
                                        div().text_color(token_color).child(before_sel.to_string()),
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
                                        div().text_color(token_color).child(after_sel.to_string()),
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
                                line_div =
                                    line_div.child(div().w(px(4.0)).h(px(18.0)).bg(rgb(0xcccccc)));
                                if !after.is_empty() {
                                    line_div = line_div.child(
                                        div().text_color(token_color).child(after.to_string()),
                                    );
                                }
                            } else {
                                line_div = line_div
                                    .child(div().text_color(token_color).child(text.clone()));
                            }
                        } else if cursor_in_token {
                            let cursor_offset = self.cursor_position - token_start;
                            let before = &text[..cursor_offset];
                            let after = &text[cursor_offset..];

                            if !before.is_empty() {
                                line_div = line_div
                                    .child(div().text_color(token_color).child(before.to_string()));
                            }
                            line_div =
                                line_div.child(div().w(px(4.0)).h(px(18.0)).bg(rgb(0xcccccc)));
                            if !after.is_empty() {
                                line_div = line_div
                                    .child(div().text_color(token_color).child(after.to_string()));
                            }
                        } else {
                            line_div =
                                line_div.child(div().text_color(token_color).child(text.clone()));
                        }

                        char_count += text.len();
                    }

                    if cursor_on_line {
                        let cursor_col = self.cursor_position - line_start;
                        if cursor_col == line.len() {
                            line_div =
                                line_div.child(div().w(px(4.0)).h(px(18.0)).bg(rgb(0xcccccc)));
                        }
                    }

                    result = result.child(line_div);
                    current_pos = line_end + 1;
                }

                result
            }))
    }
}
