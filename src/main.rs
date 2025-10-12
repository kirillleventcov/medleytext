//! MedleyText - A markdown-first text editor built with GPUI.
//!
//! This is the main entry point for the application. It handles initialization,
//! key binding configuration, and window creation.

mod editor;
mod markdown;

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

    Application::new().run(move |cx: &mut App| {
        use editor::{
            Backspace, Copy, Cut, Enter, MoveDown, MoveLeft, MoveRight, MoveUp, Paste, Quit, Save,
            SelectAll, SelectDown, SelectLeft, SelectRight, SelectUp,
        };

        // Configure global keybindings for the application.
        // These bindings are active whenever the TextEditor has focus.
        // Uses standard editor conventions (arrow keys, Ctrl+S, etc.)
        cx.bind_keys([
            KeyBinding::new("left", MoveLeft, None),
            KeyBinding::new("right", MoveRight, None),
            KeyBinding::new("up", MoveUp, None),
            KeyBinding::new("down", MoveDown, None),
            KeyBinding::new("backspace", Backspace, None),
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
        ]);

        // Create a centered window with fixed dimensions (800x600).
        // Consider making window size configurable via config file in future iterations.
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        let file_path_clone = file_path.clone();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| TextEditor::with_file(file_path_clone, cx)),
        )
        .unwrap();
    });
}
