//! Filesystem access to a Postino workspace: scanning a folder into a collection tree, and
//! loading and saving requests and environments.
//!
//! A workspace is a plain folder on disk (`plans/mvp.md`, section 3): subfolders are
//! collections, `.postino` files are requests, and a top-level `environments/` folder holds
//! `.env`/`.local.env` files instead of being shown as a collection. This crate only deals with
//! the filesystem: [`postino_format`] does the actual parsing and serialization of file content,
//! this crate is responsible for finding the files, turning them into a tree of [`Node`]s, and
//! writing them back safely (atomic writes, every id confined to the workspace root).
#![warn(missing_docs)]

mod env_edit;
mod error;
mod git;
mod ids;
mod postman;
mod sanitize;
mod scan;
mod tree;
mod workspace;

pub use env_edit::{EnvChange, EnvLayer, EnvLayers};
pub use error::WorkspaceError;
pub use git::git_branch;
pub use postman::ImportReport;
pub use sanitize::sanitize_file_name;
pub use tree::{Folder, Node, RequestEntry};
pub use workspace::Workspace;

/// The extension (without the leading dot) of a request file, `plans/mvp.md` section 3.
pub(crate) const REQUEST_EXTENSION: &str = "postino";

/// The name of the top-level folder that holds environment files instead of a collection,
/// `plans/mvp.md` section 3.4.
pub(crate) const ENVIRONMENTS_FOLDER: &str = "environments";
