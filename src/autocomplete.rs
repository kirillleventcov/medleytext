//! Autocomplete for markdown syntax suggestions.
//!
//! Provides inline completion suggestions when typing markdown syntax,
//! similar to nvim's completion menu.

use gpui::rgb;

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
    ///
    /// Returns `None` if no suggestions are available for this context.
    pub fn new(trigger: &str, line_content: &str) -> Option<Self> {
        let suggestions = Self::get_suggestions(trigger, line_content)?;

        if suggestions.is_empty() {
            None
        } else {
            Some(Self {
                suggestions,
                selected_index: 0,
            })
        }
    }

    /// Determines suggestions based on the trigger character and context.
    fn get_suggestions(trigger: &str, line_content: &str) -> Option<Vec<Suggestion>> {
        let trimmed = line_content.trim_start();

        // Heading suggestions (trigger: # at start of line)
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

        // List suggestions (trigger: - at start of line)
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

        // Code block (trigger: ` repeated 3 times)
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

        // Blockquote (trigger: > at start of line)
        if trigger == ">" && trimmed == ">" {
            return Some(vec![Suggestion {
                insert_text: "> ".to_string(),
                label: "Blockquote".to_string(),
            }]);
        }

        // Link (trigger: [ )
        if trigger == "[" && !line_content.is_empty() {
            return Some(vec![Suggestion {
                insert_text: "[text](url)".to_string(),
                label: "Link".to_string(),
            }]);
        }

        // Inline code (trigger: ` in middle of line)
        if trigger == "`" && !trimmed.is_empty() && !trimmed.starts_with("``") {
            // Check content BEFORE the just-typed backtick
            let content_before_trigger = if line_content.len() > 1 {
                &line_content[..line_content.len() - 1]
            } else {
                ""
            };

            // Count backticks before the one we just typed
            let backtick_count = content_before_trigger.matches('`').count();
            if backtick_count % 2 == 0 {
                // Even number means we're starting a new inline code
                return Some(vec![Suggestion {
                    insert_text: "``".to_string(),
                    label: "Inline code".to_string(),
                }]);
            }
            // Odd number means we're closing, don't show autocomplete
            return None;
        }

        // Bold/Italic (trigger: *)
        if trigger == "*" && !line_content.is_empty() {
            // Need to check the content BEFORE the just-typed asterisk
            // to determine if we're opening or closing
            let content_before_trigger = if line_content.len() > 1 {
                &line_content[..line_content.len() - 1]
            } else {
                ""
            };

            // Count unpaired asterisks before the one we just typed
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

            // If we have unpaired formatting markers, we're likely closing
            if unpaired_single % 2 == 1 || unpaired_double % 2 == 1 {
                return None;
            }

            // Otherwise, show suggestions for starting new formatting
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

    /// Returns the background color for autocomplete items.
    pub fn item_bg_color(is_selected: bool) -> gpui::Rgba {
        if is_selected {
            rgb(0x094771) // Selected item background
        } else {
            rgb(0x2d2d2d) // Normal item background
        }
    }

    /// Returns the text color for autocomplete items.
    pub fn item_text_color(is_selected: bool) -> gpui::Rgba {
        if is_selected {
            rgb(0xffffff) // Selected item text
        } else {
            rgb(0xd4d4d4) // Normal item text
        }
    }
}
