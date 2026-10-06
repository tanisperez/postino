//! Postino, the desktop HTTP client. Binary entry point: initializes `gpui`, the theme, the main
//! window, and opens the workspace given on the command line or remembered from the last run.

// A GUI application on Windows: never open a console window behind the main window.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod actions;
mod logging;
mod state;
mod theme;
mod views;

rust_i18n::i18n!("locales", fallback = "en");

use std::path::{Path, PathBuf};

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

use actions::{
    CloseActiveTab, NextTab, OpenCommandPalette, OpenSettings, OpenShortcuts, PreviousTab,
    SaveActiveTab, SelectEnvironment1, SelectEnvironment2, SelectEnvironment3, SelectEnvironment4,
    SelectEnvironment5, SelectEnvironment6, SelectEnvironment7, SelectEnvironment8,
    SelectEnvironment9, SelectNoEnvironment, SendActiveTab,
};
use state::settings::ThemeChoice;
use views::AppView;

/// The application id: Wayland `app_id`, X11 `WM_CLASS`, Windows AppUserModelID. The Linux
/// `codes.tanis.postino.desktop` file and the Windows installer shortcut use the same value.
const APP_ID: &str = "codes.tanis.postino";

/// The user-visible application name given to the OS together with [`APP_ID`].
const APP_NAME: &str = "Postino";

fn main() {
    // Settings first, for the log level: the logger is installed before anything else runs, so
    // every later message (gpui's own included) reaches the log file.
    let settings = state::settings::load_settings();
    let env_level = std::env::var(logging::ENV_VAR).ok();
    let log_level = logging::startup_level(settings.log_level, env_level.as_deref());
    let log_path = logging::init(log_level.to_filter());
    let from_env = env_level
        .as_deref()
        .and_then(state::settings::LogLevel::parse)
        .is_some();
    log_startup(log_level, from_env, log_path.as_deref());
    if let Some(value) = env_level.filter(|_| !from_env) {
        log::warn!("ignoring {}={value:?}: not a log level", logging::ENV_VAR);
    }

    // Before the window opens, so its first frame is already in the right language.
    let language = state::locale::resolve(settings.language);
    state::locale::apply(language);
    log::info!(
        "language: {} ({}, system locales: {})",
        language.code(),
        match settings.language.language() {
            Some(_) => "fixed",
            None => "auto",
        },
        state::locale::system_locales().join(", "),
    );

    // What to open on startup: the first CLI argument if given (a workspace folder, or a request
    // file, which opens in a tab), otherwise the folder remembered from the previous run.
    let arg = std::env::args_os().nth(1).map(PathBuf::from);
    let launch = state::launch::resolve_launch_target(
        arg.as_deref(),
        state::config::load_last_workspace().as_deref(),
    );
    let (initial_workspace, initial_request) = match launch {
        Some(target) => (Some(target.workspace), target.request_id),
        None => (None, None),
    };

    // Files the OS hands over while running (macOS Apple Events). They can arrive before the
    // window exists, so they wait in this queue until the window's task drains it.
    let open_queue = state::open_queue::OpenQueue::default();

    let theme_choice = settings.theme;

    // `with_assets` registers the bundled icon SVGs. Without it every `Icon` (tree chevrons,
    // window controls, checkboxes) renders as empty space. `gpui_kit::assets::Assets` only
    // embeds the small curated subset generated from `default-icons.txt`
    // (`gpui-kit-assets-0.6.6/src/native_assets.rs`); `AllAssets` embeds the complete Lucide
    // catalog (`gpui_kit::assets::IconName`, 1830 icons) that the design
    // uses (`gauge`, `wand-sparkles`, `send-horizontal`, ...), a strict superset of the curated
    // one, so switching to it does not affect any icon that already worked.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    let url_queue = open_queue.clone();
    app.on_open_urls(move |urls| url_queue.push(state::launch::paths_from_urls(&urls)));
    app.run(move |cx| {
        // The AppUserModelID on Windows (the installer's shortcut must use the same id) and
        // the app name for notifications elsewhere.
        cx.set_app_identity(APP_ID, APP_NAME);
        gpui_kit::init(cx);
        bind_keys(cx);
        theme::install(cx, &settings);

        // Centered at 1440x900 logical, capped to 90% of the display's visible bounds:
        // `WindowBounds::centered` only caps at 100% of the display (`Bounds::centered`'s own
        // `size.min(&visible_bounds.size)`), which is not tight enough to keep the title bar,
        // status bar and sidebar footer on screen on a display where 1440x900 is close to the full
        // visible area.
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
            // Wayland app_id and X11 WM_CLASS: must match `codes.tanis.postino.desktop` for
            // the right icon and name in the dock and alt-tab.
            app_id: Some(APP_ID.to_string()),
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

                let view = cx.new(|cx| {
                    AppView::new(
                        initial_workspace.clone(),
                        initial_request.clone(),
                        settings,
                        window,
                        cx,
                    )
                });

                // Opens files the OS delivers. Sleeps on the queue, so an idle window does
                // no work; files queued before this point are delivered right away.
                let queue = open_queue.clone();
                let weak_view = view.downgrade();
                window
                    .spawn(cx, async move |cx| {
                        loop {
                            let paths = queue.next().await;
                            let result = weak_view.update_in(cx, |view, window, cx| {
                                view.open_external_files(paths, window, cx)
                            });
                            if result.is_err() {
                                break;
                            }
                        }
                    })
                    .detach();

                // Always listen for OS appearance changes, live, regardless of the theme choice at
                // startup: the Settings view can switch the choice at any time afterwards, and
                // "System" must start following the OS the moment it is picked, even if the app
                // launched in Light or Dark. Checking the *current* setting on every OS change
                // (instead of only attaching this observer when `theme_choice == System`) is what
                // lets Light and Dark stop following it without detaching anything: the observer
                // simply no-ops while they are active.
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

/// Logs where and how Postino runs: the first lines of every session, and the ones a bug report
/// needs first.
fn log_startup(level: state::settings::LogLevel, from_env: bool, log_path: Option<&Path>) {
    log::info!(
        "Postino {} starting ({} build) on {} {}",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    log::info!(
        "log level {level:?}{}, log file {}",
        if from_env {
            format!(" (from {})", logging::ENV_VAR)
        } else {
            String::new()
        },
        log_path.map_or_else(|| "none".to_string(), |path| path.display().to_string()),
    );
    log::info!(
        "settings file {}",
        state::settings::settings_path()
            .map_or_else(|| "unknown".to_string(), |path| path.display().to_string()),
    );
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
        KeyBinding::new("cmd-k", OpenCommandPalette, None),
        KeyBinding::new("cmd-w", CloseActiveTab, None),
        KeyBinding::new("cmd-shift-/", OpenShortcuts, None),
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
        KeyBinding::new("ctrl-k", OpenCommandPalette, None),
        KeyBinding::new("ctrl-w", CloseActiveTab, None),
        KeyBinding::new("f1", OpenShortcuts, None),
    ]);
    // Ctrl+Tab is the tab switcher on every platform, macOS included. The text inputs only bind
    // plain `tab` / `shift-tab` (indent and outdent), so this still fires while one has focus.
    cx.bind_keys([
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
    ]);
}
