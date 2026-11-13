//! `postino check`: reads every `.postino` request, every environment and the project file of
//! the workspace, and reports those that cannot be read. Nothing is sent. For CI.

use std::io::Write;

use postino_workspace::Workspace;
use serde_json::json;

use crate::args::{CheckArgs, OutputFormat};
use crate::context::{self, WorkspaceLookup};
use crate::exit::{self, CliError};
use crate::run::requests;
use crate::style::Style;

/// A file that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The workspace relative path.
    pub file: String,
    /// Why.
    pub error: String,
}

/// What `check` read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Checked {
    /// Requests read.
    pub requests: usize,
    /// Environments read.
    pub environments: usize,
    /// The files that could not be read.
    pub problems: Vec<Problem>,
}

/// Reads every file of `workspace`.
pub fn check(workspace: &Workspace) -> Checked {
    let mut checked = Checked::default();
    for entry in requests(workspace.tree()) {
        checked.requests += 1;
        if let Err(error) = workspace.load_request(&entry.id) {
            checked.problems.push(Problem {
                file: entry.id.clone(),
                error: error.to_string(),
            });
        }
    }
    match workspace.list_environments() {
        Ok(names) => {
            for name in names {
                checked.environments += 1;
                if let Err(error) = workspace.load_environment(&name) {
                    checked.problems.push(Problem {
                        file: format!("environments/{name}.env"),
                        error: error.to_string(),
                    });
                }
            }
        }
        Err(error) => checked.problems.push(Problem {
            file: "environments/".to_string(),
            error: error.to_string(),
        }),
    }
    if let Err(error) = workspace.load_project() {
        checked.problems.push(Problem {
            file: ".postino/project.toml".to_string(),
            error: error.to_string(),
        });
    }
    checked
}

/// The readable report.
fn human(checked: &Checked, style: Style) -> String {
    let mut out = String::new();
    for problem in &checked.problems {
        out.push_str(&format!(
            " {} {}: {}\n",
            style.red("✗"),
            problem.file,
            problem.error
        ));
    }
    let plural = |count: usize, one: &str, many: &str| {
        if count == 1 {
            format!("1 {one}")
        } else {
            format!("{count} {many}")
        }
    };
    let what = format!(
        "{} and {}",
        plural(checked.requests, "request", "requests"),
        plural(checked.environments, "environment", "environments")
    );
    if checked.problems.is_empty() {
        out.push_str(&format!(" {} {what}, all valid\n", style.green("✓")));
    } else {
        let problems = plural(checked.problems.len(), "problem", "problems");
        out.push_str(&format!("\n {what} checked, {}\n", style.red(&problems)));
    }
    out
}

/// The JSON lines report.
fn json_lines(checked: &Checked) -> String {
    let mut out = String::new();
    for problem in &checked.problems {
        out.push_str(
            &json!({"type": "problem", "file": problem.file, "error": problem.error}).to_string(),
        );
        out.push('\n');
    }
    out.push_str(
        &json!({
            "type": "summary",
            "requests": checked.requests,
            "environments": checked.environments,
            "problems": checked.problems.len(),
        })
        .to_string(),
    );
    out.push('\n');
    out
}

/// Runs `postino check` and returns its exit code.
pub fn command(args: &CheckArgs) -> u8 {
    match execute(args) {
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

fn execute(args: &CheckArgs) -> Result<u8, CliError> {
    if args.output == OutputFormat::Junit {
        return Err(CliError::usage("check prints human or json output"));
    }
    let workspace = context::open_workspace(
        &args.common,
        WorkspaceLookup::OrFolder(context::current_dir()),
    )?
    .ok_or_else(|| CliError::file("no workspace"))?;
    let checked = check(&workspace);
    let text = match args.output {
        OutputFormat::Json => json_lines(&checked),
        _ => human(&checked, Style::stdout(args.common.no_color)),
    };
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
    Ok(if checked.problems.is_empty() {
        exit::SUCCESS
    } else {
        exit::FILE
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn broken_files_are_reported_and_the_rest_counted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("auth")).expect("folder");
        std::fs::create_dir_all(root.join("environments")).expect("envs");
        std::fs::write(root.join("auth/login.postino"), "POST https://api/login\n").expect("ok");
        std::fs::write(root.join("auth/bad.postino"), "post lowercase\n").expect("bad");
        std::fs::write(root.join("environments/dev.env"), "base=http://dev\n").expect("env");
        std::fs::write(root.join("environments/broken.env"), "no equals sign\n").expect("env");
        let workspace = Workspace::open(root).expect("workspace");

        let checked = check(&workspace);
        assert_eq!(checked.requests, 2);
        assert_eq!(checked.environments, 2);
        let files: Vec<&str> = checked.problems.iter().map(|p| p.file.as_str()).collect();
        assert_eq!(files, ["auth/bad.postino", "environments/broken.env"]);

        let text = human(&checked, Style::PLAIN);
        assert!(text.contains(" ✗ auth/bad.postino: "), "{text}");
        assert!(text.ends_with("2 requests and 2 environments checked, 2 problems\n"));
        let lines = json_lines(&checked);
        assert_eq!(lines.lines().count(), 3);
    }

    #[test]
    fn a_clean_workspace_says_so() {
        let checked = Checked {
            requests: 1,
            environments: 0,
            problems: Vec::new(),
        };
        assert_eq!(
            human(&checked, Style::PLAIN),
            " ✓ 1 request and 0 environments, all valid\n"
        );
    }
}
