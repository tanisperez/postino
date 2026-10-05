//! Plain Rust application state: everything the app remembers besides what `gpui` itself tracks
//! (focus, layout, animation state, ...). Kept free of `gpui` types so it is unit-tested without
//! a window (`plans/mvp.md`, phase 8: "unit-tested without gpui"). `views/` wraps this in `gpui`
//! entities and renders it.

pub mod about;
pub mod config;
pub mod debug_open;
pub mod define_variable;
pub mod env_color;
pub mod env_edit;
pub mod env_panel;
pub mod format;
pub mod import;
pub mod launch;
pub mod load_panel;
pub mod load_test;
pub mod locale;
pub mod navigation;
pub mod number;
pub mod open_queue;
pub mod palette;
pub mod request_edit;
pub mod response_render;
pub mod run_log;
pub mod script_heuristics;
pub mod settings;
pub mod shortcuts;
pub mod sidebar_filter;
mod tabs;
pub mod ui_tabs;
pub mod update;
pub mod workspace_log;

pub use tabs::{TabKind, TabsState};

use std::path::Path;

use postino_runner::SessionEnv;
use postino_workspace::{Workspace, WorkspaceError};

use settings::Settings;

/// Everything the app remembers about the current session.
#[derive(Default)]
pub struct AppState {
    /// The open workspace, `None` until a folder has been opened.
    pub workspace: Option<Workspace>,
    /// The requests currently open as tabs.
    pub tabs: TabsState,
    /// The name of the active environment, `None` for "No environment".
    pub active_environment: Option<String>,
    /// Environment overrides scripts have set during this session (`plans/mvp.md`, section 3.5,
    /// layer 2). Carried across requests so, for example, a login token a post script sets with
    /// `env.set` is visible to the next request. The MVP never writes this to disk.
    pub session_env: SessionEnv,
    /// The current appearance settings (`plans/ui-redesign.md` phase 6): theme choice and fonts.
    /// Loaded once at startup (`main.rs`) and from then on only changed by the Settings view,
    /// which also persists every change to `settings.toml`.
    pub settings: Settings,
    /// The in-app updater: the downloaded update waiting for a restart, if any.
    pub update: update::UpdateState,
}

impl AppState {
    /// A state with no workspace open yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the current workspace with the one at `root`.
    ///
    /// Open tabs and the active environment belonged to the previous workspace, so they are
    /// discarded; the session environment is reset too, so a new workspace never sees stale
    /// overrides left over from the last one.
    pub fn open_workspace(&mut self, root: impl AsRef<Path>) -> Result<(), WorkspaceError> {
        self.workspace = Some(Workspace::open(root)?);
        self.tabs = TabsState::default();
        self.active_environment = None;
        self.session_env = SessionEnv::new();
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_no_workspace_and_no_environment() {
        let state = AppState::new();
        assert!(state.workspace.is_none());
        assert!(state.tabs.open_tabs().is_empty());
        assert!(state.active_environment.is_none());
        assert!(state.session_env.as_slice().is_empty());
    }

    #[test]
    fn open_workspace_resets_tabs_environment_and_session_overrides() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::new();
        state.active_environment = Some("stale".to_string());
        state.session_env.set("token", "stale");

        state
            .open_workspace(dir.path())
            .expect("opening an empty folder as a workspace never fails");

        assert!(state.workspace.is_some());
        assert!(state.tabs.open_tabs().is_empty());
        assert!(state.active_environment.is_none());
        assert!(state.session_env.as_slice().is_empty());
    }

    #[test]
    fn open_workspace_on_a_missing_folder_fails_and_keeps_no_workspace() {
        let mut state = AppState::new();
        let result = state.open_workspace("/does/not/exist/postino-test");
        assert!(result.is_err());
        assert!(state.workspace.is_none());
    }
}
