//! What the command line asks of the app it is built into. The `postino` binary passes the app's
//! implementation to [`crate::run_if_cli`], so the console reuses the app's report builder and
//! import wizard instead of copies of them. [`NoHooks`] stands in where there is no app, in tests.

use std::path::{Path, PathBuf};

use crate::reports::{DocumentKind, ReportRun};

/// What an import reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// OpenAPI 3.x or Swagger 2.0 specs.
    OpenApi,
    /// Postman collections, environments and globals.
    Postman,
    /// A Bruno collection.
    Bruno,
}

/// An import of the wizard, without its questions.
#[derive(Debug, Clone, Copy)]
pub struct ImportRequest<'a> {
    /// What is imported.
    pub kind: ImportKind,
    /// A file, a folder, a `.zip`, or a URL for OpenAPI.
    pub source: &'a str,
    /// The workspace folder.
    pub root: &'a Path,
    /// The name of the collection folder, at the workspace root. `None` uses the name the wizard
    /// suggests.
    pub into: Option<&'a str>,
    /// Imports a new collection even when this source was imported before, instead of updating
    /// that collection.
    pub new_collection: bool,
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportOutcome {
    /// The collection folder, a workspace id.
    pub folder: String,
    /// The request files written, for a new collection.
    pub requests: usize,
    /// The environment files written.
    pub environments: Vec<String>,
    /// For an update of an earlier import: the requests added, changed and removed.
    pub updated: Option<(usize, usize, usize)>,
    /// What could not be converted faithfully.
    pub warnings: Vec<String>,
    /// How many requests have something to review (converted scripts, dropped settings).
    pub review: usize,
    /// The import report written to the workspace, when there is something to review.
    pub report: Option<PathBuf>,
}

/// The app's pieces the command line uses.
pub trait AppHooks {
    /// The bytes of the `kind` load test report of `run`.
    fn render_report(&self, kind: DocumentKind, run: &ReportRun<'_>) -> Result<Vec<u8>, String>;

    /// Runs an import into the workspace `request.root`.
    fn import(&self, request: &ImportRequest<'_>) -> Result<ImportOutcome, String>;
}

/// No app: no PDF or Markdown reports and no imports.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

const NEEDS_APP: &str = "this needs the postino binary";

impl AppHooks for NoHooks {
    fn render_report(&self, _kind: DocumentKind, _run: &ReportRun<'_>) -> Result<Vec<u8>, String> {
        Err(NEEDS_APP.to_string())
    }

    fn import(&self, _request: &ImportRequest<'_>) -> Result<ImportOutcome, String> {
        Err(NEEDS_APP.to_string())
    }
}
