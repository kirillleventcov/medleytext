use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, KeyBinding, KeyDownEvent, Render,
    Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, rgb, size,
};

// Define actions for text editing
actions!(
    editor,
    [
        MoveLeft, MoveRight, MoveUp, MoveDown, Backspace, Enter, Save, Open, Quit
    ]
);

struct TextEditor {
    content: String,
    cursor_position: usize,
    focus_handle: FocusHandle,
}

impl TextEditor {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            content: String::from("Welcome to MedleyText!\n\nStart typing..."),
            cursor_position: 0,
            focus_handle: cx.focus_handle(),
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
        if let Err(e) = std::fs::write("output.txt", &self.content) {
            eprintln!("Failed to save file: {}", e);
        } else {
            println!("File saved successfully!");
        }
    }

    fn handle_open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        if let Ok(content) = std::fs::read_to_string("output.txt") {
            self.content = content;
            self.cursor_position = self.content.len();
            println!("File loaded successfully!");
            cx.notify();
        } else {
            println!("No saved file found.");
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
            .on_action(cx.listener(Self::handle_open))
            .on_action(cx.listener(Self::handle_quit))
            .on_key_down(cx.listener(|editor, event: &KeyDownEvent, _, cx| {
                // Handle regular text input
                if let Some(key_char) = &event.keystroke.key_char {
                    // Only insert printable characters (not control keys)
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
            .child(
                div()
                    .mb_2()
                    .text_color(rgb(0x808080))
                    .child("MedleyText Editor - Ctrl+S: save | Ctrl+O: open | Ctrl+Q: quit"),
            )
            .child(div().flex().flex_col().gap_1().child({
                // Render text with visible cursor
                let lines: Vec<&str> = self.content.split('\n').collect();
                let mut current_pos = 0;
                let mut result = div().flex().flex_col();

                for line in lines {
                    let line_start = current_pos;
                    let line_end = current_pos + line.len();

                    if self.cursor_position >= line_start && self.cursor_position <= line_end {
                        // Cursor is on this line
                        let col = self.cursor_position - line_start;
                        let before = &line[..col];
                        let after = &line[col..];

                        result = result.child(
                            div()
                                .flex()
                                .flex_row()
                                .child(before.to_string())
                                .child(div().w(px(8.0)).h(px(18.0)).bg(rgb(0x00ff00)))
                                .child(after.to_string()),
                        );
                    } else {
                        // Normal line without cursor
                        result = result.child(div().child(line.to_string()));
                    }

                    current_pos = line_end + 1; // +1 for the newline character
                }

                result
            }))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        // Bind keyboard shortcuts to actions
        cx.bind_keys([
            KeyBinding::new("left", MoveLeft, None),
            KeyBinding::new("right", MoveRight, None),
            KeyBinding::new("up", MoveUp, None),
            KeyBinding::new("down", MoveDown, None),
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("enter", Enter, None),
            KeyBinding::new("ctrl-s", Save, None),
            KeyBinding::new("ctrl-o", Open, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);

        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(TextEditor::new),
        )
        .unwrap();
    });
}
