//! Integration tests for [`postino_workspace::Workspace`] against real temporary folders.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use postino_workspace::{Node, Workspace, WorkspaceError};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

/// Creates an empty file at `path`, creating its parent directories first.
fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, contents).expect("write fixture file");
}

#[test]
fn open_fails_on_a_missing_root() {
    let temp = TempDir::new().expect("temp dir");
    let missing = temp.path().join("does-not-exist");
    let result = Workspace::open(&missing);
    assert!(matches!(result, Err(WorkspaceError::RootNotFound(path)) if path == missing));
}

#[test]
fn open_fails_when_root_is_a_file() {
    let temp = TempDir::new().expect("temp dir");
    let file = temp.path().join("not-a-folder");
    fs::write(&file, "hello").expect("write file");
    assert!(Workspace::open(&file).is_err());
}

#[test]
fn scans_nested_folders_and_requests() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("auth/login.postino"),
        "GET https://example.com\n",
    );
    write_file(
        &temp.path().join("users/list.postino"),
        "GET https://example.com/users\n",
    );
    write_file(
        &temp.path().join("ping.postino"),
        "GET https://example.com/ping\n",
    );

    let workspace = Workspace::open(temp.path()).expect("open workspace");
    let tree = workspace.tree();

    // Folders first, then requests, natural sort within each group: "auth", "users", "ping".
    assert_eq!(tree.len(), 3);
    let names: Vec<&str> = tree.iter().map(Node::name).collect();
    assert_eq!(names, vec!["auth", "users", "ping"]);

    let Node::Folder(auth) = &tree[0] else {
        panic!("expected a folder")
    };
    assert_eq!(auth.id, "auth");
    assert_eq!(auth.children.len(), 1);
    let Node::Request(login) = &auth.children[0] else {
        panic!("expected a request")
    };
    assert_eq!(login.id, "auth/login.postino");
    assert_eq!(login.name, "login");
    assert_eq!(login.broken, None);
}

#[test]
fn ids_use_forward_slashes_regardless_of_nesting_depth() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("a/b/c/deep.postino"),
        "GET https://example.com\n",
    );
    let workspace = Workspace::open(temp.path()).expect("open workspace");

    let Node::Folder(a) = &workspace.tree()[0] else {
        panic!("expected folder a")
    };
    assert_eq!(a.id, "a");
    let Node::Folder(b) = &a.children[0] else {
        panic!("expected folder b")
    };
    assert_eq!(b.id, "a/b");
    let Node::Folder(c) = &b.children[0] else {
        panic!("expected folder c")
    };
    assert_eq!(c.id, "a/b/c");
    let Node::Request(deep) = &c.children[0] else {
        panic!("expected request")
    };
    assert_eq!(deep.id, "a/b/c/deep.postino");
}

#[test]
fn hidden_folders_and_target_are_ignored() {
    let temp = TempDir::new().expect("temp dir");
    write_file(&temp.path().join(".git/config"), "");
    write_file(&temp.path().join("target/debug/build"), "");
    write_file(
        &temp.path().join("visible/request.postino"),
        "GET https://example.com\n",
    );

    let workspace = Workspace::open(temp.path()).expect("open workspace");
    let names: Vec<&str> = workspace.tree().iter().map(Node::name).collect();
    assert_eq!(names, vec!["visible"]);
}

#[test]
fn top_level_environments_folder_is_skipped_from_the_tree() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("environments/dev.env"),
        "BASE_URL=https://dev.test\n",
    );
    write_file(
        &temp.path().join("ping.postino"),
        "GET https://example.com\n",
    );

    let workspace = Workspace::open(temp.path()).expect("open workspace");
    let names: Vec<&str> = workspace.tree().iter().map(Node::name).collect();
    assert_eq!(names, vec!["ping"]);
}

#[test]
fn a_broken_request_file_is_listed_but_does_not_fail_the_scan() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("good.postino"),
        "GET https://example.com\n",
    );
    write_file(
        &temp.path().join("broken.postino"),
        "this is not a valid request file",
    );

    let workspace = Workspace::open(temp.path()).expect("a broken file must not fail the scan");
    let tree = workspace.tree();
    assert_eq!(tree.len(), 2);

    let Node::Request(broken) = tree.iter().find(|node| node.name() == "broken").unwrap() else {
        panic!("expected a request node")
    };
    assert!(
        broken.broken.is_some(),
        "the broken file should be marked broken"
    );

    let Node::Request(good) = tree.iter().find(|node| node.name() == "good").unwrap() else {
        panic!("expected a request node")
    };
    assert_eq!(good.broken, None);
}

#[test]
fn load_request_fails_for_a_broken_file_with_a_parse_error() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("broken.postino"),
        "not a valid request file",
    );
    let workspace = Workspace::open(temp.path()).expect("open workspace");

    let result = workspace.load_request("broken.postino");
    assert!(matches!(result, Err(WorkspaceError::Parse(_))));
}

#[test]
fn load_request_fails_when_the_id_does_not_exist() {
    let temp = TempDir::new().expect("temp dir");
    let workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.load_request("missing.postino"),
        Err(WorkspaceError::NotFound(_))
    ));
}

#[test]
fn save_request_round_trips_and_is_reflected_in_a_rescan() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("ping.postino"),
        "GET https://example.com\n",
    );
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    let mut request = workspace
        .load_request("ping.postino")
        .expect("load request");
    request.url = "https://example.com/v2".to_string();
    workspace
        .save_request("ping.postino", &request)
        .expect("save request");

    let reloaded = workspace
        .load_request("ping.postino")
        .expect("reload request");
    assert_eq!(reloaded.url, "https://example.com/v2");
}

#[test]
fn save_request_does_not_leave_a_temporary_file_behind() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("ping.postino"),
        "GET https://example.com\n",
    );
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    let request = workspace
        .load_request("ping.postino")
        .expect("load request");
    workspace
        .save_request("ping.postino", &request)
        .expect("save request");

    let entries: Vec<_> = fs::read_dir(temp.path())
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(entries, vec!["ping.postino".to_string()]);
}

#[test]
fn save_request_fails_when_the_parent_folder_does_not_exist() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    let request = postino_core::Request::default();
    assert!(
        workspace
            .save_request("missing/ping.postino", &request)
            .is_err()
    );
}

#[test]
fn create_request_creates_a_parseable_file_and_appears_in_the_tree() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    let id = workspace
        .create_request(None, "New request")
        .expect("create request");
    assert_eq!(id, "New request.postino");

    let loaded = workspace
        .load_request(&id)
        .expect("a freshly created request must parse");
    assert!(!loaded.url.is_empty());

    let names: Vec<&str> = workspace.tree().iter().map(Node::name).collect();
    assert_eq!(names, vec!["New request"]);
}

#[test]
fn create_request_sanitizes_the_name() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    let id = workspace
        .create_request(None, "weird/name*here")
        .expect("create request");
    assert_eq!(id, "weird_name_here.postino");
}

#[test]
fn create_request_inside_a_folder_uses_its_id_as_parent() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    let folder_id = workspace
        .create_folder(None, "auth")
        .expect("create folder");
    let request_id = workspace
        .create_request(Some(&folder_id), "login")
        .expect("create request");
    assert_eq!(request_id, "auth/login.postino");
}

#[test]
fn create_request_fails_if_it_already_exists() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    workspace
        .create_request(None, "ping")
        .expect("create request");
    assert!(matches!(
        workspace.create_request(None, "ping"),
        Err(WorkspaceError::AlreadyExists(_))
    ));
}

#[test]
fn create_folder_fails_for_a_missing_parent() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.create_folder(Some("missing"), "child"),
        Err(WorkspaceError::NotFound(_))
    ));
}

#[test]
fn rename_a_request_keeps_its_extension_and_updates_the_id() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("old.postino"),
        "GET https://example.com\n",
    );
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    let new_id = workspace
        .rename("old.postino", "new")
        .expect("rename request");
    assert_eq!(new_id, "new.postino");
    assert!(workspace.load_request("new.postino").is_ok());
    assert!(workspace.load_request("old.postino").is_err());
}

#[test]
fn rename_a_folder_moves_its_contents() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("old/inside.postino"),
        "GET https://example.com\n",
    );
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    let new_id = workspace.rename("old", "renamed").expect("rename folder");
    assert_eq!(new_id, "renamed");
    assert!(workspace.load_request("renamed/inside.postino").is_ok());
}

#[test]
fn rename_fails_when_the_target_already_exists() {
    let temp = TempDir::new().expect("temp dir");
    write_file(&temp.path().join("a.postino"), "GET https://example.com\n");
    write_file(&temp.path().join("b.postino"), "GET https://example.com\n");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.rename("a.postino", "b"),
        Err(WorkspaceError::AlreadyExists(_))
    ));
}

#[test]
fn delete_removes_a_request_file() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("ping.postino"),
        "GET https://example.com\n",
    );
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    workspace.delete("ping.postino").expect("delete request");
    assert!(workspace.tree().is_empty());
    assert!(!temp.path().join("ping.postino").exists());
}

#[test]
fn delete_removes_a_folder_recursively() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("auth/login.postino"),
        "GET https://example.com\n",
    );
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    workspace.delete("auth").expect("delete folder");
    assert!(workspace.tree().is_empty());
    assert!(!temp.path().join("auth").exists());
}

#[test]
fn list_environments_ignores_local_env_files() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("environments/dev.env"),
        "BASE_URL=https://dev.test\n",
    );
    write_file(
        &temp.path().join("environments/dev.local.env"),
        "TOKEN=secret\n",
    );
    write_file(
        &temp.path().join("environments/prod.env"),
        "BASE_URL=https://prod.test\n",
    );

    let workspace = Workspace::open(temp.path()).expect("open workspace");
    let environments = workspace.list_environments().expect("list environments");
    assert_eq!(environments, vec!["dev".to_string(), "prod".to_string()]);
}

#[test]
fn list_environments_is_empty_without_an_environments_folder() {
    let temp = TempDir::new().expect("temp dir");
    let workspace = Workspace::open(temp.path()).expect("open workspace");
    assert_eq!(
        workspace.list_environments().expect("list environments"),
        Vec::<String>::new()
    );
}

#[test]
fn load_environment_merges_local_over_base() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("environments/dev.env"),
        "BASE_URL=https://dev.test\nTOKEN=placeholder\n",
    );
    write_file(
        &temp.path().join("environments/dev.local.env"),
        "TOKEN=real-secret\n",
    );

    let workspace = Workspace::open(temp.path()).expect("open workspace");
    let environment = workspace.load_environment("dev").expect("load environment");

    assert_eq!(environment.name, "dev");
    assert_eq!(
        environment.variables,
        vec![
            postino_core::KeyValue::new("BASE_URL", "https://dev.test"),
            postino_core::KeyValue::new("TOKEN", "real-secret"),
        ]
    );
}

#[test]
fn load_environment_works_with_only_a_local_file() {
    let temp = TempDir::new().expect("temp dir");
    write_file(
        &temp.path().join("environments/secrets.local.env"),
        "TOKEN=abc\n",
    );

    let workspace = Workspace::open(temp.path()).expect("open workspace");
    let environment = workspace
        .load_environment("secrets")
        .expect("load environment");
    assert_eq!(
        environment.variables,
        vec![postino_core::KeyValue::new("TOKEN", "abc")]
    );
}

#[test]
fn load_environment_fails_for_an_unknown_name() {
    let temp = TempDir::new().expect("temp dir");
    let workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.load_environment("nope"),
        Err(WorkspaceError::EnvironmentNotFound(_))
    ));
}

// --- Path traversal rejection --------------------------------------------------------------

#[test]
fn load_request_rejects_a_traversal_id() {
    let temp = TempDir::new().expect("temp dir");
    let workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.load_request("../secret.postino"),
        Err(WorkspaceError::InvalidId(_))
    ));
}

#[test]
fn save_request_rejects_a_traversal_id() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    let request = postino_core::Request::default();
    assert!(matches!(
        workspace.save_request("../escape.postino", &request),
        Err(WorkspaceError::InvalidId(_))
    ));
}

#[test]
fn save_request_rejects_an_absolute_id() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    let request = postino_core::Request::default();
    assert!(matches!(
        workspace.save_request("/etc/passwd", &request),
        Err(WorkspaceError::InvalidId(_))
    ));
}

#[test]
fn create_request_rejects_a_traversal_parent_id() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.create_request(Some(".."), "evil"),
        Err(WorkspaceError::InvalidId(_))
    ));
}

#[test]
fn create_folder_rejects_a_traversal_parent_id() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.create_folder(Some("../../"), "evil"),
        Err(WorkspaceError::InvalidId(_))
    ));
}

#[test]
fn rename_rejects_a_traversal_id() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.rename("../outside.postino", "new-name"),
        Err(WorkspaceError::InvalidId(_))
    ));
}

#[test]
fn delete_rejects_a_traversal_id() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(matches!(
        workspace.delete("../outside"),
        Err(WorkspaceError::InvalidId(_))
    ));
}

#[test]
fn a_dot_dot_new_name_cannot_be_used_to_escape_on_rename() {
    let temp = TempDir::new().expect("temp dir");
    write_file(&temp.path().join("a.postino"), "GET https://example.com\n");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    // Sanitization strips the trailing dots, so this becomes a harmless "untitled.postino"
    // instead of escaping the workspace root.
    let new_id = workspace
        .rename("a.postino", "..")
        .expect("rename should sanitize, not fail");
    assert_eq!(new_id, "untitled.postino");
    assert!(temp.path().join("untitled.postino").exists());
}
