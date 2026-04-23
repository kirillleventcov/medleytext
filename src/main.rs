//! MedleyText - A markdown-first text editor built with GPUI.
//!
//! This is the main entry point for the application. It handles initialization,
//! key binding configuration, and window creation.

mod autocomplete;
mod config;
mod editor;
mod find;
mod markdown;
mod palette;

use config::EditorConfig;
use editor::TextEditor;
use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, WindowBounds, WindowOptions, px, size,
};

/// Application entry point.
///
/// Accepts an optional file path as the first command-line argument.
/// If provided, the file will be loaded into the editor on startup.
/// If the file doesn't exist, a new empty buffer with that filename is created.
///
/// # Examples
///
/// ```bash
/// # Open existing file
/// medleytext document.md
///
/// # Start with empty buffer
/// medleytext
/// ```
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file_path = args.get(1).cloned();
    let editor_config = EditorConfig::load();

    Application::new().run(move |cx: &mut App| {
        use editor::{
            Backspace, Copy, Cut, Delete, Enter, FindNext, FindPrevious, MoveDown,
            MoveEnd, MoveHome, MoveLeft, MoveRight, MoveUp, MoveWordLeft, MoveWordRight, Paste,
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
            KeyBinding::new("ctrl-f", ToggleFind, None),
            KeyBinding::new("ctrl-g", ToggleGoToLine, None),
            KeyBinding::new("f3", FindNext, None),
            KeyBinding::new("shift-f3", FindPrevious, None),
            KeyBinding::new("ctrl-z", Undo, None),
            KeyBinding::new("ctrl-shift-z", Redo, None),
            KeyBinding::new("ctrl-y", Redo, None),
        ]);

        // Create a resizable window centered on screen with initial size of 800x600.
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        let file_path_clone = file_path.clone();
        let config_for_window = editor_config.clone();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                is_movable: true,
                ..Default::default()
            },
            |_window, cx| {
                cx.new(|cx| TextEditor::with_file(file_path_clone, config_for_window, cx))
            },
        )
        .unwrap();
    });
}
