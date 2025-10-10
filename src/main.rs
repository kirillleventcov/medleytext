use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, KeyBinding, KeyDownEvent, Render,
    Rgba, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, rgb, size,
};

actions!(
    editor,
    [
        MoveLeft, MoveRight, MoveUp, MoveDown, Backspace, Enter, Save, Quit
    ]
);

struct TextEditor {
    content: String,
    cursor_position: usize,
    focus_handle: FocusHandle,
    current_file: Option<String>,
}

#[derive(Debug, Clone)]
enum MarkdownToken {
    Heading(usize),    // # level
    Bold,              // **text**
    Italic,            // *text* or _text_
    Code,              // `code`
    Link,              // [text](url)
    ListItem,          // - or * or 1.
    CheckboxChecked,   // - [X] or - [x]
    CheckboxUnchecked, // - [ ]
    Blockquote,        // >
    CodeBlock,         // ```
    Normal,
}

struct MarkdownHighlighter;

impl MarkdownHighlighter {
    fn get_color(token: &MarkdownToken) -> Rgba {
        match token {
            MarkdownToken::Heading(1) => rgb(0x569CD6),        // Blue
            MarkdownToken::Heading(2) => rgb(0x4EC9B0),        // Teal
            MarkdownToken::Heading(_) => rgb(0x4FC1FF),        // Light blue
            MarkdownToken::Bold => rgb(0xDCDCAA),              // Yellow
            MarkdownToken::Italic => rgb(0xCE9178),            // Orange
            MarkdownToken::Code => rgb(0xD16969),              // Red
            MarkdownToken::Link => rgb(0x9CDCFE),              // Cyan
            MarkdownToken::ListItem => rgb(0xC586C0),          // Purple
            MarkdownToken::CheckboxChecked => rgb(0x7CB342),   // Green (bright teal)
            MarkdownToken::CheckboxUnchecked => rgb(0xF48771), // Red-ish (coral)
            MarkdownToken::Blockquote => rgb(0x6A9955),        // Green
            MarkdownToken::CodeBlock => rgb(0xD16969),         // Red
            MarkdownToken::Normal => rgb(0xD4D4D4),            // Default
        }
    }

    fn tokenize_line(line: &str) -> Vec<(String, MarkdownToken)> {
        let mut tokens = Vec::new();

        if line.starts_with("# ") {
            tokens.push((line.to_string(), MarkdownToken::Heading(1)));
            return tokens;
        } else if line.starts_with("## ") {
            tokens.push((line.to_string(), MarkdownToken::Heading(2)));
            return tokens;
        } else if line.starts_with("### ") {
            tokens.push((line.to_string(), MarkdownToken::Heading(3)));
            return tokens;
        } else if line.starts_with("#### ") {
            tokens.push((line.to_string(), MarkdownToken::Heading(4)));
            return tokens;
        } else if line.starts_with("##### ") {
            tokens.push((line.to_string(), MarkdownToken::Heading(5)));
            return tokens;
        } else if line.starts_with("###### ") {
            tokens.push((line.to_string(), MarkdownToken::Heading(6)));
            return tokens;
        }

        if line.starts_with("```") {
            tokens.push((line.to_string(), MarkdownToken::CodeBlock));
            return tokens;
        }

        if line.starts_with("- [") && line.len() >= 5 {
            let checkbox_char = line.chars().nth(3);
            if checkbox_char == Some(' ') && line.chars().nth(4) == Some(']') {
                tokens.push((line.to_string(), MarkdownToken::CheckboxUnchecked));
                return tokens;
            } else if (checkbox_char == Some('X') || checkbox_char == Some('x'))
                && line.chars().nth(4) == Some(']')
            {
                tokens.push((line.to_string(), MarkdownToken::CheckboxChecked));
                return tokens;
            }
        }

        if line.starts_with("> ") {
            tokens.push((line.to_string(), MarkdownToken::Blockquote));
            return tokens;
        }

        if line.starts_with("- ")
            || line.starts_with("* ")
            || (line.len() > 2
                && line.chars().next().unwrap().is_ascii_digit()
                && &line[1..3] == ". ")
        {
            tokens.push((line.to_string(), MarkdownToken::ListItem));
            return tokens;
        }

        let mut current = String::new();
        let mut chars = line.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '`' => {
                    if !current.is_empty() {
                        tokens.push((current.clone(), MarkdownToken::Normal));
                        current.clear();
                    }
                    current.push(ch);
                    while let Some(next_ch) = chars.next() {
                        current.push(next_ch);
                        if next_ch == '`' {
                            break;
                        }
                    }
                    tokens.push((current.clone(), MarkdownToken::Code));
                    current.clear();
                }
                '*' if chars.peek() == Some(&'*') => {
                    if !current.is_empty() {
                        tokens.push((current.clone(), MarkdownToken::Normal));
                        current.clear();
                    }
                    current.push(ch);
                    current.push(chars.next().unwrap());
                    while let Some(next_ch) = chars.next() {
                        current.push(next_ch);
                        if next_ch == '*' && chars.peek() == Some(&'*') {
                            current.push(chars.next().unwrap());
                            break;
                        }
                    }
                    tokens.push((current.clone(), MarkdownToken::Bold));
                    current.clear();
                }
                '*' | '_' => {
                    if !current.is_empty() {
                        tokens.push((current.clone(), MarkdownToken::Normal));
                        current.clear();
                    }
                    current.push(ch);
                    let delimiter = ch;

                    while let Some(next_ch) = chars.next() {
                        current.push(next_ch);
                        if next_ch == delimiter {
                            break;
                        }
                    }
                    tokens.push((current.clone(), MarkdownToken::Italic));
                    current.clear();
                }
                '[' => {
                    if !current.is_empty() {
                        tokens.push((current.clone(), MarkdownToken::Normal));
                        current.clear();
                    }
                    current.push(ch);

                    while let Some(next_ch) = chars.next() {
                        current.push(next_ch);
                        if next_ch == ')' && current.contains("](") {
                            break;
                        }
                    }
                    tokens.push((current.clone(), MarkdownToken::Link));
                    current.clear();
                }
                _ => {
                    current.push(ch);
                }
            }
        }

        if !current.is_empty() {
            tokens.push((current, MarkdownToken::Normal));
        }

        if tokens.is_empty() {
            tokens.push((line.to_string(), MarkdownToken::Normal));
        }

        tokens
    }
}

impl TextEditor {
    fn with_file(file_path: Option<String>, cx: &mut Context<Self>) -> Self {
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
                // Handle regular text input
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file_path = args.get(1).cloned();

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("left", MoveLeft, None),
            KeyBinding::new("right", MoveRight, None),
            KeyBinding::new("up", MoveUp, None),
            KeyBinding::new("down", MoveDown, None),
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("enter", Enter, None),
            KeyBinding::new("ctrl-s", Save, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);

        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        let file_path_clone = file_path.clone();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| TextEditor::with_file(file_path_clone, cx)),
        )
        .unwrap();
    });
}
