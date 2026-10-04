//! The in-app updater's state and decisions (#45), kept free of `gpui` so it is unit tested
//! without a window. The blocking work lives in `postino-update`, `views/update.rs` runs it on
//! the background executor and renders the result.
//!
//! The feature exists only in builds made with `POSTINO_UPDATER=github` (the release CI sets it
//! for macOS and Windows) on a platform `postino-update` knows. Everything compiles everywhere;
//! [`updater_enabled`] is the one switch the setting row, the palette command and the startup
//! check consult.

use std::path::{Path, PathBuf};

use postino_update::{Platform, UpdateError, install};

use super::TabsState;

/// The running version, what a manifest is compared against.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Delay between startup and the single automatic check, so it never competes with the first
/// frames.
pub const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether this build has an updater: built with `POSTINO_UPDATER=github` and running on a
/// platform with an installable asset.
pub fn updater_enabled() -> bool {
    enabled_for(option_env!("POSTINO_UPDATER"), Platform::current())
}

fn enabled_for(build_flag: Option<&str>, platform: Option<Platform>) -> bool {
    build_flag == Some("github") && platform.is_some()
}

/// Whether the automatic check at startup runs: the build has an updater and the user left
/// "Check for updates automatically" on.
pub fn should_check_at_startup(enabled: bool, setting: bool) -> bool {
    enabled && setting
}

/// Where downloaded updates are kept, `None` when the OS cache directory is unknown.
pub fn download_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|base| base.join("postino").join("updates"))
}

/// An update that is downloaded and verified, waiting for the user to restart into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyUpdate {
    /// The new version, as text.
    pub version: String,
    /// The release page with the notes.
    pub notes_url: String,
    /// The verified installer (Windows) or archive (macOS).
    pub path: PathBuf,
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// Nothing newer than the running version.
    UpToDate,
    /// A newer version, downloaded and verified.
    Ready(ReadyUpdate),
}

/// How the last check went, shown in Settings, "About".
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// No check has finished yet.
    #[default]
    Idle,
    /// A check is running.
    Checking,
    /// The last check found nothing newer.
    UpToDate,
    /// The last check failed.
    Failed,
}

/// What the app remembers about updates: the ready one (if any) and whether a check is running.
#[derive(Debug, Default)]
pub struct UpdateState {
    ready: Option<ReadyUpdate>,
    checking: bool,
    installing: bool,
    status: CheckStatus,
}

impl UpdateState {
    /// The update waiting to be installed.
    pub fn ready(&self) -> Option<&ReadyUpdate> {
        self.ready.as_ref()
    }

    /// How the last check went, or [`CheckStatus::Checking`] while one runs.
    pub fn status(&self) -> CheckStatus {
        self.status
    }

    /// Marks a check as started. Returns `false`, doing nothing, when one is already running or
    /// an update is already ready (there is nothing more to look for).
    pub fn begin_check(&mut self) -> bool {
        if self.checking || self.ready.is_some() {
            return false;
        }
        self.checking = true;
        self.status = CheckStatus::Checking;
        true
    }

    /// Records the end of a check. A ready update is remembered, never replaced by a later one.
    pub fn finish_check(&mut self, outcome: &CheckOutcome) {
        self.checking = false;
        self.status = match outcome {
            CheckOutcome::UpToDate => CheckStatus::UpToDate,
            CheckOutcome::Ready(_) => CheckStatus::Idle,
        };
        if let CheckOutcome::Ready(update) = outcome
            && self.ready.is_none()
        {
            self.ready = Some(update.clone());
        }
    }

    /// Records a failed check.
    pub fn fail_check(&mut self) {
        self.checking = false;
        self.status = CheckStatus::Failed;
    }

    /// Marks the installation as started and returns the update to install. `None`, doing
    /// nothing, when no update is ready or an installation is already running (a double click).
    pub fn begin_install(&mut self) -> Option<ReadyUpdate> {
        if self.installing {
            return None;
        }
        let ready = self.ready.clone()?;
        self.installing = true;
        Some(ready)
    }

    /// Records that the installation did not take over, so the user can try again.
    pub fn fail_install(&mut self) {
        self.installing = false;
    }
}

/// Whether restarting would lose unsaved edits, so the user must confirm first.
pub fn needs_restart_confirmation(tabs: &TabsState) -> bool {
    tabs.open_tabs().iter().any(|tab| tab.dirty)
}

/// Checks the manifest and, for a newer version, downloads and verifies the asset. Blocking.
///
/// # Errors
///
/// Whatever `postino_update` reports, or [`UpdateError::Io`] when there is no cache directory.
pub fn check_and_download(platform: Platform) -> Result<CheckOutcome, UpdateError> {
    let Some(update) =
        postino_update::check(&postino_update::manifest_url(), CURRENT_VERSION, platform)?
    else {
        return Ok(CheckOutcome::UpToDate);
    };
    log::info!("update available {}", update.version);
    let dir = download_dir()
        .ok_or_else(|| UpdateError::Io(std::io::Error::other("the cache directory is unknown")))?;
    let path = postino_update::download(&update, &dir)?;
    Ok(CheckOutcome::Ready(ReadyUpdate {
        version: update.version.to_string(),
        notes_url: update.notes_url,
        path,
    }))
}

/// How an update is installed on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    /// Run the installer, then quit.
    WindowsInstaller,
    /// Unpack next to the bundle, swap it after quitting, reopen.
    MacosSwap,
}

/// The install method of `platform`.
pub fn install_method(platform: Platform) -> InstallMethod {
    match platform {
        Platform::WindowsX86_64 => InstallMethod::WindowsInstaller,
        Platform::MacosAarch64 => InstallMethod::MacosSwap,
    }
}

/// What the app does after trying to install.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The installer or the swap script is running: quit now.
    Quit,
    /// Not possible in place (not in a bundle, folder not writable): send the user to the
    /// release page.
    Fallback,
    /// Failed, with the error text to show.
    Failed(String),
}

/// Starts the installation of `ready`. Blocking (it unpacks the archive on macOS), so run it on
/// the background executor. `exe` is the running executable.
pub fn start_install(platform: Platform, ready: &ReadyUpdate, exe: &Path) -> InstallOutcome {
    match install_method(platform) {
        InstallMethod::WindowsInstaller => match install::spawn_windows_installer(&ready.path) {
            Ok(()) => InstallOutcome::Quit,
            Err(error) => InstallOutcome::Failed(error.to_string()),
        },
        InstallMethod::MacosSwap => {
            let Some(bundle) = install::macos_bundle_from_exe(exe) else {
                return InstallOutcome::Fallback;
            };
            let staged = match install::macos_stage(&ready.path, &bundle) {
                Ok(staged) => staged,
                Err(UpdateError::Io(error))
                    if error.kind() == std::io::ErrorKind::PermissionDenied =>
                {
                    return InstallOutcome::Fallback;
                }
                Err(error) => return InstallOutcome::Failed(error.to_string()),
            };
            let script = install::macos_swap_script(std::process::id(), &bundle, &staged, "open");
            match install::spawn_detached_script(&script) {
                Ok(()) => InstallOutcome::Quit,
                Err(error) => InstallOutcome::Failed(error.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::TabsState;
    use pretty_assertions::assert_eq;

    fn ready(version: &str) -> ReadyUpdate {
        ReadyUpdate {
            version: version.to_string(),
            notes_url: "https://example.com/notes".to_string(),
            path: PathBuf::from("/tmp/postino-update"),
        }
    }

    #[test]
    fn the_gate_needs_the_build_flag_and_a_platform() {
        let mac = Some(Platform::MacosAarch64);
        assert!(enabled_for(Some("github"), mac));
        assert!(!enabled_for(None, mac));
        assert!(!enabled_for(Some("other"), mac));
        assert!(!enabled_for(Some("github"), None));
    }

    #[test]
    fn the_startup_check_needs_the_gate_and_the_setting() {
        assert!(should_check_at_startup(true, true));
        assert!(!should_check_at_startup(true, false));
        assert!(!should_check_at_startup(false, true));
    }

    #[test]
    fn a_check_runs_once_at_a_time() {
        let mut state = UpdateState::default();
        assert!(state.begin_check());
        assert!(!state.begin_check());
        state.fail_check();
        assert!(state.begin_check());
    }

    #[test]
    fn the_status_follows_the_last_check() {
        let mut state = UpdateState::default();
        assert_eq!(state.status(), CheckStatus::Idle);
        state.begin_check();
        assert_eq!(state.status(), CheckStatus::Checking);
        state.fail_check();
        assert_eq!(state.status(), CheckStatus::Failed);
        state.begin_check();
        state.finish_check(&CheckOutcome::UpToDate);
        assert_eq!(state.status(), CheckStatus::UpToDate);
        state.begin_check();
        state.finish_check(&CheckOutcome::Ready(ready("0.2.0")));
        assert_eq!(state.status(), CheckStatus::Idle);
    }

    #[test]
    fn up_to_date_leaves_nothing_ready() {
        let mut state = UpdateState::default();
        state.begin_check();
        state.finish_check(&CheckOutcome::UpToDate);
        assert_eq!(state.ready(), None);
        assert!(state.begin_check());
    }

    #[test]
    fn a_ready_update_is_kept_and_stops_further_checks() {
        let mut state = UpdateState::default();
        state.begin_check();
        state.finish_check(&CheckOutcome::Ready(ready("0.2.0")));
        assert_eq!(state.ready(), Some(&ready("0.2.0")));
        assert!(!state.begin_check());
        state.finish_check(&CheckOutcome::Ready(ready("0.3.0")));
        assert_eq!(state.ready(), Some(&ready("0.2.0")));
    }

    #[test]
    fn install_needs_a_ready_update_and_runs_once() {
        let mut state = UpdateState::default();
        assert_eq!(state.begin_install(), None);
        state.finish_check(&CheckOutcome::Ready(ready("0.2.0")));
        assert_eq!(state.begin_install(), Some(ready("0.2.0")));
        assert_eq!(state.begin_install(), None);
        state.fail_install();
        assert_eq!(state.begin_install(), Some(ready("0.2.0")));
    }

    #[test]
    fn the_install_method_follows_the_platform() {
        assert_eq!(
            install_method(Platform::WindowsX86_64),
            InstallMethod::WindowsInstaller
        );
        assert_eq!(
            install_method(Platform::MacosAarch64),
            InstallMethod::MacosSwap
        );
    }

    #[test]
    fn macos_outside_a_bundle_falls_back_to_the_release_page() {
        let outcome = start_install(
            Platform::MacosAarch64,
            &ready("0.2.0"),
            Path::new("/usr/local/bin/postino"),
        );
        assert_eq!(outcome, InstallOutcome::Fallback);
    }

    #[test]
    fn restart_needs_confirmation_only_with_dirty_tabs() {
        let mut tabs = TabsState::default();
        assert!(!needs_restart_confirmation(&tabs));
        tabs.open("a.postino", postino_core::Request::default());
        assert!(!needs_restart_confirmation(&tabs));
        tabs.mark_dirty(0);
        assert!(needs_restart_confirmation(&tabs));
    }

    #[test]
    fn downloads_go_under_the_postino_cache_dir() {
        if let Some(dir) = download_dir() {
            assert!(dir.ends_with("postino/updates"));
        }
    }
}
