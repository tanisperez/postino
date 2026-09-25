//! Writing a Postman import plan into a [`Workspace`], `plans/mvp.md` section 6, phase 7.
//!
//! `postino_format::postman` only turns Postman JSON into plain in-memory data. This module is
//! the part that actually touches the filesystem: creating the new collection folder, writing
//! every request and environment file, never overwriting anything that already exists.

use std::fs;
use std::path::{Path, PathBuf};

use postino_core::KeyValue;
use postino_format::postman::{ImportFolder, parse_collection, parse_environment};

use crate::error::WorkspaceError;
use crate::ids::path_to_id;
use crate::sanitize::sanitize_file_name;
use crate::workspace::atomic_write;
use crate::{ENVIRONMENTS_FOLDER, REQUEST_EXTENSION, Workspace};

/// The result of a Postman import: every file that was written, and every warning collected
/// while mapping the source JSON (`plans/mvp.md` section 6, phase 7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    /// The workspace ids of the files written by the import (requests and environment files).
    /// The new collection folder itself is not listed here, only the files inside it.
    pub created_files: Vec<String>,
    /// Everything that could not be mapped faithfully, carried over from the parser as is.
    pub warnings: Vec<String>,
}

impl Workspace {
    /// Imports a Postman collection export (`plans/mvp.md` section 6, phase 7).
    ///
    /// Writes a new top-level folder named after the collection (sanitized with
    /// [`crate::sanitize_file_name`], with a ` (2)`, ` (3)`, ... suffix if a folder or file of
    /// that name already exists), mirroring the collection's own folders and requests inside it.
    /// The same sanitizing and de-duplication applies to every folder and request name, and to
    /// the target folder itself. The collection's own `variable` array, if any, is written as an
    /// environment named after the collection. Existing files are never overwritten.
    pub fn import_postman_collection(
        &mut self,
        json: &str,
    ) -> Result<ImportReport, WorkspaceError> {
        let plan = parse_collection(json)?;
        let mut created = Vec::new();

        let target = unique_path(self.root(), &sanitize_file_name(&plan.collection_name));
        fs::create_dir(&target).map_err(|source| WorkspaceError::Io {
            path: target.clone(),
            source,
        })?;
        write_folder(&target, self.root(), &plan.root, &mut created)?;

        write_environment(
            self.root(),
            &plan.collection_name,
            &plan.variables,
            &[],
            &mut created,
        )?;

        self.rescan()?;
        Ok(ImportReport {
            created_files: created,
            warnings: plan.warnings,
        })
    }

    /// Imports a standalone Postman environment export (`*.postman_environment.json`,
    /// `plans/mvp.md` section 6, phase 7).
    ///
    /// Writes `environments/<name>.env` for the plain values and, if there are any,
    /// `environments/<name>.local.env` for the ones of type `"secret"`. `name` is sanitized and
    /// de-duplicated the same way as [`Workspace::import_postman_collection`]. Existing files are
    /// never overwritten.
    pub fn import_postman_environment(
        &mut self,
        json: &str,
    ) -> Result<ImportReport, WorkspaceError> {
        let imported = parse_environment(json)?;
        let mut created = Vec::new();
        write_environment(
            self.root(),
            &imported.name,
            &imported.base,
            &imported.secret,
            &mut created,
        )?;
        self.rescan()?;
        Ok(ImportReport {
            created_files: created,
            warnings: imported.warnings,
        })
    }
}

/// Writes the folders and requests of `folder` inside `dir`, recursively. Every written
/// request's workspace id (relative to `root`) is appended to `created`.
fn write_folder(
    dir: &Path,
    root: &Path,
    folder: &ImportFolder,
    created: &mut Vec<String>,
) -> Result<(), WorkspaceError> {
    for child in &folder.folders {
        let child_path = unique_path(dir, &sanitize_file_name(&child.name));
        fs::create_dir(&child_path).map_err(|source| WorkspaceError::Io {
            path: child_path.clone(),
            source,
        })?;
        write_folder(&child_path, root, child, created)?;
    }
    for (name, request) in &folder.requests {
        let path = unique_named_path(dir, &sanitize_file_name(name), REQUEST_EXTENSION);
        atomic_write(&path, &postino_format::serialize(request))?;
        created.push(path_to_id(root, &path));
    }
    Ok(())
}

/// Finds a filesystem path inside `dir` for `sanitized_name` that does not exist yet, appending
/// ` (2)`, ` (3)`, ... to the name until one is free. Used for folders, which have no extension
/// (including the new top-level collection folder itself).
fn unique_path(dir: &Path, sanitized_name: &str) -> PathBuf {
    let mut candidate = dir.join(sanitized_name);
    let mut counter = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{sanitized_name} ({counter})"));
        counter += 1;
    }
    candidate
}

/// Like [`unique_path`], but for a file with `extension`: the numeric suffix is inserted before
/// the extension (`"login (2).postino"`), not after it.
fn unique_named_path(dir: &Path, sanitized_name: &str, extension: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{sanitized_name}.{extension}"));
    let mut counter = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{sanitized_name} ({counter}).{extension}"));
        counter += 1;
    }
    candidate
}

/// Writes `base` to `environments/<name>.env` and, if `secret` is non-empty, `secret` to
/// `environments/<name>.local.env`.
///
/// `name` is sanitized first. If a file with that name already exists (checking both
/// extensions, so the base and secret files of one environment always share the same name), a
/// numeric suffix is appended, the same way [`unique_path`] does for folders and requests: this
/// crate chooses to suffix rather than fail, consistent with how every other import name clash
/// is handled, so one import never aborts partway through because of a stray existing file.
/// Writing nothing (`base` and `secret` both empty) is a no-op, the `environments/` folder is
/// not even created in that case.
fn write_environment(
    root: &Path,
    name: &str,
    base: &[KeyValue],
    secret: &[KeyValue],
    created: &mut Vec<String>,
) -> Result<(), WorkspaceError> {
    if base.is_empty() && secret.is_empty() {
        return Ok(());
    }
    let dir = root.join(ENVIRONMENTS_FOLDER);
    fs::create_dir_all(&dir).map_err(|source| WorkspaceError::Io {
        path: dir.clone(),
        source,
    })?;

    let sanitized = sanitize_file_name(name);
    let unique_name = unique_environment_name(&dir, &sanitized);

    if !base.is_empty() {
        let path = dir.join(format!("{unique_name}.env"));
        atomic_write(&path, &postino_format::env::serialize(base))?;
        created.push(path_to_id(root, &path));
    }
    if !secret.is_empty() {
        let path = dir.join(format!("{unique_name}.local.env"));
        atomic_write(&path, &postino_format::env::serialize(secret))?;
        created.push(path_to_id(root, &path));
    }
    Ok(())
}

/// Finds an environment name inside `dir` for which neither `<name>.env` nor `<name>.local.env`
/// exists yet, appending ` (2)`, ` (3)`, ... until one is free.
fn unique_environment_name(dir: &Path, sanitized_name: &str) -> String {
    let mut candidate = sanitized_name.to_string();
    let mut counter = 2;
    while dir.join(format!("{candidate}.env")).exists()
        || dir.join(format!("{candidate}.local.env")).exists()
    {
        candidate = format!("{sanitized_name} ({counter})");
        counter += 1;
    }
    candidate
}
