//! Search panel state and helpers for inline find/replace.
//!
//! Keeps all search logic self-contained so the editor can focus on UI wiring.

/// Byte range of a search hit within the buffer.
#[derive(Clone, Copy, Debug)]
pub struct SearchMatch {
    pub start: usize,
    pub end: usize,
}

/// Which virtual input row is currently active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveInput {
    Query,
    Replace,
}

/// Runtime state for the find/replace palette.
pub struct FindPanelState {
    pub query: String,
    pub replace: String,
    pub matches: Vec<SearchMatch>,
    pub selected_index: usize,
    pub show_replace: bool,
    pub active_input: ActiveInput,
    last_anchor: Option<usize>,
}

impl FindPanelState {
    /// Creates a panel with an optional initial query (for example, the current selection).
    pub fn new(initial_query: Option<String>) -> Self {
        let query = initial_query.unwrap_or_default();

        Self {
            query,
            replace: String::new(),
            matches: Vec::new(),
            selected_index: 0,
            show_replace: false,
            active_input: ActiveInput::Query,
            last_anchor: None,
        }
    }

    /// Returns true when there is a non-empty query.
    pub fn has_query(&self) -> bool {
        !self.query.is_empty()
    }

    /// Returns true when at least one match exists.
    pub fn has_matches(&self) -> bool {
        !self.matches.is_empty()
    }

    /// Currently focused match (if any).
    pub fn current_match(&self) -> Option<SearchMatch> {
        self.matches.get(self.selected_index).copied()
    }

    /// Selected index if matches exist.
    pub fn current_index(&self) -> Option<usize> {
        if self.matches.is_empty() {
            None
        } else {
            Some(self.selected_index)
        }
    }

    /// Adds a typed character to the active input field.
    pub fn push_char(&mut self, c: char, content: &str) {
        match self.active_input {
            ActiveInput::Query => {
                self.query.push(c);
                self.last_anchor = None;
                self.recompute_matches(content);
            }
            ActiveInput::Replace => self.replace.push(c),
        }
    }

    /// Deletes the last character from the active input.
    pub fn backspace(&mut self, content: &str) {
        match self.active_input {
            ActiveInput::Query => {
                self.query.pop();
                self.last_anchor = None;
                self.recompute_matches(content);
            }
            ActiveInput::Replace => {
                self.replace.pop();
            }
        }
    }

    /// Toggles the replace row visibility.
    pub fn toggle_replace(&mut self) {
        self.show_replace = !self.show_replace;
        if self.show_replace {
            self.active_input = ActiveInput::Replace;
        } else {
            self.active_input = ActiveInput::Query;
        }
    }

    /// Sets which row receives keyboard input.
    pub fn set_active_input(&mut self, input: ActiveInput) {
        self.active_input = input;
    }

    /// Rebuilds matches for the current query and content.
    pub fn recompute_matches(&mut self, content: &str) {
        if self.query.is_empty() {
            self.matches.clear();
            self.selected_index = 0;
            self.last_anchor = None;
            return;
        }

        let prev_anchor = self.current_match().map(|m| m.start).or(self.last_anchor);

        self.matches = find_all(content, &self.query);

        if self.matches.is_empty() {
            self.selected_index = 0;
            self.last_anchor = None;
            return;
        }

        if let Some(anchor) = prev_anchor {
            if let Some(idx) = self.matches.iter().position(|m| m.start >= anchor) {
                self.selected_index = idx;
            } else {
                self.selected_index = self.matches.len() - 1;
            }
        } else {
            self.selected_index = self.selected_index.min(self.matches.len() - 1);
        }

        self.last_anchor = Some(self.matches[self.selected_index].start);
    }

    /// Moves selection forward/backward through matches.
    pub fn cycle(&mut self, direction: isize) -> Option<SearchMatch> {
        if self.matches.is_empty() {
            return None;
        }

        let len = self.matches.len() as isize;
        let mut idx = self.selected_index as isize + direction;
        if idx < 0 {
            idx += len;
        } else if idx >= len {
            idx -= len;
        }
        self.selected_index = idx as usize;
        self.last_anchor = Some(self.matches[self.selected_index].start);
        self.current_match()
    }

    /// Updates the anchor to the currently selected match.
    pub fn refresh_anchor(&mut self) {
        self.last_anchor = self.current_match().map(|m| m.start);
    }
}

fn find_all(haystack: &str, needle: &str) -> Vec<SearchMatch> {
    if needle.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut offset = 0;

    while let Some(idx) = haystack[offset..].find(needle) {
        let start = offset + idx;
        let end = start + needle.len();
        matches.push(SearchMatch { start, end });
        offset = end.max(start + 1);
    }

    matches
}
