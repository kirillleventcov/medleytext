//! MedleyText — a Brief-first text editor built with GPUI.
//!
//! Brief (`.brf`) is the default markup language. Markdown (`.md`) is still
//! supported with its own highlighter so existing notes keep working.

mod autocomplete;
mod brief;
mod config;
mod editor;
mod find;
mod markdown;
mod palette;

use std::path::PathBuf;

use config::EditorConfig;
use editor::open_editor_window;
use gpui::{App, Application, KeyBinding};

/// Application entry point.
///
/// CLI argument handling, mirroring Notepad++-style launch semantics:
///
/// - `medleytext`                  → empty draft buffer, no working folder.
/// - `medleytext path/to/file.brf` → loads that file (creates a buffer for a
///                                   missing path; nothing is written to disk
///                                   until the user saves).
/// - `medleytext path/to/folder/`  → opens a draft buffer scoped to that
///                                   folder for Ctrl+P fuzzy find.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (initial_file, initial_dir) = parse_cli_arg(args.get(1).map(String::as_str));
    let editor_config = EditorConfig::load();

    Application::new().run(move |cx: &mut App| {
        use editor::{
            Backspace, Copy, Cut, Delete, Enter, FindNext, FindPrevious, MoveDown, MoveEnd,
            MoveHome, MoveLeft, MoveRight, MoveUp, MoveWordLeft, MoveWordRight, OpenFolder, Paste,
            Quit, Redo, Save, SelectAll, SelectDown, SelectLeft, SelectRight, SelectUp, ShiftTab,
            Tab, ToggleFind, ToggleGoToLine, TogglePalette, Undo,
        };

        // Configure global keybindings for the application.
        // These bindings are active whenever the TextEditor has focus.
        // Uses standard editor conventions (arrow keys, Ctrl+S, etc.)
        cx.bind_keys([
            KeyBinding::new("left", MoveLeft, None),
            KeyBinding::new("right", MoveRight, None),
            KeyBinding::new("up", MoveUp, None),
            KeyBinding::new("down", MoveDown, None),
            KeyBinding::new("ctrl-left", MoveWordLeft, None),
            KeyBinding::new("ctrl-right", MoveWordRight, None),
            KeyBinding::new("home", MoveHome, None),
            KeyBinding::new("end", MoveEnd, None),
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("delete", Delete, None),
            KeyBinding::new("tab", Tab, None),
            KeyBinding::new("shift-tab", ShiftTab, None),
            KeyBinding::new("enter", Enter, None),
            KeyBinding::new("ctrl-s", Save, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-c", Copy, None),
            KeyBinding::new("ctrl-v", Paste, None),
            KeyBinding::new("ctrl-x", Cut, None),
            KeyBinding::new("shift-left", SelectLeft, None),
            KeyBinding::new("shift-right", SelectRight, None),
            KeyBinding::new("shift-up", SelectUp, None),
            KeyBinding::new("shift-down", SelectDown, None),
            KeyBinding::new("ctrl-a", SelectAll, None),
            KeyBinding::new("ctrl-p", TogglePalette, None),
            KeyBinding::new("ctrl-o", TogglePalette, None),
            KeyBinding::new("ctrl-shift-o", OpenFolder, None),
            KeyBinding::new("ctrl-f", ToggleFind, None),
            KeyBinding::new("ctrl-g", ToggleGoToLine, None),
            KeyBinding::new("f3", FindNext, None),
            KeyBinding::new("shift-f3", FindPrevious, None),
            KeyBinding::new("ctrl-z", Undo, None),
            KeyBinding::new("ctrl-shift-z", Redo, None),
            KeyBinding::new("ctrl-y", Redo, None),
        ]);

        open_editor_window(
            initial_file.clone(),
            initial_dir.clone(),
            editor_config.clone(),
            cx,
        );
    });
}

/// Interprets the optional first CLI argument as either a file or a folder.
///
/// A path pointing at an existing directory becomes the window's
/// `working_dir`; anything else (existing file, missing file, or no arg) is
/// treated as a file path to load. The "missing path" branch matches
/// Notepad++ behavior where a user can pre-name a file before saving.
fn parse_cli_arg(arg: Option<&str>) -> (Option<String>, Option<PathBuf>) {
    let Some(arg) = arg else {
        return (None, None);
    };
    let path = PathBuf::from(arg);
    if path.is_dir() {
        (None, Some(path))
    } else {
        (Some(arg.to_string()), None)
    }
}
