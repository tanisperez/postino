//! Postino, the desktop HTTP client. Binary entry point: initializes `gpui`, the theme, the main
//! window, and opens the workspace given on the command line or remembered from the last run.

// A GUI application on Windows: never open a console window behind the main window.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod actions;
mod state;
mod theme;
mod views;

rust_i18n::i18n!("locales", fallback = "en");

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gpui_kit::component::Root;
use gpui_kit::*;
use postino_cli::IDENTITY;
use telaio_app::{logging, window};

use actions::{
    CloseActiveTab, CommitFromMessage, ConfirmCompletion, CopyLoadTestSummary, DeleteLine,
    DeleteSelectedEntry, DismissCompletion, NextCompletion, NextTab, OpenCommandPalette,
    OpenSettings, OpenShortcuts, OpenWorkspace, PreviousCompletion, PreviousTab, RedoRequestEdit,
    RenameSelectedEntry, SaveActiveTab, SelectEnvironment1, SelectEnvironment2, SelectEnvironment3,
    SelectEnvironment4, SelectEnvironment5, SelectEnvironment6, SelectEnvironment7,
    SelectEnvironment8, SelectEnvironment9, SelectNoEnvironment, SendActiveTab, ShowCompletion,
    ToggleCollections, ToggleEnvironments, ToggleLoadTests, ToggleSourceControl, UndoRequestEdit,
};
use state::settings::ThemeChoice;
use views::AppView;

fn main() -> ExitCode {
    // `--help` and `--version` print and exit before anything else: no settings, no log.
    if let Some(code) = postino_cli::run_if_cli(std::env::args_os().nth(1).as_deref()) {
        return code;
    }

    // Settings first, for the log level: the logger is installed before anything else runs, so
    // every later message (gpui's own included) reaches the log file.
    let settings = state::settings::load_settings();
    let log_env_var = logging::env_var(&IDENTITY);
    let env_level = std::env::var(&log_env_var).ok();
    let log_level = logging::startup_level(settings.log_level, env_level.as_deref());
    let log_path = logging::init(&IDENTITY, log_level.to_filter());
    let from_env = env_level
        .as_deref()
        .and_then(state::settings::LogLevel::parse)
        .is_some();
    log_startup(
        log_level,
        from_env.then_some(log_env_var.as_str()),
        log_path.as_deref(),
    );
    if let Some(value) = env_level.filter(|_| !from_env) {
        log::warn!("ignoring {log_env_var}={value:?}: not a log level");
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
    let show_welcome =
        state::welcome::show_at_startup(arg.is_some(), launch.is_some(), settings.show_welcome);
    let (initial_workspace, initial_request) = match launch {
        Some(target) => (Some(target.workspace), target.request_id),
        None => (None, None),
    };

    // Files the OS hands over while running (macOS Apple Events). They can arrive before the
    // window exists, so they wait in this queue until the window's task drains it.
    let open_queue = state::open_queue::OpenQueue::default();

    let theme_choice = settings.theme;

    let app = window::application();
    let url_queue = open_queue.clone();
    app.on_open_urls(move |urls| url_queue.push(state::launch::paths_from_urls(&urls)));
    app.run(move |cx| {
        window::init(cx, &IDENTITY);
        bind_keys(cx);
        theme::install(cx, &settings);

        // 1440x900 logical, capped to 90% of the display (see `window::options`).
        let window_options = window::options(cx, &IDENTITY, window::WindowSizes::default());

        cx.spawn(async move |cx| {
            let opened = cx.open_window(window_options, move |window, cx| {
                // "System" keeps matching the OS light/dark setting, including if the user
                // changes it while Postino is running; an explicit choice sets the mode once.
                telaio_ui::ThemeChoice::from(theme_choice).apply(Some(window), cx);

                let view = cx.new(|cx| {
                    AppView::new(
                        initial_workspace.clone(),
                        initial_request.clone(),
                        show_welcome,
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

                // Always listening, whatever the theme choice at startup: the Settings view can
                // switch to "System" at any time (see `window::follow_system_appearance`).
                let weak = view.downgrade();
                window::follow_system_appearance(window, move |cx| {
                    weak.upgrade().is_some_and(|view| {
                        view.read(cx).state.settings.theme == ThemeChoice::System
                    })
                });

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
    ExitCode::SUCCESS
}

/// Logs where and how Postino runs: the first lines of every session, and the ones a bug report
/// needs first.
fn log_startup(level: state::settings::LogLevel, from_env: Option<&str>, log_path: Option<&Path>) {
    log::info!(
        "Postino {} starting ({} build) on {} {}",
        postino_cli::version_label(),
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
        from_env.map_or_else(String::new, |variable| format!(" (from {variable})")),
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
        KeyBinding::new("cmd-o", OpenWorkspace, None),
        KeyBinding::new("cmd-w", CloseActiveTab, None),
        KeyBinding::new("cmd-shift-c", CopyLoadTestSummary, None),
        // Ctrl, not Option: Option + a digit types `|`, `@` or `#` on many layouts.
        KeyBinding::new("ctrl-1", ToggleCollections, None),
        KeyBinding::new("ctrl-2", ToggleSourceControl, None),
        KeyBinding::new("ctrl-3", ToggleEnvironments, None),
        KeyBinding::new("ctrl-4", ToggleLoadTests, None),
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
        KeyBinding::new("ctrl-o", OpenWorkspace, None),
        KeyBinding::new("ctrl-w", CloseActiveTab, None),
        KeyBinding::new("ctrl-shift-c", CopyLoadTestSummary, None),
        KeyBinding::new("alt-1", ToggleCollections, None),
        KeyBinding::new("alt-2", ToggleSourceControl, None),
        KeyBinding::new("alt-3", ToggleEnvironments, None),
        KeyBinding::new("alt-4", ToggleLoadTests, None),
    ]);
    // `F1`, or `Cmd+Shift+/` on macOS, where `F1` is the brightness key unless `Fn` is held.
    cx.bind_keys([KeyBinding::new(
        telaio_ui::components::OPEN_SHORTCUTS_KEYSTROKE,
        OpenShortcuts,
        None,
    )]);
    // Ctrl+Tab is the tab switcher on every platform, macOS included. The text inputs only bind
    // plain `tab` / `shift-tab` (indent and outdent), so this still fires while one has focus.
    cx.bind_keys([
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
    ]);
    // The commit message's Ctrl+Enter / Cmd+Enter, bound after the input's own `secondary-enter`
    // (a new line) so it runs first, and only inside the Source Control panel's message.
    cx.bind_keys([KeyBinding::new(
        "secondary-enter",
        CommitFromMessage,
        Some("CommitMessage > Input"),
    )]);
    // Autocompletion (variables, header names and values), in any text input. These are bound after gpui-kit's own input
    // bindings, so they run first; the handlers (`views/completion.rs`) act only on a
    // request editor field with the menu open and otherwise let the input's binding run. Ctrl
    // on every platform: Cmd+Space is Spotlight on macOS.
    let input = Some("Input");
    cx.bind_keys([
        KeyBinding::new("ctrl-space", ShowCompletion, input),
        KeyBinding::new("down", NextCompletion, input),
        KeyBinding::new("up", PreviousCompletion, input),
        KeyBinding::new("enter", ConfirmCompletion, input),
        KeyBinding::new("tab", ConfirmCompletion, input),
        KeyBinding::new("escape", DismissCompletion, input),
    ]);
    // Delete the line, in any text input like the completion keys above, but the handler
    // (`views/request_editor.rs`) acts only in a script editor and otherwise lets the key through.
    // On macOS `Ctrl+D` is the system's "delete forward" in text fields, so it is VS Code's key.
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new("cmd-shift-k", DeleteLine, input)]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("ctrl-d", DeleteLine, input)]);
    // Undo and redo of request edits (#109): anywhere, and in any text input, bound after
    // gpui-kit's own input bindings so they run first. The handlers
    // (`views/request_editor.rs`) act on the active request tab when the focus is outside a
    // text input or in one of the request editor's single-line fields, and otherwise let the
    // input's own undo run.
    #[cfg(target_os = "macos")]
    for context in [None, input] {
        cx.bind_keys([
            KeyBinding::new("cmd-z", UndoRequestEdit, context),
            KeyBinding::new("cmd-shift-z", RedoRequestEdit, context),
        ]);
    }
    #[cfg(not(target_os = "macos"))]
    for context in [None, input] {
        cx.bind_keys([
            KeyBinding::new("ctrl-z", UndoRequestEdit, context),
            KeyBinding::new("ctrl-shift-z", RedoRequestEdit, context),
            KeyBinding::new("ctrl-y", RedoRequestEdit, context),
        ]);
    }
    // Only while the sidebar tree has the focus (gpui-base's `Tree` key context): in a text input
    // these keys keep editing the text.
    let tree = Some(gpui_kit::base::tree_key_context());
    cx.bind_keys([
        KeyBinding::new("delete", DeleteSelectedEntry, tree),
        KeyBinding::new("f2", RenameSelectedEntry, tree),
    ]);
}
