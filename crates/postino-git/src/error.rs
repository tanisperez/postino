//! The error types of this crate.

use std::io;
use std::path::PathBuf;

use crate::Version;

/// Why no usable `git` executable could be found. See [`crate::detect`].
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    /// There is no `git` executable on the `PATH` (or at the configured path).
    #[error("git was not found")]
    NotFound,

    /// macOS only: `/usr/bin/git` is a shim that offers to install the Xcode Command Line Tools,
    /// and they are not installed. The shim is never run, since it pops up the installer.
    #[error("git needs the Xcode Command Line Tools, which are not installed")]
    XcodeToolsMissing,

    /// A `git` was found but it is older than [`crate::MIN_VERSION`].
    #[error("git {version} at {path:?} is too old (2.30.0 or newer is required)")]
    TooOld {
        /// The executable that was found.
        path: PathBuf,
        /// Its version.
        version: Version,
    },

    /// A `git` was found but `git --version` failed or printed something unexpected.
    #[error("could not run git: {0}")]
    Failed(String),
}

/// Everything that can go wrong while running a git command.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The process could not be started, or a file operation around it failed.
    #[error("could not run git: {0}")]
    Spawn(#[from] io::Error),

    /// The command ran and exited with a failure status.
    #[error("{command} failed{}: {stderr}", code.map(|c| format!(" with code {c}")).unwrap_or_default())]
    Failed {
        /// The command line, for example `git add -A -- a.txt`.
        command: String,
        /// The exit code, `None` when the process was killed by a signal.
        code: Option<i32>,
        /// What git printed on its standard error, trimmed.
        stderr: String,
    },

    /// The output of git could not be understood.
    #[error("unexpected git output: {0}")]
    Parse(String),
}
