//! What every console command sets up before it sends anything: the workspace, the variables
//! and the [`Runner`], with the same pipeline as the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use postino_core::{Environment, KeyValue};
use postino_http::{InvalidCertificates, SendOptions};
use postino_oauth::MemoryStore;
use postino_runner::{OAuthConfig, Runner};
use postino_script::QuickJsEngine;
use postino_workspace::{Workspace, WorkspaceError};

use crate::args::{CommonArgs, SendArgs};
use crate::exit::CliError;
use crate::oauth::ConsoleSignIn;

/// The folder that marks a workspace root.
const MARKER: &str = ".postino";

/// The nearest folder at or above `start` holding a `.postino/` folder.
pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(MARKER).is_dir())
        .map(Path::to_path_buf)
}

/// The current directory, or `.` when it cannot be read.
pub fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// `path` made absolute against the current directory, without resolving links.
pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| current_dir().join(path))
}

/// Which workspace a command uses when `--workspace` is not given.
#[derive(Debug, Clone)]
pub enum WorkspaceLookup {
    /// The one holding `.postino/` above this folder, or no workspace at all.
    Optional(PathBuf),
    /// The one holding `.postino/` above this folder, or this folder itself.
    OrFolder(PathBuf),
}

/// Opens the workspace a command uses: `--workspace`, or the lookup.
pub fn open_workspace(
    common: &CommonArgs,
    lookup: WorkspaceLookup,
) -> Result<Option<Workspace>, CliError> {
    let root = match (&common.workspace, lookup) {
        (Some(dir), _) => Some(absolute(dir)),
        (None, WorkspaceLookup::Optional(start)) => find_workspace_root(&start),
        (None, WorkspaceLookup::OrFolder(start)) => {
            Some(find_workspace_root(&start).unwrap_or(start))
        }
    };
    let Some(root) = root else {
        return Ok(None);
    };
    Workspace::open(&root)
        .map(Some)
        .map_err(|error| CliError::file(format!("cannot open the workspace: {error}")))
}

/// The variables of a run: the environment `--env` names (with its `.local.env`), then the
/// `--env-file` over it, then every `--var` over both.
pub fn environment(
    common: &CommonArgs,
    workspace: Option<&Workspace>,
) -> Result<Environment, CliError> {
    let mut environment = match (&common.env, workspace) {
        (None, _) => Environment::default(),
        (Some(name), Some(workspace)) => {
            workspace
                .load_environment(name)
                .map_err(|error| match error {
                    WorkspaceError::EnvironmentNotFound(_) => {
                        let names = workspace.list_environments().unwrap_or_default();
                        let known = if names.is_empty() {
                            "the workspace has none".to_string()
                        } else {
                            format!("available: {}", names.join(", "))
                        };
                        CliError::file(format!("unknown environment {name:?} ({known})"))
                    }
                    other => CliError::file(other.to_string()),
                })?
        }
        (Some(name), None) => {
            return Err(CliError::file(format!(
                "--env {name}: no workspace here, give one with --workspace"
            )));
        }
    };
    if let Some(path) = &common.env_file {
        let text = std::fs::read_to_string(path)
            .map_err(|error| CliError::file(format!("cannot read {}: {error}", path.display())))?;
        let variables = postino_format::env::parse(&text)
            .map_err(|error| CliError::file(format!("{}: {error}", path.display())))?;
        merge(&mut environment.variables, variables);
    }
    let vars = common
        .vars
        .iter()
        .map(|(key, value)| KeyValue::new(key.clone(), value.clone()))
        .collect();
    merge(&mut environment.variables, vars);
    Ok(environment)
}

/// Puts `over` on top of `base`: a key already there takes the new value in place, a new one is
/// added at the end.
fn merge(base: &mut Vec<KeyValue>, over: Vec<KeyValue>) {
    for variable in over {
        match base.iter_mut().find(|entry| entry.key == variable.key) {
            Some(entry) => {
                entry.value = variable.value;
                entry.enabled = variable.enabled;
            }
            None => base.push(variable),
        }
    }
}

/// The send options of the command line: the app's defaults, changed by the flags.
pub fn send_options(send: &SendArgs) -> Result<SendOptions, CliError> {
    if let Some(proxy) = &send.proxy {
        postino_http::check_proxy(proxy)
            .map_err(|error| CliError::usage(format!("--proxy {proxy}: {error}")))?;
    }
    let defaults = SendOptions::default();
    Ok(SendOptions {
        timeout: send.timeout.unwrap_or(defaults.timeout),
        max_response_size: send.max_size.unwrap_or(defaults.max_response_size),
        invalid_certificates: if send.insecure {
            InvalidCertificates::Accept
        } else {
            InvalidCertificates::SendWithWarning
        },
        proxy: send.proxy.clone(),
        ..defaults
    })
}

/// The runner of the app's pipeline: QuickJS scripts, multipart paths from the workspace (or the
/// current directory), the workspace's OAuth profiles with tokens kept in memory for this run, and
/// the browser sign-in of the console.
pub fn runner(workspace: Option<&Workspace>, options: SendOptions) -> Result<Runner, CliError> {
    let base_dir = workspace.map_or_else(current_dir, |workspace| workspace.root().to_path_buf());
    let oauth = match workspace {
        Some(workspace) => {
            let profiles = workspace
                .load_project()
                .map_err(|error| CliError::file(error.to_string()))?
                .oauth_profiles();
            OAuthConfig {
                workspace: postino_oauth::workspace_id(workspace.root()),
                profiles: Arc::new(profiles),
                store: Arc::new(MemoryStore::default()),
                sign_in: Arc::new(ConsoleSignIn),
            }
        }
        None => OAuthConfig::default(),
    };
    Ok(Runner::new(Arc::new(QuickJsEngine), options)
        .with_base_dir(Some(base_dir))
        .with_oauth(oauth))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn workspace_with_env() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join(".postino")).expect("marker");
        std::fs::create_dir_all(dir.path().join("environments")).expect("envs");
        std::fs::write(
            dir.path().join("environments/dev.env"),
            "base=http://dev\nuser=ada\n",
        )
        .expect("env");
        std::fs::write(dir.path().join("environments/dev.local.env"), "user=bob\n")
            .expect("local env");
        let workspace = Workspace::open(dir.path()).expect("workspace");
        (dir, workspace)
    }

    #[test]
    fn the_workspace_is_the_nearest_folder_with_a_marker() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("a/b/c");
        std::fs::create_dir_all(&nested).expect("folders");
        assert_eq!(find_workspace_root(&nested), None);
        std::fs::create_dir_all(dir.path().join("a/.postino")).expect("marker");
        assert_eq!(find_workspace_root(&nested), Some(dir.path().join("a")));
    }

    #[test]
    fn variables_stack_environment_then_file_then_vars() {
        let (dir, workspace) = workspace_with_env();
        let env_file = dir.path().join("ci.env");
        std::fs::write(&env_file, "base=http://ci\ntoken=t1\n").expect("env file");
        let common = CommonArgs {
            env: Some("dev".to_string()),
            env_file: Some(env_file),
            vars: vec![("token".to_string(), "t2".to_string())],
            ..CommonArgs::default()
        };
        let environment = environment(&common, Some(&workspace)).expect("environment");
        assert_eq!(environment.name, "dev");
        let pairs: Vec<(&str, &str)> = environment
            .variables
            .iter()
            .map(|kv| (kv.key.as_str(), kv.value.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [("base", "http://ci"), ("user", "bob"), ("token", "t2")]
        );
    }

    #[test]
    fn an_unknown_environment_is_a_file_error_naming_the_known_ones() {
        let (_dir, workspace) = workspace_with_env();
        let common = CommonArgs {
            env: Some("prod".to_string()),
            ..CommonArgs::default()
        };
        let error = environment(&common, Some(&workspace)).expect_err("unknown");
        assert_eq!(error.code, crate::exit::FILE);
        assert!(
            error.message.contains("available: dev"),
            "{}",
            error.message
        );
    }

    #[test]
    fn an_environment_needs_a_workspace() {
        let common = CommonArgs {
            env: Some("dev".to_string()),
            ..CommonArgs::default()
        };
        assert_eq!(
            environment(&common, None).expect_err("no workspace").code,
            crate::exit::FILE
        );
    }

    #[test]
    fn send_flags_change_the_defaults() {
        let options = send_options(&SendArgs {
            insecure: true,
            max_size: Some(1000),
            ..SendArgs::default()
        })
        .expect("options");
        assert_eq!(options.invalid_certificates, InvalidCertificates::Accept);
        assert_eq!(options.max_response_size, 1000);
        assert_eq!(options.timeout, SendOptions::default().timeout);
        let bad = send_options(&SendArgs {
            proxy: Some("ftp://x".to_string()),
            ..SendArgs::default()
        });
        assert_eq!(bad.expect_err("bad proxy").code, crate::exit::USAGE);
    }
}
