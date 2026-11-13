//! The app's side of the command line (#220): [`CliHooks`] gives `postino-cli` the pieces it
//! reuses from the app, the load test report builder and the import wizard, so the console and
//! the window produce the same files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use postino_cli::{AppHooks, DocumentKind, ImportKind, ImportOutcome, ImportRequest, ReportRun};
use postino_format::openapi::ManifestSource;
use postino_format::postman::IssueLevel;
use postino_runner::{Client, SendOptions};
use postino_workspace::{ImportOrigin, Workspace};

use crate::state::openapi_import::{
    ImportFormat, Loaded, SourceKind, Wizard, find_existing, load_files, load_url, report_text,
    scan_folder, source_kind_name,
};
use crate::views::openapi_import::{
    collection_folder, today, write_environment_colors, write_report,
};

/// See the module docs. Passed to `postino_cli::run_if_cli` by `main`.
pub struct CliHooks;

impl AppHooks for CliHooks {
    fn render_report(&self, kind: DocumentKind, run: &ReportRun<'_>) -> Result<Vec<u8>, String> {
        crate::views::load_test::export::render_cli_report(kind, run)
    }

    fn import(&self, request: &ImportRequest<'_>) -> Result<ImportOutcome, String> {
        import(request)
    }
}

/// The name of `format` in messages.
fn format_name(format: ImportFormat) -> &'static str {
    match format {
        ImportFormat::OpenApi => "OpenAPI",
        ImportFormat::Postman => "Postman",
        ImportFormat::Bruno => "Bruno",
    }
}

/// The `postino import` word of `format`.
fn command_name(format: ImportFormat) -> &'static str {
    match format {
        ImportFormat::OpenApi => "openapi",
        ImportFormat::Postman => "postman",
        ImportFormat::Bruno => "bruno",
    }
}

/// Reads the source as the wizard's first step does: a URL, a folder, or files.
fn load(source: &str, client: &Client) -> Result<(SourceKind, Loaded), String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let spec = load_url(source, client)?;
        return Ok((
            SourceKind::Url,
            Loaded {
                specs: vec![spec],
                ..Loaded::default()
            },
        ));
    }
    let path = PathBuf::from(source);
    if path.is_dir() {
        Ok((SourceKind::Folder, scan_folder(&path, client)?))
    } else {
        Ok((SourceKind::File, load_files(&[path], client)?))
    }
}

/// The names of the environments an import wrote, from its files.
fn environment_names(created: &[String]) -> Vec<String> {
    created
        .iter()
        .filter_map(|id| id.strip_prefix("environments/"))
        .filter_map(|name| name.strip_suffix(".env"))
        .filter(|name| !name.ends_with(".local"))
        .map(str::to_string)
        .collect()
}

/// Runs the wizard without its questions: the defaults of every step, every request selected,
/// and an update of the earlier import of the same source unless a new collection is asked for.
fn import(request: &ImportRequest<'_>) -> Result<ImportOutcome, String> {
    let format = match request.kind {
        ImportKind::OpenApi => ImportFormat::OpenApi,
        ImportKind::Postman => ImportFormat::Postman,
        ImportKind::Bruno => ImportFormat::Bruno,
    };
    let client = Client::new(SendOptions::default());
    let (kind, loaded) = load(request.source, &client)?;
    match loaded.format() {
        Some(found) if found == format => {}
        Some(found) => {
            return Err(format!(
                "{} is a {} source: use postino import {}",
                request.source,
                format_name(found),
                command_name(found)
            ));
        }
        None => {
            let reasons: Vec<String> = loaded
                .failures
                .iter()
                .map(|failure| format!("{}: {}", failure.source, failure.error))
                .collect();
            return Err(if reasons.is_empty() {
                format!("no {} found in {}", format_name(format), request.source)
            } else {
                reasons.join("; ")
            });
        }
    }
    let mut warnings: Vec<String> = loaded
        .failures
        .iter()
        .map(|failure| format!("{}: {}", failure.source, failure.error))
        .collect();

    let mut wizard = Wizard::new(format, kind);
    wizard.set_loaded(loaded);
    let source = match format {
        ImportFormat::Postman => wizard.postman_source(),
        ImportFormat::Bruno => wizard.bruno_source(),
        ImportFormat::OpenApi => ManifestSource {
            kind: source_kind_name(kind).to_string(),
            location: request.source.to_string(),
            branch: String::new(),
        },
    };
    let mut workspace = Workspace::open(request.root).map_err(|error| error.to_string())?;
    if !request.new_collection {
        wizard.set_existing(find_existing(workspace.openapi_collections(), &source));
    }
    let name = request
        .into
        .map(str::to_string)
        .unwrap_or_else(|| wizard.suggested_name());
    wizard.rebuild(&name);
    let plan = wizard
        .plan()
        .cloned()
        .ok_or_else(|| "there is nothing to import".to_string())?;
    let origin = ImportOrigin {
        source,
        date: today(),
        options: wizard.options,
    };

    if wizard.is_update() {
        let folder = wizard
            .existing()
            .map(|existing| existing.folder.clone())
            .unwrap_or_default();
        let skipped: HashSet<String> = wizard
            .changes()
            .iter()
            .filter(|change| !wizard.is_change_selected(&change.key))
            .map(|change| change.key.clone())
            .collect();
        let report = workspace
            .update_openapi(&folder, &plan, &|key| !skipped.contains(key), &origin)
            .map_err(|error| error.to_string())?;
        warnings.extend(report.warnings);
        return Ok(ImportOutcome {
            folder,
            updated: Some((report.added, report.changed, report.removed)),
            warnings,
            ..ImportOutcome::default()
        });
    }

    if let Some(into) = request.into
        && request.root.join(into).exists()
    {
        return Err(format!("--into {into}: the folder already exists"));
    }
    let issues = wizard
        .is_collection_tool()
        .then(|| wizard.selected_issues());
    let colors = wizard.bruno_environment_colors();
    let excluded = wizard.excluded().clone();
    let report = workspace
        .import_openapi(&plan, &|key| !excluded.contains(key), &origin)
        .map_err(|error| error.to_string())?;
    write_environment_colors(&workspace, &colors);
    let folder = collection_folder(&report).unwrap_or_else(|| plan.root.name.clone());
    let mut outcome = ImportOutcome {
        requests: report
            .created_files
            .iter()
            .filter(|id| id.ends_with(".postino"))
            .count(),
        environments: environment_names(&report.created_files),
        folder,
        ..ImportOutcome::default()
    };
    warnings.extend(report.warnings.iter().cloned());
    if let Some(issues) = &issues {
        outcome.review = issues
            .iter()
            .filter(|(_, issues)| issues.iter().any(|issue| issue.level() != IssueLevel::Note))
            .count();
        let text = report_text(&outcome.folder, issues, &report.warnings);
        if !text.is_empty() {
            outcome.report =
                write_report(request.root, &text).map(|path| relative(request.root, &path));
        }
    }
    outcome.warnings = warnings;
    Ok(outcome)
}

/// `path` relative to `root` when it is inside it.
fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const SPEC: &str = r#"{
  "openapi": "3.0.3",
  "info": {"title": "Shop", "version": "1"},
  "servers": [{"url": "https://shop.test/v1"}],
  "paths": {
    "/orders": {"get": {"tags": ["orders"], "responses": {"200": {"description": "ok"}}}}
  }
}"#;

    #[test]
    fn an_openapi_file_imports_then_updates() {
        let source_dir = tempfile::tempdir().expect("temp dir");
        let spec = source_dir.path().join("shop.json");
        std::fs::write(&spec, SPEC).expect("spec");
        let workspace = tempfile::tempdir().expect("temp dir");
        let source = spec.to_string_lossy().into_owned();
        let request = ImportRequest {
            kind: ImportKind::OpenApi,
            source: &source,
            root: workspace.path(),
            into: Some("shop-api"),
            new_collection: false,
        };

        let first = import(&request).expect("import");
        assert_eq!(first.folder, "shop-api");
        assert_eq!(first.requests, 1);
        assert_eq!(first.updated, None);

        let again = import(&ImportRequest {
            into: None,
            ..request
        })
        .expect("update");
        assert_eq!(again.folder, "shop-api");
        assert_eq!(again.updated, Some((0, 0, 0)));

        let wrong = import(&ImportRequest {
            kind: ImportKind::Postman,
            ..request
        })
        .expect_err("not postman");
        assert!(wrong.contains("postino import openapi"), "{wrong}");
    }
}
