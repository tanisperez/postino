//! Parsing and serialization of the `.postino` request file format and of `.env` environment
//! files. Pure, no filesystem or network access: every function takes and returns plain strings
//! or `postino-core` types, and the caller (`postino-workspace`) does the actual file IO.
//!
//! See `plans/mvp.md`, section 3, for the full format specification.
//!
//! The `postman` module (Postman collection import, section 6 phase 7) maps Postman collection
//! and environment exports into the same `postino-core` types, pure JSON in, plain data out.
#![warn(missing_docs)]

mod format;

pub mod env;
pub mod postman;

pub use format::{ParseError, ParseErrorKind, parse, serialize};
