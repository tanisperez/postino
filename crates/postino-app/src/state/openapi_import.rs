//! The OpenAPI import wizard's state (GitHub #201): the source, the loaded specs, the options,
//! the plan they build and which of its requests are selected, plus the rows of the preview tree.
//! Plain Rust, so the wizard's logic is tested without a window; `views/openapi_import.rs`
//! renders it.
//!
//! Loading a spec reads a file, scans a folder, clones a repository or fetches a URL, and with it
//! every document its `$ref`s point to. It is blocking, the view runs it on the background
//! executor.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use postino_core::{Method, ResolvedField, ResolvedRequest};
use postino_format::openapi::{
    CatalogDefinition, ChangeKind, ImportOptions, ManifestSource, NoExternalRefs, OpenApiPlan,
    PlanChange, PlanFolder, RefLoader, Spec, SpecVersion, build_plan, diff_plan,
    looks_like_catalog, looks_like_spec, parse_catalog, parse_spec,
};
use postino_runner::Client;
use postino_workspace::OpenApiCollection;

/// Where the wizard reads the spec from (the source tabs of the first step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A spec file (or a `catalog-info.yaml`) picked or dropped.
    File,
    /// A spec fetched from a URL.
    Url,
    /// Spec text pasted into the wizard.
    Paste,
    /// Every spec of a folder and its subfolders (GitHub #202).
    Folder,
    /// Every spec of a branch of a Git repository, cloned to a temporary folder (GitHub #202).
    Git,
}

/// The wizard's steps, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Pick the source and load the spec.
    Source,
    /// Collection name, folders and what to generate.
    Options,
    /// The tree of requests, to choose which ones to import.
    Preview,
    /// The import finished.
    Done,
}

/// A spec read by the wizard.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSpec {
    /// The spec.
    pub spec: Spec,
    /// What the source list shows under the title: the file name, its path in the folder or
    /// repository, or the URL. Empty for pasted text.
    pub source: String,
    /// Where it was read from (a canonical path or a URL), to read each file once.
    pub location: String,
}

/// A file that looked like a spec, or that a catalog pointed to, and could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The file, as the source list shows it.
    pub source: String,
    /// Why.
    pub error: String,
}

/// A Backstage catalog file that was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogFile {
    /// The file, as the source list shows it.
    pub source: String,
    /// How many OpenAPI APIs it declares.
    pub apis: usize,
}

/// Everything a source gave.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loaded {
    /// The specs read, in source order.
    pub specs: Vec<LoadedSpec>,
    /// The files that could not be read.
    pub failures: Vec<Failure>,
    /// The catalog files read.
    pub catalogs: Vec<CatalogFile>,
}

/// The `Accept` header of a spec download: the YAML and JSON types servers use for specs.
const SPEC_ACCEPT: &str = "application/yaml, application/x-yaml, application/json;q=0.9, */*;q=0.8";

/// The folders a folder scan never enters, besides hidden ones: dependencies and build output.
const SKIPPED_FOLDERS: &[&str] = &["node_modules", "target", "vendor", "dist", "build"];

/// How deep a folder scan goes below the folder picked.
const MAX_SCAN_DEPTH: usize = 8;

/// How many files a folder scan looks at, at most.
const MAX_SCAN_FILES: usize = 5_000;

/// The biggest file a folder scan reads.
const MAX_SCAN_FILE_SIZE: u64 = 32 * 1024 * 1024;

/// Reads the file at `path`: a spec, or a Backstage catalog whose OpenAPI APIs are read in turn.
/// `shown` is how the source list names it. A catalog with no readable spec is not an error:
/// the result lists the failures.
pub fn load_path(path: &Path, shown: &str, client: &Client) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let location = canonical(path);
    if is_catalog_name(path) || (!looks_like_spec(&text) && looks_like_catalog(&text)) {
        return load_catalog(&text, &location, shown, client);
    }
    let spec = parse_spec(&text, &location, &SourceLoader { client })
        .map_err(|error| error.to_string())?;
    Ok(Loaded {
        specs: vec![LoadedSpec {
            spec,
            source: shown.to_string(),
            location,
        }],
        ..Loaded::default()
    })
}

/// Whether `path` is named like a Backstage catalog file.
fn is_catalog_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("catalog-info.yaml")
                || name.eq_ignore_ascii_case("catalog-info.yml")
        })
}

fn canonical(path: &Path) -> String {
    dunce::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Reads the OpenAPI APIs of the catalog text found at `location`.
fn load_catalog(
    text: &str,
    location: &str,
    shown: &str,
    client: &Client,
) -> Result<Loaded, String> {
    let apis = parse_catalog(text).map_err(|error| error.to_string())?;
    let mut loaded = Loaded {
        catalogs: vec![CatalogFile {
            source: shown.to_string(),
            apis: apis.len(),
        }],
        ..Loaded::default()
    };
    let directory = Path::new(location)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let shown_directory = shown
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    for api in apis {
        let result = match &api.definition {
            CatalogDefinition::Inline(spec) => parse_spec(spec, location, &SourceLoader { client })
                .map(|spec| LoadedSpec {
                    spec,
                    source: format!("{shown} ({})", api.name),
                    location: format!("{location}#{}", api.name),
                })
                .map_err(|error| (format!("{shown} ({})", api.name), error.to_string())),
            CatalogDefinition::Reference(reference) if is_url(reference) => {
                load_url(reference, client).map_err(|error| (reference.clone(), error))
            }
            CatalogDefinition::Reference(reference) => {
                let relative = reference.trim_start_matches("./");
                let shown = if shown_directory.is_empty() {
                    relative.to_string()
                } else {
                    format!("{shown_directory}/{relative}")
                };
                load_path(&directory.join(relative), &shown, client)
                    .map_err(|error| (shown.clone(), error))
                    .map(|mut found| found.specs.remove(0))
            }
        };
        match result {
            Ok(spec) => loaded.specs.push(spec),
            Err((source, error)) => loaded.failures.push(Failure { source, error }),
        }
    }
    Ok(loaded)
}

/// Reads every spec under `root`: files that look like a spec root, and the specs of every
/// catalog file. Hidden folders, dependency and build folders are skipped. A spec found both
/// directly and through a catalog is read once.
pub fn scan_folder(root: &Path, client: &Client) -> Result<Loaded, String> {
    let mut files = Vec::new();
    collect_files(root, 0, &mut files);
    files.sort();
    let mut loaded = Loaded::default();
    let mut seen: HashSet<String> = HashSet::new();
    // Catalogs first, so their specs keep the catalog's order.
    let (catalogs, others): (Vec<PathBuf>, Vec<PathBuf>) =
        files.into_iter().partition(|path| is_catalog_name(path));
    for path in catalogs.into_iter().chain(others) {
        let shown = relative_name(root, &path);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let catalog = is_catalog_name(&path);
        if !catalog && !looks_like_spec(&text) {
            continue;
        }
        if !catalog && seen.contains(&canonical(&path)) {
            continue;
        }
        match load_path(&path, &shown, client) {
            Ok(found) => {
                for spec in found.specs {
                    if seen.insert(spec.location.clone()) {
                        loaded.specs.push(spec);
                    }
                }
                loaded.failures.extend(found.failures);
                loaded.catalogs.extend(found.catalogs);
            }
            Err(error) => loaded.failures.push(Failure {
                source: shown,
                error,
            }),
        }
    }
    Ok(loaded)
}

/// The YAML and JSON files under `dir`, up to [`MAX_SCAN_DEPTH`] folders deep and
/// [`MAX_SCAN_FILES`] files.
fn collect_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if out.len() >= MAX_SCAN_FILES {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            if depth < MAX_SCAN_DEPTH && !SKIPPED_FOLDERS.contains(&name.as_str()) {
                collect_files(&path, depth + 1, out);
            }
        } else if kind.is_file() {
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase);
            let small = entry
                .metadata()
                .is_ok_and(|metadata| metadata.len() <= MAX_SCAN_FILE_SIZE);
            if matches!(extension.as_deref(), Some("yaml" | "yml" | "json")) && small {
                out.push(path);
            }
        }
    }
}

/// `path` relative to `root`, with `/` separators.
fn relative_name(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Clones the last commit of `branch` (the default branch when empty) of `url` into a new
/// temporary folder and reads its specs. The folder is deleted when the returned guard drops.
pub fn clone_repository(
    git: &telaio_git::Git,
    url: &str,
    branch: &str,
    client: &Client,
) -> Result<(tempfile::TempDir, Loaded), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("no repository".to_string());
    }
    let parent = tempfile::Builder::new()
        .prefix("postino-openapi-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let target = parent.path().join("repository");
    let branch = Some(branch.trim()).filter(|branch| !branch.is_empty());
    git.clone_shallow(url, branch, &target)
        .map_err(|error| error.to_string())?;
    let loaded = scan_folder(&target, client)?;
    Ok((parent, loaded))
}

/// Fetches the spec at `url` with `client`, and its external references relative to it.
pub fn load_url(url: &str, client: &Client) -> Result<LoadedSpec, String> {
    let url = url.trim();
    if !is_url(url) {
        return Err("the URL must start with http:// or https://".to_string());
    }
    let text = fetch(client, url)?;
    let spec =
        parse_spec(&text, url, &SourceLoader { client }).map_err(|error| error.to_string())?;
    Ok(LoadedSpec {
        spec,
        source: url.to_string(),
        location: url.to_string(),
    })
}

/// Reads pasted spec text, which has no location for external references.
pub fn load_text(text: &str) -> Result<LoadedSpec, String> {
    let spec = parse_spec(text, "", &NoExternalRefs).map_err(|error| error.to_string())?;
    Ok(LoadedSpec {
        spec,
        source: String::new(),
        location: String::new(),
    })
}

/// Downloads `url` as text, failing on anything but a 2xx status.
fn fetch(client: &Client, url: &str) -> Result<String, String> {
    let request = ResolvedRequest {
        method: Method::Get,
        url: url.to_string(),
        headers: vec![ResolvedField {
            name: "Accept".to_string(),
            value: SPEC_ACCEPT.to_string(),
        }],
        ..ResolvedRequest::default()
    };
    let response = client.send(&request).map_err(|error| error.to_string())?;
    if !(200..300).contains(&response.status) {
        return Err(format!("the server answered HTTP {}", response.status));
    }
    Ok(String::from_utf8_lossy(&response.body).into_owned())
}

/// Loads the documents a spec's external `$ref`s point to: files relative to a file, URLs
/// relative to a URL, and absolute URLs from anywhere.
struct SourceLoader<'a> {
    client: &'a Client,
}

impl RefLoader for SourceLoader<'_> {
    fn load(&self, base: &str, reference: &str) -> Result<(String, String), String> {
        if is_url(reference) || is_url(base) {
            let url = join_url(base, reference);
            let text = fetch(self.client, &url)?;
            return Ok((url, text));
        }
        let path = Path::new(base)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(reference);
        let path = dunce::canonicalize(&path).map_err(|error| error.to_string())?;
        let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        Ok((path.to_string_lossy().into_owned(), text))
    }
}

fn is_url(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://")
}

/// Resolves `reference` against the URL `base`, with `.` and `..` segments removed.
pub fn join_url(base: &str, reference: &str) -> String {
    if is_url(reference) {
        return reference.to_string();
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return reference.to_string();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    if let Some(rest) = reference.strip_prefix("//") {
        return format!("{scheme}://{rest}");
    }
    let joined = if reference.starts_with('/') {
        reference.to_string()
    } else {
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
        format!("{directory}/{reference}")
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in joined.split('/').skip(1) {
        match segment {
            "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    format!("{scheme}://{authority}/{}", segments.join("/"))
}

/// The selection state of a folder of the preview tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Every request under it is selected.
    All,
    /// Some are.
    Some,
    /// None is.
    None,
}

/// One visible row of the preview tree.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewRow {
    /// The nesting level, 0 at the top of the collection.
    pub depth: usize,
    /// The folder's path or the request's key, unique among the rows.
    pub id: String,
    /// The folder or request name.
    pub name: String,
    /// What the row is.
    pub kind: RowKind,
}

/// The kinds of [`PreviewRow`].
#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    /// A folder.
    Folder {
        /// Whether its children are shown.
        open: bool,
        /// Its selection state.
        check: Check,
        /// How many requests under it are selected.
        selected: usize,
        /// How many requests there are under it.
        total: usize,
    },
    /// A request.
    Request {
        /// The HTTP method.
        method: Method,
        /// Whether it is selected.
        checked: bool,
    },
}

/// The separator of folder paths in [`PreviewRow::id`], a character no folder name has.
const PATH_SEPARATOR: char = '\u{1f}';

/// The wizard's state.
pub struct Wizard {
    /// The current step.
    pub step: Step,
    /// The selected source tab.
    pub source: SourceKind,
    /// The import options of the second step.
    pub options: ImportOptions,
    /// The specs loaded in the first step.
    specs: Vec<LoadedSpec>,
    /// Which of them are imported (the Versions list), one flag per spec.
    enabled: Vec<bool>,
    /// The files of the source that could not be read.
    failures: Vec<Failure>,
    /// The catalog files of the source.
    catalogs: Vec<CatalogFile>,
    /// The plan of the loaded specs with the current options and collection name.
    plan: Option<Arc<OpenApiPlan>>,
    /// The plan with every option on, for the summaries of what the options generate.
    full_plan: Option<OpenApiPlan>,
    /// The requests the user unchecked, by key. Kept across plan rebuilds, keys do not change.
    excluded: HashSet<String>,
    /// The collapsed folders, by path.
    closed: HashSet<String>,
    /// The preview rows, rebuilt when the plan, the selection or a folder changes.
    rows: Arc<[PreviewRow]>,
    /// `(selected, total, folders)` of the plan, counted with the rows so render reads them.
    counts: (usize, usize, usize),
    /// The collection an earlier import of the same source created, if any (GitHub #203).
    existing: Option<OpenApiCollection>,
    /// Whether the Preview step updates [`Self::existing`] instead of importing a new
    /// collection.
    update: bool,
    /// What changed since [`Self::existing`] was imported, the unchanged operations left out:
    /// added first, then changed, then removed.
    changes: Arc<[PlanChange]>,
    /// The changes the user unchecked, by request key. Removed operations start unchecked.
    skipped: HashSet<String>,
    /// How many operations are unchanged.
    unchanged: usize,
}

/// The kind of manifest source of `kind`, as written in `.openapi.toml`.
pub fn source_kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::File => "file",
        SourceKind::Url => "url",
        SourceKind::Paste => "paste",
        SourceKind::Folder => "folder",
        SourceKind::Git => "git",
    }
}

/// The source kind of a manifest's `kind`, `None` for pasted text or an unknown kind: those
/// cannot be read again.
pub fn source_kind_of(name: &str) -> Option<SourceKind> {
    match name {
        "file" => Some(SourceKind::File),
        "url" => Some(SourceKind::Url),
        "folder" => Some(SourceKind::Folder),
        "git" => Some(SourceKind::Git),
        _ => None,
    }
}

/// The collection of `collections` imported from `source`. Pasted text has no source to match.
pub fn find_existing(
    collections: Vec<OpenApiCollection>,
    source: &ManifestSource,
) -> Option<OpenApiCollection> {
    if source.kind == "paste" || source.location.is_empty() {
        return None;
    }
    collections.into_iter().find(|collection| {
        let known = &collection.manifest.source;
        known.kind == source.kind
            && known.location == source.location
            && known.branch.trim() == source.branch.trim()
    })
}

impl Wizard {
    /// A wizard on its first step, with `source` selected.
    pub fn new(source: SourceKind) -> Self {
        Wizard {
            step: Step::Source,
            source,
            options: ImportOptions::default(),
            specs: Vec::new(),
            enabled: Vec::new(),
            failures: Vec::new(),
            catalogs: Vec::new(),
            plan: None,
            full_plan: None,
            excluded: HashSet::new(),
            closed: HashSet::new(),
            rows: Arc::from(Vec::new()),
            counts: (0, 0, 0),
            existing: None,
            update: false,
            changes: Arc::from(Vec::new()),
            skipped: HashSet::new(),
            unchanged: 0,
        }
    }

    /// The loaded specs.
    pub fn specs(&self) -> &[LoadedSpec] {
        &self.specs
    }

    /// The files of the source that could not be read.
    pub fn failures(&self) -> &[Failure] {
        &self.failures
    }

    /// The catalog files of the source.
    pub fn catalogs(&self) -> &[CatalogFile] {
        &self.catalogs
    }

    /// Whether nothing was loaded, not even a failure.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty() && self.failures.is_empty() && self.catalogs.is_empty()
    }

    /// Replaces what the source gave, which starts the selection over.
    pub fn set_loaded(&mut self, loaded: Loaded) {
        self.set_specs(loaded.specs);
        self.failures = loaded.failures;
        self.catalogs = loaded.catalogs;
    }

    /// Replaces the loaded specs, which starts the selection over.
    pub fn set_specs(&mut self, specs: Vec<LoadedSpec>) {
        self.enabled = vec![true; specs.len()];
        self.specs = specs;
        self.failures.clear();
        self.catalogs.clear();
        self.plan = None;
        self.full_plan = None;
        self.excluded.clear();
        self.closed.clear();
        self.rows = Arc::from(Vec::new());
        self.counts = (0, 0, 0);
        self.existing = None;
        self.update = false;
        self.changes = Arc::from(Vec::new());
        self.skipped.clear();
        self.unchanged = 0;
    }

    /// Records the collection an earlier import of this source created: the Preview step then
    /// offers to update it, with its options.
    pub fn set_existing(&mut self, existing: Option<OpenApiCollection>) {
        if let Some(collection) = &existing {
            self.options = collection.manifest.options.into();
        }
        self.update = existing.is_some();
        self.existing = existing;
        self.skipped.clear();
    }

    /// The collection an earlier import of this source created.
    pub fn existing(&self) -> Option<&OpenApiCollection> {
        self.existing.as_ref()
    }

    /// Whether the Preview step updates the existing collection.
    pub fn is_update(&self) -> bool {
        self.update && self.existing.is_some()
    }

    /// Chooses between updating the existing collection and importing a new one.
    pub fn set_update(&mut self, update: bool) {
        self.update = update;
    }

    /// The changes since the existing collection was imported, unchanged operations left out.
    pub fn changes(&self) -> Arc<[PlanChange]> {
        self.changes.clone()
    }

    /// `(added, changed, removed, unchanged)`.
    pub fn change_counts(&self) -> (usize, usize, usize, usize) {
        let count = |kind| {
            self.changes
                .iter()
                .filter(|change| change.kind == kind)
                .count()
        };
        (
            count(ChangeKind::Added),
            count(ChangeKind::Changed),
            count(ChangeKind::Removed),
            self.unchanged,
        )
    }

    /// Whether the change of `key` is applied.
    pub fn is_change_selected(&self, key: &str) -> bool {
        !self.skipped.contains(key)
    }

    /// Applies the change of `key`, or not.
    pub fn toggle_change(&mut self, key: &str) {
        if !self.skipped.remove(key) {
            self.skipped.insert(key.to_string());
        }
    }

    /// How many changes are applied.
    pub fn selected_change_count(&self) -> usize {
        self.changes
            .iter()
            .filter(|change| self.is_change_selected(&change.key))
            .count()
    }

    /// The name the collection gets unless the user types another one: the first spec's title.
    pub fn suggested_name(&self) -> String {
        self.specs
            .first()
            .map(|loaded| loaded.spec.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "OpenAPI".to_string())
    }

    /// Whether the spec at `index` is imported.
    pub fn is_enabled(&self, index: usize) -> bool {
        self.enabled.get(index).copied().unwrap_or(false)
    }

    /// Imports the spec at `index`, or stops importing it. The plan has to be rebuilt.
    pub fn toggle_version(&mut self, index: usize) {
        if let Some(flag) = self.enabled.get_mut(index) {
            *flag = !*flag;
            self.full_plan = None;
        }
    }

    /// How many specs are imported.
    pub fn enabled_count(&self) -> usize {
        self.enabled.iter().filter(|enabled| **enabled).count()
    }

    /// The folder of each spec when several are imported together: `v<major>` of the API
    /// versions when they differ, else the titles when they differ, else the file names.
    pub fn version_labels(&self) -> Vec<String> {
        if self.specs.len() < 2 {
            return vec![String::new(); self.specs.len()];
        }
        let unique = |labels: &[String]| {
            let set: HashSet<&String> = labels.iter().collect();
            set.len() == labels.len() && labels.iter().all(|label| !label.is_empty())
        };
        let majors: Vec<String> = self
            .specs
            .iter()
            .map(|loaded| {
                let major = loaded
                    .spec
                    .api_version
                    .trim()
                    .trim_start_matches(['v', 'V']);
                let major = major.split('.').next().unwrap_or_default();
                if major.is_empty() {
                    String::new()
                } else {
                    format!("v{major}")
                }
            })
            .collect();
        if unique(&majors) {
            return majors;
        }
        let titles: Vec<String> = self
            .specs
            .iter()
            .map(|loaded| loaded.spec.title.clone())
            .collect();
        if unique(&titles) {
            return titles;
        }
        let mut names: Vec<String> = self
            .specs
            .iter()
            .map(|loaded| {
                let file = loaded.source.rsplit('/').next().unwrap_or_default();
                file.rsplit_once('.')
                    .map_or(file, |(stem, _)| stem)
                    .to_string()
            })
            .collect();
        if !unique(&names) {
            for (index, name) in names.iter_mut().enumerate() {
                *name = format!("{name} ({})", index + 1);
            }
        }
        names
    }

    /// Builds the plan of the loaded specs for a collection named `name`, with the current
    /// options, and the preview rows. Call it when the specs, the name or an option change,
    /// never on every render.
    pub fn rebuild(&mut self, name: &str) {
        let labels = self.version_labels();
        let versions: Vec<SpecVersion<'_>> = self
            .specs
            .iter()
            .zip(&labels)
            .zip(&self.enabled)
            .filter(|(_, enabled)| **enabled)
            .map(|((loaded, label), _)| SpecVersion {
                label,
                spec: &loaded.spec,
            })
            .collect();
        let name = if name.trim().is_empty() {
            self.suggested_name()
        } else {
            name.trim().to_string()
        };
        let plan = build_plan(&name, &versions, &self.options);
        if self.full_plan.is_none() {
            self.full_plan = Some(build_plan(&name, &versions, &ImportOptions::default()));
        }
        if let Some(existing) = &self.existing {
            let first = self.changes.is_empty() && self.unchanged == 0;
            let mut changes = diff_plan(&existing.manifest, &plan);
            self.unchanged = changes
                .iter()
                .filter(|change| change.kind == ChangeKind::Unchanged)
                .count();
            changes.retain(|change| change.kind != ChangeKind::Unchanged);
            // The design's order: added, changed, removed, each in plan order.
            changes.sort_by_key(|change| match change.kind {
                ChangeKind::Added => 0,
                ChangeKind::Changed => 1,
                _ => 2,
            });
            if first {
                // Removed operations stay unless the user checks them.
                self.skipped.extend(
                    changes
                        .iter()
                        .filter(|change| change.kind == ChangeKind::Removed)
                        .map(|change| change.key.clone()),
                );
            }
            self.changes = Arc::from(changes);
        }
        self.plan = Some(Arc::new(plan));
        self.refresh_rows();
    }

    /// The current plan, once [`Self::rebuild`] ran.
    pub fn plan(&self) -> Option<&Arc<OpenApiPlan>> {
        self.plan.as_ref()
    }

    /// The preview rows.
    pub fn rows(&self) -> Arc<[PreviewRow]> {
        self.rows.clone()
    }

    /// Whether the request `key` is selected.
    pub fn is_selected(&self, key: &str) -> bool {
        !self.excluded.contains(key)
    }

    /// The requests the user unchecked.
    pub fn excluded(&self) -> &HashSet<String> {
        &self.excluded
    }

    /// How many requests the plan has.
    pub fn total_count(&self) -> usize {
        self.counts.1
    }

    /// How many of them are selected.
    pub fn selected_count(&self) -> usize {
        self.counts.0
    }

    /// How many folders the plan has.
    pub fn folder_count(&self) -> usize {
        self.counts.2
    }

    /// A path of the specs, to show what grouping by path segments gives.
    pub fn sample_path(&self) -> Option<&str> {
        let operations = self.specs.iter().flat_map(|loaded| &loaded.spec.operations);
        // The first path with two segments shows the nesting best.
        let mut first = None;
        for operation in operations {
            first = first.or(Some(operation.path.as_str()));
            if operation.path.matches('/').count() >= 2 {
                return Some(&operation.path);
            }
        }
        first
    }

    /// A tag of the specs, to show what grouping by tags gives.
    pub fn sample_tag(&self) -> Option<&str> {
        self.specs
            .iter()
            .flat_map(|loaded| &loaded.spec.operations)
            .find_map(|operation| operation.tags.first())
            .map(String::as_str)
    }

    /// How many operations the loaded specs have.
    pub fn operation_count(&self) -> usize {
        self.specs
            .iter()
            .zip(&self.enabled)
            .filter(|(_, enabled)| **enabled)
            .map(|(loaded, _)| loaded.spec.operations.len())
            .sum()
    }

    /// The `baseUrl` of each environment the "Environment from servers" option creates.
    pub fn base_urls(&self) -> Vec<String> {
        self.full_plan.as_ref().map_or_else(Vec::new, |plan| {
            plan.environments
                .iter()
                .filter_map(|environment| environment.base.first().map(|entry| entry.value.clone()))
                .collect()
        })
    }

    /// The variables the "Auth variables" option creates, stored in `.local.env`.
    pub fn auth_variables(&self) -> Vec<String> {
        self.full_plan.as_ref().map_or_else(Vec::new, |plan| {
            plan.environments
                .first()
                .map(|environment| {
                    environment
                        .secret
                        .iter()
                        .map(|entry| entry.key.clone())
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// Flips the request `key`.
    pub fn toggle_request(&mut self, key: &str) {
        if !self.excluded.remove(key) {
            self.excluded.insert(key.to_string());
        }
        self.refresh_rows();
    }

    /// Selects every request under the folder at `path`, or unselects them all when they all
    /// are selected already.
    pub fn toggle_folder(&mut self, path: &str) {
        let Some(keys) = self
            .plan
            .as_ref()
            .and_then(|plan| folder_at(&plan.root, path))
            .map(PlanFolder::keys)
        else {
            return;
        };
        let all = keys.iter().all(|key| self.is_selected(key));
        for key in keys {
            if all {
                self.excluded.insert(key);
            } else {
                self.excluded.remove(&key);
            }
        }
        self.refresh_rows();
    }

    /// Selects every request, or none.
    pub fn select_all(&mut self, selected: bool) {
        self.excluded.clear();
        if !selected && let Some(plan) = &self.plan {
            self.excluded.extend(plan.root.keys());
        }
        self.refresh_rows();
    }

    /// Opens or collapses the folder at `path`.
    pub fn toggle_open(&mut self, path: &str) {
        if !self.closed.remove(path) {
            self.closed.insert(path.to_string());
        }
        self.refresh_rows();
    }

    fn refresh_rows(&mut self) {
        let mut rows = Vec::new();
        self.counts = (0, 0, 0);
        if let Some(plan) = &self.plan {
            self.push_rows(&plan.root, "", 0, &mut rows);
            let keys = plan.root.keys();
            let selected = keys.iter().filter(|key| self.is_selected(key)).count();
            self.counts = (selected, keys.len(), plan.root.folder_count());
        }
        self.rows = Arc::from(rows);
    }

    fn push_rows(&self, folder: &PlanFolder, path: &str, depth: usize, rows: &mut Vec<PreviewRow>) {
        for child in &folder.folders {
            let child_path = if path.is_empty() {
                child.name.clone()
            } else {
                format!("{path}{PATH_SEPARATOR}{}", child.name)
            };
            let keys = child.keys();
            let selected = keys.iter().filter(|key| self.is_selected(key)).count();
            let open = !self.closed.contains(&child_path);
            rows.push(PreviewRow {
                depth,
                id: child_path.clone(),
                name: child.name.clone(),
                kind: RowKind::Folder {
                    open,
                    check: if selected == keys.len() {
                        Check::All
                    } else if selected == 0 {
                        Check::None
                    } else {
                        Check::Some
                    },
                    selected,
                    total: keys.len(),
                },
            });
            if open {
                self.push_rows(child, &child_path, depth + 1, rows);
            }
        }
        for request in &folder.requests {
            rows.push(PreviewRow {
                depth,
                id: request.key.clone(),
                name: request.name.clone(),
                kind: RowKind::Request {
                    method: request.request.method.clone(),
                    checked: self.is_selected(&request.key),
                },
            });
        }
    }
}

/// The folder at `path` (names joined with [`PATH_SEPARATOR`]) under `root`.
fn folder_at<'a>(root: &'a PlanFolder, path: &str) -> Option<&'a PlanFolder> {
    path.split(PATH_SEPARATOR).try_fold(root, |folder, name| {
        folder.folders.iter().find(|child| child.name == name)
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const SPEC: &str = r##"
openapi: 3.1.0
info: {title: Acme Payments API, version: 2.0.0}
servers: [{url: https://api.acme.dev/v2}]
security: [{bearerAuth: []}]
components:
  securitySchemes:
    bearerAuth: {type: http, scheme: bearer}
paths:
  /customers:
    get: {summary: List customers, tags: [Customers], responses: {}}
    post: {summary: Create customer, tags: [Customers], responses: {}}
  /customers/{id}:
    get: {summary: Get customer, tags: [Customers], responses: {}}
  /charges:
    get: {summary: List charges, responses: {}}
"##;

    fn wizard() -> Wizard {
        let mut wizard = Wizard::new(SourceKind::Paste);
        wizard.set_specs(vec![load_text(SPEC).expect("valid")]);
        wizard.rebuild("");
        wizard
    }

    fn row_names(wizard: &Wizard) -> Vec<String> {
        wizard
            .rows()
            .iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), row.name))
            .collect()
    }

    #[test]
    fn rows_follow_the_tree_and_the_open_folders() {
        let mut wizard = wizard();
        assert_eq!(wizard.suggested_name(), "Acme Payments API");
        assert_eq!(
            row_names(&wizard),
            vec![
                "customers",
                "  {id}",
                "    Get customer",
                "  List customers",
                "  Create customer",
                "charges",
                "  List charges"
            ]
        );
        wizard.toggle_open("customers");
        assert_eq!(
            row_names(&wizard),
            vec!["customers", "charges", "  List charges"]
        );
        assert_eq!(wizard.folder_count(), 3);
        assert_eq!(wizard.base_urls(), vec!["https://api.acme.dev".to_string()]);
        assert_eq!(wizard.auth_variables(), vec!["bearerAuth".to_string()]);
    }

    #[test]
    fn selection_counts_and_folder_checks() {
        let mut wizard = wizard();
        assert_eq!((wizard.selected_count(), wizard.total_count()), (4, 4));
        wizard.toggle_request("GET /customers/{id}");
        let customers = &wizard.rows()[0];
        assert_eq!(
            customers.kind,
            RowKind::Folder {
                open: true,
                check: Check::Some,
                selected: 2,
                total: 3
            }
        );
        // A partly selected folder selects everything first.
        wizard.toggle_folder("customers");
        assert_eq!(wizard.selected_count(), 4);
        wizard.toggle_folder("customers");
        assert_eq!(wizard.selected_count(), 1);
        wizard.select_all(false);
        assert_eq!(wizard.selected_count(), 0);
        wizard.select_all(true);
        assert_eq!(wizard.selected_count(), 4);
    }

    #[test]
    fn the_selection_survives_option_changes() {
        let mut wizard = wizard();
        wizard.toggle_request("GET /charges");
        wizard.options.grouping = postino_format::openapi::Grouping::Tags;
        wizard.rebuild("My API");
        assert_eq!(wizard.plan().expect("built").root.name, "My API");
        assert_eq!(
            row_names(&wizard),
            vec![
                "Customers",
                "  List customers",
                "  Create customer",
                "  Get customer",
                "List charges"
            ]
        );
        assert!(!wizard.is_selected("GET /charges"));
        assert_eq!(wizard.selected_count(), 3);
    }

    #[test]
    fn samples_come_from_the_spec() {
        let wizard = wizard();
        assert_eq!(wizard.sample_path(), Some("/customers/{id}"));
        assert_eq!(wizard.sample_tag(), Some("Customers"));
    }

    #[test]
    fn loading_new_specs_starts_over() {
        let mut wizard = wizard();
        wizard.select_all(false);
        wizard.set_specs(vec![load_text(SPEC).expect("valid")]);
        wizard.rebuild("");
        assert_eq!(wizard.selected_count(), 4);
    }

    #[test]
    fn several_specs_get_version_folders() {
        let mut wizard = Wizard::new(SourceKind::File);
        let v1 = load_text(&SPEC.replace("2.0.0", "1.4.0")).expect("valid");
        let mut v2 = load_text(SPEC).expect("valid");
        v2.source = "specs/payments-v2.yaml".to_string();
        wizard.set_specs(vec![v1.clone(), v2.clone()]);
        assert_eq!(wizard.version_labels(), vec!["v1", "v2"]);
        // Same major version, different titles: the titles.
        let mut other = v2.clone();
        other.spec.title = "Other API".into();
        wizard.set_specs(vec![v2.clone(), other.clone()]);
        assert_eq!(
            wizard.version_labels(),
            vec!["Acme Payments API", "Other API"]
        );
        // Same version and title: the file names.
        other.spec.title = v2.spec.title.clone();
        other.source = "other.yaml".into();
        wizard.set_specs(vec![v2.clone(), other]);
        assert_eq!(wizard.version_labels(), vec!["payments-v2", "other"]);
        // A disabled version leaves the plan.
        wizard.set_specs(vec![v1, v2]);
        wizard.toggle_version(0);
        wizard.rebuild("");
        assert_eq!(wizard.enabled_count(), 1);
        assert_eq!(wizard.operation_count(), 4);
        assert_eq!(wizard.total_count(), 4);
        assert_eq!(wizard.plan().expect("built").root.folders[0].name, "v2");
    }

    #[test]
    fn an_existing_import_turns_the_preview_into_an_update() {
        use postino_format::openapi::{Manifest, ManifestSource};
        let mut wizard = wizard();
        let old = wizard.plan().expect("built").clone();
        let source = ManifestSource {
            kind: "url".into(),
            location: "https://x/spec.yaml".into(),
            branch: String::new(),
        };
        let mut files = std::collections::BTreeMap::new();
        for key in ["GET /customers", "GET /charges", "DELETE /gone"] {
            files.insert(key.to_string(), format!("{key}.postino"));
        }
        let mut manifest = Manifest::new(
            "2026-10-28",
            source.clone(),
            &ImportOptions::default(),
            &old,
            &files,
        );
        manifest.requests.insert(
            "DELETE /gone".into(),
            postino_format::openapi::ManifestRequest {
                method: "DELETE".into(),
                path: "/gone".into(),
                ..Default::default()
            },
        );
        let collection = OpenApiCollection {
            folder: "Acme".into(),
            manifest,
        };
        assert!(
            find_existing(
                vec![collection.clone()],
                &ManifestSource {
                    kind: "paste".into(),
                    ..source.clone()
                }
            )
            .is_none()
        );
        let found = find_existing(vec![collection], &source).expect("the same source");

        wizard.set_existing(Some(found));
        wizard.rebuild("");
        assert!(wizard.is_update());
        // POST /customers and GET /customers/{id} were not imported: added. DELETE /gone: removed.
        assert_eq!(wizard.change_counts(), (2, 0, 1, 2));
        let kinds: Vec<ChangeKind> = wizard.changes().iter().map(|change| change.kind).collect();
        assert_eq!(
            kinds,
            vec![ChangeKind::Added, ChangeKind::Added, ChangeKind::Removed]
        );
        assert_eq!(
            wizard.selected_change_count(),
            2,
            "removed ones start unchecked"
        );
        wizard.toggle_change("DELETE /gone");
        wizard.rebuild("");
        assert_eq!(
            wizard.selected_change_count(),
            3,
            "the choice survives a rebuild"
        );
        wizard.set_update(false);
        assert!(!wizard.is_update());
    }

    #[test]
    fn invalid_text_is_an_error() {
        assert!(load_text("not: a spec").is_err());
        assert!(load_url("ftp://x", &Client::new(Default::default())).is_err());
    }

    #[test]
    fn relative_references_join_urls() {
        let base = "https://api.acme.dev/specs/v2/openapi.yaml?ref=main";
        assert_eq!(
            join_url(base, "schemas/pet.yaml"),
            "https://api.acme.dev/specs/v2/schemas/pet.yaml"
        );
        assert_eq!(
            join_url(base, "../common.yaml"),
            "https://api.acme.dev/specs/common.yaml"
        );
        assert_eq!(
            join_url(base, "/root.yaml"),
            "https://api.acme.dev/root.yaml"
        );
        assert_eq!(
            join_url(base, "//cdn.test/x.yaml"),
            "https://cdn.test/x.yaml"
        );
        assert_eq!(join_url(base, "http://other.test/y"), "http://other.test/y");
    }

    #[test]
    fn files_load_with_their_relative_references() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("api.yaml"),
            "openapi: 3.0.0\ninfo: {title: T, version: '1'}\npaths:\n  /x:\n    $ref: 'paths/x.yaml'\n",
        )
        .expect("write");
        std::fs::create_dir(dir.path().join("paths")).expect("mkdir");
        std::fs::write(
            dir.path().join("paths/x.yaml"),
            "get: {summary: Get x, responses: {}}\n",
        )
        .expect("write");
        let loaded = load_path(
            &dir.path().join("api.yaml"),
            "api.yaml",
            &Client::new(Default::default()),
        )
        .expect("loads");
        let loaded = &loaded.specs[0];
        assert_eq!(loaded.source, "api.yaml");
        assert_eq!(loaded.spec.operations[0].summary, "Get x");
        assert!(
            loaded.spec.warnings.is_empty(),
            "{:?}",
            loaded.spec.warnings
        );
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    fn spec_text(title: &str, version: &str) -> String {
        format!(
            "openapi: 3.0.0\ninfo: {{title: {title}, version: '{version}'}}\npaths:\n  /x:\n    get: {{responses: {{}}}}\n"
        )
    }

    /// A repository like the design's: a catalog pointing to two specs, a spec it does not
    /// list, a fragment, a broken spec and folders a scan skips.
    fn sample_tree(dir: &Path) {
        write(
            dir,
            "catalog-info.yaml",
            "apiVersion: backstage.io/v1alpha1\nkind: API\nmetadata: {name: v1}\nspec: {type: openapi, definition: {$text: ./specs/payments-v1.yaml}}\n---\napiVersion: backstage.io/v1alpha1\nkind: API\nmetadata: {name: v2}\nspec: {type: openapi, definition: {$text: ./specs/payments-v2.yaml}}\n",
        );
        write(
            dir,
            "specs/payments-v1.yaml",
            &spec_text("Payments", "1.0.0"),
        );
        write(
            dir,
            "specs/payments-v2.yaml",
            &spec_text("Payments", "2.0.0"),
        );
        write(
            dir,
            "other/users.json",
            "{\"openapi\": \"3.1.0\", \"info\": {\"title\": \"Users\", \"version\": \"1\"}, \"paths\": {}}",
        );
        write(dir, "specs/schemas/pet.yaml", "type: object\n");
        write(dir, "broken.yaml", "openapi: 4.0.0\n");
        write(
            dir,
            "node_modules/x/openapi.yaml",
            &spec_text("Dependency", "1"),
        );
        write(dir, ".hidden/openapi.yaml", &spec_text("Hidden", "1"));
    }

    fn sources(loaded: &Loaded) -> Vec<&str> {
        loaded
            .specs
            .iter()
            .map(|spec| spec.source.as_str())
            .collect()
    }

    #[test]
    fn catalogs_read_the_specs_they_point_to() {
        let dir = tempfile::tempdir().expect("tempdir");
        sample_tree(dir.path());
        let loaded = load_path(
            &dir.path().join("catalog-info.yaml"),
            "catalog-info.yaml",
            &Client::new(Default::default()),
        )
        .expect("a catalog");
        assert_eq!(
            sources(&loaded),
            vec!["specs/payments-v1.yaml", "specs/payments-v2.yaml"]
        );
        assert_eq!(
            loaded.catalogs,
            vec![CatalogFile {
                source: "catalog-info.yaml".into(),
                apis: 2
            }]
        );
    }

    #[test]
    fn folder_scans_find_every_spec_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        sample_tree(dir.path());
        let loaded = scan_folder(dir.path(), &Client::new(Default::default())).expect("scans");
        assert_eq!(
            sources(&loaded),
            vec![
                "specs/payments-v1.yaml",
                "specs/payments-v2.yaml",
                "other/users.json"
            ]
        );
        assert_eq!(loaded.failures.len(), 1);
        assert_eq!(loaded.failures[0].source, "broken.yaml");
        assert_eq!(loaded.catalogs.len(), 1);
    }

    #[test]
    fn repositories_are_cloned_and_scanned() {
        let Ok(git) = telaio_git::detect(&telaio_git::Executable::Auto) else {
            eprintln!("skipping, no git");
            return;
        };
        let repo = tempfile::tempdir().expect("tempdir");
        sample_tree(repo.path());
        let run = |args: &[&str]| {
            let output = std::process::Command::new(git.path())
                .args(args)
                .current_dir(repo.path())
                .output()
                .expect("git runs");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init", "--quiet", "--initial-branch=main"]);
        run(&["add", "."]);
        run(&[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "specs",
        ]);
        let url = format!("file://{}", repo.path().display());
        let client = Client::new(Default::default());
        let (checkout, loaded) = clone_repository(&git, &url, "", &client).expect("clones");
        assert_eq!(loaded.specs.len(), 3);
        let path = checkout.path().to_path_buf();
        assert!(path.exists());
        drop(checkout);
        assert!(!path.exists(), "the checkout is deleted with its guard");
        assert!(clone_repository(&git, &url, "missing", &client).is_err());
    }
}
