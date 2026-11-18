//! Recursively scanning a workspace folder into a sorted list of [`Node`]s.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

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
///
/// The folders are walked first and the requests read and parsed afterwards, by several threads
/// (#287): on a slow file system each read waits on a round trip, about 8 ms per file through
/// `\\wsl.localhost` on Windows, and they add up one after the other.
pub(crate) fn scan_workspace(
    root: &Path,
    limit: Option<usize>,
) -> Result<Vec<Node>, WorkspaceError> {
    let mut budget = Budget {
        root,
        left: limit.unwrap_or(usize::MAX),
        limit: limit.unwrap_or(usize::MAX),
        requests: Vec::new(),
    };
    let mut nodes = scan_folder(root, root, true, &mut budget)?.nodes;
    let parsed = read_all(&budget.requests);
    let mut by_id: HashMap<String, Parsed> = budget
        .requests
        .into_iter()
        .map(|(id, _)| id)
        .zip(parsed)
        .collect();
    fill(&mut nodes, &mut by_id);
    Ok(nodes)
}

/// The most threads that read requests at once.
const MAX_READERS: usize = 8;

/// The requests each reader thread is worth: a small workspace is read by one thread, with no
/// thread to start.
const REQUESTS_PER_READER: usize = 16;

/// Reads and parses every request of `requests` (id and path), in the same order, sharing them
/// out among up to [`MAX_READERS`] threads.
fn read_all(requests: &[(String, PathBuf)]) -> Vec<Parsed> {
    let readers = (requests.len() / REQUESTS_PER_READER).clamp(1, MAX_READERS);
    if readers == 1 {
        return requests
            .iter()
            .map(|(_, path)| read_and_parse(path))
            .collect();
    }
    let next = AtomicUsize::new(0);
    let mut read: Vec<Option<Parsed>> = requests.iter().map(|_| None).collect();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..readers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((_, path)) = requests.get(index) else {
                            break;
                        };
                        done.push((index, read_and_parse(path)));
                    }
                    done
                })
            })
            .collect();
        for worker in workers {
            for (index, parsed) in worker.join().unwrap_or_default() {
                read[index] = Some(parsed);
            }
        }
    });
    read.into_iter()
        .map(|parsed| {
            // Only a reader thread that panicked leaves a request unread.
            parsed.unwrap_or_else(|| Parsed::broken("the file could not be read".to_string()))
        })
        .collect()
}

/// Sets the parse results of `by_id` on the requests of `nodes`, at every level.
fn fill(nodes: &mut [Node], by_id: &mut HashMap<String, Parsed>) {
    for node in nodes {
        match node {
            Node::Request(entry) => {
                if let Some(parsed) = by_id.remove(&entry.id) {
                    entry.broken = parsed.broken;
                    entry.method = parsed.method;
                    entry.oauth_profile = parsed.oauth_profile;
                }
            }
            Node::Folder(folder) => fill(&mut folder.children, by_id),
        }
    }
}

/// How many more entries a scan may visit before it gives up (see [`scan_workspace`]).
struct Budget<'a> {
    root: &'a Path,
    left: usize,
    limit: usize,
    /// The requests found so far, by id and path, to read once the folders are walked.
    requests: Vec<(String, PathBuf)>,
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
            // Read later, with the others (see `scan_workspace`).
            budget.requests.push((id.clone(), path));
            nodes.push(Node::Request(RequestEntry {
                id,
                name: stem.to_string(),
                broken: None,
                method: None,
                oauth_profile: None,
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

impl Parsed {
    /// A file that could not be read or parsed, and why.
    fn broken(message: String) -> Parsed {
        Parsed {
            broken: Some(message),
            method: None,
            oauth_profile: None,
        }
    }
}

/// Reads and parses a `.postino` file once, for [`RequestEntry::broken`],
/// [`RequestEntry::method`] and [`RequestEntry::oauth_profile`], so a broken or valid file is
/// never read or parsed twice. A file that cannot be read or is not a valid request is `broken`,
/// with a message saying why: an IO error or a [`postino_format::ParseError`].
fn read_and_parse(path: &Path) -> Parsed {
    let broken = Parsed::broken;
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

    /// Enough requests for several reader threads: every entry gets the result of its own file,
    /// wherever it sits in the tree.
    #[test]
    fn many_requests_are_read_in_parallel_into_their_own_entries() {
        let dir = TempDir::new().expect("temp dir");
        let methods = ["GET", "POST", "PUT", "PATCH", "DELETE"];
        for n in 0..50 {
            let method = methods[n % methods.len()];
            let path = dir.path().join(format!("f{}/r{n:02}.postino", n % 4));
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(&path, format!("{method} https://example.com/{n}\n")).expect("write");
        }
        fs::write(dir.path().join("f1/broken.postino"), "not a request\n").expect("write");

        let nodes = scan_workspace(dir.path(), None).expect("scan");
        let mut seen = 0;
        for folder in &nodes {
            let Node::Folder(folder) = folder else {
                panic!("only folders at the top");
            };
            for child in &folder.children {
                let Node::Request(entry) = child else {
                    panic!("only requests in the folders");
                };
                if entry.name == "broken" {
                    assert!(entry.broken.is_some());
                    assert_eq!(entry.method, None);
                    continue;
                }
                let n: usize = entry.name[1..].parse().expect("number");
                assert_eq!(entry.broken, None, "{}", entry.id);
                assert_eq!(
                    entry.method.as_ref().map(ToString::to_string).as_deref(),
                    Some(methods[n % methods.len()]),
                    "{}",
                    entry.id
                );
                seen += 1;
            }
        }
        assert_eq!(seen, 50);
    }
}
