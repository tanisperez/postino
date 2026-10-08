//! Postman import, wired to the workspace (the "Import" menu entries), and the export names an
//! import without a workspace suggests for the new one.
//!
//! These are thin wrappers around [`Workspace::import_postman_collection`] and
//! [`Workspace::import_postman_environment`], kept here so the menu handlers in
//! `views/sidebar.rs` and this module's own tests go through the exact same call, instead of the
//! menu poking at `postino-workspace` directly.

use postino_format::postman::{parse_collection, parse_environment};
use postino_workspace::{ImportReport, Workspace, WorkspaceError};

/// Imports a Postman collection export (the raw JSON text of a `.postman_collection.json` file)
/// into `workspace`.
pub fn import_postman_collection(
    workspace: &mut Workspace,
    json: &str,
) -> Result<ImportReport, WorkspaceError> {
    workspace.import_postman_collection(json)
}

/// Imports a standalone Postman environment export (the raw JSON text of a
/// `.postman_environment.json` file) into `workspace`.
pub fn import_postman_environment(
    workspace: &mut Workspace,
    json: &str,
) -> Result<ImportReport, WorkspaceError> {
    workspace.import_postman_environment(json)
}

/// The name of a Postman collection export (`info.name`), suggested as the name of the new
/// workspace an import without a workspace creates (GitHub #89). Fails like the import itself
/// would on a file that is not a collection export.
pub fn postman_collection_name(json: &str) -> Result<String, WorkspaceError> {
    Ok(parse_collection(json)?.collection_name)
}

/// The name of a standalone Postman environment export, suggested like
/// [`postman_collection_name`].
pub fn postman_environment_name(json: &str) -> Result<String, WorkspaceError> {
    Ok(parse_environment(json)?.name)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// The Postman collection fixture `postino-format` already uses for its own parser tests
    /// (`crates/postino-format/tests/fixtures/postman_collection.json`): nested folders, every
    /// body mode, auth inheritance, disabled headers, scripts and collection variables. Reused
    /// here, through this crate's own import function, to prove the menu's code path writes
    /// files that parse back without errors, end to end.
    const COLLECTION_FIXTURE: &str =
        include_str!("../../../postino-format/tests/fixtures/postman_collection.json");

    /// The matching standalone Postman environment export fixture.
    const ENVIRONMENT_FIXTURE: &str =
        include_str!("../../../postino-format/tests/fixtures/postman_environment.json");

    #[test]
    fn import_postman_collection_writes_files_that_parse_back() {
        // A fresh, empty workspace in a temporary directory, so this never touches a workspace
        // shipped with the repository.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = Workspace::open(dir.path()).expect("opening an empty folder");

        let report = import_postman_collection(&mut workspace, COLLECTION_FIXTURE)
            .expect("the fixture collection imports cleanly");

        assert!(
            !report.created_files.is_empty(),
            "the fixture collection has at least one request"
        );
        // Every created request must parse back: `Workspace::load_request` both reads the file
        // and calls `postino_format::parse`, so a broken import would fail here.
        for id in &report.created_files {
            if id.ends_with(".postino") {
                workspace
                    .load_request(id)
                    .unwrap_or_else(|error| panic!("{id} failed to parse back: {error}"));
            }
        }
    }

    #[test]
    fn import_postman_environment_writes_files_that_can_be_loaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = Workspace::open(dir.path()).expect("opening an empty folder");

        let report = import_postman_environment(&mut workspace, ENVIRONMENT_FIXTURE)
            .expect("the fixture environment imports cleanly");

        assert!(!report.created_files.is_empty());
        let environments = workspace
            .list_environments()
            .expect("environments/ was created");
        assert!(!environments.is_empty());
        for name in &environments {
            workspace
                .load_environment(name)
                .unwrap_or_else(|error| panic!("environment {name} failed to load: {error}"));
        }
    }

    #[test]
    fn export_names_come_from_the_exports() {
        assert_eq!(
            postman_collection_name(COLLECTION_FIXTURE).expect("a collection"),
            "Demo API"
        );
        assert_eq!(
            postman_environment_name(ENVIRONMENT_FIXTURE).expect("an environment"),
            "Demo"
        );
    }

    #[test]
    fn export_names_fail_on_something_else() {
        assert!(postman_collection_name("{}").is_err());
        assert!(postman_collection_name("not json").is_err());
        assert!(postman_environment_name("not json").is_err());
    }
}
