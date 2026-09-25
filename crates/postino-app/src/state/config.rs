//! Remembers the last opened workspace folder in the OS config directory, so the app can offer
//! to reopen it automatically next launch (`plans/mvp.md`, phase 8).

use std::fs;
use std::path::{Path, PathBuf};

/// The path, relative to the OS config directory, of the file that remembers the last opened
/// workspace folder.
const STATE_FILE: &str = "postino/last-workspace.txt";

/// Reads the last opened workspace folder, if one was remembered and the OS config directory is
/// known.
pub fn load_last_workspace() -> Option<PathBuf> {
    read_last_workspace(&dirs::config_dir()?)
}

/// Remembers `root` as the last opened workspace folder, for [`load_last_workspace`] to find on
/// the next launch. Failing to persist this is not worth interrupting the user over, so any
/// error (a missing config directory, a read-only filesystem, ...) is silently ignored.
pub fn save_last_workspace(root: &Path) {
    if let Some(base) = dirs::config_dir() {
        write_last_workspace(&base, root);
    }
}

/// Reads the remembered folder from `<base>/postino/last-workspace.txt`. Split out from
/// [`load_last_workspace`] so tests can point `base` at a temporary directory instead of the
/// real, per-user OS config directory.
fn read_last_workspace(base: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(base.join(STATE_FILE)).ok()?;
    let trimmed = content.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// Writes `root` to `<base>/postino/last-workspace.txt`, creating the folder if needed. Split
/// out from [`save_last_workspace`] for the same testing reason as [`read_last_workspace`].
fn write_last_workspace(base: &Path, root: &Path) {
    let path = base.join(STATE_FILE);
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let _ = fs::write(path, root.to_string_lossy().as_bytes());
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_reads_as_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(read_last_workspace(dir.path()), None);
    }

    #[test]
    fn round_trips_through_a_config_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("my-workspace");

        write_last_workspace(dir.path(), &workspace);

        assert_eq!(read_last_workspace(dir.path()), Some(workspace));
    }

    #[test]
    fn writing_again_overwrites_the_previous_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_last_workspace(dir.path(), &dir.path().join("first"));
        write_last_workspace(dir.path(), &dir.path().join("second"));

        assert_eq!(
            read_last_workspace(dir.path()),
            Some(dir.path().join("second"))
        );
    }
}
