//! The in-app updater (#45), on top of `telaio-update`: Postino's [`UPDATER`] configuration, the
//! build gate, and the one decision that needs Postino's own state ([`needs_restart_confirmation`]).
//! The checks, downloads, portable detection and installs are `telaio_update::flow`, re-exported
//! here; `views/update.rs` runs the blocking parts on the background executor and renders the
//! result.
//!
//! The feature exists only in builds made with `POSTINO_UPDATER=github` (the release CI sets it
//! for macOS, Windows and Linux) on a platform `telaio-update` knows. On Linux it only announces
//! new versions, and only in a manual install (the tarball, the install script, the AppImage):
//! a package managed copy (deb, rpm, Arch, Flatpak) shows nothing. Everything compiles
//! everywhere; [`updater_enabled`] is the one switch the setting row, the palette command and the
//! startup check consult.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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

/// The command that installs Postino on Linux and, run again, updates it. The same as on the
/// website.
pub const INSTALL_COMMAND: &str = "curl -fsSL https://postino.tanis.codes/install.sh | bash";

/// Whether this build has an updater: built with `POSTINO_UPDATER=github`, running on a platform
/// `telaio-update` knows, and not a Linux copy owned by a package manager. Read once (the
/// executable path is a syscall), so it is cheap in `render`.
pub fn updater_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let exe = std::env::current_exe().ok();
        flow::updater_enabled(option_env!("POSTINO_UPDATER"))
            && !flow::is_package_managed(Platform::current(), exe.as_deref(), flow::is_sandboxed())
    })
}

/// How a copy that downloads nothing ([`ReadyUpdate::is_manual_download`]) gets a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualUpdate {
    /// A portable Windows copy: the new zip from the release page.
    PortableZip,
    /// A Linux tarball or AppImage: the new one from the release page.
    Download,
    /// Installed by the install script: run [`INSTALL_COMMAND`] again.
    InstallScript,
}

/// The [`ManualUpdate`] of this copy. `install_script` is [`installed_by_script`].
pub fn manual_update(platform: Option<Platform>, install_script: bool) -> ManualUpdate {
    match platform {
        Some(Platform::LinuxX86_64) if install_script => ManualUpdate::InstallScript,
        Some(Platform::LinuxX86_64) => ManualUpdate::Download,
        _ => ManualUpdate::PortableZip,
    }
}

/// Whether the install script installed this executable: it writes the list of its files to
/// `<prefix>/share/postino/installed-files`, and the binary is `<prefix>/bin/postino`. Reads the
/// filesystem, so call it once.
pub fn installed_by_script(exe: &Path) -> bool {
    exe.parent()
        .filter(|bin| bin.ends_with("bin"))
        .and_then(Path::parent)
        .is_some_and(|prefix| prefix.join("share/postino/installed-files").is_file())
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
#[allow(clippy::expect_used)]
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
    fn linux_copies_update_by_hand_or_with_the_script() {
        let linux = Some(Platform::LinuxX86_64);
        assert_eq!(manual_update(linux, true), ManualUpdate::InstallScript);
        assert_eq!(manual_update(linux, false), ManualUpdate::Download);
        let windows = Some(Platform::WindowsX86_64);
        assert_eq!(manual_update(windows, false), ManualUpdate::PortableZip);
    }

    #[test]
    fn the_script_install_is_told_by_its_manifest() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin dir");
        let exe = bin.join("postino");
        std::fs::write(&exe, "").expect("binary");
        assert!(!installed_by_script(&exe));
        let share = dir.path().join("share/postino");
        std::fs::create_dir_all(&share).expect("share dir");
        std::fs::write(share.join("installed-files"), "# Postino 0.5.0\n").expect("manifest");
        assert!(installed_by_script(&exe));
        assert!(!installed_by_script(&dir.path().join("postino")));
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
