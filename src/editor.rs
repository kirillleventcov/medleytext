use gpui::{
    App, Context, FocusHandle, Focusable, KeyDownEvent, Render, Window, actions, div, prelude::*,
    px, rgb,
};

use crate::markdown::MarkdownHighlighter;

actions!(
    editor,
    [
        MoveLeft, MoveRight, MoveUp, MoveDown, Backspace, Enter, Save, Quit
    ]
);

pub struct TextEditor {
    content: String,
    cursor_position: usize,
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
            focus_handle: cx.focus_handle(),
            current_file,
        }
    }

    fn insert_char(&mut self, c: char, cx: &mut Context<Self>) {
        self.content.insert(self.cursor_position, c);
        self.cursor_position += 1;
        cx.notify();
    }

    fn handle_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.content.remove(self.cursor_position);
            cx.notify();
        }
    }

    fn handle_enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        self.content.insert(self.cursor_position, '\n');
        self.cursor_position += 1;
        cx.notify();
    }

    fn handle_move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            cx.notify();
        }
    }

    fn handle_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.cursor_position < self.content.len() {
            self.cursor_position += 1;
            cx.notify();
        }
    }

    fn handle_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
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
            cx.notify();
        }
    }

    fn handle_move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
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
            cx.notify();
        }
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
                        let token_start = char_count;
                        let token_end = char_count + text.len();

                        if cursor_on_line {
                            let cursor_col = self.cursor_position - line_start;

                            if cursor_col >= token_start && cursor_col < token_end {
                                let col_in_token = cursor_col - token_start;
                                let before = &text[..col_in_token];
                                let after = &text[col_in_token..];

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
                            } else if cursor_col == token_end && token_end == line.len() {
                                line_div = line_div
                                    .child(div().text_color(token_color).child(text.clone()));
                                if char_count + text.len() == line.len() {
                                    line_div = line_div
                                        .child(div().w(px(4.0)).h(px(18.0)).bg(rgb(0xcccccc)));
                                }
                            } else {
                                line_div = line_div
                                    .child(div().text_color(token_color).child(text.clone()));
                            }
                        } else {
                            line_div =
                                line_div.child(div().text_color(token_color).child(text.clone()));
                        }

                        char_count += text.len();
                    }

                    result = result.child(line_div);
                    current_pos = line_end + 1;
                }

                result
            }))
    }
}
