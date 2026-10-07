//! The [`Workspace`] type: the entry point of this crate.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use postino_core::{Environment, KeyValue, Request};

use crate::error::WorkspaceError;
use crate::ids::{id_to_path, path_to_id, validate_relative_path};
use crate::sanitize::sanitize_file_name;
use crate::scan::scan_folder;
use crate::tree::Node;
use crate::{ENVIRONMENTS_FOLDER, REQUEST_EXTENSION};

/// A Postino workspace: a folder on disk holding `.postino` request files, subfolders acting as
/// collections, and an optional top-level `environments/` folder.
///
/// [`Workspace::open`] scans the folder once, keeping the resulting tree in memory. Every method
/// that changes the workspace (`save_request`, `create_request`, `create_folder`, `rename`,
/// `delete`) re-scans the folder afterwards, so [`Workspace::tree`] always reflects what is on
/// disk after the call returns successfully.
#[derive(Debug)]
pub struct Workspace {
    root: PathBuf,
    tree: Vec<Node>,
}

impl Workspace {
    /// Opens a workspace by recursively scanning `root`.
    ///
    /// Directories become folders, `.postino` files become requests, and the top-level
    /// `environments/` folder is read separately (see [`Workspace::list_environments`] and
    /// [`Workspace::load_environment`]) rather than appearing in the tree. A `.postino` file
    /// that fails to parse does not fail the scan: it is still listed, marked broken (see
    /// [`crate::RequestEntry::broken`]).
    pub fn open(root: impl AsRef<Path>) -> Result<Workspace, WorkspaceError> {
        let root = root.as_ref().to_path_buf();
        let is_dir = fs::metadata(&root).is_ok_and(|metadata| metadata.is_dir());
        if !is_dir {
            return Err(WorkspaceError::RootNotFound(root));
        }
        let tree = scan_folder(&root, &root, true)?;
        Ok(Workspace { root, tree })
    }

    /// The workspace root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The collection tree, as of the last scan (the initial [`Workspace::open`], or the most
    /// recent mutating call).
    pub fn tree(&self) -> &[Node] {
        &self.tree
    }

    /// Re-scans the workspace root, refreshing the tree returned by [`Workspace::tree`].
    pub fn rescan(&mut self) -> Result<(), WorkspaceError> {
        self.tree = scan_folder(&self.root, &self.root, true)?;
        Ok(())
    }

    /// Loads and parses the request with the given id.
    pub fn load_request(&self, id: &str) -> Result<Request, WorkspaceError> {
        let path = id_to_path(&self.root, id)?;
        if !path.is_file() {
            return Err(WorkspaceError::NotFound(id.to_string()));
        }
        let text =
            fs::read_to_string(&path).map_err(|source| WorkspaceError::Io { path, source })?;
        Ok(postino_format::parse(&text)?)
    }

    /// Serializes `request` and writes it to the file with the given id.
    ///
    /// The write is atomic: the content is first written to a temporary file in the same
    /// directory, which is then renamed into place, so a reader (or a crash) never sees a
    /// partially written file. The parent folder must already exist; use
    /// [`Workspace::create_folder`] first if needed.
    pub fn save_request(&mut self, id: &str, request: &Request) -> Result<(), WorkspaceError> {
        let path = id_to_path(&self.root, id)?;
        if !has_request_extension(&path) {
            return Err(WorkspaceError::InvalidId(id.to_string()));
        }
        let parent = parent_dir(&path, id)?;
        if !parent.is_dir() {
            return Err(WorkspaceError::NotFound(id.to_string()));
        }
        atomic_write(&path, &postino_format::serialize(request))?;
        self.rescan()
    }

    /// Creates a new, blank request named `name` inside the folder `parent_id` (or at the
    /// workspace root when `parent_id` is `None`), and returns its new id.
    ///
    /// `name` is sanitized with [`crate::sanitize_file_name`] before use, and `.postino` is
    /// appended to it. Fails with [`WorkspaceError::AlreadyExists`] if a file already exists at
    /// the resulting path.
    pub fn create_request(
        &mut self,
        parent_id: Option<&str>,
        name: &str,
    ) -> Result<String, WorkspaceError> {
        let parent = self.resolve_existing_folder(parent_id)?;
        let file_name = format!("{}.{REQUEST_EXTENSION}", sanitize_file_name(name));
        let path = parent.join(&file_name);
        if path.exists() {
            return Err(WorkspaceError::AlreadyExists(file_name));
        }
        atomic_write(&path, &postino_format::serialize(&new_request()))?;
        let id = path_to_id(&self.root, &path);
        self.rescan()?;
        Ok(id)
    }

    /// Creates a new, empty folder named `name` inside the folder `parent_id` (or at the
    /// workspace root when `parent_id` is `None`), and returns its new id.
    ///
    /// `name` is sanitized with [`crate::sanitize_file_name`] before use. Fails with
    /// [`WorkspaceError::AlreadyExists`] if a file or folder already exists at the resulting
    /// path.
    pub fn create_folder(
        &mut self,
        parent_id: Option<&str>,
        name: &str,
    ) -> Result<String, WorkspaceError> {
        let parent = self.resolve_existing_folder(parent_id)?;
        let folder_name = sanitize_file_name(name);
        let path = parent.join(&folder_name);
        if path.exists() {
            return Err(WorkspaceError::AlreadyExists(folder_name));
        }
        fs::create_dir(&path).map_err(|source| WorkspaceError::Io {
            path: path.clone(),
            source,
        })?;
        let id = path_to_id(&self.root, &path);
        self.rescan()?;
        Ok(id)
    }

    /// Renames the file or folder with the given id to `new_name`, keeping it in the same parent
    /// folder, and returns its new id.
    ///
    /// `new_name` is sanitized with [`crate::sanitize_file_name`] before use; a request keeps its
    /// `.postino` extension. Fails with [`WorkspaceError::AlreadyExists`] if a file or folder
    /// already exists at the resulting path.
    pub fn rename(&mut self, id: &str, new_name: &str) -> Result<String, WorkspaceError> {
        let old_path = id_to_path(&self.root, id)?;
        if !old_path.exists() {
            return Err(WorkspaceError::NotFound(id.to_string()));
        }
        let parent = parent_dir(&old_path, id)?;
        let sanitized = sanitize_file_name(new_name);
        let new_file_name = if has_request_extension(&old_path) {
            format!("{sanitized}.{REQUEST_EXTENSION}")
        } else {
            sanitized
        };
        let new_path = parent.join(&new_file_name);
        if new_path == old_path {
            return Ok(id.to_string());
        }
        if new_path.exists() {
            return Err(WorkspaceError::AlreadyExists(new_file_name));
        }
        fs::rename(&old_path, &new_path).map_err(|source| WorkspaceError::Io {
            path: new_path.clone(),
            source,
        })?;
        let new_id = path_to_id(&self.root, &new_path);
        self.rescan()?;
        Ok(new_id)
    }

    /// Deletes the file or folder with the given id. Deleting a folder removes it and everything
    /// inside it.
    pub fn delete(&mut self, id: &str) -> Result<(), WorkspaceError> {
        let path = id_to_path(&self.root, id)?;
        let metadata = fs::symlink_metadata(&path).map_err(|source| WorkspaceError::Io {
            path: path.clone(),
            source,
        })?;
        if metadata.is_dir() {
            fs::remove_dir_all(&path).map_err(|source| WorkspaceError::Io {
                path: path.clone(),
                source,
            })?;
        } else {
            fs::remove_file(&path).map_err(|source| WorkspaceError::Io {
                path: path.clone(),
                source,
            })?;
        }
        self.rescan()
    }

    /// Lists the names of the environments found in the top-level `environments/` folder (the
    /// file name of each `<name>.env` file, without the extension). Returns an empty list if the
    /// folder does not exist. The result is sorted the same way the tree is (natural sort).
    pub fn list_environments(&self) -> Result<Vec<String>, WorkspaceError> {
        let dir = self.root.join(ENVIRONMENTS_FOLDER);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        let entries = fs::read_dir(&dir).map_err(|source| WorkspaceError::Io {
            path: dir.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| WorkspaceError::Io {
                path: dir.clone(),
                source,
            })?;
            let is_file = entry.file_type().is_ok_and(|file_type| file_type.is_file());
            if !is_file {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = file_name.strip_suffix(".env") {
                // A "<name>.local.env" file also ends in ".env", but it is not a separate
                // environment: it is merged into "<name>.env" by `load_environment`.
                // On case-insensitive file systems (macOS, Windows) "dev.LOCAL.env" is the same
                // file as "dev.local.env", so ask the file system instead of comparing spellings.
                let is_local_layer = strip_local_suffix(stem)
                    .is_some_and(|base| dir.join(format!("{base}.local.env")).is_file());
                if !is_local_layer {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort_by(|a, b| crate::tree::natural_cmp(a, b).then_with(|| a.cmp(b)));
        Ok(names)
    }

    /// Loads the environment `name`, merging `environments/<name>.local.env` over
    /// `environments/<name>.env`: a variable present in both keeps its position from the base file
    /// but takes its value from the local file. At least one of the two files must exist.
    pub fn load_environment(&self, name: &str) -> Result<Environment, WorkspaceError> {
        validate_relative_path(name)
            .map_err(|_| WorkspaceError::EnvironmentNotFound(name.to_string()))?;
        let dir = self.root.join(ENVIRONMENTS_FOLDER);
        let base_path = dir.join(format!("{name}.env"));
        let local_path = dir.join(format!("{name}.local.env"));

        if !base_path.is_file() && !local_path.is_file() {
            return Err(WorkspaceError::EnvironmentNotFound(name.to_string()));
        }

        let mut variables = if base_path.is_file() {
            self.read_env_file(&base_path, name)?
        } else {
            Vec::new()
        };
        if local_path.is_file() {
            let local_variables = self.read_env_file(&local_path, name)?;
            merge_local_over_base(&mut variables, local_variables);
        }

        Ok(Environment {
            name: name.to_string(),
            variables,
        })
    }

    /// Reads and parses a single `.env`/`.local.env` file for [`Workspace::load_environment`].
    fn read_env_file(&self, path: &Path, name: &str) -> Result<Vec<KeyValue>, WorkspaceError> {
        let text = fs::read_to_string(path).map_err(|source| WorkspaceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        postino_format::env::parse(&text).map_err(|source| WorkspaceError::EnvParse {
            name: name.to_string(),
            source,
        })
    }

    /// Creates a new, empty environment: `environments/<name>.env`, creating the environments
    /// folder if it is missing. Fails with [`WorkspaceError::AlreadyExists`] if the file already
    /// exists.
    pub fn create_environment(&self, name: &str) -> Result<(), WorkspaceError> {
        validate_relative_path(name)?;
        let dir = self.root.join(ENVIRONMENTS_FOLDER);
        fs::create_dir_all(&dir).map_err(|source| WorkspaceError::Io {
            path: dir.clone(),
            source,
        })?;
        let file_name = format!("{name}.env");
        let path = dir.join(&file_name);
        if path.exists() {
            return Err(WorkspaceError::AlreadyExists(file_name));
        }
        atomic_write(&path, "")
    }

    /// Resolves `parent_id` (or the workspace root when `None`) to an existing directory path,
    /// used by [`Workspace::create_request`] and [`Workspace::create_folder`].
    fn resolve_existing_folder(&self, parent_id: Option<&str>) -> Result<PathBuf, WorkspaceError> {
        let path = match parent_id {
            Some(id) => id_to_path(&self.root, id)?,
            None => self.root.clone(),
        };
        if !path.is_dir() {
            return Err(WorkspaceError::NotFound(
                parent_id.unwrap_or("").to_string(),
            ));
        }
        Ok(path)
    }
}

/// A blank request used as the content of a newly created `.postino` file. `Request::default`
/// alone would not do: its empty URL does not parse back (`docs/format.md` requires a
/// non-empty URL), so the new file would immediately show up as broken.
fn new_request() -> Request {
    Request {
        url: "https://example.com".to_string(),
        ..Request::default()
    }
}

/// Whether `path` has the `.postino` extension.
fn has_request_extension(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some(REQUEST_EXTENSION)
}

/// The parent directory of `path`, or [`WorkspaceError::InvalidId`] if `path` has none (which
/// should not happen for a path built from a validated, non-empty id).
fn parent_dir<'a>(path: &'a Path, id: &str) -> Result<&'a Path, WorkspaceError> {
    path.parent()
        .ok_or_else(|| WorkspaceError::InvalidId(id.to_string()))
}

/// Merges `local` over `base`: a key present in both keeps its position in `base` but takes its
/// value from `local`, a key only in `local` is appended at the end, in `local`'s order.
fn merge_local_over_base(base: &mut Vec<KeyValue>, local: Vec<KeyValue>) {
    for local_variable in local {
        match base
            .iter_mut()
            .find(|variable| variable.key == local_variable.key)
        {
            Some(existing) => existing.value = local_variable.value,
            None => base.push(local_variable),
        }
    }
}

/// Writes `contents` to `path` atomically: a temporary file is created in the same directory,
/// written, and then renamed into place, so a reader never observes a partially written file.
pub(crate) fn atomic_write(path: &Path, contents: &str) -> Result<(), WorkspaceError> {
    let dir = path
        .parent()
        .ok_or_else(|| WorkspaceError::InvalidId(path.display().to_string()))?;
    let mut temp_file =
        tempfile::NamedTempFile::new_in(dir).map_err(|source| WorkspaceError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
    temp_file
        .write_all(contents.as_bytes())
        .map_err(|source| WorkspaceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    temp_file
        .persist(path)
        .map_err(|error| WorkspaceError::Io {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    Ok(())
}

/// Returns `stem` without a trailing `.local`, compared ignoring ASCII case.
pub(crate) fn strip_local_suffix(stem: &str) -> Option<&str> {
    let split = stem.len().checked_sub(".local".len())?;
    let (base, suffix) = (stem.get(..split)?, stem.get(split..)?);
    suffix.eq_ignore_ascii_case(".local").then_some(base)
}
