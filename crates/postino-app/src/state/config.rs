//! Remembers recently opened workspace folders in the OS config directory, most recent first
//! (`plans/ui-redesign.md`, section 1 "Recent workspaces"), so the app can offer to reopen one
//! automatically at startup and show them in the workspace switcher menu.

use std::fs;
use std::path::{Path, PathBuf};

/// Maximum number of remembered workspaces.
const MAX_RECENT_WORKSPACES: usize = 10;

/// The path, relative to the OS config directory, of the file listing recently opened
/// workspaces, one absolute path per line, most recent first.
const RECENT_WORKSPACES_FILE: &str = "postino/recent-workspaces.txt";

/// The path of the single-workspace file this module replaces. Read once to migrate existing
/// users the first time [`RECENT_WORKSPACES_FILE`] does not exist yet, then ignored.
const LEGACY_LAST_WORKSPACE_FILE: &str = "postino/last-workspace.txt";

/// Returns the list of recently opened workspaces, most recent first, migrating from the legacy
/// single-workspace file the first time the new file does not exist yet.
pub fn recent_workspaces() -> Vec<PathBuf> {
    match dirs::config_dir() {
        Some(base) => read_recent_workspaces(&base),
        None => Vec::new(),
    }
}

/// Records `root` as the most recently opened workspace: moves it to the front of the list (or
/// inserts it), drops any duplicate, and keeps at most [`MAX_RECENT_WORKSPACES`] entries. Failing
/// to persist this is not worth interrupting the user over, so any error (a missing config
/// directory, a read-only filesystem, ...) is silently ignored.
pub fn record_workspace(root: &Path) {
    if let Some(base) = dirs::config_dir() {
        record_workspace_at(&base, root);
    }
}

/// The testable core of [`record_workspace`], taking the config base directory explicitly.
fn record_workspace_at(base: &Path, root: &Path) {
    let mut recents = read_recent_workspaces(base);
    recents.retain(|existing| existing != root);
    recents.insert(0, root.to_path_buf());
    recents.truncate(MAX_RECENT_WORKSPACES);
    write_recent_workspaces(base, &recents);
}

/// The workspace to reopen at startup: the first entry of [`recent_workspaces`] that still
/// exists on disk, or `None` if there are no recent workspaces or none of them exist anymore.
pub fn load_last_workspace() -> Option<PathBuf> {
    first_existing(recent_workspaces())
}

/// The first path in `paths` that is still a directory on disk, in order. Split out from
/// [`load_last_workspace`] so it can be tested without touching the real OS config directory.
fn first_existing(paths: Vec<PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.is_dir())
}

/// Reads the recent workspace list from `<base>/postino/recent-workspaces.txt`, migrating from
/// `<base>/postino/last-workspace.txt` if the new file does not exist yet. Split out from
/// [`recent_workspaces`] so tests can point `base` at a temporary directory instead of the real,
/// per-user OS config directory.
fn read_recent_workspaces(base: &Path) -> Vec<PathBuf> {
    match fs::read_to_string(base.join(RECENT_WORKSPACES_FILE)) {
        Ok(content) => parse_recent_workspaces(&content),
        Err(_) => migrate_legacy_last_workspace(base),
    }
}

/// Parses the content of a recent-workspaces file: one absolute path per line, blank lines
/// ignored.
fn parse_recent_workspaces(content: &str) -> Vec<PathBuf> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// One-time migration from the legacy `last-workspace.txt` file: if it exists, its single entry
/// becomes the whole recent list and is written to the new file, so the migration only happens
/// once. Returns an empty list if there is nothing to migrate.
fn migrate_legacy_last_workspace(base: &Path) -> Vec<PathBuf> {
    let Ok(content) = fs::read_to_string(base.join(LEGACY_LAST_WORKSPACE_FILE)) else {
        return Vec::new();
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let recents = vec![PathBuf::from(trimmed)];
    write_recent_workspaces(base, &recents);
    recents
}

/// Writes the recent workspace list to `<base>/postino/recent-workspaces.txt`, creating the
/// folder if needed, one absolute path per line, most recent first.
fn write_recent_workspaces(base: &Path, recents: &[PathBuf]) {
    let path = base.join(RECENT_WORKSPACES_FILE);
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let mut content = recents
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    let _ = fs::write(path, content);
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn missing_files_read_as_an_empty_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(read_recent_workspaces(dir.path()), Vec::<PathBuf>::new());
    }

    #[test]
    fn record_workspace_adds_a_new_entry_at_the_front() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        write_recent_workspaces(base, &[base.join("a")]);

        record_workspace_at(base, &base.join("b"));

        assert_eq!(
            read_recent_workspaces(base),
            vec![base.join("b"), base.join("a")]
        );
    }

    #[test]
    fn record_workspace_moves_an_existing_entry_to_the_front_without_duplicating_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        write_recent_workspaces(base, &[base.join("a"), base.join("b"), base.join("c")]);

        record_workspace_at(base, &base.join("b"));

        assert_eq!(
            read_recent_workspaces(base),
            vec![base.join("b"), base.join("a"), base.join("c")]
        );
    }

    #[test]
    fn record_workspace_caps_the_list_at_ten_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        let recents: Vec<PathBuf> = (0..10)
            .map(|index| base.join(format!("w{index}")))
            .collect();
        write_recent_workspaces(base, &recents);

        record_workspace_at(base, &base.join("new"));

        let stored = read_recent_workspaces(base);
        assert_eq!(stored.len(), MAX_RECENT_WORKSPACES);
        assert_eq!(stored[0], base.join("new"));
        assert!(!stored.contains(&base.join("w9")));
    }

    #[test]
    fn migrates_from_the_legacy_single_workspace_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        let legacy_path = base.join(LEGACY_LAST_WORKSPACE_FILE);
        fs::create_dir_all(legacy_path.parent().expect("parent")).expect("create postino dir");
        fs::write(
            &legacy_path,
            base.join("legacy-workspace").to_string_lossy().as_bytes(),
        )
        .expect("write legacy file");

        let recents = read_recent_workspaces(base);

        assert_eq!(recents, vec![base.join("legacy-workspace")]);
        // The migration writes the new file, so it only happens once.
        assert!(base.join(RECENT_WORKSPACES_FILE).is_file());
    }

    #[test]
    fn migration_only_happens_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        let legacy_path = base.join(LEGACY_LAST_WORKSPACE_FILE);
        fs::create_dir_all(legacy_path.parent().expect("parent")).expect("create postino dir");
        fs::write(
            &legacy_path,
            base.join("legacy-workspace").to_string_lossy().as_bytes(),
        )
        .expect("write legacy file");
        // First read migrates.
        read_recent_workspaces(base);
        // A later change to the legacy file must not be picked up again.
        fs::write(
            &legacy_path,
            base.join("ignored").to_string_lossy().as_bytes(),
        )
        .expect("rewrite legacy file");

        assert_eq!(
            read_recent_workspaces(base),
            vec![base.join("legacy-workspace")]
        );
    }

    #[test]
    fn first_existing_skips_paths_that_no_longer_exist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("missing");
        let real = dir.path().join("real");
        fs::create_dir(&real).expect("create real dir");

        assert_eq!(
            first_existing(vec![missing.clone(), real.clone()]),
            Some(real)
        );
        assert_eq!(first_existing(vec![missing]), None);
    }
}
