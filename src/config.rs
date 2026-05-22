use gpui::{Rgba, rgb};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct EditorConfig {
    font_size: f32,
    theme: Theme,
}

impl EditorConfig {
    pub const DEFAULT_FONT_SIZE: f32 = 14.0;
    const MIN_FONT_SIZE: f32 = 8.0;
    const MAX_FONT_SIZE: f32 = 72.0;
    const DEFAULT_FILE_TEMPLATE: &'static str = r#"// MedleyText configuration
// Lines starting with # or // are ignored.
// Examples:
//   theme.preset = catppuccin-mocha
//   theme.editor.background = #2d2d2d
//
// MedleyText is a Brief-first editor (.brf files). Markdown (.md) still works.

font-size = 14
theme.preset = default
"#;

    pub fn load() -> Self {
        let mut config = Self::default();
        if let Some(path) = Self::config_path() {
            ensure_default_file(&path);
            if let Ok(contents) = fs::read_to_string(&path) {
                for (key, value) in parse_entries(&contents) {
                    match key.as_str() {
                        "font-size" | "font.size" => {
                            if let Ok(size) = value.parse::<f32>() {
                                config.font_size =
                                    size.clamp(Self::MIN_FONT_SIZE, Self::MAX_FONT_SIZE);
                            }
                        }
                        "theme.preset" => {
                            if let Some(theme) = Theme::preset(&value) {
                                config.theme = theme;
                            }
                        }
                        _ => {
                            config.theme.apply_override(&key, &value);
                        }
                    }
                }
            }
        }
        config
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    fn config_path() -> Option<PathBuf> {
        home_dir().map(|home| home.join(".config/medleytext/config"))
    }
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            font_size: Self::DEFAULT_FONT_SIZE,
            theme: Theme::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub editor: EditorTheme,
    pub highlight: HighlightTheme,
    pub panel: PanelTheme,
    pub palette: PaletteTheme,
    pub autocomplete: AutocompleteTheme,
    pub syntax: SyntaxTheme,
}

impl Theme {
    fn preset(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "default" | "dark" | "medley" => Some(Self::default()),
            "catppuccin-mocha" => Some(Self::catppuccin_mocha()),
            "catppuccin-latte" => Some(Self::catppuccin_latte()),
            "catppuccin-frappe" => Some(Self::catppuccin_frappe()),
            "catppuccin-macchiato" => Some(Self::catppuccin_macchiato()),
            _ => None,
        }
    }

    pub fn apply_override(&mut self, key: &str, value: &str) {
        if key.starts_with("theme.syntax.") {
            if let Some(color) = parse_color(value) {
                let token = &key["theme.syntax.".len()..];
                self.syntax.apply_override(token, color);
            }
            return;
        }

        let Some(color) = parse_color(value) else {
            return;
        };

        match key {
            "theme.editor.background" => self.editor.background = color,
            "theme.editor.border" => self.editor.border = color,
            "theme.editor.text" => self.editor.text = color,
            "theme.editor.muted-text" | "theme.editor.muted_text" => self.editor.muted_text = color,
            "theme.editor.cursor" => self.editor.cursor = color,

            "theme.highlight.selection.background" | "theme.highlight.selection.bg" => {
                self.highlight.selection_bg = color
            }
            "theme.highlight.selection.foreground" | "theme.highlight.selection.fg" => {
                self.highlight.selection_fg = color
            }
            "theme.highlight.search-active.background"
            | "theme.highlight.search_active.background" => self.highlight.search_active_bg = color,
            "theme.highlight.search-active.foreground"
            | "theme.highlight.search_active.foreground" => self.highlight.search_active_fg = color,
            "theme.highlight.search-match.background"
            | "theme.highlight.search_match.background" => self.highlight.search_match_bg = color,
            "theme.highlight.search-match.foreground"
            | "theme.highlight.search_match.foreground" => self.highlight.search_match_fg = color,

            "theme.panel.background" => self.panel.background = color,
            "theme.panel.border" => self.panel.border = color,
            "theme.panel.active-row.background" | "theme.panel.active_row.background" => {
                self.panel.active_row_background = color
            }
            "theme.panel.inactive-row.background" | "theme.panel.inactive_row.background" => {
                self.panel.inactive_row_background = color
            }
            "theme.panel.label-text" | "theme.panel.label_text" => self.panel.label_text = color,
            "theme.panel.value-text" | "theme.panel.value_text" => self.panel.value_text = color,
            "theme.panel.placeholder-text" | "theme.panel.placeholder_text" => {
                self.panel.placeholder_text = color
            }
            "theme.panel.status-text" | "theme.panel.status_text" => self.panel.status_text = color,
            "theme.panel.shortcut-text" | "theme.panel.shortcut_text" => {
                self.panel.shortcut_text = color
            }

            "theme.palette.background" => self.palette.background = color,
            "theme.palette.border" => self.palette.border = color,
            "theme.palette.input-text" | "theme.palette.input_text" => {
                self.palette.input_text = color
            }
            "theme.palette.item.background" => self.palette.item_background = color,
            "theme.palette.item.foreground" => self.palette.item_text = color,
            "theme.palette.item-selected.background" | "theme.palette.item_selected.background" => {
                self.palette.item_selected_background = color
            }
            "theme.palette.item-selected.foreground" | "theme.palette.item_selected.foreground" => {
                self.palette.item_selected_text = color
            }
            "theme.palette.footer-text" | "theme.palette.footer_text" => {
                self.palette.footer_text = color
            }

            "theme.autocomplete.background" => self.autocomplete.background = color,
            "theme.autocomplete.border" => self.autocomplete.border = color,
            "theme.autocomplete.item.background" => self.autocomplete.item_background = color,
            "theme.autocomplete.item.foreground" => self.autocomplete.item_text = color,
            "theme.autocomplete.item-selected.background"
            | "theme.autocomplete.item_selected.background" => {
                self.autocomplete.item_selected_background = color
            }
            "theme.autocomplete.item-selected.foreground"
            | "theme.autocomplete.item_selected.foreground" => {
                self.autocomplete.item_selected_text = color
            }
            "theme.autocomplete.label-text" | "theme.autocomplete.label_text" => {
                self.autocomplete.label_text = color
            }

            _ => {}
        }
    }

    fn catppuccin_mocha() -> Self {
        Self {
            editor: EditorTheme {
                background: rgb(0x1e1e2e),
                border: rgb(0x313244),
                text: rgb(0xcdd6f4),
                muted_text: rgb(0xa6adc8),
                cursor: rgb(0xf5e0dc),
            },
            highlight: HighlightTheme {
                selection_bg: rgb(0x585b70),
                selection_fg: rgb(0xcdd6f4),
                search_active_bg: rgb(0xfab387),
                search_active_fg: rgb(0x1e1e2e),
                search_match_bg: rgb(0x89dceb),
                search_match_fg: rgb(0x1e1e2e),
            },
            panel: PanelTheme {
                background: rgb(0x181825),
                border: rgb(0x313244),
                active_row_background: rgb(0x313244),
                inactive_row_background: rgb(0x1e1e2e),
                label_text: rgb(0x89b4fa),
                value_text: rgb(0xcdd6f4),
                placeholder_text: rgb(0x585b70),
                status_text: rgb(0x94e2d5),
                shortcut_text: rgb(0xf2cdcd),
            },
            palette: PaletteTheme {
                background: rgb(0x1e1e2e),
                border: rgb(0x313244),
                input_text: rgb(0xcdd6f4),
                item_background: rgb(0x1e1e2e),
                item_text: rgb(0xcdd6f4),
                item_selected_background: rgb(0x585b70),
                item_selected_text: rgb(0xf5c2e7),
                footer_text: rgb(0xa6adc8),
            },
            autocomplete: AutocompleteTheme {
                background: rgb(0x1e1e2e),
                border: rgb(0x313244),
                item_background: rgb(0x1e1e2e),
                item_selected_background: rgb(0x585b70),
                item_text: rgb(0xcdd6f4),
                item_selected_text: rgb(0xf5c2e7),
                label_text: rgb(0xf9e2af),
            },
            syntax: SyntaxTheme::catppuccin_mocha(),
        }
    }

    fn catppuccin_latte() -> Self {
        Self {
            editor: EditorTheme {
                background: rgb(0xeff1f5),
                border: rgb(0xe6e9ef),
                text: rgb(0x4c4f69),
                muted_text: rgb(0x6c6f85),
                cursor: rgb(0xdc8a78),
            },
            highlight: HighlightTheme {
                selection_bg: rgb(0xccd0da),
                selection_fg: rgb(0x1e1e2e),
                search_active_bg: rgb(0xfe640b),
                search_active_fg: rgb(0xeff1f5),
                search_match_bg: rgb(0x7287fd),
                search_match_fg: rgb(0xeff1f5),
            },
            panel: PanelTheme {
                background: rgb(0xedeff3),
                border: rgb(0xe6e9ef),
                active_row_background: rgb(0xe6e9ef),
                inactive_row_background: rgb(0xeff1f5),
                label_text: rgb(0x179299),
                value_text: rgb(0x4c4f69),
                placeholder_text: rgb(0x9ca0b0),
                status_text: rgb(0x209fb5),
                shortcut_text: rgb(0x7c7f93),
            },
            palette: PaletteTheme {
                background: rgb(0xeff1f5),
                border: rgb(0xe6e9ef),
                input_text: rgb(0x4c4f69),
                item_background: rgb(0xeff1f5),
                item_text: rgb(0x4c4f69),
                item_selected_background: rgb(0x209fb5),
                item_selected_text: rgb(0xe6e9ef),
                footer_text: rgb(0x6c6f85),
            },
            autocomplete: AutocompleteTheme {
                background: rgb(0xeff1f5),
                border: rgb(0xe6e9ef),
                item_background: rgb(0xeff1f5),
                item_selected_background: rgb(0x209fb5),
                item_text: rgb(0x4c4f69),
                item_selected_text: rgb(0xe6e9ef),
                label_text: rgb(0xfe640b),
            },
            syntax: SyntaxTheme::catppuccin_latte(),
        }
    }

    fn catppuccin_frappe() -> Self {
        Self {
            editor: EditorTheme {
                background: rgb(0x303446),
                border: rgb(0x414559),
                text: rgb(0xc6d0f5),
                muted_text: rgb(0xa5adce),
                cursor: rgb(0xf2d5cf),
            },
            highlight: HighlightTheme {
                selection_bg: rgb(0x51576d),
                selection_fg: rgb(0xc6d0f5),
                search_active_bg: rgb(0xef9f76),
                search_active_fg: rgb(0x303446),
                search_match_bg: rgb(0x81c8be),
                search_match_fg: rgb(0x303446),
            },
            panel: PanelTheme {
                background: rgb(0x292c3c),
                border: rgb(0x414559),
                active_row_background: rgb(0x414559),
                inactive_row_background: rgb(0x303446),
                label_text: rgb(0x8caaee),
                value_text: rgb(0xc6d0f5),
                placeholder_text: rgb(0x626880),
                status_text: rgb(0x99d1db),
                shortcut_text: rgb(0xeebebe),
            },
            palette: PaletteTheme {
                background: rgb(0x303446),
                border: rgb(0x414559),
                input_text: rgb(0xc6d0f5),
                item_background: rgb(0x303446),
                item_text: rgb(0xc6d0f5),
                item_selected_background: rgb(0x51576d),
                item_selected_text: rgb(0xf4b8e4),
                footer_text: rgb(0xa5adce),
            },
            autocomplete: AutocompleteTheme {
                background: rgb(0x303446),
                border: rgb(0x414559),
                item_background: rgb(0x303446),
                item_selected_background: rgb(0x51576d),
                item_text: rgb(0xc6d0f5),
                item_selected_text: rgb(0xf4b8e4),
                label_text: rgb(0xf2d5cf),
            },
            syntax: SyntaxTheme::catppuccin_frappe(),
        }
    }

    fn catppuccin_macchiato() -> Self {
        Self {
            editor: EditorTheme {
                background: rgb(0x24273a),
                border: rgb(0x363a4f),
                text: rgb(0xcad3f5),
                muted_text: rgb(0xa5adcb),
                cursor: rgb(0xf4dbd6),
            },
            highlight: HighlightTheme {
                selection_bg: rgb(0x494d64),
                selection_fg: rgb(0xcad3f5),
                search_active_bg: rgb(0xf5a97f),
                search_active_fg: rgb(0x24273a),
                search_match_bg: rgb(0x8bd5ca),
                search_match_fg: rgb(0x24273a),
            },
            panel: PanelTheme {
                background: rgb(0x1e2030),
                border: rgb(0x363a4f),
                active_row_background: rgb(0x363a4f),
                inactive_row_background: rgb(0x24273a),
                label_text: rgb(0x8aadf4),
                value_text: rgb(0xcad3f5),
                placeholder_text: rgb(0x5b6078),
                status_text: rgb(0x91d7e3),
                shortcut_text: rgb(0xf0c6c6),
            },
            palette: PaletteTheme {
                background: rgb(0x24273a),
                border: rgb(0x363a4f),
                input_text: rgb(0xcad3f5),
                item_background: rgb(0x24273a),
                item_text: rgb(0xcad3f5),
                item_selected_background: rgb(0x494d64),
                item_selected_text: rgb(0xf4c2c2),
                footer_text: rgb(0xa5adcb),
            },
            autocomplete: AutocompleteTheme {
                background: rgb(0x24273a),
                border: rgb(0x363a4f),
                item_background: rgb(0x24273a),
                item_selected_background: rgb(0x494d64),
                item_text: rgb(0xcad3f5),
                item_selected_text: rgb(0xf4c2c2),
                label_text: rgb(0xf4dea4),
            },
            syntax: SyntaxTheme::catppuccin_macchiato(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            editor: EditorTheme {
                background: rgb(0x2d2d2d),
                border: rgb(0x454545),
                text: rgb(0xd4d4d4),
                muted_text: rgb(0x808080),
                cursor: rgb(0xcccccc),
            },
            highlight: HighlightTheme {
                selection_bg: rgb(0x264F78),
                selection_fg: rgb(0xffffff),
                search_active_bg: rgb(0xF8C555),
                search_active_fg: rgb(0x1e1e1e),
                search_match_bg: rgb(0x3d315b),
                search_match_fg: rgb(0xffffff),
            },
            panel: PanelTheme {
                background: rgb(0x1f1f1f),
                border: rgb(0x454545),
                active_row_background: rgb(0x3a3a3a),
                inactive_row_background: rgb(0x2d2d2d),
                label_text: rgb(0x808080),
                value_text: rgb(0xffffff),
                placeholder_text: rgb(0x707070),
                status_text: rgb(0xb0b0b0),
                shortcut_text: rgb(0x808080),
            },
            palette: PaletteTheme {
                background: rgb(0x2d2d2d),
                border: rgb(0x454545),
                input_text: rgb(0xcccccc),
                item_background: rgb(0x2d2d2d),
                item_text: rgb(0xd4d4d4),
                item_selected_background: rgb(0x094771),
                item_selected_text: rgb(0xffffff),
                footer_text: rgb(0x808080),
            },
            autocomplete: AutocompleteTheme {
                background: rgb(0x2d2d2d),
                border: rgb(0x454545),
                item_background: rgb(0x2d2d2d),
                item_selected_background: rgb(0x094771),
                item_text: rgb(0xd4d4d4),
                item_selected_text: rgb(0xffffff),
                label_text: rgb(0x808080),
            },
            syntax: SyntaxTheme::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EditorTheme {
    pub background: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub muted_text: Rgba,
    pub cursor: Rgba,
}

#[derive(Clone, Debug)]
pub struct HighlightTheme {
    pub selection_bg: Rgba,
    pub selection_fg: Rgba,
    pub search_active_bg: Rgba,
    pub search_active_fg: Rgba,
    pub search_match_bg: Rgba,
    pub search_match_fg: Rgba,
}

#[derive(Clone, Debug)]
pub struct PanelTheme {
    pub background: Rgba,
    pub border: Rgba,
    pub active_row_background: Rgba,
    pub inactive_row_background: Rgba,
    pub label_text: Rgba,
    pub value_text: Rgba,
    pub placeholder_text: Rgba,
    pub status_text: Rgba,
    pub shortcut_text: Rgba,
}

#[derive(Clone, Debug)]
pub struct PaletteTheme {
    pub background: Rgba,
    pub border: Rgba,
    pub input_text: Rgba,
    pub item_background: Rgba,
    pub item_text: Rgba,
    pub item_selected_background: Rgba,
    pub item_selected_text: Rgba,
    pub footer_text: Rgba,
}

#[derive(Clone, Debug)]
pub struct AutocompleteTheme {
    pub background: Rgba,
    pub border: Rgba,
    pub item_background: Rgba,
    pub item_selected_background: Rgba,
    pub item_text: Rgba,
    pub item_selected_text: Rgba,
    pub label_text: Rgba,
}

#[derive(Clone, Debug)]
pub struct SyntaxTheme {
    heading: [Rgba; 6],
    pub bold: Rgba,
    pub italic: Rgba,
    pub underline: Rgba,
    pub strikethrough: Rgba,
    pub code: Rgba,
    pub link: Rgba,
    pub list: Rgba,
    pub checkbox_checked: Rgba,
    pub checkbox_unchecked: Rgba,
    pub blockquote: Rgba,
    pub code_block: Rgba,
    pub comment: Rgba,
    pub shortcode: Rgba,
    pub table: Rgba,
    pub hr: Rgba,
    pub hard_break: Rgba,
    pub escape: Rgba,
    pub normal: Rgba,
}

impl SyntaxTheme {
    pub fn heading_color(&self, level: usize) -> Rgba {
        let idx = level.saturating_sub(1).min(self.heading.len() - 1);
        self.heading[idx]
    }

    fn apply_override(&mut self, token: &str, color: Rgba) {
        match token {
            "heading" => self.heading.fill(color),
            "heading1" => self.heading[0] = color,
            "heading2" => self.heading[1] = color,
            "heading3" => self.heading[2] = color,
            "heading4" => self.heading[3] = color,
            "heading5" => self.heading[4] = color,
            "heading6" => self.heading[5] = color,
            "bold" => self.bold = color,
            "italic" => self.italic = color,
            "underline" => self.underline = color,
            "strikethrough" | "strike" => self.strikethrough = color,
            "code" => self.code = color,
            "link" => self.link = color,
            "list" => self.list = color,
            "checkbox-checked" | "checkbox_checked" => self.checkbox_checked = color,
            "checkbox-unchecked" | "checkbox_unchecked" => self.checkbox_unchecked = color,
            "blockquote" => self.blockquote = color,
            "code-block" | "code_block" => self.code_block = color,
            "comment" => self.comment = color,
            "shortcode" => self.shortcode = color,
            "table" => self.table = color,
            "hr" | "horizontal-rule" | "horizontal_rule" => self.hr = color,
            "hard-break" | "hard_break" | "break" => self.hard_break = color,
            "escape" => self.escape = color,
            "normal" => self.normal = color,
            _ => {}
        }
    }

    fn catppuccin_mocha() -> Self {
        Self {
            heading: [
                rgb(0x89dceb),
                rgb(0x8bd5ca),
                rgb(0x8aadf4),
                rgb(0xb4befe),
                rgb(0xf5c2e7),
                rgb(0xf38ba8),
            ],
            bold: rgb(0xf5a97f),
            italic: rgb(0xf2cdcd),
            underline: rgb(0x89b4fa),
            strikethrough: rgb(0x6c7086),
            code: rgb(0xfab387),
            link: rgb(0x89b4fa),
            list: rgb(0xb4befe),
            checkbox_checked: rgb(0xa6e3a1),
            checkbox_unchecked: rgb(0xf38ba8),
            blockquote: rgb(0x94e2d5),
            code_block: rgb(0xfab387),
            comment: rgb(0x6c7086),
            shortcode: rgb(0xcba6f7),
            table: rgb(0x9399b2),
            hr: rgb(0x6c7086),
            hard_break: rgb(0x6c7086),
            escape: rgb(0x6c7086),
            normal: rgb(0xcdd6f4),
        }
    }

    fn catppuccin_latte() -> Self {
        Self {
            heading: [
                rgb(0x179299),
                rgb(0x1e66f5),
                rgb(0x8839ef),
                rgb(0xd20f39),
                rgb(0xfe640b),
                rgb(0x7287fd),
            ],
            bold: rgb(0xdf8e1d),
            italic: rgb(0xdd7878),
            underline: rgb(0x1e66f5),
            strikethrough: rgb(0x9ca0b0),
            code: rgb(0xfe640b),
            link: rgb(0x1e66f5),
            list: rgb(0x8839ef),
            checkbox_checked: rgb(0x40a02b),
            checkbox_unchecked: rgb(0xd20f39),
            blockquote: rgb(0x209fb5),
            code_block: rgb(0xfe640b),
            comment: rgb(0x9ca0b0),
            shortcode: rgb(0x8839ef),
            table: rgb(0x7c7f93),
            hr: rgb(0x9ca0b0),
            hard_break: rgb(0x9ca0b0),
            escape: rgb(0x9ca0b0),
            normal: rgb(0x4c4f69),
        }
    }

    fn catppuccin_frappe() -> Self {
        Self {
            heading: [
                rgb(0x81c8be),
                rgb(0x99d1db),
                rgb(0x8caaee),
                rgb(0xbabbf1),
                rgb(0xeebebe),
                rgb(0xf4b8e4),
            ],
            bold: rgb(0xef9f76),
            italic: rgb(0xeebebe),
            underline: rgb(0x8caaee),
            strikethrough: rgb(0x737994),
            code: rgb(0xfab387),
            link: rgb(0x8caaee),
            list: rgb(0xbabbf1),
            checkbox_checked: rgb(0xa6d189),
            checkbox_unchecked: rgb(0xe78284),
            blockquote: rgb(0x81c8be),
            code_block: rgb(0xfab387),
            comment: rgb(0x737994),
            shortcode: rgb(0xca9ee6),
            table: rgb(0x949cbb),
            hr: rgb(0x737994),
            hard_break: rgb(0x737994),
            escape: rgb(0x737994),
            normal: rgb(0xc6d0f5),
        }
    }

    fn catppuccin_macchiato() -> Self {
        Self {
            heading: [
                rgb(0x8bd5ca),
                rgb(0x91d7e3),
                rgb(0x8aadf4),
                rgb(0xb7bdf8),
                rgb(0xf0c6c6),
                rgb(0xf4b8e4),
            ],
            bold: rgb(0xf4a8d1),
            italic: rgb(0xf0c6c6),
            underline: rgb(0x8aadf4),
            strikethrough: rgb(0x6e738d),
            code: rgb(0xf5a97f),
            link: rgb(0x8aadf4),
            list: rgb(0xb7bdf8),
            checkbox_checked: rgb(0x8bd5ca),
            checkbox_unchecked: rgb(0xf0c6c6),
            blockquote: rgb(0x91d7e3),
            code_block: rgb(0xf5a97f),
            comment: rgb(0x6e738d),
            shortcode: rgb(0xc6a0f6),
            table: rgb(0x939ab7),
            hr: rgb(0x6e738d),
            hard_break: rgb(0x6e738d),
            escape: rgb(0x6e738d),
            normal: rgb(0xcad3f5),
        }
    }
}

impl Default for SyntaxTheme {
    fn default() -> Self {
        Self {
            heading: [
                rgb(0x569CD6),
                rgb(0x4EC9B0),
                rgb(0x4FC1FF),
                rgb(0x4FC1FF),
                rgb(0x4FC1FF),
                rgb(0x4FC1FF),
            ],
            bold: rgb(0xDCDCAA),
            italic: rgb(0xCE9178),
            underline: rgb(0x9CDCFE),
            strikethrough: rgb(0x808080),
            code: rgb(0xD16969),
            link: rgb(0x9CDCFE),
            list: rgb(0xC586C0),
            checkbox_checked: rgb(0x7CB342),
            checkbox_unchecked: rgb(0xF48771),
            blockquote: rgb(0x6A9955),
            code_block: rgb(0xD16969),
            comment: rgb(0x6A9955),
            shortcode: rgb(0xC586C0),
            table: rgb(0x808080),
            hr: rgb(0x808080),
            hard_break: rgb(0x808080),
            escape: rgb(0x808080),
            normal: rgb(0xD4D4D4),
        }
    }
}

fn parse_entries(contents: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        let (key, value) = match trimmed.split_once('=') {
            Some(pair) => pair,
            None => match trimmed.split_once(':') {
                Some(pair) => pair,
                None => continue,
            },
        };

        let key = key.trim().to_ascii_lowercase();
        let mut value = value.trim().to_string();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = value[1..value.len() - 1].to_string();
        } else if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
            value = value[1..value.len() - 1].to_string();
        }

        entries.push((key, value));
    }

    entries
}

fn parse_color(value: &str) -> Option<Rgba> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let hex = if let Some(rest) = trimmed.strip_prefix('#') {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("0x") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("0X") {
        rest
    } else {
        trimmed
    };

    if hex.len() != 6 {
        return None;
    }

    u32::from_str_radix(hex, 16).ok().map(rgb)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOMEDRIVE").and_then(|drive| {
                std::env::var_os("HOMEPATH").map(|path| Path::new(&drive).join(path))
            })
        })
}

fn ensure_default_file(path: &Path) {
    if path.exists() {
        return;
    }

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            eprintln!(
                "MedleyText: failed to create config directory {}: {}",
                parent.display(),
                err
            );
            return;
        }
    }

    if let Err(err) = fs::write(path, EditorConfig::DEFAULT_FILE_TEMPLATE) {
        eprintln!(
            "MedleyText: failed to write default config {}: {}",
            path.display(),
            err
        );
    }
}
