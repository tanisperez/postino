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
}
