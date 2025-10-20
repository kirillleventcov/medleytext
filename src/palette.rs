//! Command palette for fuzzy file finding and quick navigation.

use gpui::{
    App, Context, FocusHandle, Focusable, KeyDownEvent, Render, Window, div, prelude::*, px, rgb,
};
use std::path::{Path, PathBuf};

/// Represents a file entry in the palette with fuzzy match score.
#[derive(Clone, Debug)]
pub struct FileEntry {
    /// Absolute path to the file
    pub path: PathBuf,
    /// Display name (relative to working directory)
    pub display_name: String,
    /// Fuzzy match score (higher is better, None if no match)
    pub score: Option<i32>,
}

/// Command palette for fuzzy file finding.
///
/// Provides a modal overlay for searching and opening markdown files.
/// Uses a custom fuzzy matching algorithm with no external dependencies.
pub struct Palette {
    /// Current search query entered by user
    query: String,
    /// All markdown files found in working directory
    all_files: Vec<FileEntry>,
    /// Filtered and ranked files based on current query
    filtered_files: Vec<FileEntry>,
    /// Currently selected index in filtered results
    selected_index: usize,
    /// GPUI focus handle for keyboard event routing
    focus_handle: FocusHandle,
    /// Working directory path for relative path calculation
    working_dir: PathBuf,
    /// Flag indicating if user pressed Enter to select a file
    pub should_open: bool,
    /// Flag indicating if user pressed Escape to close
    pub should_close: bool,
}

impl Palette {
    /// Creates a new Palette instance and scans for markdown files.
    ///
    /// # Arguments
    ///
    /// * `working_dir` - Directory to scan for .md files
    /// * `cx` - GPUI context for initialization
    pub fn new(working_dir: PathBuf, cx: &mut Context<Self>) -> Self {
        let all_files = Self::scan_markdown_files(&working_dir);
        let filtered_files = all_files.clone();

        Self {
            query: String::new(),
            all_files,
            filtered_files,
            selected_index: 0,
            focus_handle: cx.focus_handle(),
            working_dir,
            should_open: false,
            should_close: false,
        }
    }

    /// Scans the working directory recursively for .md files.
    ///
    /// Returns a vector of FileEntry with paths and display names.
    /// Excludes hidden directories and files (starting with '.').
    fn scan_markdown_files(dir: &Path) -> Vec<FileEntry> {
        let mut files = Vec::new();

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                // Skip hidden files/directories
                if let Some(name) = path.file_name() {
                    if name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                }

                if path.is_dir() {
                    // Recursively scan subdirectories
                    files.extend(Self::scan_markdown_files(&path));
                } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    if let Ok(abs_path) = path.canonicalize() {
                        let display_name = path
                            .strip_prefix(dir)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();

                        files.push(FileEntry {
                            path: abs_path,
                            display_name,
                            score: None,
                        });
                    }
                }
            }
        }

        files
    }

    /// Custom fuzzy matching algorithm.
    ///
    /// Scores a string based on how well it matches the query pattern.
    /// Algorithm:
    /// - Sequential character matching (all query chars must appear in order)
    /// - Bonus points for consecutive matches
    /// - Bonus points for matches at word boundaries (after '/', '-', '_', ' ')
    /// - Case-insensitive matching
    ///
    /// Returns None if query doesn't match, otherwise returns score (higher is better).
    fn fuzzy_match(query: &str, target: &str) -> Option<i32> {
        if query.is_empty() {
            return Some(0);
        }

        let query_lower = query.to_lowercase();
        let target_lower = target.to_lowercase();
        let query_chars: Vec<char> = query_lower.chars().collect();
        let target_chars: Vec<char> = target_lower.chars().collect();

        let mut score = 0;
        let mut query_idx = 0;
        let mut consecutive_matches = 0;

        for (target_idx, &target_char) in target_chars.iter().enumerate() {
            if query_idx >= query_chars.len() {
                break;
            }

            if target_char == query_chars[query_idx] {
                // Base score for match
                score += 10;

                // Bonus for consecutive matches
                consecutive_matches += 1;
                score += consecutive_matches * 5;

                // Bonus for match at start of string
                if target_idx == 0 {
                    score += 20;
                }

                // Bonus for match after word boundary
                if target_idx > 0 {
                    let prev_char = target_chars[target_idx - 1];
                    if prev_char == '/' || prev_char == '-' || prev_char == '_' || prev_char == ' '
                    {
                        score += 15;
                    }
                }

                query_idx += 1;
            } else {
                consecutive_matches = 0;
            }
        }

        // Check if all query characters were matched
        if query_idx == query_chars.len() {
            Some(score)
        } else {
            None
        }
    }

    /// Updates the filtered file list based on current query.
    ///
    /// Applies fuzzy matching, filters out non-matches, and sorts by score.
    fn update_filtered_files(&mut self) {
        self.filtered_files = self
            .all_files
            .iter()
            .filter_map(|file| {
                let score = Self::fuzzy_match(&self.query, &file.display_name);
                score.map(|s| FileEntry {
                    path: file.path.clone(),
                    display_name: file.display_name.clone(),
                    score: Some(s),
                })
            })
            .collect();

        // Sort by score (highest first)
        self.filtered_files
            .sort_by(|a, b| b.score.unwrap_or(0).cmp(&a.score.unwrap_or(0)));

        // Reset selection to first item
        self.selected_index = 0;
    }

    /// Returns the currently selected file path, if any.
    pub fn get_selected_file(&self) -> Option<PathBuf> {
        self.filtered_files
            .get(self.selected_index)
            .map(|f| f.path.clone())
    }

    /// Handles character input for search query.
    fn handle_char_input(&mut self, c: char, cx: &mut Context<Self>) {
        self.query.push(c);
        self.update_filtered_files();
        cx.notify();
    }

    /// Handles backspace to delete last character from query.
    fn handle_backspace(&mut self, cx: &mut Context<Self>) {
        self.query.pop();
        self.update_filtered_files();
        cx.notify();
    }

    /// Handles up arrow to move selection up.
    fn handle_up(&mut self, cx: &mut Context<Self>) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            cx.notify();
        }
    }

    /// Handles down arrow to move selection down.
    fn handle_down(&mut self, cx: &mut Context<Self>) {
        if self.selected_index < self.filtered_files.len().saturating_sub(1) {
            self.selected_index += 1;
            cx.notify();
        }
    }
}

impl Focusable for Palette {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Palette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let max_visible_items = 10;

        div()
            .track_focus(&self.focus_handle(cx))
            .on_key_down(cx.listener(|palette, event: &KeyDownEvent, _, cx| {
                // Handle Enter - mark that user wants to open the selected file
                if event.keystroke.key == "enter" {
                    palette.should_open = true;
                    cx.notify();
                    return;
                }

                // Handle Escape - mark that user wants to close palette
                if event.keystroke.key == "escape" {
                    palette.should_close = true;
                    cx.notify();
                    return;
                }

                // Handle backspace
                if event.keystroke.key == "backspace" {
                    palette.handle_backspace(cx);
                    return;
                }

                // Handle arrow keys
                if event.keystroke.key == "up" {
                    palette.handle_up(cx);
                    return;
                }
                if event.keystroke.key == "down" {
                    palette.handle_down(cx);
                    return;
                }

                // Handle regular character input
                if let Some(key_char) = &event.keystroke.key_char {
                    if key_char.len() == 1
                        && !event.keystroke.modifiers.control
                        && !event.keystroke.modifiers.alt
                        && !event.keystroke.modifiers.platform
                    {
                        if let Some(c) = key_char.chars().next() {
                            if c.is_ascii_graphic() || c == ' ' {
                                palette.handle_char_input(c, cx);
                            }
                        }
                    }
                }
            }))
            .absolute()
            .top(px(50.0))
            .left(px(100.0))
            .w(px(600.0))
            .max_h(px(400.0))
            .bg(rgb(0x2d2d2d))
            .border_1()
            .border_color(rgb(0x454545))
            .rounded_md()
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            // Search input area
            .child(
                div().p_3().border_b_1().border_color(rgb(0x454545)).child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xcccccc))
                        .font_family("monospace")
                        .child(format!(
                            "> {}",
                            if self.query.is_empty() {
                                "Type to search...".to_string()
                            } else {
                                self.query.clone()
                            }
                        )),
                ),
            )
            // Results list
            .child(
                div()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .max_h(px(300.0))
                    .children(
                        self.filtered_files
                            .iter()
                            .take(max_visible_items)
                            .enumerate()
                            .map(|(idx, file)| {
                                let is_selected = idx == self.selected_index;
                                div()
                                    .p_2()
                                    .pl_3()
                                    .when(is_selected, |div| div.bg(rgb(0x094771)))
                                    .when(!is_selected, |div| div.bg(rgb(0x2d2d2d)))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_family("monospace")
                                            .text_color(if is_selected {
                                                rgb(0xffffff)
                                            } else {
                                                rgb(0xd4d4d4)
                                            })
                                            .child(file.display_name.clone()),
                                    )
                            }),
                    ),
            )
            // Footer with hints
            .child(div().p_2().border_t_1().border_color(rgb(0x454545)).child(
                div().text_xs().text_color(rgb(0x808080)).child(format!(
                    "{} files | ↑↓ navigate | Enter to open | Esc to close",
                    self.filtered_files.len()
                )),
            ))
    }
}
