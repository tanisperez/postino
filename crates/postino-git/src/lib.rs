//! Drives the `git` executable installed on the machine: detection, repository discovery,
//! status, staging, discarding changes, diffs, applying patches, committing, and pushing, pulling
//! and fetching.
//!
//! Everything is synchronous and blocking, the app calls it from a background executor. Git is
//! always run as a child process (never a library) with a fixed environment: English messages,
//! no terminal prompts, no optional locks and unquoted paths. Paths in the results are relative to
//! the repository root and separated by `/` on every platform.
#![warn(missing_docs)]

mod commit;
mod detect;
mod diff;
mod error;
mod file_diff;
mod repo;
mod run;
mod status;
mod sync;

pub use commit::is_secret_file;
pub use detect::{Executable, MIN_VERSION, Version, detect};
pub use diff::{ApplyTarget, DiffSource, FileVersion, NumStat, hunk_patch};
pub use error::{DetectError, FailureKind, GitError};
pub use file_diff::{
    ChangeBlock, DiffLine, FileDiff, LineKind, SplitRow, block_patch, parse_file_diff,
};
pub use repo::{Author, Git, Repo};
pub use status::{Change, Entry, Head, Status};
pub use sync::{CommitSummary, PullMode, PushTarget};
