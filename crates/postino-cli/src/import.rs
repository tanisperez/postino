//! `postino import`: OpenAPI, Postman and Bruno through the app's import wizard without its
//! questions (see [`crate::hooks::AppHooks::import`]), and a curl command as a `.postino` request.

use std::io::Read;
use std::path::Path;

use postino_format::curl::parse_curl;

use crate::args::{CurlArgs, ImportArgs, ImportCommand};
use crate::context::{self, WorkspaceLookup};
use crate::exit::{self, CliError};
use crate::hooks::{AppHooks, ImportKind, ImportOutcome, ImportRequest};
use crate::style::Style;

/// Runs `postino import` and returns its exit code.
pub fn command(command: &ImportCommand, hooks: &dyn AppHooks) -> u8 {
    let (result, no_color) = match command {
        ImportCommand::Openapi(args) => (
            collection(ImportKind::OpenApi, args, hooks),
            args.workspace.no_color,
        ),
        ImportCommand::Postman(args) => (
            collection(ImportKind::Postman, args, hooks),
            args.workspace.no_color,
        ),
        ImportCommand::Bruno(args) => (
            collection(ImportKind::Bruno, args, hooks),
            args.workspace.no_color,
        ),
        ImportCommand::Curl(args) => (curl(args), args.no_color),
    };
    match result {
        Ok(()) => exit::SUCCESS,
        Err(error) => {
            eprintln!("{} {error}", Style::stderr(no_color).red("error:"));
            error.code
        }
    }
}

/// The `--into` folder name: one folder at the workspace root.
fn folder_name(into: &str) -> Result<String, CliError> {
    let name = into.trim().trim_end_matches(['/', '\\']);
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(CliError::usage(format!(
            "--into {into}: give one folder name, created at the workspace root"
        )));
    }
    Ok(name.to_string())
}

/// What the import did, in a few lines.
fn outcome_text(outcome: &ImportOutcome, style: Style) -> String {
    // An environment export alone writes no folder, only environments.
    let only_environments =
        outcome.updated.is_none() && outcome.requests == 0 && !outcome.environments.is_empty();
    let mut out = match outcome.updated {
        Some((added, changed, removed)) => format!(
            " {} {}/: {added} added, {changed} changed, {removed} removed\n",
            style.green("Updated"),
            outcome.folder
        ),
        None if only_environments => format!(
            " {} {}: {}\n",
            style.green("Imported"),
            if outcome.environments.len() == 1 {
                "1 environment".to_string()
            } else {
                format!("{} environments", outcome.environments.len())
            },
            outcome.environments.join(", ")
        ),
        None => format!(
            " {} {} into {}/\n",
            style.green("Imported"),
            match outcome.requests {
                1 => "1 request".to_string(),
                count => format!("{count} requests"),
            },
            outcome.folder
        ),
    };
    if !outcome.environments.is_empty() && !only_environments {
        out.push_str(&format!(
            " Environments: {}\n",
            outcome.environments.join(", ")
        ));
    }
    for warning in &outcome.warnings {
        out.push_str(&format!(" {}\n", style.yellow(&format!("! {warning}"))));
    }
    if outcome.review > 0 {
        let what = if outcome.review == 1 {
            "1 request to review".to_string()
        } else {
            format!("{} requests to review", outcome.review)
        };
        match &outcome.report {
            Some(report) => out.push_str(&format!(" {what}, see {}\n", report.display())),
            None => out.push_str(&format!(" {what}\n")),
        }
    }
    out
}

fn collection(kind: ImportKind, args: &ImportArgs, hooks: &dyn AppHooks) -> Result<(), CliError> {
    let into = args.into.as_deref().map(folder_name).transpose()?;
    let is_url = args.source.starts_with("http://") || args.source.starts_with("https://");
    if is_url && kind != ImportKind::OpenApi {
        return Err(CliError::usage(
            "only an OpenAPI spec is imported from a URL",
        ));
    }
    let source = if is_url {
        args.source.clone()
    } else {
        let path = context::absolute(Path::new(&args.source));
        if !path.exists() {
            return Err(CliError::file(format!(
                "{}: no such file or folder",
                args.source
            )));
        }
        path.to_string_lossy().into_owned()
    };
    let workspace = context::open_workspace(
        &args.workspace.common(),
        WorkspaceLookup::OrFolder(context::current_dir()),
    )?
    .ok_or_else(|| CliError::file("no workspace"))?;
    let outcome = hooks
        .import(&ImportRequest {
            kind,
            source: &source,
            root: workspace.root(),
            into: into.as_deref(),
            new_collection: args.new,
        })
        .map_err(CliError::file)?;
    print!(
        "{}",
        outcome_text(&outcome, Style::stdout(args.workspace.no_color))
    );
    Ok(())
}

fn curl(args: &CurlArgs) -> Result<(), CliError> {
    let command = if args.command == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| CliError::file(format!("cannot read stdin: {error}")))?;
        text
    } else {
        args.command.clone()
    };
    let import = parse_curl(&command).map_err(|error| CliError::usage(error.to_string()))?;
    let style = Style::stderr(args.no_color);
    for warning in &import.warnings {
        eprintln!("{}", style.yellow(&format!("! {warning}")));
    }
    match &args.save {
        Some(path) => {
            crate::request::save(&import.request, path)?;
            eprintln!("Saved {}", path.display());
        }
        None => print!("{}", postino_format::serialize(&import.request)),
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn into_is_one_folder_name() {
        assert_eq!(folder_name("legacy/"), Ok("legacy".to_string()));
        assert_eq!(folder_name("api"), Ok("api".to_string()));
        assert_eq!(folder_name("a/b").expect_err("nested").code, exit::USAGE);
        assert_eq!(folder_name("..").expect_err("parent").code, exit::USAGE);
    }

    #[test]
    fn the_outcome_reads_well() {
        let text = outcome_text(
            &ImportOutcome {
                folder: "Shop".into(),
                requests: 12,
                environments: vec!["local".into(), "staging".into()],
                warnings: vec!["no server URL".into()],
                review: 1,
                report: Some(".postino/import-report.md".into()),
                updated: None,
            },
            Style::PLAIN,
        );
        assert_eq!(
            text,
            " Imported 12 requests into Shop/\n Environments: local, staging\n ! no server URL\n 1 request to review, see .postino/import-report.md\n"
        );
        let environment_only = outcome_text(
            &ImportOutcome {
                folder: "Demo".into(),
                environments: vec!["Demo".into()],
                ..ImportOutcome::default()
            },
            Style::PLAIN,
        );
        assert_eq!(environment_only, " Imported 1 environment: Demo\n");
    }
}
