//! Tests against a real `git` in temporary folders. They skip themselves when git is missing.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::process::Command;

use postino_git::{Change, Executable, Git, Head, Repo, detect};
use tempfile::TempDir;

/// A git, or `None` (and a note) when this machine has none.
fn git() -> Option<Git> {
    match detect(&Executable::Auto) {
        Ok(git) => Some(git),
        Err(err) => {
            eprintln!("skipping, no usable git: {err}");
            None
        }
    }
}

/// Runs git for test setup, with an identity so commits never depend on the global config.
fn raw(git: &Git, dir: &Path, args: &[&str]) {
    let output = Command::new(git.path())
        .args([
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A fresh repository on `main` with a local identity. Returns the folder and the repo.
fn new_repo(git: &Git) -> (TempDir, Repo) {
    let dir = tempfile::tempdir().expect("tempdir");
    git.init(dir.path()).expect("init");
    raw(
        git,
        dir.path(),
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    raw(
        git,
        dir.path(),
        &["config", "--local", "user.name", "Test User"],
    );
    raw(
        git,
        dir.path(),
        &["config", "--local", "user.email", "test@example.com"],
    );
    raw(
        git,
        dir.path(),
        &["config", "--local", "commit.gpgsign", "false"],
    );
    let repo = git.discover(dir.path()).expect("discover").expect("a repo");
    (dir, repo)
}

fn write(dir: &TempDir, path: &str, content: &str) {
    let full = dir.path().join(path);
    fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
    fs::write(full, content).expect("write");
}

fn commit_all(git: &Git, dir: &TempDir) {
    raw(git, dir.path(), &["add", "-A"]);
    raw(git, dir.path(), &["commit", "-q", "-m", "c"]);
}

fn entry<'a>(status: &'a postino_git::Status, path: &str) -> &'a postino_git::Entry {
    status
        .entries
        .iter()
        .find(|e| e.path == path)
        .unwrap_or_else(|| panic!("no entry {path}"))
}

#[test]
fn detect_reports_a_modern_version() {
    let Some(git) = git() else { return };
    assert!(git.version() >= postino_git::MIN_VERSION);
    assert!(git.path().is_file());
}

#[test]
fn discovers_at_the_root_and_in_a_subfolder() {
    let Some(git) = git() else { return };
    let (dir, repo) = new_repo(&git);
    let canonical = dir.path().canonicalize().expect("canonical");
    assert_eq!(repo.root().canonicalize().expect("canonical"), canonical);
    assert_eq!(repo.prefix(), "");

    fs::create_dir_all(dir.path().join("a/b")).expect("mkdir");
    let sub = git
        .discover(&dir.path().join("a/b"))
        .expect("discover")
        .expect("a repo");
    assert_eq!(sub.root().canonicalize().expect("canonical"), canonical);
    assert_eq!(sub.prefix(), "a/b");
}

#[test]
fn a_plain_folder_is_not_a_repository() {
    let Some(git) = git() else { return };
    let dir = tempfile::tempdir().expect("tempdir");
    // Skip if the temp dir happens to live inside a repository.
    if git.discover(dir.path()).expect("discover").is_some() {
        return;
    }
    assert!(git.discover(dir.path()).expect("discover").is_none());
}

#[test]
fn status_follows_add_modify_delete_and_rename() {
    let Some(git) = git() else { return };
    let (dir, repo) = new_repo(&git);

    let status = repo.status().expect("status");
    assert_eq!(status.head, Head::Unborn("main".into()));
    assert!(status.entries.is_empty());

    write(&dir, "keep.txt", "one\n");
    write(&dir, "gone.txt", "bye\n");
    write(
        &dir,
        "old name.txt",
        "some longer content to match the rename\nline two\n",
    );
    write(&dir, "dir/ñandú.txt", "x\n");
    let status = repo.status().expect("status");
    assert_eq!(status.entries.len(), 4);
    assert!(status.entries.iter().all(|e| e.untracked));
    assert!(status.entries.iter().any(|e| e.path == "dir/ñandú.txt"));

    commit_all(&git, &dir);
    let status = repo.status().expect("status");
    assert!(matches!(status.head, Head::Branch(ref b) if b == "main"));
    assert!(status.entries.is_empty());

    write(&dir, "keep.txt", "two\n");
    fs::remove_file(dir.path().join("gone.txt")).expect("rm");
    fs::rename(
        dir.path().join("old name.txt"),
        dir.path().join("new name.txt"),
    )
    .expect("mv");
    write(&dir, "fresh.txt", "new\n");
    raw(&git, dir.path(), &["add", "new name.txt", "old name.txt"]);

    let status = repo.status().expect("status");
    let keep = entry(&status, "keep.txt");
    assert_eq!(
        (keep.index, keep.worktree),
        (Change::Unmodified, Change::Modified)
    );
    assert_eq!(entry(&status, "gone.txt").worktree, Change::Deleted);
    let renamed = entry(&status, "new name.txt");
    assert_eq!(renamed.index, Change::Renamed);
    assert_eq!(renamed.orig_path.as_deref(), Some("old name.txt"));
    assert!(entry(&status, "fresh.txt").untracked);
}

#[test]
fn stage_and_unstage_after_a_commit() {
    let Some(git) = git() else { return };
    let (dir, repo) = new_repo(&git);
    write(&dir, "a.txt", "1\n");
    commit_all(&git, &dir);

    write(&dir, "a.txt", "2\n");
    write(&dir, "b.txt", "new\n");
    repo.stage(&["a.txt".into(), "b.txt".into()])
        .expect("stage");
    let status = repo.status().expect("status");
    assert_eq!(entry(&status, "a.txt").index, Change::Modified);
    assert_eq!(entry(&status, "b.txt").index, Change::Added);

    repo.unstage(&["a.txt".into(), "b.txt".into()])
        .expect("unstage");
    let status = repo.status().expect("status");
    assert_eq!(entry(&status, "a.txt").index, Change::Unmodified);
    assert_eq!(entry(&status, "a.txt").worktree, Change::Modified);
    assert!(entry(&status, "b.txt").untracked);
}

#[test]
fn stage_and_unstage_on_an_unborn_branch() {
    let Some(git) = git() else { return };
    let (dir, repo) = new_repo(&git);
    write(&dir, "sub/a.txt", "1\n");
    repo.stage(&["sub".into()]).expect("stage");
    assert_eq!(
        entry(&repo.status().expect("status"), "sub/a.txt").index,
        Change::Added
    );

    repo.unstage(&["sub".into()]).expect("unstage");
    assert!(entry(&repo.status().expect("status"), "sub/a.txt").untracked);
    assert!(dir.path().join("sub/a.txt").is_file());
}

#[test]
fn discard_restores_tracked_and_deletes_new_files() {
    let Some(git) = git() else { return };
    let (dir, repo) = new_repo(&git);
    write(&dir, "a.txt", "1\n");
    write(&dir, "b.txt", "1\n");
    commit_all(&git, &dir);

    write(&dir, "a.txt", "2\n");
    fs::remove_file(dir.path().join("b.txt")).expect("rm");
    write(&dir, "new/c.txt", "c\n");
    write(&dir, "staged.txt", "s\n");
    repo.stage(&["staged.txt".into()]).expect("stage");

    let all: Vec<String> = ["a.txt", "b.txt", "new", "staged.txt"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    repo.discard(&all).expect("discard");

    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).expect("read"),
        "1\n"
    );
    assert!(dir.path().join("b.txt").is_file());
    assert!(!dir.path().join("new/c.txt").exists());
    assert!(!dir.path().join("staged.txt").exists());
    assert!(repo.status().expect("status").entries.is_empty());
}

#[test]
fn discard_undoes_a_staged_rename() {
    let Some(git) = git() else { return };
    let (dir, repo) = new_repo(&git);
    write(
        &dir,
        "old.txt",
        "some content that is long enough\nsecond line\n",
    );
    commit_all(&git, &dir);
    raw(&git, dir.path(), &["mv", "old.txt", "new.txt"]);

    repo.discard(&["new.txt".into()]).expect("discard");
    assert!(dir.path().join("old.txt").is_file());
    assert!(!dir.path().join("new.txt").exists());
    assert!(repo.status().expect("status").entries.is_empty());
}

#[test]
fn split_by_prefix_on_a_real_repository() {
    let Some(git) = git() else { return };
    let (dir, repo) = new_repo(&git);
    write(&dir, "ws/a.txt", "1\n");
    write(&dir, "other.txt", "1\n");
    commit_all(&git, &dir);
    write(&dir, "ws/a.txt", "2\n");
    write(&dir, "other.txt", "2\n");
    repo.stage(&["other.txt".into()]).expect("stage");

    let status = repo.status().expect("status");
    let (inside, staged_outside) = status.split_by_prefix("ws");
    assert_eq!(inside.len(), 1);
    assert_eq!(inside[0].path, "ws/a.txt");
    assert_eq!(staged_outside, 1);
}

#[test]
fn author_reads_the_local_config() {
    let Some(git) = git() else { return };
    let (_dir, repo) = new_repo(&git);
    let author = repo.author().expect("author").expect("some author");
    assert_eq!(author.name, "Test User");
    assert_eq!(author.email, "test@example.com");
}
