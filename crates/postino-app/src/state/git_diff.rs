//! The diff tab of the Git integration (designs 2a, 2b, 7a): which file it shows and what is
//! known about its changes. The patch itself is read by `views/git/diff_tab.rs` on the
//! background executor and turned into rows by `state/diff_view.rs`; this is only plain data.

use telaio_git::{FileDiff, LineKind};

use crate::state::ui_tabs::RequestTab;

/// The id of the diff tab. There is one: clicking another changed file reuses it, like a
/// preview.
pub const TAB_ID: &str = "git:diff";

/// What the diff tab compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffTarget {
    /// The last commit against the files on disk: everything the next commit could include.
    WorkTree,
    /// What a commit changed, against its first parent (opened from the log tab).
    Commit {
        /// The commit, full id.
        oid: String,
        /// Its short id, for the titles.
        short: String,
    },
}

impl DiffTarget {
    /// The tab's detail after the file name: "Working Tree" or the short commit id.
    pub fn detail(&self) -> String {
        match self {
            DiffTarget::WorkTree => rust_i18n::t!("git.diff.working_tree").into_owned(),
            DiffTarget::Commit { short, .. } => short.clone(),
        }
    }
}

/// What a diff tab shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffTab {
    /// The file, relative to the repository root, `/` separated.
    pub repo_path: String,
    /// The file, relative to the workspace: what the header shows and, for a request, its id.
    pub path: String,
    /// What is compared.
    pub target: DiffTarget,
    /// The status letter of the file, as in the Source Control panel.
    pub letter: char,
    /// Bumped every time the patch is read again, so the view knows when to reload the viewer.
    pub generation: u64,
    /// What the last read found, `None` while it runs.
    pub summary: Option<DiffSummary>,
}

/// What a read of the patch found, for the header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffSummary {
    /// Added lines.
    pub additions: usize,
    /// Removed lines.
    pub deletions: usize,
    /// How many blocks of changes (runs of removed and added lines) the file has.
    pub blocks: usize,
    /// Why the patch could not be read, shown instead of the diff.
    pub error: Option<String>,
}

impl DiffTab {
    /// A tab for `repo_path`, shown as `path` (relative to the workspace).
    pub fn new(repo_path: String, path: String, target: DiffTarget, letter: char) -> Self {
        DiffTab {
            repo_path,
            path,
            target,
            letter,
            generation: 0,
            summary: None,
        }
    }

    /// The tab's label: the file name.
    pub fn label(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The folder of the file with a trailing `/`, empty at the workspace root.
    pub fn folder(&self) -> &str {
        match self.path.rfind('/') {
            Some(index) => &self.path[..=index],
            None => "",
        }
    }

    /// The request id when the file is a `.postino` request, for "Open file".
    pub fn request_id(&self) -> Option<&str> {
        self.path
            .ends_with(".postino")
            .then_some(self.path.as_str())
    }

    /// The environment the file holds when it is `environments/<name>.env` (or its
    /// `<name>.local.env`), for "Open file".
    pub fn environment_name(&self) -> Option<&str> {
        let file = self.path.strip_prefix("environments/")?;
        if file.contains('/') {
            return None;
        }
        let stem = file.strip_suffix(".env")?;
        let name = stem.strip_suffix(".local").unwrap_or(stem);
        (!name.is_empty()).then_some(name)
    }
}

/// Where a line of a `.postino` file is edited in the request editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestLocation {
    /// The tab of the request editor, `None` for the request line (the URL bar).
    pub tab: Option<RequestTab>,
    /// The 0 based line inside the tab's code editor (Body as text, Pre, Post, Docs).
    pub row: Option<u32>,
}

/// Where line `line` (0 based) of the `.postino` file made of `lines` is in the request editor:
/// the header block is the Headers tab, a section is its tab, and a line of a section that is
/// edited as text is also its row in that editor (format.md, sections 3 to 5).
pub fn request_location<'a>(
    lines: impl IntoIterator<Item = &'a str>,
    line: usize,
) -> RequestLocation {
    // Before the request line, then the header block, then sections.
    let mut seen_request_line = false;
    let mut in_headers = false;
    let mut location = RequestLocation {
        tab: None,
        row: None,
    };
    // The open section: its tab, whether it is text, and the index of its marker.
    let mut section: Option<(Option<RequestTab>, bool, usize)> = None;
    for (index, text) in lines.into_iter().enumerate().take(line + 1) {
        if let Some(marker) = text.strip_prefix("::: ") {
            let mut words = marker.split_whitespace();
            let (tab, is_text) = match (words.next(), words.next()) {
                (Some("query"), _) => (Some(RequestTab::Params), false),
                (Some("auth"), _) => (Some(RequestTab::Auth), false),
                (Some("body"), kind) => (
                    Some(RequestTab::Body),
                    !matches!(kind, Some("form" | "multipart")),
                ),
                (Some("pre"), _) => (Some(RequestTab::Pre), true),
                (Some("post"), _) => (Some(RequestTab::Post), true),
                (Some("docs"), _) => (Some(RequestTab::Docs), true),
                _ => (None, false),
            };
            section = Some((tab, is_text, index));
            in_headers = false;
        }
        location = match section {
            Some((tab, is_text, marker)) => RequestLocation {
                tab,
                row: is_text.then(|| index.saturating_sub(marker + 1) as u32),
            },
            None if !seen_request_line => {
                seen_request_line = !text.trim().is_empty();
                in_headers = seen_request_line;
                RequestLocation {
                    tab: None,
                    row: None,
                }
            }
            None => {
                in_headers = in_headers && !text.trim().is_empty();
                RequestLocation {
                    tab: in_headers.then_some(RequestTab::Headers),
                    row: None,
                }
            }
        };
    }
    location
}

/// The 0 based line of the new file where change block `block` of `diff` starts: its first
/// added line, or for a pure removal the line that follows it.
pub fn block_new_line(diff: &FileDiff, block: usize) -> Option<usize> {
    let block = diff.blocks.get(block)?;
    Some(block.new_start.saturating_sub(1) as usize)
}

/// The change block that list row `row` is part of, in the `unified` or side by side layout.
pub fn block_at_row(diff: &FileDiff, row: usize, unified: bool) -> Option<usize> {
    let start = |block: &telaio_git::ChangeBlock| {
        if unified {
            block.first_line
        } else {
            block.first_split_row
        }
    };
    let index = diff
        .blocks
        .partition_point(|block| start(block) <= row)
        .checked_sub(1)?;
    let block = &diff.blocks[index];
    let rows = if unified {
        block.removed + block.added
    } else {
        block.removed.max(block.added)
    };
    (row < start(block) + rows).then_some(index)
}

/// The item before or after `current` in `items`, for stepping to the previous or next file.
pub fn neighbour<T>(items: &[T], current: usize, forward: bool) -> Option<&T> {
    if forward {
        items.get(current + 1)
    } else {
        current.checked_sub(1).and_then(|index| items.get(index))
    }
}

/// Counts the added and removed lines and the change blocks of a parsed file diff.
pub fn summarize(diff: &FileDiff) -> DiffSummary {
    let count = |kind: LineKind| diff.lines.iter().filter(|line| line.kind == kind).count();
    DiffSummary {
        additions: count(LineKind::Added),
        deletions: count(LineKind::Removed),
        blocks: diff.blocks.len(),
        error: None,
    }
}

/// Whether a hunk of the work tree diff is in the index already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkStage {
    /// Not staged: "Stage" applies it to the index.
    Unstaged,
    /// Staged: "Unstage" takes it back out.
    Staged,
    /// Partly staged: only Revert is offered.
    Mixed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn counts_lines_and_blocks_but_not_the_file_header() {
        // Two hunks of git, but three blocks: a context line splits the first hunk.
        let patch = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,2 @@\n-a\n+A\n b\n-c\n\
                     @@ -9 +9,2 @@\n z\n+y\n";
        assert_eq!(
            summarize(&telaio_git::parse_file_diff(patch)),
            DiffSummary {
                additions: 2,
                deletions: 2,
                blocks: 3,
                error: None,
            }
        );
    }

    #[test]
    fn labels_and_folder() {
        let tab = DiffTab::new(
            "api/auth/login.postino".into(),
            "auth/login.postino".into(),
            DiffTarget::WorkTree,
            'M',
        );
        assert_eq!(tab.label(), "login.postino");
        assert_eq!(tab.folder(), "auth/");
        assert_eq!(tab.request_id(), Some("auth/login.postino"));
        let root = DiffTab::new(
            ".gitignore".into(),
            ".gitignore".into(),
            DiffTarget::WorkTree,
            'A',
        );
        assert_eq!(root.folder(), "");
        assert_eq!(root.request_id(), None);
    }

    #[test]
    fn environment_files_name_their_environment() {
        let tab = |path: &str| DiffTab::new(path.into(), path.into(), DiffTarget::WorkTree, 'M');
        assert_eq!(tab("environments/dev.env").environment_name(), Some("dev"));
        assert_eq!(
            tab("environments/dev.local.env").environment_name(),
            Some("dev")
        );
        assert_eq!(tab("environments/x/dev.env").environment_name(), None);
        assert_eq!(tab("dev.env").environment_name(), None);
        assert_eq!(tab("environments/.order").environment_name(), None);
    }

    #[test]
    fn locates_lines_of_a_request() {
        let file = "POST {{base}}/login\nAccept: */*\n# X-Debug: 1\n\n::: query\nverbose = true\n\n\
                    ::: body json\n{\n  \"user\": \"a\"\n}\n\n::: body form\na = 1\n\n::: post\n\
                    test(\"ok\")\n::: unknown\nx";
        let at = |line: usize| request_location(file.lines(), line);
        let location = |tab, row| RequestLocation { tab, row };
        assert_eq!(at(0), location(None, None));
        assert_eq!(at(1), location(Some(RequestTab::Headers), None));
        assert_eq!(at(2), location(Some(RequestTab::Headers), None));
        assert_eq!(at(3), location(None, None));
        assert_eq!(at(5), location(Some(RequestTab::Params), None));
        assert_eq!(at(7), location(Some(RequestTab::Body), Some(0)));
        assert_eq!(at(9), location(Some(RequestTab::Body), Some(1)));
        assert_eq!(at(13), location(Some(RequestTab::Body), None));
        assert_eq!(at(16), location(Some(RequestTab::Post), Some(0)));
        assert_eq!(at(18), location(None, None));
        // Past the end: the last line's place.
        assert_eq!(at(99), location(None, None));
    }

    #[test]
    fn a_header_block_ends_at_a_marker_without_a_blank_line() {
        let file = "GET x\nA: 1\n::: pre\nlog()";
        assert_eq!(
            request_location(file.lines(), 3),
            RequestLocation {
                tab: Some(RequestTab::Pre),
                row: Some(0),
            }
        );
    }

    #[test]
    fn finds_the_block_of_a_row_and_its_new_line() {
        // Blocks: lines 1-2 (one removed, one added), line 4 (added).
        let patch = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,4 @@\n a\n-b\n+B\n c\n+d\n";
        let diff = telaio_git::parse_file_diff(patch);
        assert_eq!(block_at_row(&diff, 0, true), None);
        assert_eq!(block_at_row(&diff, 1, true), Some(0));
        assert_eq!(block_at_row(&diff, 2, true), Some(0));
        assert_eq!(block_at_row(&diff, 3, true), None);
        assert_eq!(block_at_row(&diff, 4, true), Some(1));
        // Side by side, the first block is one row.
        assert_eq!(block_at_row(&diff, 1, false), Some(0));
        assert_eq!(block_at_row(&diff, 2, false), None);
        assert_eq!(block_at_row(&diff, 3, false), Some(1));
        assert_eq!(block_new_line(&diff, 0), Some(1));
        assert_eq!(block_new_line(&diff, 1), Some(3));
        assert_eq!(block_new_line(&diff, 2), None);
    }

    #[test]
    fn neighbours_stop_at_the_ends() {
        let items = [1, 2, 3];
        assert_eq!(neighbour(&items, 0, true), Some(&2));
        assert_eq!(neighbour(&items, 2, true), None);
        assert_eq!(neighbour(&items, 0, false), None);
        assert_eq!(neighbour(&items, 2, false), Some(&2));
    }
}
