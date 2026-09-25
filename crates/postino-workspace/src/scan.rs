//! Recursively scanning a workspace folder into a sorted list of [`Node`]s.

use std::fs;
use std::path::Path;

use crate::error::WorkspaceError;
use crate::ids::path_to_id;
use crate::tree::{Folder, Node, RequestEntry, sort_nodes};
use crate::{ENVIRONMENTS_FOLDER, REQUEST_EXTENSION};

/// Recursively scans `dir` (a subdirectory of `root`, or `root` itself when `is_root` is `true`)
/// into a sorted list of tree nodes.
///
/// Hidden entries (name starting with `.`) and a directory named `target` are skipped at every
/// level. The top-level `environments/` folder is skipped too, since it holds environments
/// rather than a collection (`plans/mvp.md`, section 3.4); a nested folder named `environments`
/// deeper in the tree is treated as a plain collection folder.
///
/// A `.postino` file that fails to read or parse is still listed, as a [`RequestEntry`] with
/// `broken` set to a readable message, so one bad file never breaks the whole scan
/// (`plans/mvp.md`, section 6, phase 3).
pub(crate) fn scan_folder(
    root: &Path,
    dir: &Path,
    is_root: bool,
) -> Result<Vec<Node>, WorkspaceError> {
    let mut nodes = Vec::new();
    let entries = fs::read_dir(dir).map_err(|source| WorkspaceError::Io {
        path: dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| WorkspaceError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type().map_err(|source| WorkspaceError::Io {
            path: path.clone(),
            source,
        })?;

        if file_type.is_dir() {
            if name.starts_with('.') || name == "target" {
                continue;
            }
            if is_root && name == ENVIRONMENTS_FOLDER {
                continue;
            }
            let children = scan_folder(root, &path, false)?;
            let id = path_to_id(root, &path);
            nodes.push(Node::Folder(Folder { id, name, children }));
        } else if file_type.is_file() {
            if name.starts_with('.') {
                continue;
            }
            let Some(stem) = name.strip_suffix(&format!(".{REQUEST_EXTENSION}")) else {
                continue;
            };
            let id = path_to_id(root, &path);
            let broken = read_and_check(&path);
            nodes.push(Node::Request(RequestEntry {
                id,
                name: stem.to_string(),
                broken,
            }));
        }
        // Symlinks and other special file types are neither a folder nor a request, so they are
        // silently skipped.
    }

    sort_nodes(&mut nodes);
    Ok(nodes)
}

/// Reads and parses a `.postino` file just to detect whether it is broken.
///
/// Returns `None` when the file is valid, or `Some(message)` describing why it is not: either an
/// IO error (an unreadable file) or a [`postino_format::ParseError`].
fn read_and_check(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => postino_format::parse(&text)
            .err()
            .map(|error| error.to_string()),
        Err(error) => Some(error.to_string()),
    }
}
