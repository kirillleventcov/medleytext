# MedleyText

A lightweight **Brief-first** text editor built from scratch with [GPUI](https://crates.io/crates/gpui). Native syntax highlighting for [Brief](https://github.com/kirillleventcov/brief) (`.brf`) and Markdown (`.md`) in the same buffer. Brief docs: [docs.brief.kirillleventcov.com](https://docs.brief.kirillleventcov.com/).

## Why Brief-first

Brief is a strict markup language where every construct has one canonical spelling — `*bold*` (not `**bold**`), `-` bullets only (not `*` or `+`), two-space indentation, single-marker emphasis, and a single extension point via `@shortcodes`. It is designed to be human-readable and token-economic for LLMs.

MedleyText treats Brief as the default language for any unknown or `.brf`-extension buffer; Markdown stays supported for `.md` / `.markdown` files so existing notes keep working without conversion.

## Features

- **Brief syntax highlighting** — headings, single-marker emphasis (`*bold*`, `_italic_`, `+underline+`, `~strike~`), inline code, shortcodes (`@link`, `@callout`, `@image`, …), block directives (`@t`, `@dl`, `@end`), task markers, line / block comments, horizontal rules, tables.
- **Markdown syntax highlighting** retained for `.md` files (CommonMark-style `**bold**`, `*italic*`, link / list / checkbox / blockquote / code-block, etc.).
- **Fuzzy file finder** (Ctrl+P / Ctrl+O) — surfaces `.brf` files first, then `.md`, scored across the working directory.
- **Smart list continuation** that respects each language's rules (Brief: `-` only; Markdown: `-` / `*` / `+`).
- **Shortcode autocomplete** triggered by `@` inside `.brf` buffers (`@link`, `@kbd`, `@math`, `@callout`, `@details`, `@t`, `@dl`, …).
- Keyboard-driven workflow, GPU-accelerated rendering, configurable themes (Default + the four Catppuccin flavors), undo / redo, drag-select, word wrap, find / replace, go-to-line.

## Usage

```bash
medleytext              # opens an empty Brief buffer
medleytext notes.brf    # Brief
medleytext notes.md     # Markdown (highlighter switches automatically)
```

**Keybindings**

- `Ctrl+O` / `Ctrl+P` — open fuzzy file finder
- `Ctrl+S` — save
- `Ctrl+Q` — quit
- `Ctrl+A` — select all
- `Ctrl+C` / `Ctrl+V` / `Ctrl+X` — copy / paste / cut
- `Ctrl+Z` / `Ctrl+Shift+Z` (or `Ctrl+Y`) — undo / redo
- `Ctrl+F` — find / replace
- `Ctrl+G` — go to line
- `F3` / `Shift+F3` — next / previous match
- `Delete` — forward delete
- `Tab` — indent (2 spaces, or indent selected lines — Brief requires 2-space indent)
- `Shift+Tab` — unindent selected lines
- Arrow keys — navigate (Shift to select)
- Mouse drag — select text

**Fuzzy file finder**

- Type to filter `.brf` and `.md` files with fuzzy matching
- `↑` / `↓` — navigate results
- `Enter` — open
- `Esc` — close

## Building

```bash
git clone https://github.com/kirillleventcov/medleytext.git
cd medleytext
cargo build --release
./target/release/medleytext demo.brf
```

The Brief compiler is pulled from crates.io as [`brief-core`](https://crates.io/crates/brief-core).

### macOS notes

Follow [Zed's macOS build guide](https://github.com/zed-industries/zed/blob/main/docs/src/development/macos.md) for system dependencies.

## Configuration

MedleyText reads optional settings from `~/.config/medleytext/config`. Each non-empty line uses either `key=value` or `key: value`. Lines beginning with `#` or `//` are ignored. The file is created automatically with sensible defaults on first launch.

### Core options

- `font-size` — UI font size (clamped between 8 and 72, default `14`)

### Theme presets

Use a preset as the base palette, then override individual keys as needed:

```
theme.preset = catppuccin-mocha
```

Available presets: `default`, `catppuccin-mocha`, `catppuccin-macchiato`, `catppuccin-frappe`, `catppuccin-latte`.

### Color overrides

Color values accept `#RRGGBB` or `0xRRGGBB`. Any unspecified key falls back to the current preset / default.

- **Editor surface** — `theme.editor.{background,border,text,muted-text,cursor}`
- **Highlights** — `theme.highlight.selection.{background,foreground}`, `theme.highlight.search-active.{background,foreground}`, `theme.highlight.search-match.{background,foreground}`
- **Panels (find dialog)** — `theme.panel.{background,border,active-row.background,inactive-row.background,label-text,value-text,placeholder-text,status-text,shortcut-text}`
- **Command palette** — `theme.palette.{background,border,input-text,item.background,item.foreground,item-selected.background,item-selected.foreground,footer-text}`
- **Autocomplete menu** — `theme.autocomplete.{background,border,item.background,item.foreground,item-selected.background,item-selected.foreground,label-text}`
- **Syntax** — `theme.syntax.{heading1…heading6,bold,italic,underline,strikethrough,code,code-block,link,list,checkbox-checked,checkbox-unchecked,blockquote,comment,shortcode,table,hr,hard-break,escape,normal}`

The `underline`, `strikethrough`, `comment`, `shortcode`, `table`, `hr`, `hard-break`, and `escape` keys are exposed for Brief; they're inert in Markdown documents (no token of that kind is produced) but you can still set them safely.

### Example: Catppuccin Mocha + tweaks

```
font-size = 16
theme.preset = catppuccin-mocha

// Personal accents
theme.highlight.selection.background = #F5C2E7
theme.highlight.selection.foreground = #1E1E2E
theme.syntax.shortcode = #cba6f7
theme.syntax.comment   = #6c7086
```

Restart MedleyText (or close / open the window) after editing the config file to load new values.

## Brief cheatsheet

```brief
// line comment

# Heading 1 .. ###### Heading 6 (exactly one space, max 6)

A paragraph with *bold*, _italic_, +underline+, ~strike~ words.\
A trailing backslash forces a hard break inside the paragraph.

- bullet (only `-` is valid)
- [x] done task
- [ ] todo task
1. ordered list (must start at 1, sequential)
2. ordered list

> blockquote
>> nested

```rust
fn main() { println!("hi"); }
```

---  (horizontal rule)

@t
| Name | Age | City
| Ada  | 36  | London

@callout(kind: note)
Block shortcodes open with @name(args) and close with @end.
@end

See @link[the spec](https://docs.brief.kirillleventcov.com/) and press @kbd[Ctrl+S].
```

The complete grammar lives in `LearnXinYminutes.brf` in the upstream Brief repo.

## Documentation

Built with [GPUI](https://docs.rs/gpui/latest/gpui/), a GPU-accelerated UI framework for Rust. Brief compiler lives at [github.com/kirillleventcov/brief](https://github.com/kirillleventcov/brief) — see [`LearnXinYminutes.brf`](https://github.com/kirillleventcov/brief/blob/main/LearnXinYminutes.brf) for the full grammar in one file.
