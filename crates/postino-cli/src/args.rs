//! The arguments of the console commands (#220), parsed with `clap`. Only the subcommands live
//! here: `postino` alone, a path, `--help` and `--version` are handled before `clap` sees them
//! (see [`crate::Command::parse`]).

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::units::{parse_duration, parse_size};

/// The names that make Postino run in console mode when they are the first argument. A folder
/// named like one of them opens in the app with `postino ./name`.
pub const SUBCOMMANDS: [&str; 11] = [
    "run", "check", "load", "get", "post", "put", "patch", "delete", "head", "options", "http",
];

/// `postino <command> ...`.
#[derive(Debug, Parser)]
#[command(
    name = "postino",
    disable_version_flag = true,
    about = "A lightweight, native HTTP client. Requests are plain local files."
)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub command: ConsoleCommand,
}

/// A command that runs in the terminal, without the window.
#[derive(Debug, Subcommand)]
pub enum ConsoleCommand {
    /// Run .postino requests: files, folders (recursively) or the whole workspace.
    Run(RunArgs),
    /// Check that every .postino and .env file of the workspace can be read.
    Check(CheckArgs),
    /// Run a load test on requests or folders, with a live dashboard.
    Load(LoadArgs),
    /// Send a GET request.
    Get(RequestArgs),
    /// Send a POST request.
    Post(RequestArgs),
    /// Send a PUT request.
    Put(RequestArgs),
    /// Send a PATCH request.
    Patch(RequestArgs),
    /// Send a DELETE request.
    Delete(RequestArgs),
    /// Send a HEAD request.
    Head(RequestArgs),
    /// Send an OPTIONS request.
    Options(RequestArgs),
    /// Send a request with any method, custom ones too.
    Http(HttpArgs),
}

/// The options every command takes: where the workspace is and the variables.
#[derive(Debug, Clone, Default, Args)]
pub struct CommonArgs {
    /// The workspace folder. By default, the folder holding `.postino/` above the given path or
    /// the current directory.
    #[arg(long, value_name = "DIR")]
    pub workspace: Option<PathBuf>,
    /// The environment to use, a file name under environments/ (its .local.env included).
    #[arg(short = 'e', long = "env", value_name = "NAME")]
    pub env: Option<String>,
    /// A variable, wins over the environment. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    pub vars: Vec<(String, String)>,
    /// A .env file whose variables win over the environment's.
    #[arg(long = "env-file", value_name = "FILE")]
    pub env_file: Option<PathBuf>,
    /// Never print colors. The NO_COLOR environment variable does the same.
    #[arg(long)]
    pub no_color: bool,
}

/// How requests are sent, the same settings as the app's.
#[derive(Debug, Clone, Default, Args)]
pub struct SendArgs {
    /// Accept any TLS certificate, with no warning.
    #[arg(short = 'k', long)]
    pub insecure: bool,
    /// The time limit of a request, such as 30s, 500ms or 2m.
    #[arg(long, value_name = "TIME", value_parser = parse_duration)]
    pub timeout: Option<Duration>,
    /// The largest response body read, such as 50MB.
    #[arg(long, value_name = "SIZE", value_parser = parse_size)]
    pub max_size: Option<u64>,
    /// The proxy, such as http://proxy:3128 or socks5://localhost:1080.
    #[arg(long, value_name = "URL")]
    pub proxy: Option<String>,
}

/// The output format of `run` and `check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum OutputFormat {
    /// Readable lines, in color on a terminal.
    #[default]
    Human,
    /// One JSON object per line.
    Json,
    /// A JUnit XML report, for CI.
    Junit,
}

/// `postino run [paths...]`.
#[derive(Debug, Clone, Default, Args)]
pub struct RunArgs {
    /// Request files or folders. Without any, the whole workspace runs.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,
    /// Stop at the first request that fails.
    #[arg(long)]
    pub bail: bool,
    /// Only the requests whose path matches, such as 'auth/*'. Repeatable.
    #[arg(long, value_name = "PATTERN")]
    pub filter: Vec<String>,
    /// Print each response body.
    #[arg(long)]
    pub show_body: bool,
    /// The output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub output: OutputFormat,
    /// Write the json or junit output to this file, and the readable lines to the terminal.
    #[arg(short = 'o', long = "output-file", value_name = "FILE")]
    pub output_file: Option<PathBuf>,
    #[command(flatten)]
    pub common: CommonArgs,
    #[command(flatten)]
    pub send: SendArgs,
}

/// `postino check`.
#[derive(Debug, Clone, Default, Args)]
pub struct CheckArgs {
    /// The output format: human or json.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub output: OutputFormat,
    #[command(flatten)]
    pub common: CommonArgs,
}

/// `postino load [paths...]`.
#[derive(Debug, Clone, Args)]
pub struct LoadArgs {
    /// Request files or folders, looped over in order. Without any, the whole workspace.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,
    /// Virtual users, 1 to 500.
    #[arg(long, value_name = "N", default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=500))]
    pub vus: u32,
    /// How long the run lasts, such as 60s or 2m.
    #[arg(long, value_name = "TIME", default_value = "60s", value_parser = parse_duration)]
    pub duration: Duration,
    /// How long it takes to start every virtual user.
    #[arg(long, value_name = "TIME", default_value = "10s", value_parser = parse_duration)]
    pub ramp_up: Duration,
    /// The pause of each virtual user between two passes over the requests.
    #[arg(long, value_name = "TIME", default_value = "0s", value_parser = parse_duration)]
    pub think: Duration,
    /// Open a new connection for every request instead of reusing them.
    #[arg(long)]
    pub no_keep_alive: bool,
    /// Stop early above this error rate, such as 5%.
    #[arg(long, value_name = "RATE", value_parser = parse_percent)]
    pub stop_on_error: Option<f64>,
    /// Fail the run when this is true, such as 'p95>500ms', 'errors>1%' or 'rps<100'.
    /// Repeatable.
    #[arg(long = "fail-if", value_name = "CONDITION", value_parser = crate::load::fail_if::FailIf::parse)]
    pub fail_if: Vec<crate::load::fail_if::FailIf>,
    /// Print plain progress lines instead of the dashboard.
    #[arg(long)]
    pub no_tui: bool,
    /// Write a report at the end: .pdf, .md, .json or .csv, by extension.
    #[arg(long, value_name = "FILE")]
    pub report: Option<PathBuf>,
    /// Do not save the run to the workspace history (.postino/runs/).
    #[arg(long)]
    pub no_save: bool,
    /// Compare with a saved run: last, or a run number.
    #[arg(long, value_name = "RUN")]
    pub compare: Option<String>,
    #[command(flatten)]
    pub common: CommonArgs,
    #[command(flatten)]
    pub send: SendArgs,
}

/// `5%`, `0.5%` or `5`: a percentage, returned as a fraction.
fn parse_percent(text: &str) -> Result<f64, String> {
    let number = text.trim().trim_end_matches('%').trim();
    match number.parse::<f64>() {
        Ok(value) if value.is_finite() && (0.0..=100.0).contains(&value) => Ok(value / 100.0),
        _ => Err(format!("expected a percentage such as 5%, got {text:?}")),
    }
}

/// `postino get|post|... <url>`.
#[derive(Debug, Clone, Default, Args)]
pub struct RequestArgs {
    /// The URL. It may hold {{variables}}.
    #[arg(value_name = "URL")]
    pub url: String,
    /// A header, 'Name: Value'. Repeatable.
    #[arg(short = 'H', long = "header", value_name = "HEADER")]
    pub headers: Vec<String>,
    /// A query parameter, key=value. Repeatable.
    #[arg(short = 'q', long = "query", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    pub query: Vec<(String, String)>,
    /// A raw body, or @file to send a file's text.
    #[arg(short = 'd', long = "data", value_name = "TEXT", group = "body")]
    pub data: Option<String>,
    /// A JSON body, with its Content-Type.
    #[arg(long, value_name = "TEXT", group = "body")]
    pub json: Option<String>,
    /// A multipart field, field=value or field=@file. Repeatable.
    #[arg(short = 'F', long = "form", value_name = "FIELD=VALUE", group = "body", value_parser = parse_key_value)]
    pub form: Vec<(String, String)>,
    /// Basic authorization, user:password.
    #[arg(
        short = 'u',
        long = "user",
        value_name = "USER:PASSWORD",
        group = "auth"
    )]
    pub user: Option<String>,
    /// Bearer authorization.
    #[arg(long, value_name = "TOKEN", group = "auth")]
    pub bearer: Option<String>,
    /// Print the response status and headers before the body.
    #[arg(short = 'i', long = "include")]
    pub include: bool,
    /// Also print what was sent, secrets masked, to stderr.
    #[arg(short = 'v', long)]
    pub verbose: bool,
    /// Save the body to a file instead of printing it, with a progress bar.
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Save the request as a .postino file.
    #[arg(long, value_name = "PATH")]
    pub save: Option<PathBuf>,
    #[command(flatten)]
    pub common: CommonArgs,
    #[command(flatten)]
    pub send: SendArgs,
}

/// `postino http <METHOD> <url>`.
#[derive(Debug, Clone, Args)]
pub struct HttpArgs {
    /// The method, such as GET or PURGE.
    #[arg(value_name = "METHOD")]
    pub method: String,
    #[command(flatten)]
    pub request: RequestArgs,
}

/// `key=value`, split at the first `=`.
fn parse_key_value(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((key, value)) if !key.trim().is_empty() => {
            Ok((key.trim().to_string(), value.to_string()))
        }
        _ => Err(format!("expected key=value, got {text:?}")),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("postino").chain(args.iter().copied()))
    }

    #[test]
    fn key_values_split_at_the_first_equals_sign() {
        assert_eq!(
            parse_key_value("token=a=b"),
            Ok(("token".to_string(), "a=b".to_string()))
        );
        assert_eq!(
            parse_key_value("empty="),
            Ok(("empty".to_string(), String::new()))
        );
        assert!(parse_key_value("novalue").is_err());
        assert!(parse_key_value("=value").is_err());
    }

    #[test]
    fn run_takes_paths_and_its_options() {
        let cli = parse(&[
            "run",
            "auth/",
            "crud/create.postino",
            "--env",
            "dev",
            "--var",
            "user=ada",
            "--bail",
            "--output",
            "junit",
            "-o",
            "report.xml",
        ])
        .expect("valid");
        let ConsoleCommand::Run(run) = cli.command else {
            panic!("not run");
        };
        assert_eq!(run.paths.len(), 2);
        assert_eq!(run.common.env.as_deref(), Some("dev"));
        assert_eq!(run.common.vars, [("user".to_string(), "ada".to_string())]);
        assert!(run.bail);
        assert_eq!(run.output, OutputFormat::Junit);
        assert_eq!(run.output_file, Some(PathBuf::from("report.xml")));
    }

    #[test]
    fn one_off_requests_take_curl_like_options() {
        let cli = parse(&[
            "post",
            "https://api.test/login",
            "-H",
            "X-Trace: 1",
            "--json",
            "{}",
            "-u",
            "ada:secret",
            "--timeout",
            "5s",
            "--max-size",
            "50MB",
        ])
        .expect("valid");
        let ConsoleCommand::Post(request) = cli.command else {
            panic!("not post");
        };
        assert_eq!(request.url, "https://api.test/login");
        assert_eq!(request.headers, ["X-Trace: 1"]);
        assert_eq!(request.json.as_deref(), Some("{}"));
        assert_eq!(request.send.timeout, Some(Duration::from_secs(5)));
        assert_eq!(request.send.max_size, Some(50_000_000));
    }

    #[test]
    fn only_one_body_and_one_auth() {
        assert!(parse(&["post", "u", "-d", "a", "--json", "{}"]).is_err());
        assert!(parse(&["get", "u", "-u", "a:b", "--bearer", "t"]).is_err());
    }

    #[test]
    fn load_has_the_app_defaults_and_its_own_options() {
        let cli = parse(&["load", "crud/"]).expect("valid");
        let ConsoleCommand::Load(load) = cli.command else {
            panic!("not load");
        };
        assert_eq!(load.vus, 50);
        assert_eq!(load.duration, Duration::from_secs(60));
        assert_eq!(load.ramp_up, Duration::from_secs(10));
        assert_eq!(load.think, Duration::ZERO);
        assert!(!load.no_keep_alive);

        let cli = parse(&[
            "load",
            "a.postino",
            "--vus",
            "20",
            "--duration",
            "2m",
            "--stop-on-error",
            "5%",
            "--fail-if",
            "p95>500ms",
            "--fail-if",
            "errors>1%",
            "--compare",
            "last",
        ])
        .expect("valid");
        let ConsoleCommand::Load(load) = cli.command else {
            panic!("not load");
        };
        assert_eq!(load.vus, 20);
        assert_eq!(load.stop_on_error, Some(0.05));
        assert_eq!(load.fail_if.len(), 2);
        assert!(parse(&["load", "--vus", "0"]).is_err());
        assert!(parse(&["load", "--vus", "501"]).is_err());
        assert!(parse(&["load", "--fail-if", "p95"]).is_err());
    }

    #[test]
    fn percentages_become_fractions() {
        assert_eq!(parse_percent("5%"), Ok(0.05));
        assert_eq!(parse_percent("0.5"), Ok(0.005));
        assert!(parse_percent("150%").is_err());
    }

    #[test]
    fn http_takes_the_method_first() {
        let cli = parse(&["http", "PURGE", "https://cdn.test/x"]).expect("valid");
        let ConsoleCommand::Http(http) = cli.command else {
            panic!("not http");
        };
        assert_eq!(http.method, "PURGE");
        assert_eq!(http.request.url, "https://cdn.test/x");
    }

    #[test]
    fn every_subcommand_name_is_a_command() {
        for name in SUBCOMMANDS {
            let args: &[&str] = match name {
                "run" | "check" | "load" => &[],
                "http" => &["GET", "u"],
                _ => &["u"],
            };
            let mut all = vec![name];
            all.extend_from_slice(args);
            assert!(parse(&all).is_ok(), "{name}");
        }
    }
}
