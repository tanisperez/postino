//! What the launcher shows (the design's "Empty States" page, 1a): with a workspace open and no
//! tab, the main area offers what to do next and the requests last opened in this workspace.
//! Plain Rust over the workspace tree and the remembered recent requests
//! (`state::config::recent_requests`), unit tested without a window. Built once when the tree or
//! the recent list changes (`views/launcher.rs` caches it), never on every frame.

use std::collections::HashMap;

use postino_core::Method;
use postino_workspace::Node;

use super::config::RecentRequest;

/// How many recent requests the launcher lists.
pub const RECENT_LIMIT: usize = 5;

/// One row of the Recent list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentRow {
    /// The request id, what a click opens.
    pub id: String,
    /// Its method, `None` when the file is broken.
    pub method: Option<Method>,
    /// Its name.
    pub name: String,
    /// The folder it is in, with a trailing `/`, or `/` at the workspace root.
    pub folder: String,
    /// When it was last opened, in Unix seconds.
    pub opened: i64,
}

/// What the launcher shows about the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LauncherModel {
    /// How many requests the workspace has.
    pub requests: usize,
    /// How many folders it has, nested ones included.
    pub folders: usize,
    /// The requests last opened that still exist, most recent first, at most
    /// [`RECENT_LIMIT`].
    pub recents: Vec<RecentRow>,
}

/// The launcher of the workspace with `tree` and the recent requests `recent`, most recent first.
pub fn build(tree: &[Node], recent: &[RecentRequest]) -> LauncherModel {
    let mut model = LauncherModel::default();
    let mut requests: HashMap<&str, (Option<Method>, &str)> = HashMap::new();
    let mut stack: Vec<&Node> = tree.iter().collect();
    while let Some(node) = stack.pop() {
        match node {
            Node::Folder(folder) => {
                model.folders += 1;
                stack.extend(folder.children.iter());
            }
            Node::Request(request) => {
                model.requests += 1;
                requests.insert(request.id.as_str(), (request.method.clone(), &request.name));
            }
        }
    }
    model.recents = recent
        .iter()
        .filter_map(|entry| {
            let (method, name) = requests.get(entry.id.as_str())?;
            Some(RecentRow {
                id: entry.id.clone(),
                method: method.clone(),
                name: (*name).to_string(),
                folder: folder_of(&entry.id),
                opened: entry.opened,
            })
        })
        .take(RECENT_LIMIT)
        .collect();
    model
}

/// The folder a new request goes in when `selected` (a sidebar tree id) is selected: the folder
/// itself, or the folder of a selected request. `None` is the workspace root.
pub fn new_request_folder(selected: &str) -> Option<String> {
    if !selected.ends_with(".postino") {
        return Some(selected.to_string()).filter(|id| !id.is_empty());
    }
    selected
        .rsplit_once('/')
        .map(|(folder, _)| folder.to_string())
}

/// The folder part of a request id, with a trailing `/`: `auth/` for `auth/login.postino`, `/`
/// for a request at the root.
fn folder_of(id: &str) -> String {
    match id.rsplit_once('/') {
        Some((folder, _)) => format!("{folder}/"),
        None => "/".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use postino_workspace::{Folder, RequestEntry};
    use pretty_assertions::assert_eq;

    fn request(id: &str, method: Method) -> Node {
        let name = id
            .rsplit('/')
            .next()
            .unwrap_or(id)
            .trim_end_matches(".postino")
            .to_string();
        Node::Request(RequestEntry {
            id: id.to_string(),
            name,
            broken: None,
            method: Some(method),
            oauth_profile: None,
        })
    }

    fn folder(id: &str, children: Vec<Node>) -> Node {
        Node::Folder(Folder {
            id: id.to_string(),
            name: id.rsplit('/').next().unwrap_or(id).to_string(),
            children,
        })
    }

    fn recent(id: &str, opened: i64) -> RecentRequest {
        RecentRequest {
            id: id.to_string(),
            opened,
        }
    }

    fn tree() -> Vec<Node> {
        vec![
            folder(
                "auth",
                vec![
                    folder(
                        "auth/admin",
                        vec![request("auth/admin/ban.postino", Method::Delete)],
                    ),
                    request("auth/login.postino", Method::Post),
                ],
            ),
            request("index.postino", Method::Get),
        ]
    }

    #[test]
    fn a_new_request_goes_in_the_selected_folder() {
        assert_eq!(new_request_folder("auth"), Some("auth".to_string()));
        assert_eq!(
            new_request_folder("auth/login.postino"),
            Some("auth".to_string())
        );
        assert_eq!(new_request_folder("index.postino"), None);
    }

    #[test]
    fn counts_every_request_and_folder() {
        let model = build(&tree(), &[]);
        assert_eq!((model.requests, model.folders), (3, 2));
        assert!(model.recents.is_empty());
    }

    #[test]
    fn recents_skip_requests_that_are_gone_and_name_their_folder() {
        let model = build(
            &tree(),
            &[
                recent("index.postino", 30),
                recent("deleted.postino", 20),
                recent("auth/login.postino", 10),
            ],
        );
        assert_eq!(
            model.recents,
            [
                RecentRow {
                    id: "index.postino".to_string(),
                    method: Some(Method::Get),
                    name: "index".to_string(),
                    folder: "/".to_string(),
                    opened: 30,
                },
                RecentRow {
                    id: "auth/login.postino".to_string(),
                    method: Some(Method::Post),
                    name: "login".to_string(),
                    folder: "auth/".to_string(),
                    opened: 10,
                },
            ]
        );
    }

    #[test]
    fn recents_stop_at_the_limit() {
        let tree: Vec<Node> = (0..8)
            .map(|n| request(&format!("r{n}.postino"), Method::Get))
            .collect();
        let recent: Vec<RecentRequest> = (0..8)
            .map(|n| recent(&format!("r{n}.postino"), n))
            .collect();
        assert_eq!(build(&tree, &recent).recents.len(), RECENT_LIMIT);
    }
}
