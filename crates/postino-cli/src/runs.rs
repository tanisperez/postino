//! The saved load test runs (`.postino/runs/`): `postino runs` lists them, `runs show` prints
//! the summary of one, `runs compare` the changes between two, and `postino report` writes the
//! report of one, the same files as the app's Export menu.

use std::io::{IsTerminal, Write};
use std::path::Path;

use postino_load::history::{self, RunRecord};
use postino_load::{LoadSnapshot, SampleLog, Threshold, ThresholdMetric};
use serde_json::json;

use crate::args::{OutputFormat, ReportArgs, ReportFormatArg, RunsAction, RunsArgs};
use crate::context::{self, WorkspaceLookup};
use crate::exit::{self, CliError};
use crate::hooks::AppHooks;
use crate::load::fail_if::{FailIf, Metric, Op};
use crate::load::live::Live;
use crate::load::summary;
use crate::reports::{self, ReportFormat, ReportRun};
use crate::style::Style;

/// The timeout reports name in the title of their timeout errors: the default, since a saved run
/// does not keep the one it was sent with.
const REPORT_TIMEOUT_SECS: u64 = 30;

/// The workspace folder of a command with `workspace` options.
fn workspace_root(args: &crate::args::WorkspaceArgs) -> Result<std::path::PathBuf, CliError> {
    let workspace = context::open_workspace(
        &args.common(),
        WorkspaceLookup::OrFolder(context::current_dir()),
    )?
    .ok_or_else(|| CliError::file("no workspace"))?;
    Ok(workspace.root().to_path_buf())
}

/// The number `text` names: a number, or `last`, the newest run.
pub fn run_number(root: &Path, text: &str) -> Result<u32, CliError> {
    if text.eq_ignore_ascii_case("last") {
        return history::list(root)
            .first()
            .map(|header| header.number)
            .ok_or_else(|| CliError::file("there is no saved run in this workspace"));
    }
    text.trim()
        .trim_start_matches('#')
        .parse()
        .map_err(|_| CliError::usage(format!("{text:?}: expected last or a run number")))
}

/// Reads run `number`.
pub fn load_run(root: &Path, number: u32) -> Result<RunRecord, CliError> {
    history::load(root, number).map_err(|error| match error {
        history::HistoryError::Io { source, .. }
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            CliError::file(format!(
                "run {number} does not exist; postino runs lists them"
            ))
        }
        error => CliError::file(format!("run {number}: {error}")),
    })
}

/// The requests of run `number`, empty for a run saved before they were kept.
fn load_samples(root: &Path, number: u32) -> SampleLog {
    history::load_samples(root, number)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// The `--fail-if` condition a saved threshold stands for: it fails when the goal is missed.
pub fn condition_of(threshold: &Threshold) -> FailIf {
    let (metric, op, limit) = match threshold.metric {
        ThresholdMetric::P50 => (Metric::P50, Op::Ge, threshold.limit),
        ThresholdMetric::P95 => (Metric::P95, Op::Ge, threshold.limit),
        ThresholdMetric::P99 => (Metric::P99, Op::Ge, threshold.limit),
        ThresholdMetric::ErrorRate => (Metric::Errors, Op::Ge, threshold.limit),
        ThresholdMetric::Rps => (Metric::Rps, Op::Lt, threshold.limit),
    };
    FailIf { metric, op, limit }
}

/// `2026-11-13 10:19`, local time.
fn local_time(unix: u64) -> String {
    i64::try_from(unix)
        .ok()
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The first summary line of a saved run.
pub fn header_line(record: &RunRecord) -> String {
    let config = &record.config;
    format!(
        "Run {} · {} · {} VUs · {} · ramp-up {} · think {} · {}",
        record.number,
        record.target_label,
        config.vus,
        summary::span(std::time::Duration::from_secs(config.duration_secs)),
        summary::span(std::time::Duration::from_secs(config.ramp_up_secs)),
        summary::span(std::time::Duration::from_millis(config.think_time_ms)),
        if config.reuse_connections {
            "keep-alive"
        } else {
            "new connections"
        }
    )
}

/// The readable list of runs.
fn list_text(records: &[RunRecord], style: Style) -> String {
    if records.is_empty() {
        return " No saved runs. postino load and the app save them in .postino/runs.\n"
            .to_string();
    }
    let width = records
        .iter()
        .map(|record| record.target_label.chars().count())
        .max()
        .unwrap_or(0)
        .max(6);
    let mut out = format!(
        " {:>4}  {:<16}  {:<width$}  {:>4}  {:>9}  {:>8}  {:>8}  {:>7}\n",
        "Run", "Date", "Target", "VUs", "Requests", "RPS", "p95", "Errors"
    );
    for record in records {
        let snapshot = &record.snapshot;
        let errors = format!("{:.1} %", snapshot.error_rate * 100.0);
        let errors = if snapshot.error_rate > 0.0 {
            style.red(&format!("{errors:>7}"))
        } else {
            format!("{errors:>7}")
        };
        out.push_str(&format!(
            " {:>4}  {:<16}  {:<width$}  {:>4}  {:>9}  {:>8.1}  {:>8}  {errors}{}\n",
            record.number,
            local_time(record.started_at_unix),
            record.target_label,
            record.config.vus,
            snapshot.total,
            snapshot.rps,
            summary::micros(snapshot.p95),
            if record.stopped_early {
                style.dim("  stopped")
            } else {
                String::new()
            }
        ));
    }
    out
}

/// The JSON line of a run of the list.
fn list_json(record: &RunRecord) -> String {
    let snapshot = &record.snapshot;
    json!({
        "run": record.number,
        "started_at": record.started_at_unix,
        "target": record.target_label,
        "vus": record.config.vus,
        "duration_secs": record.config.duration_secs,
        "requests": snapshot.total,
        "rps": snapshot.rps,
        "p50_ms": f64::from(snapshot.p50) / 1000.0,
        "p95_ms": f64::from(snapshot.p95) / 1000.0,
        "p99_ms": f64::from(snapshot.p99) / 1000.0,
        "error_rate": snapshot.error_rate,
        "stopped_early": record.stopped_early,
    })
    .to_string()
}

/// The summary of a saved run, as `postino load` prints it at the end.
fn show_text(root: &Path, record: &RunRecord, style: Style) -> String {
    let mut live = Live::default();
    live.finish(load_samples(root, record.number));
    let values = live.values(&record.snapshot);
    let conditions = record
        .thresholds
        .iter()
        .map(|threshold| {
            let condition = condition_of(threshold);
            (condition, condition.hit(&values))
        })
        .collect();
    let ending = format!(
        "{} {} after {}",
        local_time(record.started_at_unix),
        if record.stopped_early {
            "· stopped"
        } else {
            "· finished"
        },
        summary::clock(record.snapshot.elapsed)
    );
    summary::summary(
        &summary::Summary {
            header: header_line(record),
            ending,
            saved: None,
            snapshot: &record.snapshot,
            values,
            targets: &record.target_labels,
            compare: None,
            conditions,
        },
        style,
    )
}

/// The changes from `before` to `after`.
fn compare_text(before: &RunRecord, after: &RunRecord, style: Style) -> String {
    let mut out = format!(
        "\n {}\n {}\n\n Changes from run {} to run {}\n",
        style.dim(&header_line(before)),
        style.dim(&header_line(after)),
        before.number,
        after.number
    );
    for delta in postino_load::compare(&before.snapshot, &after.snapshot) {
        out.push_str(&format!(" {}\n", summary::delta_line(&delta, style)));
    }
    out
}

/// Runs `postino runs` and returns its exit code.
pub fn runs_command(args: &RunsArgs) -> u8 {
    let style = Style::stdout(args.workspace.no_color);
    let result = (|| -> Result<String, CliError> {
        let root = workspace_root(&args.workspace)?;
        match &args.action {
            None => {
                let records: Vec<RunRecord> = history::list(&root)
                    .iter()
                    .filter_map(|header| history::load(&root, header.number).ok())
                    .collect();
                Ok(match args.output {
                    OutputFormat::Json => records
                        .iter()
                        .map(|record| format!("{}\n", list_json(record)))
                        .collect(),
                    _ => list_text(&records, style),
                })
            }
            Some(RunsAction::Show { run }) => {
                let record = load_run(&root, run_number(&root, run)?)?;
                Ok(show_text(&root, &record, style))
            }
            Some(RunsAction::Compare { before, after }) => {
                let before = load_run(&root, run_number(&root, before)?)?;
                let after = load_run(&root, run_number(&root, after)?)?;
                Ok(compare_text(&before, &after, style))
            }
        }
    })();
    print_or_fail(result, args.workspace.no_color)
}

/// Prints `result` to stdout, or its error to stderr.
fn print_or_fail(result: Result<String, CliError>, no_color: bool) -> u8 {
    match result {
        Ok(text) => {
            let _ = std::io::stdout().lock().write_all(text.as_bytes());
            exit::SUCCESS
        }
        Err(error) => {
            eprintln!("{} {error}", Style::stderr(no_color).red("error:"));
            error.code
        }
    }
}

/// The run `--compare` names for the report of `record`: `last` is the newest earlier run of the
/// same target.
fn compared(root: &Path, record: &RunRecord, text: &str) -> Result<(u32, LoadSnapshot), CliError> {
    let number = if text.eq_ignore_ascii_case("last") {
        history::list(root)
            .into_iter()
            .find(|header| {
                header.number < record.number && header.target_label == record.target_label
            })
            .map(|header| header.number)
            .ok_or_else(|| {
                CliError::file(format!(
                    "--compare last: no run of {} before run {}",
                    record.target_label, record.number
                ))
            })?
    } else {
        run_number(root, text)?
    };
    Ok((number, load_run(root, number)?.snapshot))
}

/// Runs `postino report` and returns its exit code.
pub fn report_command(args: &ReportArgs, hooks: &dyn AppHooks) -> u8 {
    match report(args, hooks) {
        Ok(Some(path)) => {
            eprintln!("Report saved to {path}");
            exit::SUCCESS
        }
        Ok(None) => exit::SUCCESS,
        Err(error) => {
            eprintln!(
                "{} {error}",
                Style::stderr(args.workspace.no_color).red("error:")
            );
            error.code
        }
    }
}

/// Writes the report; returns the file written, `None` when it went to stdout.
fn report(args: &ReportArgs, hooks: &dyn AppHooks) -> Result<Option<String>, CliError> {
    let format = match (&args.output, args.format) {
        (_, Some(format)) => match format {
            ReportFormatArg::Pdf => ReportFormat::Pdf,
            ReportFormatArg::Md => ReportFormat::Markdown,
            ReportFormatArg::Json => ReportFormat::Json,
            ReportFormatArg::Csv => ReportFormat::Csv,
        },
        (Some(path), None) => ReportFormat::from_path(path)?,
        (None, None) => return Err(CliError::usage("give a file with -o, or a --format")),
    };
    if format == ReportFormat::Pdf && args.output.is_none() && std::io::stdout().is_terminal() {
        return Err(CliError::usage(
            "a PDF is not printed to a terminal: write it with -o report.pdf",
        ));
    }
    let root = workspace_root(&args.workspace)?;
    let record = load_run(&root, run_number(&root, &args.run)?)?;
    let log = load_samples(&root, record.number);
    let compare = args
        .compare
        .as_deref()
        .map(|text| compared(&root, &record, text))
        .transpose()?;
    let run = ReportRun {
        record: &record,
        log: &log,
        compare: compare
            .as_ref()
            .map(|(number, snapshot)| (*number, snapshot)),
        timeout_secs: REPORT_TIMEOUT_SECS,
        root: Some(&root),
    };
    let bytes = reports::render(format, &run, hooks).map_err(CliError::file)?;
    match &args.output {
        Some(path) => {
            reports::write(path, &bytes)
                .map_err(|error| CliError::file(format!("{}: {error}", path.display())))?;
            Ok(Some(path.display().to_string()))
        }
        None => {
            match std::io::stdout().lock().write_all(&bytes) {
                // A closed pipe (`| head`) only means nobody reads the rest.
                Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => {
                    Err(CliError::file(format!("cannot write: {error}")))
                }
                _ => Ok(None),
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn saved_goals_read_as_conditions_that_fail_when_missed() {
        let p95 = condition_of(&Threshold {
            metric: ThresholdMetric::P95,
            limit: 250.0,
        });
        assert_eq!(p95.to_string(), "p95 >= 250 ms");
        let rps = condition_of(&Threshold {
            metric: ThresholdMetric::Rps,
            limit: 100.0,
        });
        assert_eq!(rps.to_string(), "rps < 100.0");
    }

    #[test]
    fn run_numbers_are_numbers_or_last() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(run_number(dir.path(), "12"), Ok(12));
        assert_eq!(run_number(dir.path(), "#3"), Ok(3));
        assert_eq!(
            run_number(dir.path(), "x").expect_err("bad").code,
            exit::USAGE
        );
        assert_eq!(
            run_number(dir.path(), "last").expect_err("none").code,
            exit::FILE
        );
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod history_tests {
    use super::*;
    use crate::args::{CommonArgs, LoadArgs, SendArgs, WorkspaceArgs};
    use crate::hooks::NoHooks;
    use postino_http::test_support::{StubResponse, TestServer};
    use std::time::Duration;

    /// A workspace with two saved runs of `list`.
    fn two_runs() -> tempfile::TempDir {
        let server = TestServer::start();
        let handle = server.serve(|_| StubResponse::new(200).with_body("[]"));
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join(".postino")).expect("marker");
        std::fs::write(
            dir.path().join("list.postino"),
            format!("GET {}/list\n", server.url()),
        )
        .expect("request");
        for _ in 0..2 {
            let args = LoadArgs {
                paths: vec![dir.path().join("list.postino")],
                vus: 1,
                duration: Duration::from_millis(300),
                ramp_up: Duration::ZERO,
                think: Duration::from_millis(20),
                no_keep_alive: false,
                stop_on_error: None,
                fail_if: vec![FailIf::parse("p95>5s").expect("condition")],
                no_tui: true,
                report: None,
                no_save: false,
                compare: None,
                common: CommonArgs {
                    no_color: true,
                    ..CommonArgs::default()
                },
                send: SendArgs::default(),
            };
            crate::load::execute(&args, &NoHooks).expect("run");
        }
        drop(server);
        handle.join().expect("server");
        dir
    }

    #[test]
    fn saved_runs_list_show_compare_and_report() {
        let dir = two_runs();
        let root = dir.path();
        let records: Vec<RunRecord> = history::list(root)
            .iter()
            .map(|header| load_run(root, header.number).expect("record"))
            .collect();
        let list = list_text(&records, Style::PLAIN);
        assert!(list.starts_with("  Run  Date"), "{list}");
        assert_eq!(list.lines().count(), 3, "{list}");
        assert_eq!(run_number(root, "last"), Ok(2));

        let show = show_text(root, &records[0], Style::PLAIN);
        assert!(show.contains("Run 2 · list · 1 VUs"), "{show}");
        assert!(show.contains("✓ p95 >= 5.0 s"), "{show}");

        let compare = compare_text(&records[1], &records[0], Style::PLAIN);
        assert!(compare.contains("Changes from run 1 to run 2"), "{compare}");
        assert!(compare.contains("Requests/s"), "{compare}");

        let path = root.join("run.csv");
        let written = report(
            &ReportArgs {
                run: "last".into(),
                output: Some(path.clone()),
                format: None,
                compare: Some("last".into()),
                workspace: WorkspaceArgs {
                    workspace: Some(root.to_path_buf()),
                    no_color: true,
                },
            },
            &NoHooks,
        )
        .expect("report");
        assert!(written.is_some());
        let csv = std::fs::read_to_string(&path).expect("csv");
        assert!(csv.starts_with("id,time_ms,"), "{csv}");
        let pdf = report(
            &ReportArgs {
                run: "1".into(),
                output: Some(root.join("run.pdf")),
                format: None,
                compare: None,
                workspace: WorkspaceArgs {
                    workspace: Some(root.to_path_buf()),
                    no_color: true,
                },
            },
            &NoHooks,
        );
        assert_eq!(pdf.expect_err("no app").code, exit::FILE);
    }
}
