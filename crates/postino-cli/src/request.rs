//! One-off requests, like curl or wget: `postino get|post|put|patch|delete|head|options <url>`
//! and `postino http <METHOD> <url>`. The response body goes to stdout untouched, everything
//! else to stderr. The request goes through the same pipeline as the app, so `{{variables}}`
//! resolve with `--env` and `--var`.

use std::io::{Read, Write};
use std::path::Path;

use postino_core::{Auth, Body, KeyValue, Method, Request, log_safe};
use postino_runner::RunResult;

use crate::args::RequestArgs;
use crate::context::{self, WorkspaceLookup};
use crate::exit::CliError;
use crate::outcome::{format_duration, format_size, outcome};
use crate::progress::Progress;
use crate::style::Style;

/// The request the arguments describe, before interpolation.
pub fn build_request(method: Method, args: &RequestArgs) -> Result<Request, CliError> {
    let mut headers = Vec::new();
    for header in &args.headers {
        let (name, value) = header
            .split_once(':')
            .ok_or_else(|| CliError::usage(format!("-H {header:?}: expected 'Name: Value'")))?;
        if name.trim().is_empty() {
            return Err(CliError::usage(format!("-H {header:?}: the name is empty")));
        }
        headers.push(KeyValue::new(name.trim(), value.trim()));
    }
    let query = args
        .query
        .iter()
        .map(|(key, value)| KeyValue::new(key.clone(), value.clone()))
        .collect();
    let body = if let Some(json) = &args.json {
        Body::Json(json.clone())
    } else if let Some(data) = &args.data {
        Body::Text(read_data(data)?)
    } else if !args.form.is_empty() {
        Body::Multipart(
            args.form
                .iter()
                .map(|(field, value)| KeyValue::new(field.clone(), multipart_value(value)))
                .collect(),
        )
    } else {
        Body::None
    };
    let auth = if let Some(user) = &args.user {
        let (username, password) = user.split_once(':').unwrap_or((user, ""));
        Auth::Basic {
            username: username.to_string(),
            password: password.to_string(),
        }
    } else if let Some(token) = &args.bearer {
        Auth::Bearer {
            token: token.clone(),
        }
    } else {
        Auth::None
    };
    Ok(Request {
        method,
        url: args.url.clone(),
        headers,
        query,
        auth,
        body,
        ..Request::default()
    })
}

/// The text of `-d`: itself, a file's text with `@file`, or stdin with `@-`.
fn read_data(data: &str) -> Result<String, CliError> {
    match data.strip_prefix('@') {
        Some("-") => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|error| CliError::file(format!("cannot read stdin: {error}")))?;
            Ok(text)
        }
        Some(path) => std::fs::read_to_string(path)
            .map_err(|error| CliError::file(format!("-d @{path}: {error}"))),
        None => Ok(data.to_string()),
    }
}

/// A `-F` value with its file made absolute against the current directory, since the runner
/// reads relative files from the workspace.
fn multipart_value(value: &str) -> String {
    match value.strip_prefix('@') {
        Some(path) if !path.starts_with('@') => {
            format!("@{}", context::absolute(Path::new(path)).display())
        }
        _ => value.to_string(),
    }
}

/// Writes `request` to `path` as a `.postino` file, refusing to replace one.
fn save(request: &Request, path: &Path) -> Result<(), CliError> {
    if path.exists() {
        return Err(CliError::file(format!(
            "--save {}: the file already exists",
            path.display()
        )));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| CliError::file(format!("--save {}: {error}", path.display())))?;
    }
    std::fs::write(path, postino_format::serialize(request))
        .map_err(|error| CliError::file(format!("--save {}: {error}", path.display())))
}

/// What was sent, for `-v`: the request line and the headers, secrets masked.
fn sent_lines(result: &RunResult) -> String {
    let Some(request) = &result.resolved_request else {
        return String::new();
    };
    let mut out = format!(
        "> {} {}\n",
        request.method,
        log_safe::url_for_log(&request.url)
    );
    for header in &request.headers {
        out.push_str(&format!(
            "> {}: {}\n",
            header.name,
            log_safe::header_value_for_log(&header.name, &header.value)
        ));
    }
    out.push_str(">\n");
    out
}

/// The status line and headers of `-i`.
fn head_lines(result: &RunResult) -> String {
    let Some(response) = &result.response else {
        return String::new();
    };
    let mut out = format!("HTTP {}\n", response.status);
    for header in &response.headers {
        out.push_str(&format!("{}: {}\n", header.name, header.value));
    }
    out.push('\n');
    out
}

/// Runs a one-off request with `method` and returns the exit code.
pub fn command(method: Method, args: &RequestArgs) -> u8 {
    let err_style = Style::stderr(args.common.no_color);
    match execute(method, args, err_style) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{} {error}", err_style.red("error:"));
            error.code
        }
    }
}

fn execute(method: Method, args: &RequestArgs, style: Style) -> Result<u8, CliError> {
    let request = build_request(method, args)?;
    let workspace = context::open_workspace(
        &args.common,
        WorkspaceLookup::Optional(context::current_dir()),
    )?;
    let environment = context::environment(&args.common, workspace.as_ref())?;
    let mut options = context::send_options(&args.send)?;
    if args.output.is_some() && args.send.max_size.is_none() {
        // A download goes to a file, not to memory: no limit unless one is asked for.
        options.max_response_size = u64::MAX;
    }
    let runner = context::runner(workspace.as_ref(), options)?;
    if let Some(path) = &args.save {
        save(&request, path)?;
        eprintln!("Saved {}", path.display());
    }

    let mut session = postino_runner::SessionEnv::new();
    let result = match &args.output {
        Some(path) => {
            let mut file = std::fs::File::create(path)
                .map_err(|error| CliError::file(format!("-o {}: {error}", path.display())))?;
            let client = runner.client();
            let mut progress = Progress::new();
            let result = runner.run_sending(&request, &environment, &mut session, |resolved| {
                client.download(resolved, &mut file, &mut |done, total| {
                    progress.update(done, total)
                })
            });
            progress.finish();
            result
        }
        None => runner.run(&request, &environment, &mut session),
    };

    if args.verbose {
        eprint!("{}", sent_lines(&result));
    }
    let verdict = outcome(&result);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if let Some(response) = &result.response {
        let write = |out: &mut dyn Write, bytes: &[u8]| {
            out.write_all(bytes)
                .map_err(|error| CliError::file(format!("cannot write: {error}")))
        };
        if args.include {
            write(&mut out, head_lines(&result).as_bytes())?;
        }
        if args.output.is_none() {
            write(&mut out, &response.body)?;
        }
        let _ = out.flush();
        let mut details = format!(
            " · {} · {}",
            format_duration(response.time),
            format_size(response.size as u64)
        );
        if let Some(path) = &args.output {
            details.push_str(&format!(" · saved to {}", path.display()));
        }
        // The body may not end in a newline: the summary starts on its own line on a terminal.
        let ends_clean = response.body.is_empty() || response.body.ends_with(b"\n");
        let on_terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
        let newline = if args.output.is_none() && !ends_clean && on_terminal {
            "\n"
        } else {
            ""
        };
        eprintln!(
            "{newline}{}{}",
            style.status(response.status),
            style.dim(&details)
        );
        if let Some(warning) = &response.tls_warning {
            eprintln!("{}", style.yellow(&format!("! {warning}")));
        }
    } else if let Some(path) = &args.output {
        // Nothing came back: do not leave an empty file behind.
        let _ = std::fs::remove_file(path);
    }
    for warning in &result.warnings {
        eprintln!("{}", style.yellow(&format!("! {warning}")));
    }
    if let Some(error) = &verdict.error {
        eprintln!("{} {error}", style.red("error:"));
    }
    Ok(verdict.code)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::exit;
    use pretty_assertions::assert_eq;

    fn args(url: &str) -> RequestArgs {
        RequestArgs {
            url: url.to_string(),
            ..RequestArgs::default()
        }
    }

    #[test]
    fn headers_query_and_auth_become_the_request() {
        let request = build_request(
            Method::Get,
            &RequestArgs {
                headers: vec!["X-Trace: 1".into(), "Accept:application/json".into()],
                query: vec![("page".into(), "2".into())],
                user: Some("ada:s3cret".into()),
                ..args("{{base}}/users")
            },
        )
        .expect("request");
        assert_eq!(request.url, "{{base}}/users");
        assert_eq!(
            request.headers,
            [
                KeyValue::new("X-Trace", "1"),
                KeyValue::new("Accept", "application/json")
            ]
        );
        assert_eq!(request.query, [KeyValue::new("page", "2")]);
        assert_eq!(
            request.auth,
            Auth::Basic {
                username: "ada".into(),
                password: "s3cret".into()
            }
        );
    }

    #[test]
    fn a_header_needs_a_colon() {
        let error = build_request(
            Method::Get,
            &RequestArgs {
                headers: vec!["X-Trace 1".into()],
                ..args("u")
            },
        )
        .expect_err("bad header");
        assert_eq!(error.code, exit::USAGE);
    }

    #[test]
    fn bodies_come_from_json_data_or_form() {
        let json = build_request(
            Method::Post,
            &RequestArgs {
                json: Some("{\"a\":1}".into()),
                ..args("u")
            },
        )
        .expect("json");
        assert_eq!(json.body, Body::Json("{\"a\":1}".into()));

        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("body.txt");
        std::fs::write(&file, "from a file").expect("write");
        let data = build_request(
            Method::Post,
            &RequestArgs {
                data: Some(format!("@{}", file.display())),
                ..args("u")
            },
        )
        .expect("data");
        assert_eq!(data.body, Body::Text("from a file".into()));

        let form = build_request(
            Method::Post,
            &RequestArgs {
                form: vec![
                    ("name".into(), "ada".into()),
                    ("photo".into(), "@photo.png".into()),
                    ("at".into(), "@@home".into()),
                ],
                ..args("u")
            },
        )
        .expect("form");
        let Body::Multipart(fields) = form.body else {
            panic!("not multipart");
        };
        assert_eq!(fields[0].value, "ada");
        assert!(fields[1].value.starts_with('@'));
        assert!(Path::new(&fields[1].value[1..]).is_absolute());
        assert_eq!(fields[2].value, "@@home");
    }

    #[test]
    fn save_writes_a_request_file_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("api/login.postino");
        let request = build_request(Method::Post, &args("{{base}}/login")).expect("request");
        save(&request, &path).expect("saved");
        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(postino_format::parse(&text).expect("parse"), request);
        assert_eq!(save(&request, &path).expect_err("exists").code, exit::FILE);
    }
}
