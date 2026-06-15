mod app;
mod core;
mod ui;

use gpui::*;
use crate::app::VelocisApp;
use crate::core::state::CommandPalette;
use crate::core::state::GoHome;
use crate::core::state::NewProject;
use crate::core::state::SaveProject;
use std::borrow::Cow;

fn main() {
    Application::new().run(|cx: &mut App| {
        let font_data = include_bytes!("../fonts/Lexend-VariableFont_wght.ttf");
        if let Err(e) = cx.text_system().add_fonts(vec![Cow::Borrowed(&font_data[..])]) {
            eprintln!("font load error: {e}");
        }
        cx.activate(true);

        let bounds = Bounds::centered(None, size(px(1600.0), px(900.0)), cx);
        let Ok(handle) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Maximized(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| VelocisApp::new()),
        ) else {
            eprintln!("failed to open window");
            return;
        };

        cx.on_action({
            let h = handle;
            move |_: &CommandPalette, cx: &mut App| {
                if let Err(e) = h.update(cx, |app, _window, cx| {
                    app.toggle_command_palette();
                    cx.notify();
                }) {
                    eprintln!("CommandPalette error: {e}");
                }
            }
        });

        cx.on_action({
            let h = handle;
            move |_: &GoHome, cx: &mut App| {
                if let Err(e) = h.update(cx, |app, _window, cx| {
                    app.go_home();
                    cx.notify();
                }) {
                    eprintln!("GoHome error: {e}");
                }
            }
        });

        cx.on_action({
            let h = handle;
            move |_: &NewProject, cx: &mut App| {
                if let Err(e) = h.update(cx, |app, _window, cx| {
                    app.start_new_project();
                    cx.notify();
                }) {
                    eprintln!("NewProject error: {e}");
                }
            }
        });

        cx.on_action({
            let h = handle;
            move |_: &SaveProject, cx: &mut App| {
                if let Err(e) = h.update(cx, |app, _window, cx| {
                    app.save_current_project();
                    cx.notify();
                }) {
                    eprintln!("SaveProject error: {e}");
                }
            }
        });

        cx.bind_keys([
            KeyBinding::new("cmd-k", CommandPalette, None),
            KeyBinding::new("ctrl-k", CommandPalette, None),
            KeyBinding::new("cmd-shift-h", GoHome, None),
            KeyBinding::new("ctrl-shift-h", GoHome, None),
            KeyBinding::new("cmd-n", NewProject, None),
            KeyBinding::new("ctrl-n", NewProject, None),
            KeyBinding::new("cmd-s", SaveProject, None),
            KeyBinding::new("ctrl-s", SaveProject, None),
        ]);
    });
}
