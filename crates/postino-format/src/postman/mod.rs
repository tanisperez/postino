//! Importing Postman collections (v2.1, v2.0 accepted when it parses the same) and Postman
//! environment exports, `plans/mvp.md` section 6, phase 7.
//!
//! This module is pure, like the rest of `postino-format`: it only turns JSON text into plain
//! in-memory data ([`ImportPlan`], [`ImportedEnvironment`]), it never touches the filesystem.
//! `postino-workspace` is the crate that writes an import plan into a workspace.

mod collection;
mod environment;
mod error;
mod model;
mod value;

pub use collection::{ImportFolder, ImportPlan, parse_collection};
pub use environment::{ImportedEnvironment, parse_environment};
pub use error::PostmanError;
