//! Drives the `git` executable installed on the machine: detection, repository discovery,
//! status, staging, discarding changes, diffs and applying patches.
//!
//! Everything is synchronous and blocking, the app calls it from a background executor. Git is
//! always run as a child process (never a library) with a fixed environment: English messages,
//! no terminal prompts, no optional locks and unquoted paths. Paths in the results are relative to
//! the repository root and separated by `/` on every platform.
#![warn(missing_docs)]

mod detect;
mod diff;
mod error;
mod file_diff;
mod repo;
mod run;
mod status;

pub use detect::{Executable, MIN_VERSION, Version, detect};
pub use diff::{ApplyTarget, DiffSource, FileVersion, NumStat, hunk_patch};
pub use error::{DetectError, GitError};
pub use file_diff::{
    ChangeBlock, DiffLine, FileDiff, LineKind, SplitRow, block_patch, parse_file_diff,
};
pub use repo::{Author, Git, Repo};
pub use status::{Change, Entry, Head, Status};
