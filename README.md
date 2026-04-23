# MedleyText

A lightweight markdown-first text editor built from scratch with [GPUI](https://crates.io/crates/gpui).

## Features

- **Fuzzy File Finder** - Quick-open palette (Ctrl+P) for instant navigation across markdown files
- Markdown syntax highlighting (headings, bold, italic, code, links, lists, checkboxes, blockquotes)
- Color-coded checkbox states (complete/incomplete)
- Minimal interface focused on writing
- Keyboard-driven workflow
- Zero external dependencies (except GPUI)

## Usage

```bash
medleytext demo.md
```

**Keybindings:**

- `Ctrl+O` / `Ctrl+P` - Open fuzzy file finder
- `Ctrl+S` - Save
- `Ctrl+Q` - Quit
- `Ctrl+A` - Select all
- `Ctrl+C/V/X` - Copy/Paste/Cut
- `Ctrl+G` - Go to line
- `Ctrl+F` - Find
- `Delete` - Forward delete
- `Tab` - Indent (2 spaces, or indent selected lines)
- `Shift+Tab` - Unindent selected lines
- Arrow keys - Navigate (Shift to select)
- Mouse drag - Select text
- Standard typing and editing

**Fuzzy File Finder:**

- Type to search files with fuzzy matching
- `↑/↓` - Navigate results
- `Enter` - Open selected file
- `Esc` - Close palette

## Building

### MacOS Notes

Follow [Zed's Guide on MacOS building](https://github.com/zed-industries/zed/blob/main/docs/src/development/macos.md) to install the necessary dependencies.

```bash
cargo build --release
```

## Configuration

MedleyText reads optional settings from `~/.config/medleytext/config`. Each non-empty line uses either `key=value` or `key: value`. Lines beginning with `#` or `//` are ignored.

The file is created automatically with sensible defaults the first time you launch MedleyText, so you can open it and tweak values right away.

### Core options

- `font-size` &mdash; UI font size (clamped between 8 and 72, default `14`)

### Theme presets

Use a preset as the base palette, then override individual keys as needed:

```
theme.preset = catppuccin-mocha
```

Available presets: `default`, `catppuccin-mocha`, `catppuccin-macchiato`, `catppuccin-frappe`, `catppuccin-latte`.

### Color overrides

Color values accept `#RRGGBB` or `0xRRGGBB`. Any unspecified key falls back to the current preset/default.

- **Editor surface**
  - `theme.editor.background`
  - `theme.editor.border`
  - `theme.editor.text`
  - `theme.editor.muted-text`
  - `theme.editor.cursor`
- **Highlights**
  - `theme.highlight.selection.background`
  - `theme.highlight.selection.foreground`
  - `theme.highlight.search-active.background`
  - `theme.highlight.search-active.foreground`
  - `theme.highlight.search-match.background`
  - `theme.highlight.search-match.foreground`
- **Panels (find dialog)**
  - `theme.panel.background`
  - `theme.panel.border`
  - `theme.panel.active-row.background`
  - `theme.panel.inactive-row.background`
  - `theme.panel.label-text`
  - `theme.panel.value-text`
  - `theme.panel.placeholder-text`
  - `theme.panel.status-text`
  - `theme.panel.shortcut-text`
- **Command palette**
  - `theme.palette.background`
  - `theme.palette.border`
  - `theme.palette.input-text`
  - `theme.palette.item.background`
  - `theme.palette.item.foreground`
  - `theme.palette.item-selected.background`
  - `theme.palette.item-selected.foreground`
  - `theme.palette.footer-text`
- **Autocomplete menu**
  - `theme.autocomplete.background`
  - `theme.autocomplete.border`
  - `theme.autocomplete.item.background`
  - `theme.autocomplete.item.foreground`
  - `theme.autocomplete.item-selected.background`
  - `theme.autocomplete.item-selected.foreground`
  - `theme.autocomplete.label-text`
- **Markdown syntax**
  - `theme.syntax.heading1` &hellip; `theme.syntax.heading6`
  - `theme.syntax.bold`
  - `theme.syntax.italic`
  - `theme.syntax.code`
  - `theme.syntax.code-block`
  - `theme.syntax.link`
  - `theme.syntax.list`
  - `theme.syntax.checkbox-checked`
  - `theme.syntax.checkbox-unchecked`
  - `theme.syntax.blockquote`
  - `theme.syntax.normal`

### Example: Catppuccin Mocha + tweaks

```
font-size = 16
theme.preset = catppuccin-mocha

# Personal accents
theme.highlight.selection.background = #F5C2E7
theme.highlight.selection.foreground = #1E1E2E
theme.palette.item-selected.background = #B4BEFE
theme.palette.item-selected.foreground = #1E1E2E
```

Restart MedleyText (or close/open the window) after editing the config file to load new values.

## Documentation

Built with [GPUI](https://docs.rs/gpui/latest/gpui/), a GPU-accelerated UI framework for Rust.
