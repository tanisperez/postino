//! `postino run [paths...]`: runs `.postino` requests through the app's pipeline (OAuth, pre and
//! post scripts, `test()`), in sidebar order, sharing the variables scripts set.

use std::io::Write;
use std::path::Path;

use postino_core::log_safe;
use postino_runner::{RunResult, SessionEnv};
use postino_workspace::{Node, RequestEntry, Workspace};

use crate::args::{OutputFormat, RunArgs};
use crate::context::{self, WorkspaceLookup};
use crate::exit::{self, CliError};
use crate::filter;
use crate::outcome::{elapsed, format_duration, outcome};
use crate::report::{self, Record, Totals};
use crate::style::Style;

/// The widest request name column; longer names push the rest of their line.
const MAX_NAME_WIDTH: usize = 40;

/// Every request of the tree in sidebar order, folders first.
pub fn requests(nodes: &[Node]) -> Vec<&RequestEntry> {
    let mut found = Vec::new();
    for node in nodes {
        match node {
            Node::Folder(folder) => found.extend(requests(&folder.children)),
            Node::Request(request) => found.push(request),
        }
    }
    found
}

/// The workspace id of `path`, `""` for the root itself.
fn id_of(root: &Path, path: &Path) -> Result<String, CliError> {
    let absolute = context::absolute(path);
    // Compare canonical paths when both exist, so links and `..` do not get in the way.
    let canonical = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let relative = canonical(&absolute)
        .strip_prefix(canonical(root))
        .map(Path::to_path_buf)
        .map_err(|_| {
            CliError::file(format!(
                "{} is outside the workspace {}",
                path.display(),
                root.display()
            ))
        })?;
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(parts.join("/"))
}

/// The requests `paths` name, in the order given and then sidebar order, without repeats. No
/// path means the whole workspace.
pub fn select<'a>(
    workspace: &'a Workspace,
    paths: &[std::path::PathBuf],
    patterns: &[String],
) -> Result<Vec<&'a RequestEntry>, CliError> {
    let all = requests(workspace.tree());
    let mut selected: Vec<&RequestEntry> = Vec::new();
    if paths.is_empty() {
        selected = all;
    } else {
        for path in paths {
            if !path.exists() {
                return Err(CliError::file(format!(
                    "{}: no such file or folder",
                    path.display()
                )));
            }
            let id = id_of(workspace.root(), path)?;
            let found: Vec<&RequestEntry> = if path.is_dir() {
                let prefix = format!("{id}/");
                all.iter()
                    .copied()
                    .filter(|entry| id.is_empty() || entry.id.starts_with(&prefix))
                    .collect()
            } else {
                let entry = all.iter().copied().find(|entry| entry.id == id);
                vec![entry.ok_or_else(|| {
                    CliError::file(format!("{}: not a .postino request", path.display()))
                })?]
            };
            for entry in found {
                if !selected.iter().any(|known| known.id == entry.id) {
                    selected.push(entry);
                }
            }
        }
    }
    selected.retain(|entry| filter::matches_any(patterns, &entry.id));
    Ok(selected)
}

/// The record of a request that ran.
fn record_of(id: &str, method: String, result: &RunResult) -> Record {
    let verdict = outcome(result);
    let mut warnings: Vec<String> = result.warnings.iter().map(ToString::to_string).collect();
    if let Some(warning) = result.response.as_ref().and_then(|r| r.tls_warning.clone()) {
        warnings.push(warning);
    }
    Record {
        id: id.to_string(),
        method,
        url: result
            .resolved_request
            .as_ref()
            .map(|request| log_safe::url_for_log(&request.url).into_owned()),
        status: result.response.as_ref().map(|response| response.status),
        time: elapsed(result),
        size: result.response.as_ref().map(|response| response.size),
        tests: result.tests.clone(),
        error: verdict.error,
        warnings,
        code: verdict.code,
    }
}

/// The readable lines of one request.
fn human_lines(record: &Record, width: usize, style: Style) -> String {
    let name = record.id.strip_suffix(".postino").unwrap_or(&record.id);
    let mark = if record.passed() {
        style.green("✓")
    } else {
        style.red("✗")
    };
    let mut line = format!(" {mark} {name:<width$}");
    if record.method.is_empty() {
        line.push_str(&format!("  {}", style.red("not run")));
    } else {
        let status = match record.status {
            Some(status) => style.status(status),
            None => style.red("---"),
        };
        line.push_str(&format!(" {:<7} {status}", record.method));
        line.push_str(&style.dim(&format!(" {:>7}", format_duration(record.time))));
        if !record.tests.is_empty() {
            let passed = record.tests.iter().filter(|test| test.passed).count();
            line.push_str(&format!("   {passed}/{} tests", record.tests.len()));
        }
    }
    let mut out = format!("{line}\n");
    for test in record.tests.iter().filter(|test| !test.passed) {
        let message = test.message.as_deref().unwrap_or("failed");
        out.push_str(&format!(
            "     {} {}: {message}\n",
            style.red("✗"),
            test.name
        ));
    }
    if let Some(error) = &record.error {
        out.push_str(&format!("     {}\n", style.red(error)));
    }
    for warning in &record.warnings {
        out.push_str(&format!("     {}\n", style.yellow(&format!("! {warning}"))));
    }
    out
}

/// The last readable line.
fn human_summary(totals: &Totals, style: Style) -> String {
    let requests = if totals.requests == 1 {
        "1 request".to_string()
    } else {
        format!("{} requests", totals.requests)
    };
    let failed = if totals.failed == 0 {
        style.green("all passed")
    } else {
        style.red(&format!("{} failed", totals.failed))
    };
    let tests = if totals.tests == 0 {
        String::new()
    } else {
        format!(" · {}/{} tests passed", totals.passed_tests, totals.tests)
    };
    format!(
        "\n {requests}, {failed}{tests} · {}\n",
        format_duration(totals.time)
    )
}

/// Runs `postino run` and returns its exit code.
pub fn command(args: &RunArgs) -> u8 {
    let style = Style::stdout(args.common.no_color);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match execute(args, &mut out, style) {
        Ok(code) => code,
        Err(error) => {
            let _ = out.flush();
            eprintln!(
                "{} {error}",
                Style::stderr(args.common.no_color).red("error:")
            );
            error.code
        }
    }
}

/// `postino run`, writing to `out`.
pub fn execute(args: &RunArgs, out: &mut dyn Write, style: Style) -> Result<u8, CliError> {
    if args.output == OutputFormat::Human && args.output_file.is_some() {
        return Err(CliError::usage("-o needs --output json or --output junit"));
    }
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
    let selected = select(&workspace, &args.paths, &args.filter)?;
    if selected.is_empty() {
        return Err(CliError::file("no request to run"));
    }
    let environment = context::environment(&args.common, Some(&workspace))?;
    let runner = context::runner(Some(&workspace), context::send_options(&args.send)?)?;

    // The readable lines go to stdout unless the report does.
    let human = args.output == OutputFormat::Human || args.output_file.is_some();
    let width = selected
        .iter()
        .map(|entry| entry.id.trim_end_matches(".postino").chars().count())
        .max()
        .unwrap_or(0)
        .min(MAX_NAME_WIDTH);
    let write_err = |error: std::io::Error| CliError::file(format!("cannot write: {error}"));

    let mut session = SessionEnv::new();
    let mut records = Vec::new();
    for entry in selected {
        let record = match workspace.load_request(&entry.id) {
            Ok(request) => {
                let result = runner.run(&request, &environment, &mut session);
                let record = record_of(&entry.id, request.method.to_string(), &result);
                if human {
                    out.write_all(human_lines(&record, width, style).as_bytes())
                        .map_err(write_err)?;
                    if !record.passed() {
                        for line in &result.console {
                            let text = format!("     │ {}", line.text);
                            writeln!(out, "{}", style.dim(&text)).map_err(write_err)?;
                        }
                    }
                    if args.show_body
                        && let Some(response) = &result.response
                    {
                        out.write_all(&response.body).map_err(write_err)?;
                        if !response.body.ends_with(b"\n") {
                            writeln!(out).map_err(write_err)?;
                        }
                    }
                }
                record
            }
            Err(error) => {
                let record = Record {
                    id: entry.id.clone(),
                    method: String::new(),
                    url: None,
                    status: None,
                    time: std::time::Duration::ZERO,
                    size: None,
                    tests: Vec::new(),
                    error: Some(format!("cannot read the request: {error}")),
                    warnings: Vec::new(),
                    code: exit::FILE,
                };
                if human {
                    out.write_all(human_lines(&record, width, style).as_bytes())
                        .map_err(write_err)?;
                }
                record
            }
        };
        if args.output == OutputFormat::Json && args.output_file.is_none() {
            writeln!(out, "{}", report::json_line(&record)).map_err(write_err)?;
        }
        let failed = !record.passed();
        records.push(record);
        if failed && args.bail {
            break;
        }
    }

    let totals = Totals::of(&records);
    if human {
        out.write_all(human_summary(&totals, style).as_bytes())
            .map_err(write_err)?;
    }
    let report = match args.output {
        OutputFormat::Human => None,
        OutputFormat::Json => Some(match args.output_file {
            Some(_) => report::json(&records),
            None => format!("{}\n", report::json_summary(&totals)),
        }),
        OutputFormat::Junit => Some(report::junit(&records)),
    };
    if let Some(report) = report {
        match &args.output_file {
            Some(path) => std::fs::write(path, report).map_err(|error| {
                CliError::file(format!("cannot write {}: {error}", path.display()))
            })?,
            None => out.write_all(report.as_bytes()).map_err(write_err)?,
        }
    }
    out.flush().map_err(write_err)?;
    Ok(totals.code)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::args::CommonArgs;
    use postino_http::test_support::{StubResponse, TestServer};
    use pretty_assertions::assert_eq;

    /// A workspace with a login that stores a token, a request that sends it, and one whose
    /// test fails.
    fn workspace(base: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        for folder in [".postino", "auth", "environments"] {
            std::fs::create_dir_all(root.join(folder)).expect("folder");
        }
        std::fs::write(root.join("environments/dev.env"), format!("base={base}\n")).expect("env");
        std::fs::write(
            root.join("auth/1-login.postino"),
            "POST {{base}}/login\n\n::: post\nenv.set(\"token\", res.json().token);\n\
             test(\"logged in\", () => expect(res.status).toBe(200));\n",
        )
        .expect("login");
        std::fs::write(
            root.join("auth/2-me.postino"),
            "GET {{base}}/me\nAuthorization: Bearer {{token}}\n\n::: post\n\
             test(\"token sent\", () => expect(res.status).toBe(200));\n",
        )
        .expect("me");
        std::fs::write(
            root.join("broken.postino"),
            "GET {{base}}/teapot\n\n::: post\ntest(\"is 200\", () => expect(res.status).toBe(200));\n",
        )
        .expect("broken");
        dir
    }

    fn args(root: &Path) -> RunArgs {
        RunArgs {
            common: CommonArgs {
                workspace: Some(root.to_path_buf()),
                env: Some("dev".to_string()),
                ..CommonArgs::default()
            },
            ..RunArgs::default()
        }
    }

    fn serve() -> (TestServer, std::thread::JoinHandle<()>) {
        let server = TestServer::start();
        let handle = server.serve(|captured| match captured.url.as_str() {
            "/login" => StubResponse::new(200).with_body("{\"token\":\"t-42\"}"),
            "/me" => {
                let authorized = captured.headers.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization") && value == "Bearer t-42"
                });
                StubResponse::new(if authorized { 200 } else { 401 })
            }
            _ => StubResponse::new(418),
        });
        (server, handle)
    }

    #[test]
    fn a_folder_runs_in_order_sharing_what_scripts_set() {
        let (server, handle) = serve();
        let dir = workspace(&server.url());
        let mut out = Vec::new();
        let code = execute(
            &RunArgs {
                paths: vec![dir.path().join("auth")],
                ..args(dir.path())
            },
            &mut out,
            Style::PLAIN,
        )
        .expect("ran");
        drop(server);
        handle.join().expect("server");
        let text = String::from_utf8(out).expect("utf-8");
        assert_eq!(code, exit::SUCCESS, "{text}");
        assert!(text.contains(" ✓ auth/1-login POST    200"), "{text}");
        assert!(text.contains(" ✓ auth/2-me    GET     200"), "{text}");
        assert!(
            text.contains("2 requests, all passed · 2/2 tests passed"),
            "{text}"
        );
    }

    #[test]
    fn a_red_test_exits_with_1_and_bail_stops_there() {
        let (server, handle) = serve();
        let dir = workspace(&server.url());
        let mut out = Vec::new();
        let code = execute(
            &RunArgs {
                paths: vec![dir.path().join("broken.postino"), dir.path().join("auth")],
                bail: true,
                output: OutputFormat::Json,
                ..args(dir.path())
            },
            &mut out,
            Style::PLAIN,
        )
        .expect("ran");
        drop(server);
        handle.join().expect("server");
        let text = String::from_utf8(out).expect("utf-8");
        assert_eq!(code, exit::FAILED);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "one request and the summary: {text}");
        assert!(lines[0].contains("\"request\":\"broken.postino\""));
        assert!(lines[1].contains("\"exit_code\":1"));
    }

    #[test]
    fn filters_and_unknown_paths() {
        let dir = workspace("http://127.0.0.1:1");
        let workspace = Workspace::open(dir.path()).expect("workspace");
        let ids = |selected: Vec<&RequestEntry>| {
            selected
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>()
        };
        let all = select(&workspace, &[], &[]).expect("all");
        assert_eq!(
            ids(all),
            [
                "auth/1-login.postino",
                "auth/2-me.postino",
                "broken.postino"
            ]
        );
        let filtered = select(&workspace, &[], &["*me".to_string()]).expect("filtered");
        assert_eq!(ids(filtered), ["auth/2-me.postino"]);
        let missing = select(&workspace, &[dir.path().join("nope")], &[]);
        assert_eq!(missing.expect_err("missing").code, exit::FILE);
    }

    #[test]
    fn a_human_output_file_is_a_usage_error() {
        let error = execute(
            &RunArgs {
                output_file: Some("x.txt".into()),
                ..RunArgs::default()
            },
            &mut Vec::new(),
            Style::PLAIN,
        )
        .expect_err("usage");
        assert_eq!(error.code, exit::USAGE);
    }
}
