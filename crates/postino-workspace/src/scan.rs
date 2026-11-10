//! Recursively scanning a workspace folder into a sorted list of [`Node`]s.

use std::fs;
use std::path::Path;

use postino_core::Method;

use crate::error::WorkspaceError;
use crate::ids::path_to_id;
use crate::tree::{Folder, Node, RequestEntry, sort_nodes};
use crate::{ENVIRONMENTS_FOLDER, REQUEST_EXTENSION};

/// Folder names skipped at every level, besides hidden ones: build and dependency folders that
/// never hold requests and can hold hundreds of thousands of files.
const SKIPPED_FOLDERS: [&str; 2] = ["target", "node_modules"];

/// Scans the workspace at `root` into its sorted collection tree.
///
/// - Hidden entries (name starting with `.`) and the folders in [`SKIPPED_FOLDERS`] are skipped
///   at every level. The top-level `environments/` folder is skipped too, since it holds
///   environments rather than a collection; a nested folder named `environments` deeper in the
///   tree is treated as a plain collection folder.
/// - A folder with no request anywhere below it is left out when it holds other files, so a
///   workspace inside a code repository does not list `src/` or `docs/` as collections. An
///   empty folder is kept, so one just created can be filled.
/// - A subfolder that cannot be read is skipped with a warning in the log instead of failing the
///   whole scan. Only an unreadable `root` is an error.
/// - A `.postino` file that fails to read or parse is still listed, as a [`RequestEntry`] with
///   `broken` set to a readable message, so one bad file never breaks the whole scan.
///
/// With `limit`, the scan gives up with [`WorkspaceError::TooLarge`] once it has visited more
/// than `limit` entries (files and folders, not counting skipped ones).
pub(crate) fn scan_workspace(
    root: &Path,
    limit: Option<usize>,
) -> Result<Vec<Node>, WorkspaceError> {
    let mut budget = Budget {
        root,
        left: limit.unwrap_or(usize::MAX),
        limit: limit.unwrap_or(usize::MAX),
    };
    Ok(scan_folder(root, root, true, &mut budget)?.nodes)
}

/// How many more entries a scan may visit before it gives up (see [`scan_workspace`]).
struct Budget<'a> {
    root: &'a Path,
    left: usize,
    limit: usize,
}

impl Budget<'_> {
    /// Counts one visited entry, failing once the limit is exceeded.
    fn take(&mut self) -> Result<(), WorkspaceError> {
        if self.left == 0 {
            return Err(WorkspaceError::TooLarge {
                path: self.root.to_path_buf(),
                limit: self.limit,
            });
        }
        self.left -= 1;
        Ok(())
    }
}

/// What scanning one folder found.
struct Scanned {
    /// The folder's children, sorted, already pruned.
    nodes: Vec<Node>,
    /// Whether the folder holds anything besides hidden entries and skipped folders, so an
    /// empty folder can be told apart from one whose content was all pruned.
    has_entries: bool,
}

/// Scans `dir` (a subdirectory of `root`, or `root` itself when `is_root` is `true`), following
/// the rules of [`scan_workspace`]. The only [`WorkspaceError::Io`] it returns is `dir` itself
/// failing to open.
fn scan_folder(
    root: &Path,
    dir: &Path,
    is_root: bool,
    budget: &mut Budget,
) -> Result<Scanned, WorkspaceError> {
    let mut nodes = Vec::new();
    let mut has_entries = false;
    let entries = fs::read_dir(dir).map_err(|source| WorkspaceError::Io {
        path: dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                log::warn!("scan: skipping an entry of {}: {error}", dir.display());
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                log::warn!("scan: skipping {}: {error}", path.display());
                continue;
            }
        };

        if file_type.is_dir() {
            if SKIPPED_FOLDERS.contains(&name.as_str()) || (is_root && name == ENVIRONMENTS_FOLDER)
            {
                continue;
            }
            has_entries = true;
            budget.take()?;
            let children = match scan_folder(root, &path, false, budget) {
                Ok(children) => children,
                Err(WorkspaceError::Io { path, source }) => {
                    log::warn!(
                        "scan: skipping unreadable folder {}: {source}",
                        path.display()
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            if children.nodes.is_empty() && children.has_entries {
                continue;
            }
            let id = path_to_id(root, &path);
            nodes.push(Node::Folder(Folder {
                id,
                name,
                children: children.nodes,
            }));
        } else if file_type.is_file() {
            has_entries = true;
            budget.take()?;
            let Some(stem) = name.strip_suffix(&format!(".{REQUEST_EXTENSION}")) else {
                continue;
            };
            let id = path_to_id(root, &path);
            let parsed = read_and_parse(&path);
            nodes.push(Node::Request(RequestEntry {
                id,
                name: stem.to_string(),
                broken: parsed.broken,
                method: parsed.method,
                oauth_profile: parsed.oauth_profile,
            }));
        }
        // Symlinks and other special file types are neither a folder nor a request, so they are
        // silently skipped.
    }

    sort_nodes(&mut nodes);
    Ok(Scanned { nodes, has_entries })
}

/// What the scan keeps of one `.postino` file.
struct Parsed {
    broken: Option<String>,
    method: Option<Method>,
    oauth_profile: Option<String>,
}

/// Reads and parses a `.postino` file once, for [`RequestEntry::broken`],
/// [`RequestEntry::method`] and [`RequestEntry::oauth_profile`], so a broken or valid file is
/// never read or parsed twice. A file that cannot be read or is not a valid request is `broken`,
/// with a message saying why: an IO error or a [`postino_format::ParseError`].
fn read_and_parse(path: &Path) -> Parsed {
    let broken = |message: String| Parsed {
        broken: Some(message),
        method: None,
        oauth_profile: None,
    };
    match fs::read_to_string(path) {
        Ok(text) => match postino_format::parse(&text) {
            Ok(request) => Parsed {
                broken: None,
                method: Some(request.method),
                oauth_profile: match request.auth {
                    postino_core::Auth::OAuth2 { profile, .. } => Some(profile),
                    _ => None,
                },
            },
            Err(error) => broken(error.to_string()),
        },
        Err(error) => broken(error.to_string()),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent directories");
        }
        fs::write(path, "GET https://example.com\n").expect("write file");
    }

    #[test]
    fn gives_up_once_the_limit_is_exceeded() {
        let temp = TempDir::new().expect("temp dir");
        for name in ["a", "b", "c", "d"] {
            touch(&temp.path().join(format!("{name}.postino")));
        }

        assert!(scan_workspace(temp.path(), Some(4)).is_ok());
        let error = scan_workspace(temp.path(), Some(3)).expect_err("over the limit");
        assert!(
            matches!(&error, WorkspaceError::TooLarge { path, limit: 3 } if path == temp.path())
        );
        assert!(scan_workspace(temp.path(), None).is_ok());
    }

    #[test]
    fn folders_count_toward_the_limit_but_skipped_ones_do_not() {
        let temp = TempDir::new().expect("temp dir");
        touch(&temp.path().join("api/ping.postino"));
        for index in 0..10 {
            touch(&temp.path().join(format!("node_modules/pkg/{index}.js")));
            touch(&temp.path().join(format!(".git/objects/{index}")));
        }

        // `api` and `api/ping.postino`.
        assert!(scan_workspace(temp.path(), Some(2)).is_ok());
        assert!(scan_workspace(temp.path(), Some(1)).is_err());
    }
}
