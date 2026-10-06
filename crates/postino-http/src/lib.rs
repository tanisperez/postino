//! Sending a resolved request over HTTP with `ureq`, measuring timing.
//!
//! The single entry point is [`send`]: it takes a [`postino_core::ResolvedRequest`] (already
//! interpolated, nothing left to resolve) and a [`SendOptions`], and returns a
//! [`postino_core::Response`]. HTTP responses with a 4xx or 5xx status are normal responses,
//! not errors: [`HttpError`] only covers failures that prevent us from getting a response at
//! all, such as a bad URL, a timeout or a connection failure.
#![warn(missing_docs)]

mod error;
mod options;
mod send;

#[cfg(feature = "test-support")]
pub mod test_support;

pub use error::HttpError;
pub use options::{DEFAULT_MAX_RESPONSE_SIZE, InvalidCertificates, SendOptions};
pub use send::send;
