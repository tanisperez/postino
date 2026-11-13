//! `postino load [paths...]`: a load test from the terminal (#241), with the app's engine
//! (`postino-load`). A live dashboard on a terminal, plain progress lines otherwise or with
//! `--no-tui`. The run is saved to the workspace history as the app does, `--fail-if` conditions
//! set the exit code, and `--report` writes a report at the end.

pub mod dashboard;
pub mod fail_if;
mod live;
mod summary;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use postino_core::Request;
use postino_load::history::{self, LoadConfigSummary, RunRecord};
use postino_load::{LoadConfig, LoadRun, LoadSnapshot, LoadTarget, SampleLog, StatusKey};
use postino_runner::SessionEnv;
use postino_workspace::{RequestEntry, Workspace};

use crate::args::LoadArgs;
use crate::context::{self, WorkspaceLookup};
use crate::exit::{self, CliError};
use crate::reports::{self, ReportFormat, ReportRenderer, ReportRun};
use crate::style::Style;
use live::Live;

/// The time between two `--no-tui` progress lines.
const PROGRESS_EVERY: Duration = Duration::from_secs(5);

/// Which saved run `--compare` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareWith {
    /// The latest earlier run of the same target.
    Last,
    /// This run number.
    Number(u32),
}

fn parse_compare(text: &str) -> Result<CompareWith, CliError> {
    if text.eq_ignore_ascii_case("last") {
        return Ok(CompareWith::Last);
    }
    text.trim()
        .parse()
        .map(CompareWith::Number)
        .map_err(|_| CliError::usage(format!("--compare {text}: expected last or a run number")))
}

/// The name of a request, its file stem, as the app labels load test targets.
fn stem(id: &str) -> &str {
    let name = id.rsplit('/').next().unwrap_or(id);
    name.strip_suffix(".postino").unwrap_or(name)
}

/// What the run targets, as the app labels it: a request's name, a folder's name, the
/// workspace's name, or how many requests there are.
fn target_label(workspace: &Workspace, paths: &[PathBuf], selected: &[&RequestEntry]) -> String {
    match (paths, selected) {
        (_, [one]) => stem(&one.id).to_string(),
        ([], _) => workspace.root().file_name().map_or_else(
            || "workspace".to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        ([path], _) => context::absolute(path).file_name().map_or_else(
            || "workspace".to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        _ => format!("{} requests", selected.len()),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The run `--compare` names, with its number and final snapshot.
fn compared_run(
    root: &Path,
    with: CompareWith,
    label: &str,
) -> Result<(u32, LoadSnapshot), CliError> {
    let number = match with {
        CompareWith::Number(number) => number,
        CompareWith::Last => history::list(root)
            .into_iter()
            .find(|header| header.target_label == label)
            .map(|header| header.number)
            .ok_or_else(|| CliError::file(format!("--compare last: no earlier run of {label}")))?,
    };
    let record = history::load(root, number)
        .map_err(|error| CliError::file(format!("--compare {number}: {error}")))?;
    Ok((number, record.snapshot))
}

/// Ctrl+C outside the dashboard: the first one stops the run cleanly, the second quits.
fn install_ctrl_c(stop: Arc<AtomicBool>) {
    let presses = AtomicU8::new(0);
    let result = ctrlc::set_handler(move || {
        if presses.fetch_add(1, Ordering::SeqCst) > 0 {
            std::process::exit(130);
        }
        eprintln!("Stopping, Ctrl+C again quits at once");
        stop.store(true, Ordering::SeqCst);
    });
    if let Err(error) = result {
        eprintln!("Ctrl+C will not stop the run cleanly: {error}");
    }
}

/// Runs `postino load` and returns its exit code.
pub fn command(args: &LoadArgs, renderer: &dyn ReportRenderer) -> u8 {
    match execute(args, renderer) {
        Ok(code) => code,
        Err(error) => {
            eprintln!(
                "{} {error}",
                Style::stderr(args.common.no_color).red("error:")
            );
            error.code
        }
    }
}

fn execute(args: &LoadArgs, renderer: &dyn ReportRenderer) -> Result<u8, CliError> {
    let report_format = args
        .report
        .as_deref()
        .map(ReportFormat::from_path)
        .transpose()?;
    let compare_with = args.compare.as_deref().map(parse_compare).transpose()?;
    let start = match args.paths.first() {
        Some(path) => {
            let path = context::absolute(path);
            if path.is_file() {
                path.parent().map(Path::to_path_buf).unwrap_or(path)
            } else {
                path
            }
        }
        None => context::current_dir(),
    };
    let workspace = context::open_workspace(&args.common, WorkspaceLookup::OrFolder(start))?
        .ok_or_else(|| CliError::file("no workspace"))?;
    let selected = crate::run::select(&workspace, &args.paths, &[])?;
    if selected.is_empty() {
        return Err(CliError::file("no request to run"));
    }
    let mut targets = Vec::new();
    for entry in &selected {
        let request = workspace
            .load_request(&entry.id)
            .map_err(|error| CliError::file(format!("{}: {error}", entry.id)))?;
        targets.push(LoadTarget::new(stem(&entry.id), request));
    }
    let target_rows: Vec<(String, String)> = targets
        .iter()
        .map(|target| (target.request.method.to_string(), target.id.clone()))
        .collect();
    let label = target_label(&workspace, &args.paths, &selected);
    let root = workspace.root().to_path_buf();
    let compare = compare_with
        .map(|with| compared_run(&root, with, &label))
        .transpose()?;

    let environment = context::environment(&args.common, Some(&workspace))?;
    let options = context::send_options(&args.send)?;
    let timeout_secs = options.timeout.as_secs();
    let runner = Arc::new(context::runner(Some(&workspace), options)?);

    // The OAuth tokens first, outside the dashboard: a browser sign-in prints its address, and a
    // profile that fails would fail every request of the run.
    let session = SessionEnv::new();
    let requests: Vec<&Request> = targets.iter().map(|target| &target.request).collect();
    for run in runner.prepare_oauth(&requests, &environment, &session) {
        if let Err(error) = &run.result {
            let code = match error {
                postino_oauth::OAuthError::Send(_) => exit::NETWORK,
                _ => exit::FAILED,
            };
            let message = match error {
                postino_oauth::OAuthError::SignInRequired => {
                    crate::oauth::sign_in_message(&run.profile)
                }
                _ => format!("OAuth token of \"{}\": {error}", run.profile),
            };
            return Err(CliError { code, message });
        }
    }

    let number = history::next_number(&root);
    let config = LoadConfig {
        targets,
        vus: args.vus,
        duration: args.duration,
        ramp_up: args.ramp_up,
        think_time: args.think,
        stop_on_error_rate: args.stop_on_error,
        reuse_connections: !args.no_keep_alive,
    };
    let config_summary = LoadConfigSummary::from(&config);
    let thresholds: Vec<_> = args
        .fail_if
        .iter()
        .copied()
        .filter_map(fail_if::FailIf::to_threshold)
        .collect();
    let header_line = format!(
        "postino load · {label} · {} VUs · {} · ramp-up {} · think {} · {}",
        args.vus,
        summary::span(args.duration),
        summary::span(args.ramp_up),
        summary::span(args.think),
        if args.no_keep_alive {
            "new connections"
        } else {
            "keep-alive"
        }
    );
    let record_of = |snapshot: LoadSnapshot, stopped_early: bool, started_at_unix: u64| RunRecord {
        number,
        started_at_unix,
        target_label: label.clone(),
        config: config_summary.clone(),
        snapshot,
        stopped_early,
        target_labels: target_rows.clone(),
        thresholds: thresholds.clone(),
    };

    let tui = !args.no_tui && std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
    let started_at = unix_now();
    let run = LoadRun::start(config, environment, session, runner);
    let mut live = Live::default();
    let user_stopped = if tui {
        let header = dashboard::Header {
            label: label.clone(),
            vus: args.vus,
            duration: args.duration,
            ramp_up: args.ramp_up,
            targets: target_rows.clone(),
            color: Style::stdout(args.common.no_color).has_color(),
        };
        let mut save = |snapshot: &LoadSnapshot, log: &SampleLog| -> Result<String, String> {
            let path = args
                .report
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("run-{number:04}.md")));
            let format = ReportFormat::from_path(&path).map_err(|error| error.message)?;
            let record = record_of(snapshot.clone(), true, started_at);
            let run = ReportRun {
                record: &record,
                log,
                compare: compare
                    .as_ref()
                    .map(|(number, snapshot)| (*number, snapshot)),
                timeout_secs,
                root: Some(&root),
            };
            let bytes = reports::render(format, &run, renderer)?;
            reports::write(&path, &bytes)?;
            Ok(format!("Report saved to {}", path.display()))
        };
        match dashboard::show(&run, &mut live, &header, &mut save) {
            Ok(stopped) => stopped,
            Err(error) => {
                run.stop();
                eprintln!("The dashboard failed: {error}");
                true
            }
        }
    } else {
        let stop = Arc::new(AtomicBool::new(false));
        install_ctrl_c(Arc::clone(&stop));
        eprintln!("{header_line}");
        let mut last_line = Instant::now();
        while !run.is_finished() {
            std::thread::sleep(Duration::from_millis(100));
            if stop.load(Ordering::SeqCst) {
                run.stop();
            }
            if last_line.elapsed() >= PROGRESS_EVERY {
                last_line = Instant::now();
                eprintln!(
                    "{}",
                    summary::progress_line(&run.snapshot(), args.duration, args.vus, false)
                );
            }
        }
        stop.load(Ordering::SeqCst)
    };

    let result = run.join();
    live.finish(result.log);
    let record = record_of(result.snapshot, result.stopped_early, started_at);
    let style = Style::stdout(args.common.no_color);

    let mut code = exit::SUCCESS;
    let saved = if args.no_save {
        None
    } else {
        let saved = history::save(&root, &record)
            .and_then(|()| history::save_samples(&root, number, live.log()));
        match saved {
            Ok(()) => Some(format!("Saved as run {number} in .postino/runs")),
            Err(error) => {
                eprintln!(
                    "{} could not save run {number}: {error}",
                    style.red("error:")
                );
                code = exit::FILE;
                None
            }
        }
    };
    if let (Some(path), Some(format)) = (&args.report, report_format) {
        let run = ReportRun {
            record: &record,
            log: live.log(),
            compare: compare
                .as_ref()
                .map(|(number, snapshot)| (*number, snapshot)),
            timeout_secs,
            root: Some(&root),
        };
        match reports::render(format, &run, renderer).and_then(|bytes| reports::write(path, &bytes))
        {
            Ok(()) => eprintln!("Report saved to {}", path.display()),
            Err(error) => {
                eprintln!(
                    "{} could not write {}: {error}",
                    style.red("error:"),
                    path.display()
                );
                code = exit::worst(code, exit::FILE);
            }
        }
    }

    let snapshot = &record.snapshot;
    let values = live.values(snapshot);
    let conditions: Vec<_> = args
        .fail_if
        .iter()
        .map(|condition| (*condition, condition.hit(&values)))
        .collect();
    let elapsed = summary::clock(snapshot.elapsed);
    let error_stop = record.stopped_early && !user_stopped && args.stop_on_error.is_some();
    let ending = if error_stop {
        style.red(&format!(
            "Stopped after {elapsed}: the error rate went above {:.1} %",
            args.stop_on_error.unwrap_or_default() * 100.0
        ))
    } else if record.stopped_early {
        format!("Stopped after {elapsed}")
    } else {
        format!("Finished after {elapsed}")
    };
    let text = summary::summary(
        &summary::Summary {
            header: header_line,
            ending,
            saved,
            snapshot,
            values,
            targets: &target_rows,
            compare: compare
                .as_ref()
                .map(|(number, previous)| (*number, postino_load::compare(previous, snapshot))),
            conditions: conditions.clone(),
        },
        style,
    );
    print!("{text}");

    let no_response = snapshot.total > 0
        && snapshot
            .status_counts
            .iter()
            .all(|(status, _)| matches!(status, StatusKey::Timeout | StatusKey::Failed));
    if no_response {
        code = exit::worst(code, exit::NETWORK);
    }
    if snapshot.total == 0 || error_stop || conditions.iter().any(|(_, hit)| *hit) {
        code = exit::worst(code, exit::FAILED);
    }
    Ok(code)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::args::CommonArgs;
    use crate::reports::NoRenderer;
    use postino_http::test_support::{StubResponse, TestServer};
    use pretty_assertions::assert_eq;

    fn workspace(base: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join(".postino")).expect("marker");
        std::fs::create_dir_all(dir.path().join("api")).expect("folder");
        std::fs::write(
            dir.path().join("api/list.postino"),
            format!("GET {base}/list\n"),
        )
        .expect("request");
        dir
    }

    fn args(root: &Path) -> LoadArgs {
        LoadArgs {
            paths: vec![root.join("api/list.postino")],
            vus: 2,
            duration: Duration::from_millis(400),
            ramp_up: Duration::ZERO,
            think: Duration::from_millis(10),
            no_keep_alive: false,
            stop_on_error: None,
            fail_if: Vec::new(),
            no_tui: true,
            report: None,
            no_save: false,
            compare: None,
            common: CommonArgs {
                no_color: true,
                ..CommonArgs::default()
            },
            send: crate::args::SendArgs::default(),
        }
    }

    #[test]
    fn a_run_is_saved_reported_and_checked() {
        let server = TestServer::start();
        let handle = server.serve(|_| StubResponse::new(200).with_body("[]"));
        let dir = workspace(&server.url());
        let report = dir.path().join("out.json");

        let first = execute(&args(dir.path()), &NoRenderer).expect("first run");
        assert_eq!(first, exit::SUCCESS);
        let second = execute(
            &LoadArgs {
                fail_if: vec![fail_if::FailIf::parse("rps<1000000").expect("condition")],
                report: Some(report.clone()),
                compare: Some("last".to_string()),
                ..args(dir.path())
            },
            &NoRenderer,
        )
        .expect("second run");
        drop(server);
        handle.join().expect("server");

        assert_eq!(second, exit::FAILED, "the condition holds");
        let runs = history::list(dir.path());
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].target_label, "list");
        let saved = history::load(dir.path(), runs[0].number).expect("record");
        assert_eq!(
            saved.thresholds.len(),
            1,
            "the condition is saved as a goal"
        );
        let json = std::fs::read_to_string(&report).expect("report");
        assert!(json.contains("\"per_request\""), "{json}");
    }

    #[test]
    fn nothing_answering_is_a_network_failure() {
        let dir = workspace("http://127.0.0.1:1");
        let code = execute(
            &LoadArgs {
                no_save: true,
                ..args(dir.path())
            },
            &NoRenderer,
        )
        .expect("ran");
        assert_eq!(code, exit::NETWORK);
        assert!(history::list(dir.path()).is_empty());
    }

    #[test]
    fn bad_report_and_compare_options_are_usage_errors() {
        let dir = workspace("http://127.0.0.1:1");
        let error = execute(
            &LoadArgs {
                report: Some("r.txt".into()),
                ..args(dir.path())
            },
            &NoRenderer,
        )
        .expect_err("report");
        assert_eq!(error.code, exit::USAGE);
        let error = execute(
            &LoadArgs {
                compare: Some("yesterday".into()),
                ..args(dir.path())
            },
            &NoRenderer,
        )
        .expect_err("compare");
        assert_eq!(error.code, exit::USAGE);
        let error = execute(
            &LoadArgs {
                compare: Some("last".into()),
                ..args(dir.path())
            },
            &NoRenderer,
        )
        .expect_err("no earlier run");
        assert_eq!(error.code, exit::FILE);
    }
}
