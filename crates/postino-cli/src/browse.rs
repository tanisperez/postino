//! The commands that read the workspace and send nothing: `postino list` (the request tree),
//! `postino env` (the environments and their variables), `postino snippet` (a request as code)
//! and `postino completions` (the shell completion scripts).

use std::io::Write;
use std::path::Path;

use clap::CommandFactory;
use postino_format::snippet::{SnippetLanguage, render_snippet};
use postino_runner::SessionEnv;
use postino_workspace::{Node, Workspace};
use serde_json::json;

use crate::args::{
    Cli, CompletionShell, CompletionsArgs, EnvArgs, ListArgs, OutputFormat, SnippetArgs,
    SnippetLang,
};
use crate::context::{self, WorkspaceLookup};
use crate::exit::{self, CliError};
use crate::style::Style;

/// What a hidden value prints.
const HIDDEN: &str = "••••••";

/// Words in a variable name that mark its value as secret.
const SECRET_WORDS: [&str; 8] = [
    "secret",
    "password",
    "passwd",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "credential",
];

/// Whether the value of `name` is hidden: it lives in a `.local.env` file, which is kept out of
/// git for secrets, or its name says it is one.
pub fn is_secret(name: &str, local: bool) -> bool {
    let lower = name.to_ascii_lowercase();
    local || SECRET_WORDS.iter().any(|word| lower.contains(word))
}

/// Prints `result` to stdout, or its error to stderr, and returns the exit code.
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

fn open(args: &crate::args::WorkspaceArgs) -> Result<Workspace, CliError> {
    context::open_workspace(
        &args.common(),
        WorkspaceLookup::OrFolder(context::current_dir()),
    )?
    .ok_or_else(|| CliError::file("no workspace"))
}

/// The nodes of the folder `id`, or of the whole workspace for `""`.
fn nodes_of<'a>(nodes: &'a [Node], id: &str) -> Option<&'a [Node]> {
    if id.is_empty() {
        return Some(nodes);
    }
    nodes.iter().find_map(|node| match node {
        Node::Folder(folder) if folder.id == id => Some(folder.children.as_slice()),
        Node::Folder(folder) if id.starts_with(&format!("{}/", folder.id)) => {
            nodes_of(&folder.children, id)
        }
        _ => None,
    })
}

/// The tree under `nodes`, one line per folder and request.
fn tree_text(nodes: &[Node], depth: usize, style: Style, out: &mut String) {
    let indent = "  ".repeat(depth);
    for node in nodes {
        match node {
            Node::Folder(folder) => {
                out.push_str(&format!(
                    " {indent}{}\n",
                    style.bold(&format!("{}/", folder.name))
                ));
                tree_text(&folder.children, depth + 1, style, out);
            }
            Node::Request(request) => match (&request.broken, &request.method) {
                (Some(_), _) => out.push_str(&format!(
                    " {indent}{} {}  {}\n",
                    style.red(&format!("{:<7}", "?")),
                    request.name,
                    style.red("cannot be read")
                )),
                (None, method) => {
                    let method = method.as_ref().map(ToString::to_string).unwrap_or_default();
                    out.push_str(&format!(
                        " {indent}{} {}\n",
                        style.dim(&format!("{method:<7}")),
                        request.name
                    ));
                }
            },
        }
    }
}

/// One JSON line per request under `nodes`.
fn tree_json(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Folder(folder) => tree_json(&folder.children, out),
            Node::Request(request) => {
                out.push_str(
                    &json!({
                        "request": request.id,
                        "name": request.name,
                        "method": request.method.as_ref().map(ToString::to_string),
                        "broken": request.broken,
                    })
                    .to_string(),
                );
                out.push('\n');
            }
        }
    }
}

/// Runs `postino list` and returns its exit code.
pub fn list_command(args: &ListArgs) -> u8 {
    let style = Style::stdout(args.workspace.no_color);
    let result = (|| -> Result<String, CliError> {
        let workspace = open(&args.workspace)?;
        let id = match &args.folder {
            Some(folder) => crate::run::id_of(workspace.root(), folder)?,
            None => String::new(),
        };
        let nodes = nodes_of(workspace.tree(), &id)
            .ok_or_else(|| CliError::file(format!("{id}: not a folder of the workspace")))?;
        let mut out = String::new();
        match args.output {
            OutputFormat::Json => tree_json(nodes, &mut out),
            _ => tree_text(nodes, 0, style, &mut out),
        }
        Ok(out)
    })();
    print_or_fail(result, args.workspace.no_color)
}

/// A variable of an environment as `env --show` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownVariable {
    /// The name.
    pub key: String,
    /// The value, `None` when it is hidden.
    pub value: Option<String>,
    /// Whether it comes from the `.local.env` file.
    pub local: bool,
}

/// The variables of the environment `name`, the local file over the base one, secrets hidden.
pub fn shown_variables(workspace: &Workspace, name: &str) -> Result<Vec<ShownVariable>, CliError> {
    let layers = workspace
        .load_environment_layers(name)
        .map_err(|error| CliError::file(error.to_string()))?;
    if layers.base.is_none() && layers.local.is_none() {
        return Err(CliError::file(format!("unknown environment {name:?}")));
    }
    let mut shown: Vec<ShownVariable> = Vec::new();
    for (variables, local) in [(layers.base, false), (layers.local, true)] {
        for variable in variables.unwrap_or_default() {
            let value = (!is_secret(&variable.key, local)).then(|| variable.value.clone());
            match shown.iter_mut().find(|known| known.key == variable.key) {
                Some(known) => {
                    known.value = value;
                    known.local = local;
                }
                None => shown.push(ShownVariable {
                    key: variable.key,
                    value,
                    local,
                }),
            }
        }
    }
    Ok(shown)
}

/// Runs `postino env` and returns its exit code.
pub fn env_command(args: &EnvArgs) -> u8 {
    let style = Style::stdout(args.workspace.no_color);
    let result = (|| -> Result<String, CliError> {
        let workspace = open(&args.workspace)?;
        let json = args.output == OutputFormat::Json;
        let mut out = String::new();
        match &args.show {
            Some(name) => {
                let variables = shown_variables(&workspace, name)?;
                let width = variables
                    .iter()
                    .map(|v| v.key.chars().count())
                    .max()
                    .unwrap_or(0);
                for variable in &variables {
                    if json {
                        out.push_str(
                            &json!({
                                "key": variable.key,
                                "value": variable.value,
                                "hidden": variable.value.is_none(),
                                "local": variable.local,
                            })
                            .to_string(),
                        );
                        out.push('\n');
                    } else {
                        let value = match &variable.value {
                            Some(value) => value.clone(),
                            None => style.dim(HIDDEN),
                        };
                        let local = if variable.local {
                            style.dim("  (.local.env)")
                        } else {
                            String::new()
                        };
                        out.push_str(&format!(" {:<width$}  {value}{local}\n", variable.key));
                    }
                }
                if variables.is_empty() && !json {
                    out.push_str(&format!(" The environment {name} has no variables.\n"));
                }
            }
            None => {
                let names = workspace
                    .list_environments()
                    .map_err(|error| CliError::file(error.to_string()))?;
                let width = names
                    .iter()
                    .map(|name| name.chars().count())
                    .max()
                    .unwrap_or(0);
                for name in &names {
                    let count = shown_variables(&workspace, name).map_or(0, |v| v.len());
                    if json {
                        out.push_str(&json!({"environment": name, "variables": count}).to_string());
                        out.push('\n');
                    } else {
                        let noun = if count == 1 { "variable" } else { "variables" };
                        out.push_str(&format!(
                            " {name:<width$}  {}\n",
                            style.dim(&format!("{count} {noun}"))
                        ));
                    }
                }
                if names.is_empty() && !json {
                    out.push_str(" No environments. They are .env files in environments/.\n");
                }
            }
        }
        Ok(out)
    })();
    print_or_fail(result, args.workspace.no_color)
}

/// Runs `postino snippet` and returns its exit code.
pub fn snippet_command(args: &SnippetArgs) -> u8 {
    let result = (|| -> Result<String, CliError> {
        let file = context::absolute(&args.file);
        if !file.is_file() {
            return Err(CliError::file(format!(
                "{}: no such file",
                args.file.display()
            )));
        }
        let start = file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(context::current_dir);
        let workspace = context::open_workspace(&args.common, WorkspaceLookup::OrFolder(start))?
            .ok_or_else(|| CliError::file("no workspace"))?;
        let id = crate::run::id_of(workspace.root(), &file)?;
        let request = workspace
            .load_request(&id)
            .map_err(|error| CliError::file(format!("{}: {error}", args.file.display())))?;
        let environment = context::environment(&args.common, Some(&workspace))?;
        // The console keeps no token between runs: an OAuth 2.0 auth shows a placeholder (#269).
        let request = context::oauth_config(Some(&workspace))?
            .with_snippet_token(&request, &environment.name);
        let preview = postino_runner::preview(&request, &environment, &SessionEnv::new());
        for unknown in &preview.unknown_variables {
            eprintln!(
                "{}",
                Style::stderr(args.common.no_color)
                    .yellow(&format!("! unknown variable {{{{{}}}}}", unknown.name))
            );
        }
        let language = match args.lang {
            SnippetLang::Curl => SnippetLanguage::Curl,
            SnippetLang::Fetch => SnippetLanguage::JsFetch,
            SnippetLang::Python => SnippetLanguage::PythonRequests,
            SnippetLang::Postino => SnippetLanguage::PostinoCli,
        };
        let mut text = render_snippet(&preview.resolved, language);
        if !text.ends_with('\n') {
            text.push('\n');
        }
        Ok(text)
    })();
    print_or_fail(result, args.common.no_color)
}

/// Runs `postino completions` and returns its exit code.
pub fn completions_command(args: &CompletionsArgs) -> u8 {
    use clap_complete::Shell;
    let shell = match args.shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Fish => Shell::Fish,
        CompletionShell::Powershell => Shell::PowerShell,
    };
    clap_complete::generate(
        shell,
        &mut Cli::command(),
        "postino",
        &mut std::io::stdout(),
    );
    exit::SUCCESS
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        for folder in [".postino", "auth", "environments"] {
            std::fs::create_dir_all(root.join(folder)).expect("folder");
        }
        std::fs::write(root.join("auth/login.postino"), "POST {{base}}/login\n").expect("login");
        std::fs::write(root.join("auth/bad.postino"), "post nope\n").expect("bad");
        std::fs::write(root.join("health.postino"), "GET {{base}}/health\n").expect("health");
        std::fs::write(
            root.join("environments/dev.env"),
            "base=http://dev\napiToken=abc\nuser=ada\n",
        )
        .expect("env");
        std::fs::write(root.join("environments/dev.local.env"), "user=bob\n").expect("local");
        let workspace = Workspace::open(root).expect("workspace");
        (dir, workspace)
    }

    #[test]
    fn the_tree_lists_folders_requests_and_broken_files() {
        let (_dir, workspace) = workspace();
        let mut out = String::new();
        tree_text(workspace.tree(), 0, Style::PLAIN, &mut out);
        assert_eq!(
            out,
            " auth/\n   ?       bad  cannot be read\n   POST    login\n GET     health\n"
        );
        let auth = nodes_of(workspace.tree(), "auth").expect("auth");
        assert_eq!(auth.len(), 2);
        assert!(nodes_of(workspace.tree(), "nope").is_none());
    }

    #[test]
    fn secrets_and_local_values_are_hidden() {
        let (_dir, workspace) = workspace();
        let shown = shown_variables(&workspace, "dev").expect("dev");
        assert_eq!(
            shown,
            [
                ShownVariable {
                    key: "base".into(),
                    value: Some("http://dev".into()),
                    local: false
                },
                ShownVariable {
                    key: "apiToken".into(),
                    value: None,
                    local: false
                },
                ShownVariable {
                    key: "user".into(),
                    value: None,
                    local: true
                },
            ]
        );
        assert_eq!(
            shown_variables(&workspace, "prod")
                .expect_err("unknown")
                .code,
            exit::FILE
        );
    }

    #[test]
    fn completions_cover_every_command() {
        let mut script = Vec::new();
        clap_complete::generate(
            clap_complete::Shell::Bash,
            &mut Cli::command(),
            "postino",
            &mut script,
        );
        let script = String::from_utf8(script).expect("utf-8");
        for name in crate::args::SUBCOMMANDS {
            assert!(script.contains(name), "{name}");
        }
    }
}
