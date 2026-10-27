//! The diff tab of the Git integration (designs 2a, 2b, 7a): which file it shows and what is
//! known about its changes. The patch itself is read by `views/git/diff_tab.rs` on the
//! background executor and handed to gpui-component's diff viewer; this is only plain data.

/// The id of the diff tab. There is one: clicking another changed file reuses it, like a
/// preview.
pub const TAB_ID: &str = "git:diff";

/// What the diff tab compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffTarget {
    /// The last commit against the files on disk: everything the next commit could include.
    WorkTree,
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
    /// How many hunks (blocks of changes) the patch has.
    pub hunks: usize,
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

    /// The request id when the file is a `.postino` request, for "Open request".
    pub fn request_id(&self) -> Option<&str> {
        self.path
            .ends_with(".postino")
            .then_some(self.path.as_str())
    }
}

/// Counts the additions, deletions and hunks of a single file patch (`git diff` output).
pub fn summarize(patch: &str) -> DiffSummary {
    let mut summary = DiffSummary::default();
    let mut in_hunk = false;
    for line in patch.lines() {
        if line.starts_with("@@") {
            summary.hunks += 1;
            in_hunk = true;
        } else if !in_hunk {
            continue;
        } else if line.starts_with('+') {
            summary.additions += 1;
        } else if line.starts_with('-') {
            summary.deletions += 1;
        }
    }
    summary
}

/// Where a hunk's Stage and Revert buttons go: under the last line of the hunk, so they follow
/// the block they act on. On the new side, or on the old side when the hunk leaves no line on the
/// new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HunkAnchor {
    /// Whether `line` is on the new (modified) side.
    pub modified_side: bool,
    /// The 1 based line number.
    pub line: usize,
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

/// The anchor of every hunk of a single file patch, in order.
pub fn hunk_anchors(patch: &str) -> Vec<HunkAnchor> {
    patch
        .lines()
        .filter(|line| line.starts_with("@@ "))
        .filter_map(|header| {
            let mut ranges = header.split(' ').skip(1);
            let old = parse_range(ranges.next()?.strip_prefix('-')?)?;
            let new = parse_range(ranges.next()?.strip_prefix('+')?)?;
            Some(if new.1 > 0 {
                HunkAnchor {
                    modified_side: true,
                    line: (new.0 + new.1 - 1).max(1),
                }
            } else {
                HunkAnchor {
                    modified_side: false,
                    line: (old.0 + old.1 - 1).max(1),
                }
            })
        })
        .collect()
}

/// Parses `start,count` (or `start`, a count of 1) of a hunk header.
fn parse_range(range: &str) -> Option<(usize, usize)> {
    match range.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn counts_hunks_and_lines_but_not_the_file_header() {
        let patch = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n\
                     @@ -9 +9,2 @@\n z\n+y\n";
        assert_eq!(
            summarize(patch),
            DiffSummary {
                additions: 2,
                deletions: 1,
                hunks: 2,
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
    fn anchors_go_on_the_last_new_line_unless_the_hunk_only_removes() {
        let patch = "@@ -1,3 +1,4 @@ ctx\n a\n@@ -10 +11,0 @@\n-x\n@@ -0,0 +1 @@\n+y\n";
        assert_eq!(
            hunk_anchors(patch),
            vec![
                HunkAnchor {
                    modified_side: true,
                    line: 4
                },
                HunkAnchor {
                    modified_side: false,
                    line: 10
                },
                HunkAnchor {
                    modified_side: true,
                    line: 1
                },
            ]
        );
    }
}
