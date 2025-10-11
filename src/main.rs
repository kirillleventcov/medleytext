mod editor;
mod markdown;

use editor::TextEditor;
use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, WindowBounds, WindowOptions, px, size,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file_path = args.get(1).cloned();

    Application::new().run(move |cx: &mut App| {
        use editor::{Backspace, Enter, MoveDown, MoveLeft, MoveRight, MoveUp, Quit, Save};

        cx.bind_keys([
            KeyBinding::new("left", MoveLeft, None),
            KeyBinding::new("right", MoveRight, None),
            KeyBinding::new("up", MoveUp, None),
            KeyBinding::new("down", MoveDown, None),
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("enter", Enter, None),
            KeyBinding::new("ctrl-s", Save, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);

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
