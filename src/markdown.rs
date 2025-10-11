use gpui::Rgba;

#[derive(Debug, Clone)]
pub enum MarkdownToken {
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

pub struct MarkdownHighlighter;

impl MarkdownHighlighter {
    pub fn get_color(token: &MarkdownToken) -> Rgba {
        use gpui::rgb;

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

    pub fn tokenize_line(line: &str) -> Vec<(String, MarkdownToken)> {
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
