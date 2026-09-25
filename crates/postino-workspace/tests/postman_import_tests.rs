//! Integration tests for [`postino_workspace::Workspace`]'s Postman import, `plans/mvp.md`
//! section 6, phase 7. `tests/fixtures/postman_collection.json` has a nested folder, a duplicate
//! request name (two "Login" requests in the same folder), a `formdata` body (unsupported, must
//! warn) and a collection variable. `tests/fixtures/postman_environment.json` has one plain
//! value and one secret value.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use postino_workspace::Workspace;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const COLLECTION: &str = include_str!("fixtures/postman_collection.json");
const ENVIRONMENT: &str = include_str!("fixtures/postman_environment.json");

#[test]
fn import_collection_creates_a_folder_named_after_it_with_its_requests() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    let report = workspace
        .import_postman_collection(COLLECTION)
        .expect("import collection");

    assert!(temp.path().join("Sample API").is_dir());
    assert!(temp.path().join("Sample API/Auth/Login.postino").is_file());
    assert!(
        temp.path()
            .join("Sample API/Auth/Login (2).postino")
            .is_file(),
        "a second request named \"Login\" must get a \" (2)\" suffix"
    );
    assert!(temp.path().join("Sample API/Upload.postino").is_file());
    assert_eq!(
        report.created_files,
        vec![
            "Sample API/Auth/Login.postino".to_string(),
            "Sample API/Auth/Login (2).postino".to_string(),
            "Sample API/Upload.postino".to_string(),
            "environments/Sample API.env".to_string(),
        ]
    );
}

#[test]
fn import_collection_reports_a_warning_for_the_unsupported_formdata_body() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    let report = workspace
        .import_postman_collection(COLLECTION)
        .expect("import collection");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("formdata"))
    );
}

#[test]
fn import_collection_writes_its_variables_as_an_environment() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    workspace
        .import_postman_collection(COLLECTION)
        .expect("import collection");

    let content = fs::read_to_string(temp.path().join("environments/Sample API.env"))
        .expect("read environment file");
    assert_eq!(content, "token=abc123\n");
}

#[test]
fn importing_the_same_collection_twice_suffixes_the_second_folder_and_environment() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    workspace
        .import_postman_collection(COLLECTION)
        .expect("first import");
    let second = workspace
        .import_postman_collection(COLLECTION)
        .expect("second import");

    assert!(temp.path().join("Sample API (2)").is_dir());
    assert!(
        temp.path()
            .join("Sample API (2)/Auth/Login.postino")
            .is_file()
    );
    assert!(
        second
            .created_files
            .contains(&"environments/Sample API (2).env".to_string())
    );
    assert!(temp.path().join("environments/Sample API.env").is_file());
    assert!(
        temp.path()
            .join("environments/Sample API (2).env")
            .is_file()
    );
}

#[test]
fn import_collection_never_overwrites_an_existing_file_or_folder() {
    let temp = TempDir::new().expect("temp dir");
    fs::write(temp.path().join("Sample API"), "not a folder, a stray file")
        .expect("write stray file");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    workspace
        .import_postman_collection(COLLECTION)
        .expect("import collection despite the name clash");

    assert!(temp.path().join("Sample API").is_file(), "left untouched");
    assert!(temp.path().join("Sample API (2)").is_dir());
}

#[test]
fn import_environment_writes_base_and_secret_files() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    let report = workspace
        .import_postman_environment(ENVIRONMENT)
        .expect("import environment");

    let base = fs::read_to_string(temp.path().join("environments/Sample Env.env"))
        .expect("read base env file");
    assert_eq!(base, "host=example.com\n");
    let secret = fs::read_to_string(temp.path().join("environments/Sample Env.local.env"))
        .expect("read secret env file");
    assert_eq!(secret, "apiSecret=shh\n");
    assert_eq!(
        report.created_files,
        vec![
            "environments/Sample Env.env".to_string(),
            "environments/Sample Env.local.env".to_string(),
        ]
    );

    let environments = workspace.list_environments().expect("list environments");
    assert!(environments.contains(&"Sample Env".to_string()));
}

#[test]
fn import_environment_does_not_overwrite_an_existing_env_file() {
    let temp = TempDir::new().expect("temp dir");
    fs::create_dir_all(temp.path().join("environments")).expect("create environments dir");
    fs::write(
        temp.path().join("environments/Sample Env.env"),
        "PRE_EXISTING=1\n",
    )
    .expect("write pre-existing env file");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");

    workspace
        .import_postman_environment(ENVIRONMENT)
        .expect("import environment despite the name clash");

    let untouched = fs::read_to_string(temp.path().join("environments/Sample Env.env"))
        .expect("read pre-existing env file");
    assert_eq!(untouched, "PRE_EXISTING=1\n");
    let suffixed = fs::read_to_string(temp.path().join("environments/Sample Env (2).env"))
        .expect("read suffixed env file");
    assert_eq!(suffixed, "host=example.com\n");
}

#[test]
fn import_environment_without_secret_values_only_writes_the_base_file() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    workspace
        .import_postman_environment(r#"{"name": "Plain", "values": []}"#)
        .expect("import minimal environment");

    // No values at all: nothing should be written, not even the "environments" folder.
    assert!(!temp.path().join("environments").exists());
}

#[test]
fn every_file_written_by_a_collection_import_parses_back_without_errors() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    let report = workspace
        .import_postman_collection(COLLECTION)
        .expect("import collection");

    for id in &report.created_files {
        if let Some(name) = id.strip_prefix("environments/") {
            let path = temp.path().join("environments").join(name);
            let text = fs::read_to_string(&path).expect("read environment file");
            postino_format::env::parse(&text)
                .unwrap_or_else(|error| panic!("{id} must parse as an env file: {error}"));
        } else {
            workspace
                .load_request(id)
                .unwrap_or_else(|error| panic!("{id} must parse back: {error}"));
        }
    }
}

#[test]
fn every_file_written_by_an_environment_import_parses_back_without_errors() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    let report = workspace
        .import_postman_environment(ENVIRONMENT)
        .expect("import environment");

    for id in &report.created_files {
        let name = id
            .strip_prefix("environments/")
            .expect("environment import only writes into environments/");
        let path = temp.path().join("environments").join(name);
        let text = fs::read_to_string(&path).expect("read environment file");
        postino_format::env::parse(&text)
            .unwrap_or_else(|error| panic!("{id} must parse as an env file: {error}"));
    }
}

#[test]
fn import_collection_rejects_invalid_json() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    assert!(workspace.import_postman_collection("not json").is_err());
}

/// Sanity check that the workspace rescans after a collection import: the new folder and
/// requests show up in [`Workspace::tree`] without a manual `rescan`.
#[test]
fn import_collection_is_visible_in_the_tree_afterwards() {
    let temp = TempDir::new().expect("temp dir");
    let mut workspace = Workspace::open(temp.path()).expect("open workspace");
    workspace
        .import_postman_collection(COLLECTION)
        .expect("import collection");

    let names: Vec<&str> = workspace
        .tree()
        .iter()
        .map(postino_workspace::Node::name)
        .collect();
    assert!(names.contains(&"Sample API"));
}
