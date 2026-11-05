//! The "Import OpenAPI" wizard (GitHub #201, Claude Design "OpenAPI Import", 1a): three steps,
//! Source, Options and Preview, in one modal, then a Done step with the result. The logic lives
//! in `state/openapi_import.rs`; this renders it and runs the blocking parts (reading a file,
//! downloading a URL) on the background executor.
//!
//! Without a workspace open, Import closes the wizard and opens the "New workspace" dialog, which
//! runs the import once the new workspace is open, like a Postman import (GitHub #89).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_format::openapi::{ChangeKind, Grouping, ManifestSource, OpenApiPlan, PlanChange};
use postino_runner::Client;
use postino_workspace::{ImportOrigin, ImportReport, UpdateReport};
use telaio_ui::components::{CheckState, TextArea, TriCheckbox};
use telaio_ui::metrics::CHECKBOX_HALO_PADDING;

use crate::state::locale::plural;
use crate::state::number::format_integer;
use crate::state::openapi_import::{
    Check, Loaded, LoadedSpec, PreviewRow, RowKind, SourceKind, Step, Wizard, clone_repository,
    find_existing, load_path, load_text, load_url, scan_folder, source_kind_name, source_kind_of,
};
use crate::theme::metrics::{RADIUS_MD, RADIUS_SM};
use crate::theme::{Palette, PaletteExt};
use crate::views::components::{
    GhostButton, IconButton, InlineMessage, InlineMessageKind, MethodBadge, ModalPlacement,
    PrimaryButton, SecondaryButton, SectionLabel, TreeRow, framed_dialog, text_field,
};

use super::new_workspace::{self, NewWorkspacePurpose};
use super::root::AppView;

/// The card's size, from the design. Fixed, so the steps do not change the dialog's size.
const DIALOG_WIDTH: f32 = 780.0;
const DIALOG_HEIGHT: f32 = 600.0;
/// Horizontal padding of the header, the stepper, the body and the footer.
const PADDING_X: f32 = 20.0;
/// Size of a stepper dot.
const STEP_DOT: f32 = 22.0;
/// Height of a source tab.
const SOURCE_TAB_HEIGHT: f32 = 30.0;
/// Height of the file drop zone.
const DROP_ZONE_HEIGHT: f32 = 150.0;
/// Height of the paste text area.
const PASTE_HEIGHT: f32 = 200.0;
/// Width of the Git source's branch field, from the design.
const GIT_BRANCH_WIDTH: f32 = 140.0;
/// Height of a row of the preview tree and of the list of changes.
const ROW_HEIGHT: f32 = 28.0;
/// Width of the change kind column of the list of changes ("Added", "Removed").
const CHANGE_KIND_WIDTH: f32 = 72.0;
/// Widest a change's details get before they are cut.
const CHANGE_DETAIL_WIDTH: f32 = 240.0;
/// How many steps the stepper shows (Done is not one of them).
const STEP_COUNT: usize = 3;

/// A spec waiting for the new workspace the "New workspace" dialog creates for it.
pub(crate) struct PendingOpenApi {
    /// The plan to write.
    pub(crate) plan: Arc<OpenApiPlan>,
    /// The requests the user unchecked.
    pub(crate) excluded: HashSet<String>,
    /// The source, date and options, for the collection's manifest.
    pub(crate) origin: ImportOrigin,
}

/// The result shown by the Done step.
struct Done {
    count: usize,
    name: String,
    warnings: Vec<String>,
    /// `(added, changed, removed)` when the wizard updated an existing collection.
    updated: Option<(usize, usize, usize)>,
}

/// The wizard's entity: the plain state plus the inputs and what is being loaded.
pub(crate) struct ImportWizard {
    state: Wizard,
    view: WeakEntity<AppView>,
    /// Sends the spec downloads, with the TLS settings of requests. Built on the first load: the
    /// wizard may open from inside an `AppView` update, where `AppView` cannot be read.
    client: Option<Arc<Client>>,
    name: Entity<InputState>,
    url: Entity<InputState>,
    paste: Entity<TextareaState>,
    repository: Entity<InputState>,
    branch: Entity<InputState>,
    /// The folder of the Folder source.
    folder: Option<PathBuf>,
    /// The source of the specs loaded, for the manifest and to find an earlier import of it.
    source: Option<ManifestSource>,
    /// The git Postino uses, read on the first clone like the client.
    git: Option<Option<telaio_git::Git>>,
    /// The temporary checkout of the Git source, deleted when the wizard closes or the source
    /// changes.
    checkout: Option<tempfile::TempDir>,
    /// Whether a file is being read or a URL downloaded.
    loading: bool,
    /// Why the last load failed.
    error: Option<String>,
    /// Whether the name input still has to be filled from the loaded spec.
    name_pending: bool,
    /// Moves to the next step once the load in progress succeeds (Next on a URL or pasted text).
    advance_after_load: bool,
    done: Option<Done>,
    rows_scroll: UniformListScrollHandle,
    body_scroll: ScrollHandle,
    /// Holds the keyboard on the steps without an input, so Enter still reaches the dialog.
    focus: FocusHandle,
    /// The step the keyboard focus was last placed for.
    focused_step: Option<Step>,
}

impl ImportWizard {
    fn load(&mut self, kind: SourceKind, input: LoadInput, advance: bool, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        self.advance_after_load = advance;
        cx.notify();
        let client = match &self.client {
            Some(client) => client.clone(),
            None => {
                let options = self
                    .view
                    .upgrade()
                    .map(|view| view.read(cx).send_options.clone())
                    .unwrap_or_default();
                let client = Arc::new(Client::new(options));
                self.client = Some(client.clone());
                client
            }
        };
        if matches!(input, LoadInput::Git { .. }) && self.git.is_none() {
            let git = self
                .view
                .upgrade()
                .and_then(|view| view.read(cx).git.availability.git().cloned());
            self.git = Some(git);
        }
        let git = self.git.clone().flatten();
        let source = manifest_source(kind, &input);
        let task = cx.background_executor().spawn(async move {
            let single = |result: Result<LoadedSpec, String>| {
                result.map(|spec| {
                    (
                        None,
                        Loaded {
                            specs: vec![spec],
                            ..Loaded::default()
                        },
                    )
                })
            };
            match input {
                LoadInput::File(path) => {
                    let shown = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |name| name.to_string_lossy().into_owned(),
                    );
                    load_path(&path, &shown, &client).map(|loaded| (None, loaded))
                }
                LoadInput::Folder(path) => scan_folder(&path, &client).map(|loaded| (None, loaded)),
                LoadInput::Git { url, branch } => match git {
                    Some(git) => clone_repository(&git, &url, &branch, &client)
                        .map(|(checkout, loaded)| (Some(checkout), loaded)),
                    None => Err(t!("openapi.git.missing").into_owned()),
                },
                LoadInput::Url(url) => single(load_url(&url, &client)),
                LoadInput::Text(text) => single(load_text(&text)),
            }
        });
        cx.spawn(async move |wizard, cx| {
            let result = task.await;
            let _ = wizard.update(cx, |wizard, cx| wizard.loaded(kind, source, result, cx));
        })
        .detach();
    }

    fn loaded(
        &mut self,
        kind: SourceKind,
        source: ManifestSource,
        result: Result<(Option<tempfile::TempDir>, Loaded), String>,
        cx: &mut Context<Self>,
    ) {
        self.loading = false;
        // The user switched to another source meanwhile.
        if self.state.source != kind {
            cx.notify();
            return;
        }
        match result {
            Ok((checkout, loaded)) => {
                log::info!(
                    "OpenAPI import: read {} specs ({} operations), {} files failed, {} catalogs",
                    loaded.specs.len(),
                    loaded
                        .specs
                        .iter()
                        .map(|spec| spec.spec.operations.len())
                        .sum::<usize>(),
                    loaded.failures.len(),
                    loaded.catalogs.len()
                );
                let found = !loaded.specs.is_empty();
                if !found && loaded.failures.is_empty() {
                    self.error = Some(t!("openapi.none_found").into_owned());
                }
                self.checkout = checkout;
                self.state.set_loaded(loaded);
                self.name_pending = found;
                if found {
                    // An earlier import of the same source turns the Preview into an update.
                    let collections = self
                        .view
                        .upgrade()
                        .and_then(|view| {
                            view.read(cx)
                                .state
                                .workspace
                                .as_ref()
                                .map(|workspace| workspace.openapi_collections())
                        })
                        .unwrap_or_default();
                    self.state.set_existing(find_existing(collections, &source));
                    self.source = Some(source);
                }
                if self.advance_after_load && found {
                    self.state.step = Step::Options;
                }
            }
            Err(error) => {
                log::warn!("OpenAPI import: could not read the spec: {error}");
                self.error = Some(t!("openapi.read_failed", error = error).into_owned());
            }
        }
        self.advance_after_load = false;
        cx.notify();
    }

    fn select_source(&mut self, source: SourceKind, cx: &mut Context<Self>) {
        if self.state.source == source {
            return;
        }
        self.state.source = source;
        self.state.set_specs(Vec::new());
        self.checkout = None;
        self.error = None;
        cx.notify();
    }

    /// Clears what a source input loaded once its text changes.
    fn source_edited(&mut self, cx: &mut Context<Self>) {
        if !self.state.is_empty() || self.error.is_some() {
            self.state.set_specs(Vec::new());
            self.checkout = None;
            self.error = None;
            cx.notify();
        }
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().to_string();
        self.state.rebuild(&name);
        cx.notify();
    }

    /// Whether the primary button does something now.
    fn can_advance(&self, cx: &App) -> bool {
        if self.loading {
            return false;
        }
        match self.state.step {
            Step::Source => match self.state.source {
                SourceKind::File => !self.state.specs().is_empty(),
                SourceKind::Url => {
                    !self.state.specs().is_empty() || !self.url.read(cx).value().trim().is_empty()
                }
                SourceKind::Paste => {
                    !self.state.specs().is_empty() || !self.paste.read(cx).value().trim().is_empty()
                }
                SourceKind::Folder => !self.state.specs().is_empty(),
                SourceKind::Git => {
                    !self.state.specs().is_empty()
                        || !self.repository.read(cx).value().trim().is_empty()
                }
            },
            Step::Options => self.state.enabled_count() > 0,
            Step::Preview if self.state.is_update() => self.state.selected_change_count() > 0,
            Step::Preview => self.state.selected_count() > 0,
            Step::Done => true,
        }
    }
}

/// The manifest source of a load of `input` from the `kind` source.
fn manifest_source(kind: SourceKind, input: &LoadInput) -> ManifestSource {
    let canonical = |path: &PathBuf| {
        dunce::canonicalize(path)
            .unwrap_or_else(|_| path.clone())
            .to_string_lossy()
            .into_owned()
    };
    let (location, branch) = match input {
        LoadInput::File(path) | LoadInput::Folder(path) => (canonical(path), String::new()),
        LoadInput::Git { url, branch } => (url.trim().to_string(), branch.trim().to_string()),
        LoadInput::Url(url) => (url.trim().to_string(), String::new()),
        LoadInput::Text(_) => (String::new(), String::new()),
    };
    ManifestSource {
        kind: source_kind_name(kind).to_string(),
        location,
        branch,
    }
}

/// Today, as a manifest writes it.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// What to load.
enum LoadInput {
    File(PathBuf),
    Folder(PathBuf),
    Git { url: String, branch: String },
    Url(String),
    Text(String),
}

/// Opens the wizard on its first step with `source` selected.
pub(crate) fn open_openapi_wizard(
    view: WeakEntity<AppView>,
    source: SourceKind,
    window: &mut Window,
    cx: &mut App,
) {
    open_wizard(view, source, window, cx);
}

/// Opens the wizard on the source an earlier import recorded, and reads it right away: once
/// read, the Preview step offers to update the collection (GitHub #203). Pasted text cannot be
/// read again and opens nothing.
pub(crate) fn open_openapi_update(
    view: WeakEntity<AppView>,
    source: &ManifestSource,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(kind) = source_kind_of(&source.kind) else {
        return;
    };
    let wizard = open_wizard(view, kind, window, cx);
    let location = source.location.clone();
    let branch = source.branch.clone();
    wizard.update(cx, |wizard, cx| {
        let input = match kind {
            SourceKind::File => LoadInput::File(PathBuf::from(&location)),
            SourceKind::Folder => {
                wizard.folder = Some(PathBuf::from(&location));
                LoadInput::Folder(PathBuf::from(&location))
            }
            SourceKind::Url => {
                wizard.url.update(cx, |state, cx| {
                    state.set_value(location.clone(), window, cx)
                });
                LoadInput::Url(location)
            }
            SourceKind::Git => {
                wizard.repository.update(cx, |state, cx| {
                    state.set_value(location.clone(), window, cx)
                });
                wizard
                    .branch
                    .update(cx, |state, cx| state.set_value(branch.clone(), window, cx));
                LoadInput::Git {
                    url: location,
                    branch,
                }
            }
            SourceKind::Paste => return,
        };
        wizard.load(kind, input, false, cx);
    });
}

/// Builds the wizard on `source` and opens it.
fn open_wizard(
    view: WeakEntity<AppView>,
    source: SourceKind,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ImportWizard> {
    let name = cx.new(|cx| InputState::new(window, cx));
    let url = cx
        .new(|cx| InputState::new(window, cx).placeholder("https://api.example.com/openapi.yaml"));
    let paste =
        cx.new(|cx| TextareaState::new(window, cx).placeholder(t!("openapi.paste.placeholder")));
    let repository =
        cx.new(|cx| InputState::new(window, cx).placeholder("git@github.com:acme/payments.git"));
    let branch =
        cx.new(|cx| InputState::new(window, cx).placeholder(t!("openapi.git.branch_placeholder")));
    let wizard = cx.new(|cx| ImportWizard {
        state: Wizard::new(source),
        view: view.clone(),
        client: None,
        name: name.clone(),
        url: url.clone(),
        paste: paste.clone(),
        repository: repository.clone(),
        branch: branch.clone(),
        folder: None,
        source: None,
        git: None,
        checkout: None,
        loading: false,
        error: None,
        name_pending: false,
        advance_after_load: false,
        done: None,
        rows_scroll: UniformListScrollHandle::new(),
        body_scroll: ScrollHandle::new(),
        focus: cx.focus_handle(),
        focused_step: Some(Step::Source),
    });
    {
        // Weak, or the subscriptions would keep the wizard alive forever.
        let weak = wizard.downgrade();
        window
            .subscribe(&url, cx, move |_, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let _ = weak.update(cx, |wizard, cx| wizard.source_edited(cx));
                }
            })
            .detach();
        let weak = wizard.downgrade();
        window
            .subscribe(&paste, cx, move |_, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let _ = weak.update(cx, |wizard, cx| wizard.source_edited(cx));
                }
            })
            .detach();
        for input in [&repository, &branch] {
            let weak = wizard.downgrade();
            window
                .subscribe(input, cx, move |_, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let _ = weak.update(cx, |wizard, cx| wizard.source_edited(cx));
                    }
                })
                .detach();
        }
    }
    let dialog_wizard = wizard.clone();
    window.open_dialog(cx, move |dialog, window, _| {
        let wizard = dialog_wizard.clone();
        let enter = wizard.clone();
        // Enter is the primary button; the dialog stays open unless the wizard is done.
        let dialog = dialog.on_ok(move |_, window, cx| primary_action(&enter, window, cx));
        let wizard = wizard.clone();
        framed_dialog(
            dialog,
            DIALOG_WIDTH,
            Some(DIALOG_HEIGHT),
            ModalPlacement::Centered,
            window,
            move |window, cx| render_wizard(&wizard, window, cx),
        )
    });
    match source {
        SourceKind::Url => url.update(cx, |state, cx| state.focus(window, cx)),
        SourceKind::Git => repository.update(cx, |state, cx| state.focus(window, cx)),
        _ => {}
    }
    wizard
}

/// Runs the primary button (Next, Import or Close). Returns whether the dialog closes.
fn primary_action(wizard: &Entity<ImportWizard>, window: &mut Window, cx: &mut App) -> bool {
    if !wizard.read(cx).can_advance(cx) {
        return false;
    }
    let step = wizard.read(cx).state.step;
    match step {
        Step::Source => {
            wizard.update(cx, |wizard, cx| {
                if !wizard.state.specs().is_empty() {
                    wizard.state.step = Step::Options;
                    cx.notify();
                    return;
                }
                match wizard.state.source {
                    SourceKind::File | SourceKind::Folder => {}
                    SourceKind::Git => {
                        let url = wizard.repository.read(cx).value().to_string();
                        let branch = wizard.branch.read(cx).value().to_string();
                        wizard.load(SourceKind::Git, LoadInput::Git { url, branch }, true, cx);
                    }
                    SourceKind::Url => {
                        let url = wizard.url.read(cx).value().to_string();
                        wizard.load(SourceKind::Url, LoadInput::Url(url), true, cx);
                    }
                    SourceKind::Paste => {
                        let text = wizard.paste.read(cx).value().to_string();
                        wizard.load(SourceKind::Paste, LoadInput::Text(text), true, cx);
                    }
                }
            });
            false
        }
        Step::Options => {
            wizard.update(cx, |wizard, cx| {
                wizard.rebuild(cx);
                wizard.state.step = Step::Preview;
                cx.notify();
            });
            false
        }
        Step::Preview => run_import(wizard, window, cx),
        Step::Done => true,
    }
}

/// Imports the selected requests. Into the open workspace the wizard moves to Done; without one
/// it closes and the "New workspace" dialog takes over.
fn run_import(wizard: &Entity<ImportWizard>, window: &mut Window, cx: &mut App) -> bool {
    let (view, plan, excluded, origin, update) = {
        let wizard = wizard.read(cx);
        let Some(plan) = wizard.state.plan().cloned() else {
            return false;
        };
        let origin = ImportOrigin {
            source: wizard.source.clone().unwrap_or_else(|| ManifestSource {
                kind: "paste".to_string(),
                ..ManifestSource::default()
            }),
            date: today(),
            options: wizard.state.options,
        };
        let update = wizard.state.is_update().then(|| {
            let folder = wizard
                .state
                .existing()
                .map(|existing| existing.folder.clone())
                .unwrap_or_default();
            let skipped: HashSet<String> = wizard
                .state
                .changes()
                .iter()
                .filter(|change| !wizard.state.is_change_selected(&change.key))
                .map(|change| change.key.clone())
                .collect();
            (folder, skipped)
        });
        (
            wizard.view.clone(),
            plan,
            wizard.state.excluded().clone(),
            origin,
            update,
        )
    };
    let Some(app) = view.upgrade() else {
        return true;
    };
    if let Some((folder, skipped)) = update {
        let result = app.update(cx, |app, cx| {
            app.update_openapi(&folder, &plan, &skipped, &origin, cx)
        });
        wizard.update(cx, |wizard, cx| {
            match result {
                Ok(report) => {
                    wizard.done = Some(Done {
                        count: report.added + report.changed + report.removed,
                        name: folder.clone(),
                        warnings: report.warnings,
                        updated: Some((report.added, report.changed, report.removed)),
                    });
                    wizard.state.step = Step::Done;
                }
                Err(error) => wizard.error = Some(error),
            }
            cx.notify();
        });
        return false;
    }
    if app.read(cx).state.workspace.is_none() {
        let name = plan.root.name.clone();
        window.close_dialog(cx);
        new_workspace::open_new_workspace_dialog(
            view,
            NewWorkspacePurpose::OpenApi(PendingOpenApi {
                plan,
                excluded,
                origin,
            }),
            Some(name),
            window,
            cx,
        );
        return false;
    }
    let result = app.update(cx, |app, cx| {
        app.import_openapi(&plan, &excluded, &origin, cx)
    });
    wizard.update(cx, |wizard, cx| {
        match result {
            Ok(report) => {
                wizard.done = Some(Done {
                    count: report
                        .created_files
                        .iter()
                        .filter(|id| id.ends_with(".postino"))
                        .count(),
                    name: plan.root.name.clone(),
                    warnings: report.warnings,
                    updated: None,
                });
                wizard.state.step = Step::Done;
            }
            Err(error) => wizard.error = Some(error),
        }
        cx.notify();
    });
    false
}

impl AppView {
    /// Writes `plan` into the open workspace, without the `excluded` requests, then refreshes
    /// the sidebar with the new collection open and the environments.
    pub(crate) fn import_openapi(
        &mut self,
        plan: &OpenApiPlan,
        excluded: &HashSet<String>,
        origin: &ImportOrigin,
        cx: &mut Context<Self>,
    ) -> Result<ImportReport, String> {
        let Some(workspace) = self.state.workspace.as_mut() else {
            return Err("no workspace is open".to_string());
        };
        match workspace.import_openapi(plan, &|key| !excluded.contains(key), origin) {
            Ok(report) => {
                log::info!(
                    "imported an OpenAPI spec: {} files created, {} warnings",
                    report.created_files.len(),
                    report.warnings.len()
                );
                for warning in &report.warnings {
                    log::debug!("import warning: {warning}");
                }
                let collection = report
                    .created_files
                    .iter()
                    .find(|id| id.ends_with(".postino"))
                    .and_then(|id| id.split('/').next())
                    .map(str::to_string);
                self.workspace_error = None;
                self.refresh_tree_with(cx, |expansion| {
                    if let Some(collection) = collection {
                        expansion.insert(collection, true);
                    }
                });
                self.refresh_environments();
                cx.notify();
                Ok(report)
            }
            Err(error) => {
                crate::state::workspace_log::log_workspace_error("OpenAPI import", &error);
                Err(error.to_string())
            }
        }
    }

    /// Updates the collection in `folder` from `plan`, applying every change but the `skipped`
    /// ones, then refreshes the sidebar with the collection open.
    pub(crate) fn update_openapi(
        &mut self,
        folder: &str,
        plan: &OpenApiPlan,
        skipped: &HashSet<String>,
        origin: &ImportOrigin,
        cx: &mut Context<Self>,
    ) -> Result<UpdateReport, String> {
        let Some(workspace) = self.state.workspace.as_mut() else {
            return Err("no workspace is open".to_string());
        };
        match workspace.update_openapi(folder, plan, &|key| !skipped.contains(key), origin) {
            Ok(report) => {
                log::info!(
                    "updated an OpenAPI collection: {} added, {} changed, {} removed, {} warnings",
                    report.added,
                    report.changed,
                    report.removed,
                    report.warnings.len()
                );
                self.workspace_error = None;
                let folder = folder.to_string();
                self.refresh_tree_with(cx, |expansion| {
                    expansion.insert(folder, true);
                });
                cx.notify();
                Ok(report)
            }
            Err(error) => {
                crate::state::workspace_log::log_workspace_error("OpenAPI update", &error);
                Err(error.to_string())
            }
        }
    }
}

fn render_wizard(wizard: &Entity<ImportWizard>, window: &mut Window, cx: &mut App) -> AnyElement {
    // The name input is filled from the spec once, when the Options step first shows it.
    if wizard.read(cx).name_pending && wizard.read(cx).state.step == Step::Options {
        let (name, suggested) = {
            let wizard = wizard.read(cx);
            (wizard.name.clone(), wizard.state.suggested_name())
        };
        name.update(cx, |state, cx| state.set_value(suggested, window, cx));
        wizard.update(cx, |wizard, cx| {
            wizard.name_pending = false;
            wizard.rebuild(cx);
        });
    }
    let step = wizard.read(cx).state.step;
    // A new step moves the keyboard focus: the input it removed may have had it, and without a
    // focused element inside the dialog Enter would not reach it.
    if wizard.read(cx).focused_step != Some(step) {
        let (name, focus) = {
            let wizard = wizard.read(cx);
            (wizard.name.clone(), wizard.focus.clone())
        };
        match step {
            Step::Options => name.update(cx, |state, cx| state.focus(window, cx)),
            _ => window.focus(&focus, cx),
        }
        wizard.update(cx, |wizard, _| wizard.focused_step = Some(step));
    }
    let palette = cx.palette();
    let body = match step {
        Step::Source => render_source(wizard, &palette, cx),
        Step::Options => render_options(wizard, &palette, cx),
        Step::Preview => render_preview(wizard, &palette, cx),
        Step::Done => render_done(wizard, &palette, cx),
    };
    let scrolls = matches!(step, Step::Source | Step::Options | Step::Done);
    let body_scroll = wizard.read(cx).body_scroll.clone();
    let content = div()
        .id("openapi-body")
        .size_full()
        .px(px(PADDING_X))
        .py(px(18.0))
        .child(body)
        .when(scrolls, |content| {
            content.overflow_y_scroll().track_scroll(&body_scroll)
        });
    let focus = wizard.read(cx).focus.clone();
    v_flex()
        .size_full()
        .track_focus(&focus)
        .text_size(px(13.0))
        .text_color(palette.fg)
        .child(render_header(step, &palette))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(content)
                .when(scrolls, |area| {
                    area.child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(Scrollbar::vertical(&body_scroll)),
                    )
                }),
        )
        .child(render_footer(wizard, &palette, cx))
        .into_any_element()
}

/// The title row and the stepper, over one bottom border.
fn render_header(step: Step, palette: &Palette) -> AnyElement {
    let labels: [SharedString; STEP_COUNT] = [
        t!("openapi.step.source").into(),
        t!("openapi.step.options").into(),
        t!("openapi.step.preview").into(),
    ];
    let current = match step {
        Step::Source => 1,
        Step::Options => 2,
        Step::Preview => 3,
        Step::Done => 4,
    };
    let stepper = h_flex()
        .gap(px(10.0))
        .px(px(PADDING_X))
        .pb(px(14.0))
        .children(labels.into_iter().enumerate().map(|(index, label)| {
            let number = index + 1;
            let done = current > number;
            let active = current == number;
            let last = number == STEP_COUNT;
            let dot = div()
                .size(px(STEP_DOT))
                .flex_none()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .map(|dot| {
                    if active {
                        dot.bg(palette.accent)
                            .text_color(palette.accent_fg)
                            .child(number.to_string())
                    } else if done {
                        dot.bg(palette.border).child(
                            Icon::new(IconName::Check)
                                .size(px(12.0))
                                .text_color(palette.accent_text),
                        )
                    } else {
                        dot.text_color(palette.fg_subtle).child(number.to_string())
                    }
                });
            h_flex()
                .gap(px(12.0))
                .when(!last, |row| row.flex_1())
                .child(
                    h_flex().flex_none().gap(px(8.0)).child(dot).child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if active || done {
                                palette.fg
                            } else {
                                palette.fg_subtle
                            })
                            .child(label),
                    ),
                )
                .when(!last, |row| {
                    row.child(div().flex_1().h(px(1.0)).bg(if done {
                        palette.accent
                    } else {
                        palette.border
                    }))
                })
        }));
    v_flex()
        .flex_none()
        .border_b_1()
        .border_color(palette.border)
        .child(
            h_flex()
                .gap(px(10.0))
                .px(px(PADDING_X))
                .pt(px(14.0))
                .pb(px(10.0))
                .child(
                    Icon::new(IconName::FileCode)
                        .size(px(16.0))
                        .text_color(palette.accent_text),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t!("openapi.title")),
                )
                .child(
                    IconButton::new("openapi-close", IconName::X)
                        .large()
                        .tooltip(t!("common.close"))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
        )
        .child(stepper)
        .into_any_element()
}

/// The footer: a note at the left, Back and the primary button at the right.
fn render_footer(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let step = state.state.step;
    let selected = state.state.selected_count();
    let note: SharedString = match step {
        Step::Source => t!("openapi.supported").into(),
        Step::Options if state.state.specs().len() > 1 => t!(
            &telaio_app::language::plural_key("openapi.from_versions", state.state.enabled_count()),
            operations = plural("openapi.operations", state.state.operation_count()),
            count = format_integer(state.state.enabled_count() as u64)
        )
        .into(),
        Step::Options => plural("openapi.operations", state.state.operation_count()).into(),
        Step::Preview if state.state.is_update() => {
            plural("openapi.selected", state.state.selected_change_count()).into()
        }
        Step::Preview => plural("openapi.selected", selected).into(),
        Step::Done => SharedString::default(),
    };
    let label: SharedString = match step {
        Step::Source if state.loading && state.advance_after_load => match state.state.source {
            SourceKind::Url => t!("openapi.url.fetching").into(),
            SourceKind::Git => t!("openapi.git.cloning").into(),
            _ => t!("openapi.file.reading").into(),
        },
        Step::Source | Step::Options => t!("openapi.next").into(),
        Step::Preview if state.state.is_update() => {
            plural("openapi.update.button", state.state.selected_change_count()).into()
        }
        Step::Preview => plural("openapi.import", selected).into(),
        Step::Done => t!("common.close").into(),
    };
    let can_advance = state.can_advance(cx);
    let back = wizard.downgrade();
    let next = wizard.clone();
    h_flex()
        .flex_none()
        .gap(px(8.0))
        .px(px(PADDING_X))
        .py(px(12.0))
        .border_t_1()
        .border_color(palette.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.0))
                .text_color(palette.fg_subtle)
                .child(note),
        )
        .when(matches!(step, Step::Options | Step::Preview), |footer| {
            footer.child(
                GhostButton::new("openapi-back", t!("openapi.back")).on_click(move |_, _, cx| {
                    let _ = back.update(cx, |wizard, cx| {
                        wizard.state.step = match wizard.state.step {
                            Step::Preview => Step::Options,
                            _ => Step::Source,
                        };
                        cx.notify();
                    });
                }),
            )
        })
        .child(
            PrimaryButton::new("openapi-next", label)
                .disabled(!can_advance)
                .on_click(move |_, window, cx| {
                    if primary_action(&next, window, cx) {
                        window.close_dialog(cx);
                    }
                }),
        )
        .into_any_element()
}

/// Step 1: the source tabs, the input of the selected source, the error and what was found.
fn render_source(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let source = state.state.source;
    let tabs = [
        (
            SourceKind::File,
            t!("openapi.source.file"),
            IconName::FileUp,
        ),
        (SourceKind::Url, t!("openapi.source.url"), IconName::Link),
        (
            SourceKind::Paste,
            t!("openapi.source.paste"),
            IconName::Clipboard,
        ),
        (
            SourceKind::Folder,
            t!("openapi.source.folder"),
            IconName::Folder,
        ),
        (
            SourceKind::Git,
            t!("openapi.source.git"),
            IconName::GitBranch,
        ),
    ];
    let tab_row = h_flex()
        .gap(px(6.0))
        .flex_wrap()
        .children(tabs.into_iter().map(|(kind, label, icon)| {
            let selected = kind == source;
            let weak = wizard.downgrade();
            let (rest, hover, pressed) = if selected {
                (
                    palette.raised,
                    palette.hover_over(palette.raised),
                    palette.pressed_over(palette.raised),
                )
            } else {
                (
                    gpui_kit::transparent_black(),
                    palette.hover,
                    palette.pressed,
                )
            };
            h_flex()
                .id(SharedString::from(format!("openapi-source-{kind:?}")))
                .h(px(SOURCE_TAB_HEIGHT))
                .gap(px(6.0))
                .px(px(12.0))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(if selected {
                    palette.accent
                } else {
                    palette.border
                })
                .bg(rest)
                .text_color(if selected {
                    palette.fg
                } else {
                    palette.fg_muted
                })
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .active(move |style| style.bg(pressed))
                .child(Icon::new(icon).size(px(14.0)))
                .child(label)
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |wizard, cx| {
                        wizard.select_source(kind, cx);
                        // The input of the new source takes the keyboard, as the design's focus ring.
                        match kind {
                            SourceKind::Url => {
                                wizard.url.update(cx, |state, cx| state.focus(window, cx))
                            }
                            SourceKind::Paste => {
                                wizard.paste.update(cx, |state, cx| state.focus(window, cx));
                            }
                            SourceKind::Git => {
                                wizard
                                    .repository
                                    .update(cx, |state, cx| state.focus(window, cx));
                            }
                            SourceKind::File | SourceKind::Folder => {}
                        }
                    });
                })
        }));

    let input = match source {
        SourceKind::File => render_drop_zone(wizard, palette, cx),
        SourceKind::Url => render_url_input(wizard, palette, cx),
        SourceKind::Paste => v_flex()
            .gap(px(6.0))
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(12.0))
                    .child(
                        TextArea::new(&state.paste)
                            .h(px(PASTE_HEIGHT))
                            .disabled(state.loading),
                    ),
            )
            .child(hint(t!("openapi.paste.hint"), palette))
            .into_any_element(),
        SourceKind::Folder => render_folder_input(wizard, palette, cx),
        SourceKind::Git => render_git_input(wizard, palette, cx),
    };

    v_flex()
        .gap(px(16.0))
        .child(tab_row)
        .child(input)
        .children(
            state
                .error
                .clone()
                .map(|error| InlineMessage::new(InlineMessageKind::Danger, error)),
        )
        .when(!state.state.is_empty(), |column| {
            column.child(render_found(&state.state, palette, cx))
        })
        .into_any_element()
}

fn hint(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .text_size(px(12.0))
        .text_color(palette.fg_subtle)
        .child(text.into())
}

fn field_label(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .text_size(px(12.0))
        .text_color(palette.fg_muted)
        .child(text.into())
}

/// The dashed zone that takes a dropped file, or opens the file picker on a click.
fn render_drop_zone(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let loading = wizard.read(cx).loading;
    let pick = wizard.downgrade();
    let drop = wizard.downgrade();
    let (hover, pressed, accent_subtle) = (palette.hover, palette.pressed, palette.accent_subtle);
    let (border_strong, accent) = (palette.fg_subtle, palette.accent);
    v_flex()
        .id("openapi-drop-zone")
        .h(px(DROP_ZONE_HEIGHT))
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .rounded(px(10.0))
        .border_1()
        .border_dashed()
        .border_color(palette.border_strong)
        .text_color(palette.fg_muted)
        .when(!loading, |zone| {
            zone.cursor_pointer()
                .hover(move |style| style.bg(hover).border_color(border_strong))
                .active(move |style| style.bg(pressed))
                .drag_over::<ExternalPaths>(move |style, _, _, _| {
                    style.bg(accent_subtle).border_color(accent)
                })
                .on_drop(move |paths: &ExternalPaths, _, cx| {
                    if let Some(path) = paths.paths().first().cloned() {
                        let _ = drop.update(cx, |wizard, cx| {
                            wizard.load(SourceKind::File, LoadInput::File(path), false, cx);
                        });
                    }
                })
                .on_click(move |_, _, cx| pick_file(pick.clone(), cx))
        })
        .child(Icon::new(IconName::Upload).size(px(22.0)))
        .map(|zone| {
            if loading {
                zone.child(t!("openapi.file.reading"))
            } else {
                zone.child(
                    h_flex().gap(px(4.0)).child(t!("openapi.file.drop")).child(
                        div()
                            .text_color(palette.accent_text)
                            .child(t!("openapi.file.browse")),
                    ),
                )
                .child(hint(t!("openapi.file.types"), palette))
            }
        })
        .into_any_element()
}

/// Opens the native file picker and reads the picked spec.
fn pick_file(wizard: WeakEntity<ImportWizard>, cx: &mut App) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(mut paths))) = receiver.await else {
            return;
        };
        let Some(path) = paths.pop() else {
            return;
        };
        let _ = wizard.update(cx, |wizard, cx| {
            wizard.load(SourceKind::File, LoadInput::File(path), false, cx);
        });
    })
    .detach();
}

fn render_url_input(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let fetch = wizard.downgrade();
    let empty = state.url.read(cx).value().trim().is_empty();
    v_flex()
        .gap(px(6.0))
        .child(field_label(t!("openapi.url.label"), palette))
        .child(
            h_flex()
                .gap(px(8.0))
                .child(div().flex_1().min_w_0().child(text_field(&state.url, cx)))
                .child(
                    SecondaryButton::new(
                        "openapi-fetch",
                        if state.loading {
                            t!("openapi.url.fetching")
                        } else {
                            t!("openapi.url.fetch")
                        },
                    )
                    .disabled(state.loading || empty)
                    .on_click(move |_, _, cx| {
                        let _ = fetch.update(cx, |wizard, cx| {
                            let url = wizard.url.read(cx).value().to_string();
                            wizard.load(SourceKind::Url, LoadInput::Url(url), false, cx);
                        });
                    }),
                ),
        )
        .child(hint(t!("openapi.url.hint"), palette))
        .into_any_element()
}

/// What the source gave: its catalog files, then one card per spec read and per file that
/// could not be read.
fn render_found(state: &Wizard, palette: &Palette, cx: &App) -> AnyElement {
    let mono = cx.theme().mono_font_family.clone();
    let card = || {
        h_flex()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(palette.border)
    };
    let source_line = |text: String| {
        div()
            .truncate()
            .font_family(mono.clone())
            .text_size(px(11.0))
            .text_color(palette.fg_subtle)
            .child(text)
    };
    let catalogs = state.catalogs().iter().map(|catalog| {
        h_flex()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(RADIUS_MD))
            .bg(palette.raised)
            .child(
                Icon::new(IconName::FileText)
                    .size(px(15.0))
                    .text_color(palette.fg_muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(mono.clone())
                    .text_size(px(12.0))
                    .child(catalog.source.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.0))
                    .text_color(palette.fg_muted)
                    .child(plural("openapi.catalog", catalog.apis)),
            )
    });
    let specs = state.specs().iter().map(|loaded| {
        let spec = &loaded.spec;
        let format = if spec.spec_version == "2.0" {
            "Swagger 2.0".to_string()
        } else {
            format!("OpenAPI {}", spec.spec_version)
        };
        let meta = format!(
            "{format} · {}",
            plural("openapi.operations", spec.operations.len())
        );
        let title = if spec.api_version.is_empty() {
            spec.title.clone()
        } else {
            format!("{} {}", spec.title, spec.api_version)
        };
        card()
            .child(
                Icon::new(IconName::CircleCheck)
                    .size(px(15.0))
                    .text_color(palette.success),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(div().truncate().child(title))
                    .when(!loaded.source.is_empty(), |column| {
                        column.child(source_line(loaded.source.clone()))
                    }),
            )
            .child(
                v_flex()
                    .flex_none()
                    .items_end()
                    .gap(px(2.0))
                    .text_size(px(12.0))
                    .text_color(palette.fg_muted)
                    .child(meta)
                    .when(!spec.warnings.is_empty(), |column| {
                        column.child(
                            div()
                                .text_color(palette.warning)
                                .child(plural("openapi.warnings", spec.warnings.len())),
                        )
                    }),
            )
    });
    let failures = state.failures().iter().map(|failure| {
        card()
            .child(
                Icon::new(IconName::CircleAlert)
                    .size(px(15.0))
                    .text_color(palette.danger),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(source_line(failure.source.clone()).text_color(palette.fg_muted))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(palette.danger)
                            .child(failure.error.clone()),
                    ),
            )
    });
    v_flex()
        .gap(px(8.0))
        .children(catalogs)
        .child(
            div()
                .pt(px(4.0))
                .child(SectionLabel::new(t!("openapi.found"))),
        )
        .children(specs)
        .children(failures)
        .into_any_element()
}

/// The Folder source: the chosen folder and the button that opens the folder picker.
fn render_folder_input(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let pick = wizard.downgrade();
    let shown: SharedString = match &state.folder {
        Some(folder) => {
            crate::state::format::shorten_path(folder, dirs::home_dir().as_deref()).into()
        }
        None => t!("openapi.folder.none").into(),
    };
    v_flex()
        .gap(px(6.0))
        .child(
            h_flex()
                .gap(px(8.0))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .h(px(telaio_ui::components::text_field::FIELD_HEIGHT))
                        .gap(px(8.0))
                        .px(px(10.0))
                        .rounded(px(RADIUS_MD))
                        .bg(palette.surface)
                        .border_1()
                        .border_color(palette.border)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(12.0))
                        .text_color(if state.folder.is_some() {
                            palette.fg
                        } else {
                            palette.fg_subtle
                        })
                        .child(
                            Icon::new(IconName::Folder)
                                .size(px(14.0))
                                .text_color(palette.fg_muted),
                        )
                        .child(div().truncate().child(shown)),
                )
                .child(
                    SecondaryButton::new(
                        "openapi-choose-folder",
                        if state.loading {
                            t!("openapi.folder.scanning")
                        } else {
                            t!("openapi.folder.choose")
                        },
                    )
                    .disabled(state.loading)
                    .on_click(move |_, _, cx| pick_folder(pick.clone(), cx)),
                ),
        )
        .child(hint(t!("openapi.folder.hint"), palette))
        .into_any_element()
}

/// Opens the native folder picker and scans the picked folder.
fn pick_folder(wizard: WeakEntity<ImportWizard>, cx: &mut App) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(mut paths))) = receiver.await else {
            return;
        };
        let Some(path) = paths.pop() else {
            return;
        };
        let _ = wizard.update(cx, |wizard, cx| {
            wizard.folder = Some(path.clone());
            wizard.load(SourceKind::Folder, LoadInput::Folder(path), false, cx);
        });
    })
    .detach();
}

/// The Git source: the repository, the branch and the Clone button.
fn render_git_input(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let clone = wizard.downgrade();
    let empty = state.repository.read(cx).value().trim().is_empty();
    v_flex()
        .gap(px(6.0))
        .child(
            h_flex()
                .items_start()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(6.0))
                        .child(field_label(t!("openapi.git.repository"), palette))
                        .child(text_field(&state.repository, cx)),
                )
                .child(
                    v_flex()
                        .w(px(GIT_BRANCH_WIDTH))
                        .flex_none()
                        .gap(px(6.0))
                        .child(field_label(t!("openapi.git.branch"), palette))
                        .child(text_field(&state.branch, cx)),
                )
                .child(
                    v_flex()
                        .flex_none()
                        .gap(px(6.0))
                        // An empty label line, so the button lines up with the fields.
                        .child(field_label(" ", palette))
                        .child(
                            SecondaryButton::new(
                                "openapi-clone",
                                if state.loading {
                                    t!("openapi.git.cloning")
                                } else {
                                    t!("openapi.git.clone")
                                },
                            )
                            .disabled(state.loading || empty)
                            .on_click(move |_, _, cx| {
                                let _ = clone.update(cx, |wizard, cx| {
                                    let url = wizard.repository.read(cx).value().to_string();
                                    let branch = wizard.branch.read(cx).value().to_string();
                                    wizard.load(
                                        SourceKind::Git,
                                        LoadInput::Git { url, branch },
                                        false,
                                        cx,
                                    );
                                });
                            }),
                        ),
                ),
        )
        .child(hint(t!("openapi.git.hint"), palette))
        .into_any_element()
}

/// Step 2: the collection name and folders on the left, what to generate on the right.
fn render_options(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let options = state.state.options;
    let mono = cx.theme().mono_font_family.clone();

    let groupings = [
        (
            Grouping::PathSegments,
            t!("openapi.options.group_path"),
            state
                .state
                .sample_path()
                .unwrap_or("/customers/{id}")
                .to_string(),
        ),
        (
            Grouping::Tags,
            t!("openapi.options.group_tags"),
            state.state.sample_tag().unwrap_or("Customers").to_string(),
        ),
    ];
    let folders = v_flex()
        .gap(px(4.0))
        .child(
            div()
                .pb(px(4.0))
                .child(SectionLabel::new(t!("openapi.options.folders"))),
        )
        .children(groupings.into_iter().map(|(grouping, label, sample)| {
            let weak = wizard.downgrade();
            h_flex()
                .gap(px(10.0))
                .py(px(6.0))
                .child(
                    telaio_ui::components::labelled_radio(
                        SharedString::from(format!("openapi-group-{grouping:?}")),
                        options.grouping == grouping,
                        label,
                        palette,
                    )
                    .on_change(move |_, _, cx| {
                        let _ = weak.update(cx, |wizard, cx| {
                            wizard.state.options.grouping = grouping;
                            cx.notify();
                        });
                    }),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(mono.clone())
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(sample),
                )
        }));

    let labels = state.state.version_labels();
    let versions = (state.state.specs().len() > 1).then(|| {
        v_flex()
            .gap(px(4.0))
            .child(
                div()
                    .pb(px(4.0))
                    .child(SectionLabel::new(t!("openapi.options.versions"))),
            )
            .children(
                state
                    .state
                    .specs()
                    .iter()
                    .enumerate()
                    .map(|(index, loaded)| {
                        let weak = wizard.downgrade();
                        let file = loaded.source.rsplit('/').next().unwrap_or_default();
                        let label = if file.is_empty() {
                            labels[index].clone()
                        } else {
                            format!("{} · {file}", labels[index])
                        };
                        let format = if loaded.spec.spec_version == "2.0" {
                            "Swagger 2.0".to_string()
                        } else {
                            loaded.spec.spec_version.clone()
                        };
                        h_flex()
                            .gap(px(10.0))
                            .py(px(6.0))
                            .child(
                                div().min_w_0().child(
                                    telaio_ui::components::labelled_checkbox(
                                        SharedString::from(format!("openapi-version-{index}")),
                                        state.state.is_enabled(index),
                                        label,
                                        palette,
                                    )
                                    .on_change(
                                        move |_, _, cx| {
                                            let _ = weak.update(cx, |wizard, cx| {
                                                wizard.state.toggle_version(index);
                                                wizard.rebuild(cx);
                                            });
                                        },
                                    ),
                                ),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(12.0))
                                    .text_color(palette.fg_subtle)
                                    .child(format!(
                                        "{format} · {}",
                                        plural("openapi.operations", loaded.spec.operations.len())
                                    )),
                            )
                    }),
            )
    });

    let left = v_flex()
        .gap(px(16.0))
        .child(
            v_flex()
                .gap(px(6.0))
                .child(field_label(t!("openapi.options.name"), palette))
                .child(text_field(&state.name, cx)),
        )
        .children(versions)
        .child(folders);

    let base_urls = state.state.base_urls();
    let env_sub: SharedString = match base_urls.split_first() {
        Some((first, [])) => t!("openapi.gen.env_sub", url = first).into(),
        Some((first, rest)) => t!(
            &telaio_app::language::plural_key("openapi.gen.env_more", rest.len()),
            url = first,
            count = format_integer(rest.len() as u64)
        )
        .into(),
        None => t!("openapi.gen.env_sub", url = "http://localhost").into(),
    };
    let variables = state.state.auth_variables();
    let auth_sub: SharedString = if variables.is_empty() {
        t!("openapi.gen.auth_none").into()
    } else {
        let list: Vec<String> = variables
            .iter()
            .map(|name| format!("{{{{{name}}}}}"))
            .collect();
        t!("openapi.gen.auth_sub", variables = list.join(", ")).into()
    };
    type Toggle = fn(&mut postino_format::openapi::ImportOptions) -> &mut bool;
    let generate: [(&str, SharedString, SharedString, bool, Toggle); 5] = [
        (
            "env",
            t!("openapi.gen.env").into(),
            env_sub,
            options.environments,
            |o| &mut o.environments,
        ),
        (
            "auth",
            t!("openapi.gen.auth").into(),
            auth_sub,
            options.auth,
            |o| &mut o.auth,
        ),
        (
            "bodies",
            t!("openapi.gen.bodies").into(),
            t!("openapi.gen.bodies_sub").into(),
            options.bodies,
            |o| &mut o.bodies,
        ),
        (
            "tests",
            t!("openapi.gen.tests").into(),
            t!("openapi.gen.tests_sub").into(),
            options.tests,
            |o| &mut o.tests,
        ),
        (
            "docs",
            t!("openapi.gen.docs").into(),
            t!("openapi.gen.docs_sub").into(),
            options.docs,
            |o| &mut o.docs,
        ),
    ];
    let right = v_flex()
        .gap(px(4.0))
        .child(
            div()
                .pb(px(4.0))
                .child(SectionLabel::new(t!("openapi.options.generate"))),
        )
        .children(
            generate
                .into_iter()
                .map(|(id, label, sub, checked, field)| {
                    let toggle = {
                        let weak = wizard.downgrade();
                        move |cx: &mut App| {
                            let _ = weak.update(cx, |wizard, cx| {
                                let flag = field(&mut wizard.state.options);
                                *flag = !*flag;
                                cx.notify();
                            });
                        }
                    };
                    described_checkbox(id, checked, label, sub, toggle, palette)
                }),
        );

    h_flex()
        .items_start()
        .gap(px(20.0))
        .child(div().flex_1().min_w_0().child(left))
        .child(div().flex_1().min_w_0().child(right))
        .into_any_element()
}

/// A checkbox with a label and a description under it. Like telaio's `labelled_checkbox`, the
/// halo lights up under the pointer anywhere on the row, and a click on the text toggles it.
fn described_checkbox(
    id: &str,
    checked: bool,
    label: SharedString,
    description: SharedString,
    toggle: impl Fn(&mut App) + Clone + 'static,
    palette: &Palette,
) -> AnyElement {
    let group = SharedString::from(format!("openapi-gen-{id}-group"));
    let (hover, pressed) = (palette.pressed, palette.border_strong);
    let box_toggle = toggle.clone();
    h_flex()
        .group(group.clone())
        .items_start()
        .gap(px(10.0))
        .py(px(6.0))
        .child(
            div().pt(px(1.0)).child(
                Checkbox::new(SharedString::from(format!("openapi-gen-{id}")))
                    .checked(checked)
                    .p(px(CHECKBOX_HALO_PADDING))
                    .m(px(-CHECKBOX_HALO_PADDING))
                    .rounded(px(RADIUS_SM))
                    .group_hover(group.clone(), move |style| style.bg(hover))
                    .group_active(group.clone(), move |style| style.bg(pressed))
                    .on_click(move |_, _, cx| box_toggle(cx)),
            ),
        )
        .child(
            v_flex()
                .id(SharedString::from(format!("openapi-gen-{id}-label")))
                .flex_1()
                .min_w_0()
                .gap(px(2.0))
                .cursor_pointer()
                .child(label)
                .child(
                    div()
                        .text_size(px(12.0))
                        .line_height(relative(1.4))
                        .text_color(palette.fg_subtle)
                        .child(description),
                )
                .on_click(move |_, _, cx| toggle(cx)),
        )
        .into_any_element()
}

/// Step 3: the counts, the tree of requests to pick from and what else is written.
fn render_preview(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let Some(existing) = state.state.existing() else {
        return render_tree_preview(wizard, palette, cx);
    };
    let update = state.state.is_update();
    let choice = |id: &'static str, label: SharedString, selected: bool, value: bool| {
        let weak = wizard.downgrade();
        choice_pill(id, label, selected, palette).on_click(move |_, _, cx| {
            let _ = weak.update(cx, |wizard, cx| {
                wizard.state.set_update(value);
                cx.notify();
            });
        })
    };
    v_flex()
        .size_full()
        .gap(px(12.0))
        .child(
            h_flex()
                .flex_none()
                .gap(px(10.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(RADIUS_MD))
                .bg(palette.raised)
                .child(
                    Icon::new(IconName::Info)
                        .size(px(15.0))
                        .text_color(palette.accent_text),
                )
                .child(div().flex_1().min_w_0().child(t!(
                    "openapi.update.imported",
                    name = existing.folder.clone(),
                    date = existing.manifest.imported.clone()
                ))),
        )
        .child(
            h_flex()
                .flex_none()
                .gap(px(6.0))
                .child(choice(
                    "openapi-update-existing",
                    t!("openapi.update.existing").into(),
                    update,
                    true,
                ))
                .child(choice(
                    "openapi-update-new",
                    t!("openapi.update.new").into(),
                    !update,
                    false,
                )),
        )
        .child(div().flex_1().min_h_0().w_full().map(|area| {
            if update {
                area.child(render_changes(wizard, palette, cx))
            } else {
                area.child(render_tree_preview(wizard, palette, cx))
            }
        }))
        .into_any_element()
}

/// A pill button of a two way choice, styled like the source buttons of the first step.
fn choice_pill(
    id: &'static str,
    label: SharedString,
    selected: bool,
    palette: &Palette,
) -> Stateful<Div> {
    let (rest, hover, pressed) = if selected {
        (
            palette.raised,
            palette.hover_over(palette.raised),
            palette.pressed_over(palette.raised),
        )
    } else {
        (
            gpui_kit::transparent_black(),
            palette.hover,
            palette.pressed,
        )
    };
    h_flex()
        .id(id)
        .h(px(SOURCE_TAB_HEIGHT - 2.0))
        .px(px(12.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(if selected {
            palette.accent
        } else {
            palette.border
        })
        .bg(rest)
        .text_color(if selected {
            palette.fg
        } else {
            palette.fg_muted
        })
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .active(move |style| style.bg(pressed))
        .child(label)
}

/// The changes since the existing collection was imported, each with its checkbox (design 1e).
fn render_changes(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let (added, changed, removed, unchanged) = state.state.change_counts();
    let changes = state.state.changes();
    let count = changes.len();
    let note = div()
        .flex_none()
        .text_size(px(12.0))
        .line_height(relative(1.5))
        .text_color(palette.fg_subtle)
        .child(t!("openapi.update.note"));
    let summary = div()
        .flex_none()
        .text_size(px(12.0))
        .text_color(palette.fg_muted)
        .child(t!(
            "openapi.update.summary",
            added = format_integer(added as u64),
            changed = format_integer(changed as u64),
            removed = format_integer(removed as u64),
            unchanged = format_integer(unchanged as u64)
        ));
    if count == 0 {
        return v_flex()
            .size_full()
            .gap(px(12.0))
            .child(summary)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(palette.fg_muted)
                    .child(t!("openapi.update.up_to_date")),
            )
            .into_any_element();
    }
    let selected: Vec<bool> = changes
        .iter()
        .map(|change| state.state.is_change_selected(&change.key))
        .collect();
    let weak = wizard.downgrade();
    let row_palette = palette.clone();
    let mono = cx.theme().mono_font_family.clone();
    let list = uniform_list("openapi-changes", count, move |range, _, _| {
        range
            .map(|index| {
                render_change(
                    index,
                    &changes[index],
                    selected[index],
                    &weak,
                    &row_palette,
                    &mono,
                )
            })
            .collect::<Vec<_>>()
    })
    .track_scroll(&state.rows_scroll)
    .size_full();
    let rows_scroll = state.rows_scroll.clone();
    v_flex()
        .size_full()
        .gap(px(12.0))
        .child(summary)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .py(px(6.0))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(palette.border)
                .bg(palette.bg)
                .child(list)
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(Scrollbar::vertical(&rows_scroll)),
                ),
        )
        .child(note)
        .into_any_element()
}

/// One change: its checkbox, what happened, the method, the path and what changed. The whole
/// row toggles it.
fn render_change(
    index: usize,
    change: &PlanChange,
    selected: bool,
    wizard: &WeakEntity<ImportWizard>,
    palette: &Palette,
    mono: &SharedString,
) -> AnyElement {
    let (label, color) = match change.kind {
        ChangeKind::Added => (t!("openapi.update.added"), palette.success),
        ChangeKind::Changed => (t!("openapi.update.changed"), palette.warning),
        _ => (t!("openapi.update.removed"), palette.danger),
    };
    let detail = if change.kind == ChangeKind::Removed && !selected {
        t!("openapi.update.kept").into_owned()
    } else {
        change.details.join(", ")
    };
    let toggle = |weak: WeakEntity<ImportWizard>, key: String| {
        move |cx: &mut App| {
            let _ = weak.update(cx, |wizard, cx| {
                wizard.state.toggle_change(&key);
                cx.notify();
            });
        }
    };
    let box_toggle = toggle(wizard.clone(), change.key.clone());
    let row_toggle = toggle(wizard.clone(), change.key.clone());
    let state = if selected {
        CheckState::Checked
    } else {
        CheckState::Unchecked
    };
    let method = change
        .method
        .parse::<postino_core::Method>()
        .unwrap_or_default();
    h_flex()
        .id(("openapi-change", index))
        .w_full()
        .h(px(ROW_HEIGHT))
        .gap(px(10.0))
        .px(px(12.0))
        .text_size(px(12.0))
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover))
        .active(|style| style.bg(palette.pressed))
        .on_click(move |_, _, cx| row_toggle(cx))
        .child(
            TriCheckbox::new(("openapi-change-check", index), state)
                .on_click(move |_, _, cx| box_toggle(cx)),
        )
        .child(
            div()
                .flex_none()
                .w(px(CHANGE_KIND_WIDTH))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(label),
        )
        .child(div().flex_none().child(MethodBadge::label(method)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(mono.clone())
                .child(change.path.clone()),
        )
        .child(
            div()
                .flex_none()
                .max_w(px(CHANGE_DETAIL_WIDTH))
                .truncate()
                .text_color(palette.fg_subtle)
                .child(detail),
        )
        .into_any_element()
}

/// The new collection's tree of requests, with the counts and what else is written.
fn render_tree_preview(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let state = wizard.read(cx);
    let total = state.state.total_count();
    let selected = state.state.selected_count();
    let summary = format!(
        "{} · {}",
        t!(
            &telaio_app::language::plural_key("openapi.preview.requests", total),
            selected = format_integer(selected as u64),
            count = format_integer(total as u64)
        ),
        plural("openapi.preview.folders", state.state.folder_count())
    );
    let link = |id: &'static str, label: SharedString, all: bool| {
        let weak = wizard.downgrade();
        div()
            .id(id)
            .text_size(px(12.0))
            .text_color(palette.accent_text)
            .cursor_pointer()
            .hover(|style| style.underline())
            .active(|style| style.opacity(0.7))
            .child(label)
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |wizard, cx| {
                    wizard.state.select_all(all);
                    cx.notify();
                });
            })
    };

    let rows = state.state.rows();
    let count = rows.len();
    let weak = wizard.downgrade();
    let row_palette = palette.clone();
    let mono = cx.theme().mono_font_family.clone();
    let list = uniform_list("openapi-preview-rows", count, move |range, _, _| {
        range
            .map(|index| {
                // A row spans the list, so its hover and its count at the end reach the edge.
                div()
                    .w_full()
                    .child(render_row(index, &rows[index], &weak, &row_palette, &mono))
            })
            .collect::<Vec<_>>()
    })
    .track_scroll(&state.rows_scroll)
    .size_full();
    let rows_scroll = state.rows_scroll.clone();

    let plan = state.state.plan();
    let environments: Vec<String> = plan
        .map(|plan| {
            plan.environments
                .iter()
                .filter(|environment| !environment.base.is_empty())
                .map(|environment| environment.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let secrets: Vec<String> = plan
        .and_then(|plan| plan.environments.first())
        .map(|environment| {
            environment
                .secret
                .iter()
                .map(|entry| entry.key.clone())
                .collect()
        })
        .unwrap_or_default();
    let mut env_text = if environments.is_empty() {
        t!("openapi.preview.no_environment").into_owned()
    } else {
        t!(
            &telaio_app::language::plural_key("openapi.preview.environments", environments.len()),
            count = format_integer(environments.len() as u64),
            names = environments.join(", ")
        )
        .into_owned()
    };
    if !secrets.is_empty() {
        env_text.push_str(" · ");
        env_text.push_str(&plural("openapi.preview.auth", secrets.len()));
    }

    v_flex()
        .size_full()
        .gap(px(12.0))
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(palette.fg_muted)
                        .child(summary),
                )
                .child(link(
                    "openapi-select-all",
                    t!("openapi.preview.select_all").into(),
                    true,
                ))
                .child(link(
                    "openapi-select-none",
                    t!("openapi.preview.select_none").into(),
                    false,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .py(px(6.0))
                .px(px(4.0))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(palette.border)
                .bg(palette.bg)
                .child(list)
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(Scrollbar::vertical(&rows_scroll)),
                ),
        )
        .child(
            h_flex()
                .flex_none()
                .gap(px(10.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(RADIUS_MD))
                .bg(palette.raised)
                .child(
                    Icon::new(IconName::Layers)
                        .size(px(15.0))
                        .text_color(palette.fg_muted),
                )
                .child(div().flex_1().min_w_0().truncate().child(env_text))
                .when(!secrets.is_empty(), |card| {
                    card.child(
                        div()
                            .flex_none()
                            .max_w(px(260.0))
                            .truncate()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(px(12.0))
                            .text_color(palette.fg_subtle)
                            .child(format!("{} → .local.env", secrets.join(", "))),
                    )
                }),
        )
        .into_any_element()
}

/// One row of the preview tree. A folder's row opens and collapses it, a request's row toggles
/// it; the checkbox of a folder selects everything under it.
fn render_row(
    index: usize,
    row: &PreviewRow,
    wizard: &WeakEntity<ImportWizard>,
    palette: &Palette,
    mono: &SharedString,
) -> AnyElement {
    let id = row.id.clone();
    let tree_row = TreeRow::new(("openapi-row", index), row.name.clone()).depth(row.depth);
    match &row.kind {
        RowKind::Folder {
            open,
            check,
            selected,
            total,
        } => {
            let check_id = id.clone();
            let check_weak = wizard.clone();
            let checkbox = TriCheckbox::new(("openapi-row-check", index), check_state(*check))
                .on_click(move |_, _, cx| {
                    let _ = check_weak.update(cx, |wizard, cx| {
                        wizard.state.toggle_folder(&check_id);
                        cx.notify();
                    });
                });
            let weak = wizard.clone();
            tree_row
                .folder(*open)
                .leading(
                    h_flex().gap(px(6.0)).child(checkbox).child(
                        Icon::new(if *open {
                            IconName::FolderOpen
                        } else {
                            IconName::Folder
                        })
                        .size(px(14.0))
                        .text_color(palette.fg_muted),
                    ),
                )
                .suffix(
                    div()
                        .pr(px(8.0))
                        .font_family(mono.clone())
                        .text_size(px(11.0))
                        .text_color(palette.fg_subtle)
                        .child(format!(
                            "{}/{}",
                            format_integer(*selected as u64),
                            format_integer(*total as u64)
                        )),
                )
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |wizard, cx| {
                        wizard.state.toggle_open(&id);
                        cx.notify();
                    });
                })
                .into_any_element()
        }
        RowKind::Request { method, checked } => {
            let check_id = id.clone();
            let check_weak = wizard.clone();
            let state = if *checked {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            };
            let checkbox =
                TriCheckbox::new(("openapi-row-check", index), state).on_click(move |_, _, cx| {
                    let _ = check_weak.update(cx, |wizard, cx| {
                        wizard.state.toggle_request(&check_id);
                        cx.notify();
                    });
                });
            let weak = wizard.clone();
            tree_row
                .leading(
                    h_flex()
                        .gap(px(6.0))
                        .child(checkbox)
                        .child(MethodBadge::label(method.clone())),
                )
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |wizard, cx| {
                        wizard.state.toggle_request(&id);
                        cx.notify();
                    });
                })
                .into_any_element()
        }
    }
}

fn check_state(check: Check) -> CheckState {
    match check {
        Check::All => CheckState::Checked,
        Check::Some => CheckState::Mixed,
        Check::None => CheckState::Unchecked,
    }
}

/// The Done step: what was imported, and the warnings.
fn render_done(wizard: &Entity<ImportWizard>, palette: &Palette, cx: &App) -> AnyElement {
    let Some(done) = &wizard.read(cx).done else {
        return div().into_any_element();
    };
    v_flex()
        .min_h_full()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .child(
            Icon::new(IconName::CircleCheck)
                .size(px(32.0))
                .text_color(palette.success),
        )
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_center()
                .child(match done.updated {
                    Some((added, changed, removed)) => t!(
                        "openapi.done.updated",
                        name = done.name.clone(),
                        added = format_integer(added as u64),
                        changed = format_integer(changed as u64),
                        removed = format_integer(removed as u64)
                    )
                    .into_owned(),
                    None => t!(
                        &telaio_app::language::plural_key("openapi.done.title", done.count),
                        count = format_integer(done.count as u64),
                        name = done.name.clone()
                    )
                    .into_owned(),
                }),
        )
        .child(
            div()
                .text_color(palette.fg_muted)
                .child(t!("openapi.done.note")),
        )
        .when(!done.warnings.is_empty(), |column| {
            column.child(
                v_flex()
                    .w_full()
                    .mt(px(8.0))
                    .gap(px(4.0))
                    .px(px(12.0))
                    .py(px(10.0))
                    .rounded(px(RADIUS_MD))
                    .bg(palette.warning_subtle)
                    .text_size(px(12.0))
                    .child(
                        div()
                            .text_color(palette.warning)
                            .child(t!("openapi.done.warnings")),
                    )
                    .children(done.warnings.iter().map(|warning| {
                        div()
                            .text_color(palette.fg_muted)
                            .child(format!("- {warning}"))
                    })),
            )
        })
        .into_any_element()
}

/// Shows the result of an import that ran after the "New workspace" dialog, when the wizard is
/// already closed: the same title and warnings as the Done step, in an alert.
pub(crate) fn show_report(name: &str, report: &ImportReport, window: &mut Window, cx: &mut App) {
    let count = report
        .created_files
        .iter()
        .filter(|id| id.ends_with(".postino"))
        .count();
    let title = t!(
        &telaio_app::language::plural_key("openapi.done.title", count),
        count = format_integer(count as u64),
        name = name
    )
    .into_owned();
    let mut text = t!("openapi.done.note").into_owned();
    if !report.warnings.is_empty() {
        text.push_str("\n\n");
        text.push_str(&t!("openapi.done.warnings"));
        for warning in &report.warnings {
            text.push_str("\n- ");
            text.push_str(warning);
        }
    }
    crate::views::components::open_confirm_dialog(
        window,
        cx,
        crate::views::components::ConfirmDialog {
            title: title.into(),
            text: text.into(),
            confirm_label: t!("common.ok").into(),
            destructive: false,
            show_cancel: false,
            on_confirm: std::rc::Rc::new(|_, _| {}),
        },
    );
}
