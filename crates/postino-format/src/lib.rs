//! Parsing and serialization of the `.postino` request file format and of `.env` environment
//! files. Pure, no filesystem or network access: every function takes and returns plain strings
//! or `postino-core` types, and the caller (`postino-workspace`) does the actual file IO.
//!
//! See `docs/format.md` for the full format specification.
//!
//! The `postman` module (Postman collection import, see `docs/postman-import.md`) maps Postman collection
//! and environment exports into the same `postino-core` types, pure JSON in, plain data out.
//!
//! The `snippet` module turns a resolved request into copyable code (curl, JavaScript `fetch`,
//! Python `requests`) for the UI's "Code" action.
//!
//! The `semantic_diff` module compares two versions of a `.postino` file part by part (method and
//! URL, headers, query, auth, body, scripts, docs) for the diff tab's "Request" mode.
#![warn(missing_docs)]

mod format;
mod semantic_diff;

pub mod env;
pub mod postman;
pub mod snippet;

pub use format::{ParseError, ParseErrorKind, parse, serialize};
pub use semantic_diff::{
    DiffRow, DiffRowKind, PartDiff, PartSummary, RequestDiff, RequestDiffError, RequestPart,
    RowNote, diff_requests,
};
