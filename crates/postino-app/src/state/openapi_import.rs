//! The OpenAPI import wizard's state (GitHub #201): the source, the loaded specs, the options,
//! the plan they build and which of its requests are selected, plus the rows of the preview tree.
//! Plain Rust, so the wizard's logic is tested without a window; `views/openapi_import.rs`
//! renders it.
//!
//! Loading a spec reads a file or fetches a URL, and with it every document its `$ref`s point
//! to. It is blocking, the view runs it on the background executor.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use postino_core::{Method, ResolvedField, ResolvedRequest};
use postino_format::openapi::{
    ImportOptions, NoExternalRefs, OpenApiPlan, PlanFolder, RefLoader, Spec, SpecVersion,
    build_plan, parse_spec,
};
use postino_runner::Client;

/// Where the wizard reads the spec from (the source tabs of the first step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A spec file picked or dropped.
    File,
    /// A spec fetched from a URL.
    Url,
    /// Spec text pasted into the wizard.
    Paste,
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
    /// What the source list shows under the title: the file name or the URL, empty for pasted
    /// text.
    pub source: String,
}

/// The `Accept` header of a spec download: the YAML and JSON types servers use for specs.
const SPEC_ACCEPT: &str = "application/yaml, application/x-yaml, application/json;q=0.9, */*;q=0.8";

/// Reads the spec file at `path`. External references are read relative to it, or fetched
/// with `client` when they are URLs.
pub fn load_file(path: &Path, client: &Client) -> Result<LoadedSpec, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let location = path.to_string_lossy();
    let spec = parse_spec(&text, &location, &SourceLoader { client })
        .map_err(|error| error.to_string())?;
    Ok(LoadedSpec {
        spec,
        source: path.file_name().map_or_else(
            || location.to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
    })
}

/// Fetches the spec at `url` with `client`, and its external references relative to it.
pub fn load_url(url: &str, client: &Client) -> Result<LoadedSpec, String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("the URL must start with http:// or https://".to_string());
    }
    let text = fetch(client, url)?;
    let spec =
        parse_spec(&text, url, &SourceLoader { client }).map_err(|error| error.to_string())?;
    Ok(LoadedSpec {
        spec,
        source: url.to_string(),
    })
}

/// Reads pasted spec text, which has no location for external references.
pub fn load_text(text: &str) -> Result<LoadedSpec, String> {
    let spec = parse_spec(text, "", &NoExternalRefs).map_err(|error| error.to_string())?;
    Ok(LoadedSpec {
        spec,
        source: String::new(),
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
}

impl Wizard {
    /// A wizard on its first step, with `source` selected.
    pub fn new(source: SourceKind) -> Self {
        Wizard {
            step: Step::Source,
            source,
            options: ImportOptions::default(),
            specs: Vec::new(),
            plan: None,
            full_plan: None,
            excluded: HashSet::new(),
            closed: HashSet::new(),
            rows: Arc::from(Vec::new()),
            counts: (0, 0, 0),
        }
    }

    /// The loaded specs.
    pub fn specs(&self) -> &[LoadedSpec] {
        &self.specs
    }

    /// Replaces the loaded specs, which starts the selection over.
    pub fn set_specs(&mut self, specs: Vec<LoadedSpec>) {
        self.specs = specs;
        self.plan = None;
        self.full_plan = None;
        self.excluded.clear();
        self.closed.clear();
        self.rows = Arc::from(Vec::new());
        self.counts = (0, 0, 0);
    }

    /// The name the collection gets unless the user types another one: the first spec's title.
    pub fn suggested_name(&self) -> String {
        self.specs
            .first()
            .map(|loaded| loaded.spec.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "OpenAPI".to_string())
    }

    /// The version folder of each spec: none for a single spec, `v<major>` of the API version
    /// otherwise, or the spec's source when two specs would share one.
    fn version_labels(&self) -> Vec<String> {
        if self.specs.len() < 2 {
            return vec![String::new(); self.specs.len()];
        }
        let majors: Vec<String> = self
            .specs
            .iter()
            .map(|loaded| {
                let major = loaded.spec.api_version.trim_start_matches(['v', 'V']);
                let major = major.split('.').next().unwrap_or_default();
                format!("v{major}")
            })
            .collect();
        majors
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let shared = majors.iter().filter(|other| *other == label).count() > 1;
                if label == "v" || shared {
                    self.specs[index].source.clone()
                } else {
                    label.clone()
                }
            })
            .collect()
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
            .map(|(loaded, label)| SpecVersion {
                label,
                spec: &loaded.spec,
            })
            .collect();
        let name = if name.trim().is_empty() {
            self.suggested_name()
        } else {
            name.trim().to_string()
        };
        self.plan = Some(Arc::new(build_plan(&name, &versions, &self.options)));
        if self.full_plan.is_none() {
            self.full_plan = Some(build_plan(&name, &versions, &ImportOptions::default()));
        }
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
            .map(|loaded| loaded.spec.operations.len())
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
        v2.source = "payments-v2.yaml".to_string();
        wizard.set_specs(vec![v1.clone(), v2.clone()]);
        assert_eq!(wizard.version_labels(), vec!["v1", "v2"]);
        // Two specs of the same major version fall back to their source.
        wizard.set_specs(vec![
            v2.clone(),
            LoadedSpec {
                source: "other.yaml".into(),
                ..v2
            },
        ]);
        assert_eq!(
            wizard.version_labels(),
            vec!["payments-v2.yaml", "other.yaml"]
        );
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
        let loaded = load_file(
            &dir.path().join("api.yaml"),
            &Client::new(Default::default()),
        )
        .expect("loads");
        assert_eq!(loaded.source, "api.yaml");
        assert_eq!(loaded.spec.operations[0].summary, "Get x");
        assert!(
            loaded.spec.warnings.is_empty(),
            "{:?}",
            loaded.spec.warnings
        );
    }
}
