//! Load test reports from the command line: `.json` and `.csv` come from `postino-load`, and the
//! `.pdf` and `.md` documents from the app's own report builder, which the `postino` binary
//! hands over as a [`ReportRenderer`] (it needs the app's texts, colors and fonts).

use std::path::Path;

use postino_load::export::csv::requests_csv;
use postino_load::export::json::run_json;
use postino_load::history::RunRecord;
use postino_load::{LoadSnapshot, SampleLog, window_stats};

use crate::exit::CliError;

/// A report format, chosen by the file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// The PDF document.
    Pdf,
    /// The Markdown document.
    Markdown,
    /// The whole run as JSON.
    Json,
    /// One CSV row per request.
    Csv,
}

impl ReportFormat {
    /// The format of `path`, by its extension.
    pub fn from_path(path: &Path) -> Result<Self, CliError> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "pdf" => Ok(ReportFormat::Pdf),
            "md" | "markdown" => Ok(ReportFormat::Markdown),
            "json" => Ok(ReportFormat::Json),
            "csv" => Ok(ReportFormat::Csv),
            _ => Err(CliError::usage(format!(
                "{}: the report must end in .pdf, .md, .json or .csv",
                path.display()
            ))),
        }
    }
}

/// A document built by the app's report builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// PDF.
    Pdf,
    /// Markdown.
    Markdown,
}

/// The run a report is about.
#[derive(Debug, Clone, Copy)]
pub struct ReportRun<'a> {
    /// The run, as saved in the history.
    pub record: &'a RunRecord,
    /// Every request it made.
    pub log: &'a SampleLog,
    /// The compared run's number and final snapshot.
    pub compare: Option<(u32, &'a LoadSnapshot)>,
    /// The request timeout, in seconds, for the title of the timeout errors.
    pub timeout_secs: u64,
    /// The workspace folder.
    pub root: Option<&'a Path>,
}

/// Builds the PDF and Markdown reports. The `postino` binary passes the app's builder, so a
/// report from the command line is the same as one exported from the app.
pub trait ReportRenderer {
    /// The bytes of the `kind` document of `run`.
    fn render(&self, kind: DocumentKind, run: &ReportRun<'_>) -> Result<Vec<u8>, String>;
}

/// No document builder: only JSON and CSV reports.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRenderer;

impl ReportRenderer for NoRenderer {
    fn render(&self, _kind: DocumentKind, _run: &ReportRun<'_>) -> Result<Vec<u8>, String> {
        Err("PDF and Markdown reports need the postino binary".to_string())
    }
}

/// The bytes of the `format` report of `run`.
pub fn render(
    format: ReportFormat,
    run: &ReportRun<'_>,
    renderer: &dyn ReportRenderer,
) -> Result<Vec<u8>, String> {
    let record = run.record;
    match format {
        ReportFormat::Pdf => renderer.render(DocumentKind::Pdf, run),
        ReportFormat::Markdown => renderer.render(DocumentKind::Markdown, run),
        ReportFormat::Csv => Ok(requests_csv(run.log, &record.target_labels, None).into_bytes()),
        ReportFormat::Json => {
            let elapsed_secs = record.snapshot.elapsed.as_secs_f64();
            let run_secs = u32::try_from(record.config.duration_secs)
                .unwrap_or(u32::MAX)
                .max(elapsed_secs.floor() as u32)
                .max(1);
            let stats = window_stats(
                run.log,
                record.config.target_count,
                run_secs,
                None,
                elapsed_secs,
            );
            Ok(run_json(record, Some(run.log), &stats, None).into_bytes())
        }
    }
}

/// Writes `bytes` to `path` through a temporary file next to it, so a failure never leaves half
/// a report over an existing one.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(".{file_name}.partial"));
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        error.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_picks_the_format() {
        assert_eq!(
            ReportFormat::from_path(Path::new("r.PDF")),
            Ok(ReportFormat::Pdf)
        );
        assert_eq!(
            ReportFormat::from_path(Path::new("out/r.md")),
            Ok(ReportFormat::Markdown)
        );
        assert_eq!(
            ReportFormat::from_path(Path::new("r.csv")),
            Ok(ReportFormat::Csv)
        );
        assert!(ReportFormat::from_path(Path::new("r.txt")).is_err());
        assert!(ReportFormat::from_path(Path::new("report")).is_err());
    }
}
