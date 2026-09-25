//! Postino, the desktop HTTP client. Binary entry point: initializes `gpui`, the theme, the main
//! window, and opens the workspace given on the command line or remembered from the last run
//! (`plans/mvp.md`, phase 8).

mod actions;
mod state;
mod views;

use std::path::PathBuf;

use gpui_kit::component::{Root, TitleBar, theme::Theme};
use gpui_kit::*;

use actions::{SaveActiveTab, SendActiveTab};
use views::AppView;

fn main() {
    // The workspace folder to open on startup: the first CLI argument if given (handy for
    // testing), otherwise the folder remembered from the previous run.
    let initial_workspace: Option<PathBuf> = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(state::config::load_last_workspace);

    // `with_assets` registers the bundled icon SVGs. Without it every `Icon` (tree chevrons,
    // window controls, checkboxes) renders as empty space.
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            bind_keys(cx);

            let window_options = WindowOptions {
                window_min_size: Some(size(px(760.), px(480.))),
                // On Linux the default is server side decorations, which makes the compositor draw
                // its own title bar on top of ours. Client side decorations let `TitleBar` be the
                // only one; `Root` then draws the window border and resize edges. Other platforms
                // ignore this option.
                window_decorations: Some(WindowDecorations::Client),
                ..TitleBar::window_options()
            };

            cx.spawn(async move |cx| {
                let opened = cx.open_window(window_options, move |window, cx| {
                    // Match the OS light/dark setting on open, and keep matching it if the user
                    // changes it while Postino is running.
                    Theme::sync_system_appearance(Some(window), cx);
                    window
                        .observe_window_appearance(|window, cx| {
                            Theme::sync_system_appearance(Some(window), cx);
                        })
                        .detach();

                    let view = cx.new(|cx| AppView::new(initial_workspace.clone(), cx));
                    cx.new(|cx| Root::new(view, window, cx))
                });
                // Opening the very first window failing is not recoverable: there is nothing left
                // for the app to do, so this is one of the "truly impossible state" exceptions
                // AGENTS.md allows an `expect()` for.
                #[allow(clippy::expect_used)]
                opened.expect("failed to open the main window");
            })
            .detach();
        });
}

/// Registers the app's key bindings. `None` as the context means the binding applies anywhere in
/// the window, not just inside a specific focused control.
fn bind_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-s", SaveActiveTab, None),
        KeyBinding::new("cmd-enter", SendActiveTab, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-s", SaveActiveTab, None),
        KeyBinding::new("ctrl-enter", SendActiveTab, None),
    ]);
}
