//! Markdown syntax tokenizer and color scheme.
//!
//! Provides line-based markdown parsing for syntax highlighting.
//! Implements a simplified subset of CommonMark focused on visual distinction.

use gpui::Rgba;

/// Markdown token types for syntax highlighting.
///
/// Each variant represents a distinct syntactic element with its own color.
/// Tokens are applied at the line or inline level depending on type.
#[derive(Debug, Clone)]
pub enum MarkdownToken {
    /// Heading level (1-6). Level stored for different color gradients.
    Heading(usize),

    /// Bold text wrapped in `**bold**`.
    Bold,

    /// Italic text wrapped in `*italic*` or `_italic_`.
    Italic,

    /// Inline code wrapped in backticks.
    Code,

    /// Markdown link `[text](url)`.
    Link,

    /// Unordered list item starting with `- ` or `* `, or ordered list `1. `.
    ListItem,

    /// Completed checkbox `- [x]` or `- [X]`.
    CheckboxChecked,

    /// Uncompleted checkbox `- [ ]`.
    CheckboxUnchecked,

    /// Blockquote starting with `> `.
    Blockquote,

    /// Code block fence line (triple backticks).
    CodeBlock,

    /// Normal text with no special formatting.
    Normal,
}

/// Stateless markdown syntax highlighter.
///
/// Provides line-based tokenization and color mapping.
/// Does not maintain state between lines (no multi-line block tracking).
pub struct MarkdownHighlighter;

impl MarkdownHighlighter {
    /// Maps token types to GPUI colors.
    ///
    /// Color scheme inspired by VS Code Dark+ theme for familiarity.
    /// All colors are specified as RGB hex values.
    ///
    /// # Color Palette
    ///
    /// - Headings: Blue gradient (H1 darkest -> H6 lightest)
    /// - Bold: Yellow
    /// - Italic: Orange
    /// - Code/CodeBlock: Red
    /// - Links: Cyan
    /// - Lists: Purple
    /// - Checkboxes: Green (checked) / Coral (unchecked)
    /// - Blockquotes: Green
    /// - Normal: Light gray
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

    /// Tokenizes a single line of markdown into styled segments.
    ///
    /// # Algorithm
    ///
    /// 1. Check for line-level patterns (headings, lists, blockquotes, code fences)
    /// 2. If matched, return entire line as single token
    /// 3. Otherwise, scan for inline patterns (bold, italic, code, links)
    /// 4. Return list of (text, token_type) tuples
    ///
    /// # Parsing Strategy
    ///
    /// - **Line-level tokens**: Matched first, consume entire line (early return)
    /// - **Inline tokens**: Scanned left-to-right with greedy matching
    /// - **Nesting**: Not supported (e.g., `**bold *and italic***` won't parse correctly)
    /// - **Escaping**: Not implemented (can't escape `*` or `` ` ``)
    ///
    /// # Known Limitations
    ///
    /// - No multi-line constructs (code blocks, blockquotes, lists spanning lines)
    /// - No emphasis nesting (`***bold and italic***`)
    /// - Link parsing is simplistic (doesn't validate URLs)
    /// - Checkbox detection doesn't validate if inside list
    ///
    /// # Future Improvements
    ///
    /// - Use state machine or proper parser for better accuracy
    /// - Support CommonMark nesting rules
    /// - Add escape sequence handling
    /// - Track multi-line block state
    pub fn tokenize_line(line: &str) -> Vec<(String, MarkdownToken)> {
        let mut tokens = Vec::new();

        // Line-level patterns: headings (H1-H6)
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

        // Code block fence marker
        if line.starts_with("```") {
            tokens.push((line.to_string(), MarkdownToken::CodeBlock));
            return tokens;
        }

        // Checkbox items (checked/unchecked)
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

        // Blockquotes
        if line.starts_with("> ") {
            tokens.push((line.to_string(), MarkdownToken::Blockquote));
            return tokens;
        }

        // List items (unordered: `- ` or `* `, ordered: `1. `)
        if line.starts_with("- ")
            || line.starts_with("* ")
            || (line.len() > 2
                && line.chars().next().unwrap().is_ascii_digit()
                && &line[1..3] == ". ")
        {
            tokens.push((line.to_string(), MarkdownToken::ListItem));
            return tokens;
        }

        // Inline patterns: scan character by character
        let mut current = String::new();
        let mut chars = line.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                // Inline code: `code`
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
                // Bold: **text**
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
                // Italic: *text* or _text_
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
                // Links: [text](url)
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
                // Normal text
                _ => {
                    current.push(ch);
                }
            }
        }

        // Flush remaining text as normal token
        if !current.is_empty() {
            tokens.push((current, MarkdownToken::Normal));
        }

        // Ensure we always return at least one token
        if tokens.is_empty() {
            tokens.push((line.to_string(), MarkdownToken::Normal));
        }

        tokens
    }
}
