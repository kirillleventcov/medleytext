//! Autocomplete for Brief and Markdown syntax suggestions.
//!
//! Provides inline completion suggestions when typing markup syntax. The
//! suggestion list is language-aware: Brief offers single-marker emphasis
//! (`*bold*`, `_italic_`, `+underline+`, `~strike~`) and shortcodes
//! (`@link`, `@callout`, …); Markdown keeps its existing `**bold**` style.

use crate::editor::Language;

/// Represents a single autocomplete suggestion.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// The text to insert when this suggestion is selected
    pub insert_text: String,
    /// Human-friendly label shown on the right side
    pub label: String,
}

/// Autocomplete suggestion provider for markdown syntax.
pub struct Autocomplete {
    /// Currently displayed suggestions
    suggestions: Vec<Suggestion>,
    /// Index of the selected suggestion
    selected_index: usize,
}

impl Autocomplete {
    /// Creates a new Autocomplete instance with suggestions based on trigger.
    ///
    /// # Arguments
    ///
    /// * `trigger` - The character or pattern that triggered autocomplete
    /// * `line_content` - Content of the current line up to cursor
    /// * `language` - Active markup language for the buffer
    ///
    /// Returns `None` if no suggestions are available for this context.
    pub fn new(trigger: &str, line_content: &str, language: Language) -> Option<Self> {
        let suggestions = match language {
            Language::Brief => Self::brief_suggestions(trigger, line_content)?,
            Language::Markdown => Self::markdown_suggestions(trigger, line_content)?,
        };

        if suggestions.is_empty() {
            None
        } else {
            Some(Self {
                suggestions,
                selected_index: 0,
            })
        }
    }

    /// Brief-flavored suggestions. Differences from Markdown:
    /// - Bold/italic use single-character markers.
    /// - `@` opens shortcodes (`@link`, `@callout`, `@image`, `@kbd`, …).
    /// - Code blocks default to a `brief` language tag.
    fn brief_suggestions(trigger: &str, line_content: &str) -> Option<Vec<Suggestion>> {
        let trimmed = line_content.trim_start();

        // Heading suggestions: `#` at start of line, levels 1–6.
        if trigger == "#" && trimmed.starts_with('#') && !trimmed.starts_with("######") {
            return Some(vec![
                Suggestion {
                    insert_text: "# ".to_string(),
                    label: "Heading 1".to_string(),
                },
                Suggestion {
                    insert_text: "## ".to_string(),
                    label: "Heading 2".to_string(),
                },
                Suggestion {
                    insert_text: "### ".to_string(),
                    label: "Heading 3".to_string(),
                },
                Suggestion {
                    insert_text: "#### ".to_string(),
                    label: "Heading 4".to_string(),
                },
                Suggestion {
                    insert_text: "##### ".to_string(),
                    label: "Heading 5".to_string(),
                },
                Suggestion {
                    insert_text: "###### ".to_string(),
                    label: "Heading 6".to_string(),
                },
            ]);
        }

        // Bullet list / task markers: `- ` at start of line.
        if trigger == "-" && trimmed == "-" {
            return Some(vec![
                Suggestion {
                    insert_text: "- ".to_string(),
                    label: "Bullet".to_string(),
                },
                Suggestion {
                    insert_text: "- [ ] ".to_string(),
                    label: "Task: todo".to_string(),
                },
                Suggestion {
                    insert_text: "- [x] ".to_string(),
                    label: "Task: done".to_string(),
                },
            ]);
        }

        // Code fence: `` ``` `` triggers fence variants.
        if trigger == "`" && trimmed.starts_with("``") {
            return Some(vec![
                Suggestion {
                    insert_text: "```\n\n```".to_string(),
                    label: "Code block".to_string(),
                },
                Suggestion {
                    insert_text: "```rust\n\n```".to_string(),
                    label: "Rust".to_string(),
                },
                Suggestion {
                    insert_text: "```javascript\n\n```".to_string(),
                    label: "JavaScript".to_string(),
                },
                Suggestion {
                    insert_text: "```python\n\n```".to_string(),
                    label: "Python".to_string(),
                },
            ]);
        }

        // Inline code: a single ` mid-line.
        if trigger == "`" && !trimmed.is_empty() && !trimmed.starts_with("``") {
            let content_before_trigger = if line_content.len() > 1 {
                &line_content[..line_content.len() - 1]
            } else {
                ""
            };
            let backtick_count = content_before_trigger.matches('`').count();
            if backtick_count % 2 == 0 {
                return Some(vec![Suggestion {
                    insert_text: "`".to_string(),
                    label: "Inline code".to_string(),
                }]);
            }
            return None;
        }

        // Blockquote.
        if trigger == ">" && trimmed == ">" {
            return Some(vec![Suggestion {
                insert_text: "> ".to_string(),
                label: "Blockquote".to_string(),
            }]);
        }

        // `[` opens a link/image/kbd/etc. body — Brief uses shortcodes for
        // links, so this is a partial completion of `@link[text]` once the
        // user has already typed `@link`. Bare `[` mid-text gets the
        // markdown-style `@link[text](url)` snippet.
        if trigger == "[" && !line_content.is_empty() {
            return Some(vec![Suggestion {
                insert_text: "[text](url)".to_string(),
                label: "Link body + URL".to_string(),
            }]);
        }

        // `@` opens a shortcode menu.
        if trigger == "@" {
            return Some(vec![
                Suggestion {
                    insert_text: "@link[text](url)".to_string(),
                    label: "@link inline".to_string(),
                },
                Suggestion {
                    insert_text: "@image(src: \"\", alt: \"\")[]".to_string(),
                    label: "@image".to_string(),
                },
                Suggestion {
                    insert_text: "@kbd[Ctrl+]".to_string(),
                    label: "@kbd".to_string(),
                },
                Suggestion {
                    insert_text: "@math[]".to_string(),
                    label: "@math inline".to_string(),
                },
                Suggestion {
                    insert_text: "@footnote[]".to_string(),
                    label: "@footnote".to_string(),
                },
                Suggestion {
                    insert_text: "@callout(kind: note)\n\n@end".to_string(),
                    label: "@callout block".to_string(),
                },
                Suggestion {
                    insert_text: "@details(summary: \"\")\n\n@end".to_string(),
                    label: "@details block".to_string(),
                },
                Suggestion {
                    insert_text: "@t\n| ".to_string(),
                    label: "@t table".to_string(),
                },
                Suggestion {
                    insert_text: "@dl\n\n@end".to_string(),
                    label: "@dl definition list".to_string(),
                },
            ]);
        }

        None
    }

    /// Original Markdown suggestions, preserved for `.md` files.
    fn markdown_suggestions(trigger: &str, line_content: &str) -> Option<Vec<Suggestion>> {
        let trimmed = line_content.trim_start();

        if trigger == "#" && trimmed.starts_with('#') && !trimmed.starts_with("######") {
            return Some(vec![
                Suggestion {
                    insert_text: "# ".to_string(),
                    label: "Heading 1".to_string(),
                },
                Suggestion {
                    insert_text: "## ".to_string(),
                    label: "Heading 2".to_string(),
                },
                Suggestion {
                    insert_text: "### ".to_string(),
                    label: "Heading 3".to_string(),
                },
                Suggestion {
                    insert_text: "#### ".to_string(),
                    label: "Heading 4".to_string(),
                },
                Suggestion {
                    insert_text: "##### ".to_string(),
                    label: "Heading 5".to_string(),
                },
                Suggestion {
                    insert_text: "###### ".to_string(),
                    label: "Heading 6".to_string(),
                },
            ]);
        }

        if trigger == "-" && trimmed == "-" {
            return Some(vec![
                Suggestion {
                    insert_text: "- ".to_string(),
                    label: "Unordered list".to_string(),
                },
                Suggestion {
                    insert_text: "- [ ] ".to_string(),
                    label: "Unchecked checkbox".to_string(),
                },
                Suggestion {
                    insert_text: "- [x] ".to_string(),
                    label: "Checked checkbox".to_string(),
                },
            ]);
        }

        if trigger == "`" && trimmed.starts_with("``") {
            return Some(vec![
                Suggestion {
                    insert_text: "```\n\n```".to_string(),
                    label: "Code block".to_string(),
                },
                Suggestion {
                    insert_text: "```rust\n\n```".to_string(),
                    label: "Rust code block".to_string(),
                },
                Suggestion {
                    insert_text: "```javascript\n\n```".to_string(),
                    label: "JavaScript code block".to_string(),
                },
                Suggestion {
                    insert_text: "```python\n\n```".to_string(),
                    label: "Python code block".to_string(),
                },
            ]);
        }

        if trigger == ">" && trimmed == ">" {
            return Some(vec![Suggestion {
                insert_text: "> ".to_string(),
                label: "Blockquote".to_string(),
            }]);
        }

        if trigger == "[" && !line_content.is_empty() {
            return Some(vec![Suggestion {
                insert_text: "[text](url)".to_string(),
                label: "Link".to_string(),
            }]);
        }

        if trigger == "`" && !trimmed.is_empty() && !trimmed.starts_with("``") {
            let content_before_trigger = if line_content.len() > 1 {
                &line_content[..line_content.len() - 1]
            } else {
                ""
            };
            let backtick_count = content_before_trigger.matches('`').count();
            if backtick_count % 2 == 0 {
                return Some(vec![Suggestion {
                    insert_text: "``".to_string(),
                    label: "Inline code".to_string(),
                }]);
            }
            return None;
        }

        if trigger == "*" && !line_content.is_empty() {
            let content_before_trigger = if line_content.len() > 1 {
                &line_content[..line_content.len() - 1]
            } else {
                ""
            };
            let mut unpaired_single = 0;
            let mut unpaired_double = 0;
            let mut chars = content_before_trigger.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '*' {
                    if chars.peek() == Some(&'*') {
                        chars.next();
                        unpaired_double += 1;
                    } else {
                        unpaired_single += 1;
                    }
                }
            }
            if unpaired_single % 2 == 1 || unpaired_double % 2 == 1 {
                return None;
            }
            return Some(vec![
                Suggestion {
                    insert_text: "**".to_string(),
                    label: "Bold".to_string(),
                },
                Suggestion {
                    insert_text: "*".to_string(),
                    label: "Italic".to_string(),
                },
            ]);
        }

        None
    }

    /// Returns the currently selected suggestion.
    pub fn get_selected(&self) -> Option<&Suggestion> {
        self.suggestions.get(self.selected_index)
    }

    /// Moves selection up in the suggestion list.
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Moves selection down in the suggestion list.
    pub fn move_down(&mut self) {
        if self.selected_index < self.suggestions.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Returns all suggestions with their selection state.
    pub fn get_suggestions_display(&self) -> Vec<(bool, &Suggestion)> {
        self.suggestions
            .iter()
            .enumerate()
            .map(|(idx, sug)| (idx == self.selected_index, sug))
            .collect()
    }
}
