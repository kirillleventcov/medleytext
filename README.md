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

- `Ctrl+P` - Open fuzzy file finder
- `Ctrl+S` - Save
- `Ctrl+Q` - Quit
- `Ctrl+A` - Select all
- `Ctrl+C/V/X` - Copy/Paste/Cut
- Arrow keys - Navigate (Shift to select)
- Standard typing and editing

**Fuzzy File Finder:**

- Type to search files with fuzzy matching
- `↑/↓` - Navigate results
- `Enter` - Open selected file
- `Esc` - Close palette

## Documentation

Built with [GPUI](https://docs.rs/gpui/latest/gpui/), a GPU-accelerated UI framework for Rust.
