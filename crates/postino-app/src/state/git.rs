//! Plain data behind the Git integration (milestone 0.5.0): whether git is available, the
//! repository of the open workspace with its last status, and the rows of the Source Control
//! panel (design 2b). Running git is IO, so `views/git` calls `postino-git` on the background
//! executor and stores the results here; render only reads them.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use postino_core::Method;
use postino_git::{Change, DetectError, Entry, Git, Head, Repo, Status};
use postino_workspace::{Node, natural_cmp};

/// Whether the git executable could be used, as last detected.
#[derive(Debug, Clone, Default)]
pub enum GitAvailability {
    /// Detection has not finished yet.
    #[default]
    Detecting,
    /// No usable git: not found, too old or failed to run.
    Missing(Arc<DetectError>),
    /// A git that Postino can use.
    Ready(Git),
}

impl GitAvailability {
    /// The usable git, if any.
    pub fn git(&self) -> Option<&Git> {
        match self {
            GitAvailability::Ready(git) => Some(git),
            _ => None,
        }
    }
}

/// The repository of the open workspace.
#[derive(Debug, Clone, Default)]
pub enum RepoState {
    /// No workspace, no git, or discovery has not finished yet.
    #[default]
    Unknown,
    /// The workspace is not inside a git work tree.
    NotARepo,
    /// The workspace is inside this repository. `status` is `None` until the first
    /// `git status` finishes.
    Repo {
        /// The repository, with the workspace's prefix.
        repo: Repo,
        /// The last status read, shared so a clone is O(1).
        status: Option<Arc<Status>>,
    },
    /// Discovery or status failed. The message is git's own, shown verbatim.
    Error(String),
}

/// The Git state of the window: availability, repository, and what the panel folds.
#[derive(Debug, Clone, Default)]
pub struct GitState {
    /// Whether git can be used.
    pub availability: GitAvailability,
    /// The open workspace's repository.
    pub repo: RepoState,
    /// The panel's collapsed groups and folders, by [`ChangeRow::key`].
    pub collapsed: HashSet<String>,
    /// Whether the panel lists files without their folders (its "Group by folder" button off).
    pub flat: bool,
    /// The panel's rows, rebuilt when the status, the tree or a folding changes, never in render.
    pub rows: Arc<Vec<ChangeRow>>,
    /// [`staged_outside`] of the last status.
    pub staged_outside: Arc<Vec<String>>,
    /// The commit author from git config (`user.name` and `user.email`), read when the
    /// repository is discovered. `None` without a repository or without an author configured.
    pub author: Option<postino_git::Author>,
}

impl GitState {
    /// The repository and its last status, when both are known.
    pub fn status(&self) -> Option<(&Repo, &Arc<Status>)> {
        match &self.repo {
            RepoState::Repo {
                repo,
                status: Some(status),
            } => Some((repo, status)),
            _ => None,
        }
    }

    /// The repository, whether or not its status has been read yet.
    pub fn repo(&self) -> Option<&Repo> {
        match &self.repo {
            RepoState::Repo { repo, .. } => Some(repo),
            _ => None,
        }
    }

    /// The branch for the status bar: its name, the short hash of a detached `HEAD`, or the
    /// name of a branch with no commit yet. `None` before the first status.
    pub fn branch_label(&self) -> Option<String> {
        self.status().map(|(_, status)| match &status.head {
            Head::Branch(name) | Head::Unborn(name) | Head::Detached(name) => name.clone(),
        })
    }

    /// How many files inside the workspace have changes: the rail's counter.
    pub fn change_count(&self) -> usize {
        self.status().map_or(0, |(repo, status)| {
            status.split_by_prefix(repo.prefix()).0.len()
        })
    }
}

/// The checkbox of a panel row. It mirrors the git index, so it agrees with the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Every change is staged.
    On,
    /// Nothing is staged.
    Off,
    /// Some changes are staged and some are not.
    Partial,
}

impl Check {
    /// The check of a group of rows: `On` or `Off` when they all agree, `Partial` otherwise.
    fn combine(checks: impl IntoIterator<Item = Check>) -> Check {
        let mut result = None;
        for check in checks {
            result = match (result, check) {
                (None, check) => Some(check),
                (Some(a), b) if a == b => Some(a),
                _ => return Check::Partial,
            };
        }
        result.unwrap_or(Check::Off)
    }
}

/// The two top level groups of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeGroup {
    /// Tracked files with changes, conflicts included.
    Changes,
    /// Files git does not track yet.
    Unversioned,
}

/// What a panel row stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeRowKind {
    /// A group header with its file count.
    Group(ChangeGroup),
    /// A folder, with how many changed files it holds.
    Folder,
    /// A changed file.
    File {
        /// The status letter: `M`, `A`, `D`, `R`, `T`, `U` (untracked) or `!` (conflict).
        letter: char,
        /// Whether the file is deleted in the work tree (drawn struck through).
        deleted: bool,
    },
}

/// One row of the Source Control panel, in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRow {
    /// A stable key: the group name, or the group and the workspace relative path.
    pub key: String,
    /// What the row is.
    pub kind: ChangeRowKind,
    /// Nesting depth, 0 for a group header.
    pub depth: usize,
    /// The text shown: the file or folder name (for a `.postino` file, without the extension).
    pub label: String,
    /// The workspace relative path of a file or folder (`/` separated), empty for a group.
    pub path: String,
    /// How many files the group or folder holds, 0 for a file.
    pub count: usize,
    /// The checkbox.
    pub check: Check,
    /// Whether a group or folder is expanded.
    pub expanded: bool,
    /// The HTTP method of a `.postino` file, from the collection tree ([`fill_methods`]).
    pub method: Option<Method>,
    /// The repository relative paths of every file under the row, what staging or discarding it
    /// touches. Never empty.
    pub repo_paths: Vec<String>,
}

/// The rows of the panel for the entries of `status` inside `prefix` (the workspace's place in
/// the repository, see [`Repo::prefix`]). Groups and folders in `collapsed` are listed without
/// their children. `flat` lists the files of each group without their folders.
pub fn change_rows(
    status: &Status,
    prefix: &str,
    collapsed: &HashSet<String>,
    flat: bool,
) -> Vec<ChangeRow> {
    let (entries, _) = status.split_by_prefix(prefix);
    let mut rows = Vec::new();
    for group in [ChangeGroup::Changes, ChangeGroup::Unversioned] {
        let files: Vec<FileInfo> = entries
            .iter()
            .filter(|entry| entry.untracked == (group == ChangeGroup::Unversioned))
            .map(|entry| FileInfo::new(entry, prefix))
            .collect();
        if files.is_empty() {
            continue;
        }
        let key = group_key(group).to_string();
        let expanded = !collapsed.contains(&key);
        rows.push(ChangeRow {
            key: key.clone(),
            kind: ChangeRowKind::Group(group),
            depth: 0,
            label: String::new(),
            path: String::new(),
            count: files.len(),
            check: Check::combine(files.iter().map(|file| file.check)),
            expanded,
            method: None,
            repo_paths: files.iter().map(|file| file.repo_path.clone()).collect(),
        });
        if expanded {
            let mut tree = FolderNode::default();
            for file in files {
                if flat {
                    tree.files.push(file);
                } else {
                    tree.insert(file);
                }
            }
            tree.push_rows(&key, "", 1, collapsed, &mut rows);
        }
    }
    rows
}

/// The repository relative paths outside the workspace that have a staged change: they would go
/// into the next commit although the panel does not list them.
pub fn staged_outside(status: &Status, prefix: &str) -> Vec<String> {
    status
        .entries
        .iter()
        .filter(|entry| entry.index != Change::Unmodified && !inside(&entry.path, prefix))
        .map(|entry| entry.path.clone())
        .collect()
}

/// Whether the repository relative `path` is inside `prefix`.
fn inside(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Fills [`ChangeRow::method`] of the `.postino` rows from the workspace's collection tree, which
/// already holds every request's method. Deleted and broken requests keep `None`.
pub fn fill_methods(rows: &mut [ChangeRow], tree: &[Node]) {
    fn walk(nodes: &[Node], methods: &mut HashMap<String, Method>) {
        for node in nodes {
            match node {
                Node::Folder(folder) => walk(&folder.children, methods),
                Node::Request(request) => {
                    if let Some(method) = &request.method {
                        methods.insert(request.id.clone(), method.clone());
                    }
                }
            }
        }
    }
    if !rows.iter().any(|row| row.path.ends_with(".postino")) {
        return;
    }
    let mut methods = HashMap::new();
    walk(tree, &mut methods);
    for row in rows {
        if matches!(row.kind, ChangeRowKind::File { .. }) {
            row.method = methods.get(&row.path).cloned();
        }
    }
}

fn group_key(group: ChangeGroup) -> &'static str {
    match group {
        ChangeGroup::Changes => "changes",
        ChangeGroup::Unversioned => "unversioned",
    }
}

/// A changed file, ready to be placed in the tree.
struct FileInfo {
    /// Workspace relative path.
    path: String,
    repo_path: String,
    check: Check,
    letter: char,
    deleted: bool,
}

impl FileInfo {
    fn new(entry: &Entry, prefix: &str) -> Self {
        let path = if prefix.is_empty() {
            entry.path.clone()
        } else {
            entry.path[prefix.len()..]
                .trim_start_matches('/')
                .to_string()
        };
        let staged = entry.index != Change::Unmodified;
        let unstaged = entry.untracked || entry.worktree != Change::Unmodified;
        let check = match (staged, unstaged) {
            (true, false) => Check::On,
            (true, true) => Check::Partial,
            _ => Check::Off,
        };
        let change = if entry.worktree != Change::Unmodified {
            entry.worktree
        } else {
            entry.index
        };
        let letter = if entry.conflicted {
            '!'
        } else if entry.untracked {
            'U'
        } else {
            match change {
                Change::Added | Change::Copied => 'A',
                Change::Deleted => 'D',
                Change::Renamed => 'R',
                Change::TypeChanged => 'T',
                Change::Modified | Change::Unmodified => 'M',
            }
        };
        FileInfo {
            path,
            repo_path: entry.path.clone(),
            check,
            letter,
            deleted: change == Change::Deleted,
        }
    }
}

/// A folder of the panel tree while it is being built.
#[derive(Default)]
struct FolderNode {
    folders: BTreeMap<String, FolderNode>,
    files: Vec<FileInfo>,
}

impl FolderNode {
    fn insert(&mut self, file: FileInfo) {
        let parts: Vec<String> = file.path.split('/').map(str::to_string).collect();
        let mut node = self;
        for part in &parts[..parts.len().saturating_sub(1)] {
            node = node.folders.entry(part.clone()).or_default();
        }
        node.files.push(file);
    }

    fn files_recursive(&self) -> Vec<&FileInfo> {
        let mut out: Vec<&FileInfo> = self.files.iter().collect();
        for folder in self.folders.values() {
            out.extend(folder.files_recursive());
        }
        out
    }

    fn push_rows(
        &self,
        group_key: &str,
        parent: &str,
        depth: usize,
        collapsed: &HashSet<String>,
        rows: &mut Vec<ChangeRow>,
    ) {
        let mut folders: Vec<(&String, &FolderNode)> = self.folders.iter().collect();
        folders.sort_by(|a, b| name_order(a.0, b.0));
        for (name, folder) in folders {
            let path = join(parent, name);
            let key = format!("{group_key}:{path}");
            let expanded = !collapsed.contains(&key);
            let files = folder.files_recursive();
            rows.push(ChangeRow {
                key: key.clone(),
                kind: ChangeRowKind::Folder,
                depth,
                label: name.clone(),
                path: path.clone(),
                count: files.len(),
                check: Check::combine(files.iter().map(|file| file.check)),
                expanded,
                method: None,
                repo_paths: files.iter().map(|file| file.repo_path.clone()).collect(),
            });
            if expanded {
                folder.push_rows(group_key, &path, depth + 1, collapsed, rows);
            }
        }
        let mut files: Vec<&FileInfo> = self.files.iter().collect();
        files.sort_by(|a, b| name_order(&a.path, &b.path));
        for file in files {
            let name = file.path.rsplit('/').next().unwrap_or(&file.path);
            let label = name.strip_suffix(".postino").unwrap_or(name).to_string();
            rows.push(ChangeRow {
                key: format!("{group_key}:{}", file.path),
                kind: ChangeRowKind::File {
                    letter: file.letter,
                    deleted: file.deleted,
                },
                depth,
                label,
                path: file.path.clone(),
                count: 0,
                check: file.check,
                expanded: false,
                method: None,
                repo_paths: vec![file.repo_path.clone()],
            });
        }
    }
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn name_order(a: &str, b: &str) -> Ordering {
    natural_cmp(a, b).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn entry(path: &str, index: Change, worktree: Change) -> Entry {
        Entry {
            path: path.to_string(),
            orig_path: None,
            index,
            worktree,
            conflicted: false,
            untracked: false,
        }
    }

    fn untracked(path: &str) -> Entry {
        Entry {
            untracked: true,
            ..entry(path, Change::Unmodified, Change::Added)
        }
    }

    fn status(entries: Vec<Entry>) -> Status {
        Status {
            head: Head::Branch("main".to_string()),
            head_oid: Some("a41c9e2".to_string()),
            upstream: None,
            ahead: 0,
            behind: 0,
            entries,
        }
    }

    fn summary(rows: &[ChangeRow]) -> Vec<String> {
        rows.iter()
            .map(|row| {
                let check = match row.check {
                    Check::On => "x",
                    Check::Off => " ",
                    Check::Partial => "-",
                };
                let name = match &row.kind {
                    ChangeRowKind::Group(group) => format!("{}({})", group_key(*group), row.count),
                    ChangeRowKind::Folder => format!("{}/ ({})", row.label, row.count),
                    ChangeRowKind::File { letter, .. } => format!("{} {letter}", row.label),
                };
                format!("{}[{check}] {name}", "  ".repeat(row.depth))
            })
            .collect()
    }

    #[test]
    fn builds_the_tree_with_groups_folders_and_checks() {
        let status = status(vec![
            entry("auth/login.postino", Change::Modified, Change::Unmodified),
            entry("auth/refresh.postino", Change::Added, Change::Unmodified),
            entry("users/create.postino", Change::Modified, Change::Unmodified),
            entry("users/list.postino", Change::Unmodified, Change::Modified),
            entry("health.postino", Change::Unmodified, Change::Deleted),
            untracked("users/delete.postino"),
        ]);
        let rows = change_rows(&status, "", &HashSet::new(), false);
        assert_eq!(
            summary(&rows),
            vec![
                "[-] changes(5)",
                "  [x] auth/ (2)",
                "    [x] login M",
                "    [x] refresh A",
                "  [-] users/ (2)",
                "    [x] create M",
                "    [ ] list M",
                "  [ ] health D",
                "[ ] unversioned(1)",
                "  [ ] users/ (1)",
                "    [ ] delete U",
            ]
        );
        assert_eq!(rows[0].repo_paths.len(), 5);
        assert!(matches!(
            rows[7].kind,
            ChangeRowKind::File { deleted: true, .. }
        ));
    }

    #[test]
    fn a_file_with_staged_and_unstaged_changes_is_partial() {
        let status = status(vec![entry("a.postino", Change::Modified, Change::Modified)]);
        let rows = change_rows(&status, "", &HashSet::new(), false);
        assert_eq!(rows[1].check, Check::Partial);
    }

    #[test]
    fn only_lists_the_workspace_and_counts_staged_files_outside_it() {
        let status = status(vec![
            entry(
                "api/auth/login.postino",
                Change::Modified,
                Change::Unmodified,
            ),
            entry("src/main.rs", Change::Modified, Change::Unmodified),
            entry("README.md", Change::Unmodified, Change::Modified),
        ]);
        let rows = change_rows(&status, "api", &HashSet::new(), false);
        assert_eq!(
            summary(&rows),
            vec!["[x] changes(1)", "  [x] auth/ (1)", "    [x] login M"]
        );
        assert_eq!(rows[2].path, "auth/login.postino");
        assert_eq!(rows[2].repo_paths, vec!["api/auth/login.postino"]);
        assert_eq!(staged_outside(&status, "api"), vec!["src/main.rs"]);
        assert!(staged_outside(&status, "").is_empty());
    }

    #[test]
    fn collapsed_groups_and_folders_hide_their_children() {
        let status = status(vec![
            entry("auth/login.postino", Change::Modified, Change::Unmodified),
            untracked("new.postino"),
        ]);
        let collapsed: HashSet<String> = ["changes:auth".to_string(), "unversioned".to_string()]
            .into_iter()
            .collect();
        let rows = change_rows(&status, "", &collapsed, false);
        assert_eq!(
            summary(&rows),
            vec!["[x] changes(1)", "  [x] auth/ (1)", "[ ] unversioned(1)"]
        );
        assert!(!rows[1].expanded);
    }

    #[test]
    fn flat_lists_files_without_folders() {
        let status = status(vec![
            entry("b/x.postino", Change::Modified, Change::Unmodified),
            entry("a/y.postino", Change::Unmodified, Change::Modified),
        ]);
        let rows = change_rows(&status, "", &HashSet::new(), true);
        assert_eq!(
            summary(&rows),
            vec!["[-] changes(2)", "  [ ] y M", "  [x] x M"]
        );
    }

    #[test]
    fn conflicts_are_marked() {
        let status = status(vec![Entry {
            conflicted: true,
            ..entry("a.postino", Change::Modified, Change::Modified)
        }]);
        let rows = change_rows(&status, "", &HashSet::new(), false);
        assert!(matches!(
            rows[1].kind,
            ChangeRowKind::File { letter: '!', .. }
        ));
    }

    #[test]
    fn folders_sort_naturally() {
        let status = status(vec![
            entry("v10/a.postino", Change::Unmodified, Change::Modified),
            entry("v2/a.postino", Change::Unmodified, Change::Modified),
        ]);
        let rows = change_rows(&status, "", &HashSet::new(), false);
        assert_eq!(rows[1].label, "v2");
        assert_eq!(rows[3].label, "v10");
    }
}
