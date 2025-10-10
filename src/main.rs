use gpui::{
    App, Application, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};

struct TextEditor {
    content: String,
    cursor_position: usize,
}

impl TextEditor {
    fn new() -> Self {
        Self {
            content: String::from("Welcome to MedleyText!\n\nStart typing..."),
            cursor_position: 0,
        }
    }

    fn insert_char(&mut self, c: char) {
        self.content.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    fn backspace(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.content.remove(self.cursor_position);
        }
    }

    fn insert_newline(&mut self) {
        self.content.insert(self.cursor_position, '\n');
        self.cursor_position += 1;
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_position < self.content.len() {
            self.cursor_position += 1;
        }
    }

    fn move_cursor_up(&mut self) {
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

    fn move_cursor_down(&mut self) {
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

    fn save_file(&self) {
        if let Err(e) = std::fs::write("output.txt", &self.content) {
            eprintln!("Failed to save file: {}", e);
        } else {
            println!("File saved successfully!");
        }
    }

    fn load_file(&mut self) {
        if let Ok(content) = std::fs::read_to_string("output.txt") {
            self.content = content;
            self.cursor_position = self.content.len();
            println!("File loaded successfully!");
        } else {
            println!("No saved file found.");
        }
    }
}

impl Render for TextEditor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let text_before_cursor = &self.content[..self.cursor_position];
        let text_after_cursor = &self.content[self.cursor_position..];

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xd4d4d4))
            .p_4()
            .font_family("monospace")
            .text_sm()
            .child(
                div()
                    .mb_2()
                    .text_color(rgb(0x808080))
                    .child("MedleyText Editor - Ctrl+S: save | Ctrl+O: open | Ctrl+Q: quit"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .child(text_before_cursor.to_string())
                    .child(div().w(px(2.0)).h(px(16.0)).bg(rgb(0xffffff)))
                    .child(text_after_cursor.to_string()),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|_| TextEditor::new()),
        )
        .unwrap();
    });
}
