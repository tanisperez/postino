//! The in-app updater (#45), on top of `telaio-update`: Postino's [`UPDATER`] configuration, the
//! build gate, and the one decision that needs Postino's own state ([`needs_restart_confirmation`]).
//! The checks, downloads, portable detection and installs are `telaio_update::flow`, re-exported
//! here; `views/update.rs` runs the blocking parts on the background executor and renders the
//! result.
//!
//! The feature exists only in builds made with `POSTINO_UPDATER=github` (the release CI sets it
//! for macOS and Windows) on a platform `telaio-update` knows. Everything compiles everywhere;
//! [`updater_enabled`] is the one switch the setting row, the palette command and the startup
//! check consult.

use std::path::{Path, PathBuf};

use telaio_update::{Platform, UpdateError, UpdaterConfig, flow};

pub use telaio_update::flow::{
    CheckOutcome, CheckStatus, InstallOutcome, ReadyUpdate, STARTUP_DELAY, UpdateState,
    is_portable, should_check_at_startup,
};

use super::TabsState;

/// Postino's updater: releases of `tanisperez/postino`, `POSTINO_UPDATE_URL` to test an update
/// between pre-releases (`docs/releasing.md`).
pub const UPDATER: UpdaterConfig = UpdaterConfig {
    owner: "tanisperez",
    repo: "postino",
    app_name: "Postino",
    url_env: "POSTINO_UPDATE_URL",
    version: CURRENT_VERSION,
};

/// The running version, what a manifest is compared against.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether this build has an updater: built with `POSTINO_UPDATER=github` and running on a
/// platform with an installable asset.
pub fn updater_enabled() -> bool {
    flow::updater_enabled(option_env!("POSTINO_UPDATER"))
}

/// Where updates are downloaded: `postino/updates` under the OS cache folder.
pub fn download_dir() -> Option<PathBuf> {
    UPDATER.download_dir()
}

/// Whether restarting would lose unsaved edits, so the user must confirm first.
pub fn needs_restart_confirmation(tabs: &TabsState) -> bool {
    tabs.open_tabs().iter().any(|tab| tab.dirty)
}

/// [`flow::check_and_download`] for Postino. Blocking.
///
/// # Errors
///
/// Whatever `telaio_update` reports.
pub fn check_and_download(platform: Platform, portable: bool) -> Result<CheckOutcome, UpdateError> {
    flow::check_and_download(&UPDATER, platform, portable)
}

/// [`flow::start_install`] for Postino. Blocking.
pub fn start_install(platform: Platform, ready: &ReadyUpdate, exe: &Path) -> InstallOutcome {
    flow::start_install(&UPDATER, platform, ready, exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_needs_confirmation_only_with_dirty_tabs() {
        let mut tabs = TabsState::default();
        assert!(!needs_restart_confirmation(&tabs));
        tabs.open("a.postino", postino_core::Request::default());
        assert!(!needs_restart_confirmation(&tabs));
        tabs.set_dirty(0, true);
        assert!(needs_restart_confirmation(&tabs));
    }

    #[test]
    fn the_manifest_is_postino_s_latest_release() {
        assert_eq!(
            UPDATER.latest_url(),
            "https://github.com/tanisperez/postino/releases/latest/download/latest.json"
        );
    }

    #[test]
    fn downloads_go_under_the_postino_cache_dir() {
        if let Some(dir) = download_dir() {
            assert!(dir.ends_with("postino/updates"));
        }
    }
}
