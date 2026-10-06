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
#![warn(missing_docs)]

mod format;

pub mod env;
pub mod postman;
pub mod snippet;

pub use format::{ParseError, ParseErrorKind, parse, serialize};
