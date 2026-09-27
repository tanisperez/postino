//! Postino, the desktop HTTP client. Binary entry point: initializes `gpui`, the theme, the main
//! window, and opens the workspace given on the command line or remembered from the last run
//! (`plans/mvp.md`, phase 8).

mod actions;
mod state;
mod theme;
mod views;

use std::path::PathBuf;

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

use actions::{
    OpenSettings, SaveActiveTab, SelectEnvironment1, SelectEnvironment2, SelectEnvironment3,
    SelectEnvironment4, SelectEnvironment5, SelectEnvironment6, SelectEnvironment7,
    SelectEnvironment8, SelectEnvironment9, SelectNoEnvironment, SendActiveTab,
};
use state::settings::ThemeChoice;
use views::AppView;

fn main() {
    // The workspace folder to open on startup: the first CLI argument if given (handy for
    // testing), otherwise the folder remembered from the previous run.
    let initial_workspace: Option<PathBuf> = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(state::config::load_last_workspace);

    // Settings are not wired to a UI yet (Phase 6), but startup already honors whatever is on
    // disk (or the documented defaults: System theme, Geist 13px, Geist Mono 12.5px).
    let settings = state::settings::load_settings();
    let theme_choice = settings.theme;

    // `with_assets` registers the bundled icon SVGs. Without it every `Icon` (tree chevrons,
    // window controls, checkboxes) renders as empty space. `gpui_kit::assets::Assets` only
    // embeds the small curated subset generated from `default-icons.txt`
    // (`gpui-kit-assets-0.6.6/src/native_assets.rs`); `AllAssets` embeds the complete Lucide
    // catalog (`gpui_kit::assets::IconName`, 1830 icons) that `plans/ui-redesign.md`'s design
    // uses (`gauge`, `wand-sparkles`, `send-horizontal`, ...), a strict superset of the curated
    // one, so switching to it does not affect any icon that already worked.
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
            theme::install(cx, &settings);

            // Centered at 1440x900 logical, capped to 90% of the display's visible bounds
            // (`plans/ui-redesign.md` phase 5, reviewer fix item 10): `WindowBounds::centered`
            // only caps at 100% of the display (`Bounds::centered`'s own `size.min(&visible_
            // bounds.size)`), which is not tight enough to keep the title bar, status bar and
            // sidebar footer on screen on a display where 1440x900 is close to the full visible
            // area.
            //
            // `cx.primary_display()` (and `cx.displays()`) can legitimately return nothing here:
            // confirmed with temporary logging on this session's KDE Plasma/Wayland setup, where
            // both are empty at this point in `.run()`'s callback, and even `window.display(cx)`
            // right after `open_window` still returns `None` (the compositor hands over
            // output/display info only after the window's first configure event, which no
            // synchronous call at window-creation time can observe). Without this fallback,
            // `window_bounds: None` used to defer to `gpui`'s own default placement, which on
            // that same session came out as 1536x1095, taller than the visible screen (a 1920x
            // 1080 physical display at 1.2 scale, with a 54 px physical taskbar, leaves about
            // 1600x855 logical), hiding the status bar and sidebar footer. 1280x760 comfortably
            // fits that, and any other Wayland compositor's typical visible area.
            let window_bounds = match cx.primary_display() {
                Some(display) => {
                    let visible = display.visible_bounds();
                    let cap = size(visible.size.width * 0.9, visible.size.height * 0.9);
                    let target = size(px(1440.0), px(900.0)).min(&cap);
                    WindowBounds::Windowed(Bounds::centered_at(visible.center(), target))
                }
                None => WindowBounds::Windowed(Bounds {
                    origin: point(px(0.0), px(0.0)),
                    size: size(px(1280.0), px(760.0)),
                }),
            };

            let window_options = WindowOptions {
                window_bounds: Some(window_bounds),
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
                    // "System" keeps matching the OS light/dark setting, including if the user
                    // changes it while Postino is running; an explicit choice sets the mode once.
                    match theme_choice {
                        ThemeChoice::System => Theme::sync_system_appearance(Some(window), cx),
                        ThemeChoice::Light => Theme::change(ThemeMode::Light, Some(window), cx),
                        ThemeChoice::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
                    }

                    let view =
                        cx.new(|cx| AppView::new(initial_workspace.clone(), settings, window, cx));

                    // Always listen for OS appearance changes, live, regardless of the theme
                    // choice at startup: the Settings view (`plans/ui-redesign.md` phase 6) can
                    // switch the choice at any time afterwards, and "System" must start following
                    // the OS the moment it is picked, even if the app launched in Light or Dark.
                    // Checking the *current* setting on every OS change (instead of only
                    // attaching this observer when `theme_choice == System`) is what lets Light
                    // and Dark stop following it without detaching anything: the observer simply
                    // no-ops while they are active.
                    let weak = view.downgrade();
                    window
                        .observe_window_appearance(move |window, cx| {
                            let follows_system = weak.upgrade().is_some_and(|view| {
                                view.read(cx).state.settings.theme == ThemeChoice::System
                            });
                            if follows_system {
                                Theme::sync_system_appearance(Some(window), cx);
                            }
                        })
                        .detach();

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
        KeyBinding::new("cmd-1", SelectEnvironment1, None),
        KeyBinding::new("cmd-2", SelectEnvironment2, None),
        KeyBinding::new("cmd-3", SelectEnvironment3, None),
        KeyBinding::new("cmd-4", SelectEnvironment4, None),
        KeyBinding::new("cmd-5", SelectEnvironment5, None),
        KeyBinding::new("cmd-6", SelectEnvironment6, None),
        KeyBinding::new("cmd-7", SelectEnvironment7, None),
        KeyBinding::new("cmd-8", SelectEnvironment8, None),
        KeyBinding::new("cmd-9", SelectEnvironment9, None),
        KeyBinding::new("cmd-0", SelectNoEnvironment, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-s", SaveActiveTab, None),
        KeyBinding::new("ctrl-enter", SendActiveTab, None),
        KeyBinding::new("ctrl-1", SelectEnvironment1, None),
        KeyBinding::new("ctrl-2", SelectEnvironment2, None),
        KeyBinding::new("ctrl-3", SelectEnvironment3, None),
        KeyBinding::new("ctrl-4", SelectEnvironment4, None),
        KeyBinding::new("ctrl-5", SelectEnvironment5, None),
        KeyBinding::new("ctrl-6", SelectEnvironment6, None),
        KeyBinding::new("ctrl-7", SelectEnvironment7, None),
        KeyBinding::new("ctrl-8", SelectEnvironment8, None),
        KeyBinding::new("ctrl-9", SelectEnvironment9, None),
        KeyBinding::new("ctrl-0", SelectNoEnvironment, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
    ]);
}
