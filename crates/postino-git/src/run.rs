//! The one place that spawns `git`, so every command gets the same environment and logging.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::GitError;

/// Runs `git` with the global options and environment every invocation shares, and returns its
/// standard output. A non zero exit becomes [`GitError::Failed`].
///
/// Messages are forced to English (`LC_MESSAGES=C`) so the few places that look at stderr can
/// rely on them, and git never waits for a terminal prompt.
pub(crate) fn run(git: &Path, cwd: Option<&Path>, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let mut cmd = Command::new(git);
    cmd.args([
        "-c",
        "core.quotepath=false",
        "--no-optional-locks",
        "--literal-pathspecs",
    ])
    .args(args)
    .env("GIT_TERMINAL_PROMPT", "0")
    .env("LC_MESSAGES", "C")
    .env_remove("LC_ALL")
    .env_remove("LANGUAGE")
    .stdin(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW, so no console window flashes for every command.
        cmd.creation_flags(0x0800_0000);
    }

    let command = format!("git {}", args.join(" "));
    let started = Instant::now();
    let output = cmd.output()?;
    log::debug!("{command} ({} ms)", started.elapsed().as_millis());

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(GitError::Failed {
            command,
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}
