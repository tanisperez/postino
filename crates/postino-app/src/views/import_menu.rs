//! The "Import" dropdown in the sidebar's header: Postman collection and environment import, with
//! the result shown in an in-app dialog. Without a workspace open, an import first creates one
//! through the "New workspace" dialog (GitHub #89). The actual import call goes through
//! `state::import`, the same functions its own unit tests exercise.

use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_workspace::ImportReport;

use crate::state::import;
use crate::state::locale;
use crate::theme::PaletteExt;
use crate::theme::metrics::ICON_BUTTON_HEADER;

use super::new_workspace::{self, NewWorkspacePurpose};
use super::root::AppView;

/// Which kind of Postman export a picked file is, chosen by which menu item was clicked.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ImportKind {
    /// A `*.postman_collection.json` export.
    Collection,
    /// A `*.postman_environment.json` export.
    Environment,
}

impl AppView {
    /// Renders the "Import" dropdown button, an icon button styled like the sidebar header's
    /// `IconButton`s next to it. Enabled without a workspace too: the import then creates one
    /// ([`AppView::import_postman`]).
    pub(crate) fn render_import_menu(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let collection_weak = weak.clone();
        let environment_weak = weak;
        Button::new("import-menu")
            .ghost()
            .small()
            .size(px(ICON_BUTTON_HEADER))
            .icon(Icon::new(gpui_kit::assets::IconName::Download))
            .text_color(cx.palette().fg_muted)
            .tooltip(t!("shell.import.button"))
            .dropdown_menu(move |mut menu, _, _| {
                let weak = collection_weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!("shell.import.menu_collection")).on_click(
                        move |_, window, cx| {
                            pick_and_import(weak.clone(), ImportKind::Collection, window, cx);
                        },
                    ),
                );
                let weak = environment_weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!("shell.import.menu_environment")).on_click(
                        move |_, window, cx| {
                            pick_and_import(weak.clone(), ImportKind::Environment, window, cx);
                        },
                    ),
                );
                menu
            })
            .into_any_element()
    }
}

/// Opens a native file picker for a Postman export and imports the chosen file
/// ([`AppView::import_postman`]): into the open workspace, or into a new one when none is open.
/// A file that cannot be read shows in the usual error banner. Also called from the command
/// palette's "Import Postman collection..."/"Import Postman environment..." actions and the
/// welcome tab.
pub(crate) fn pick_and_import(
    view: WeakEntity<AppView>,
    kind: ImportKind,
    window: &mut Window,
    cx: &mut App,
) {
    // Captured now, while a `Window` is at hand: the import runs later, from inside an async
    // block that only has an `AsyncApp` and must look the window back up to open its dialogs.
    let window_handle = window.window_handle();
    let options = PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    };
    let receiver = cx.prompt_for_paths(options);
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(mut paths))) = receiver.await else {
            return;
        };
        let Some(path) = paths.pop() else {
            return;
        };
        let json = match std::fs::read_to_string(&path) {
            Ok(json) => json,
            Err(error) => {
                log::warn!("import: could not read {}: {error}", path.display());
                let _ = view.update(cx, |view, cx| {
                    view.workspace_error = Some(
                        t!(
                            "shell.import.read_failed",
                            path = path.display().to_string(),
                            error = error.to_string()
                        )
                        .into_owned(),
                    );
                    cx.notify();
                });
                return;
            }
        };
        let _ = cx.update_window(window_handle, move |_, window, cx| {
            let _ = view.update(cx, |view, cx| {
                view.import_postman(kind, json, path, window, cx);
            });
        });
    })
    .detach();
}

/// A Postman export picked while no workspace was open, waiting for the new workspace the "New
/// workspace" dialog creates for it (GitHub #89).
pub(crate) struct PendingImport {
    /// Which kind of export it is.
    pub(crate) kind: ImportKind,
    /// The export's JSON text, already read.
    pub(crate) json: String,
    /// The file it was read from, for the dialog's note and the log.
    pub(crate) source: PathBuf,
}

impl AppView {
    /// Imports the Postman export `json`, read from `source`. With a workspace open it goes into
    /// that workspace. Without one, the "New workspace" dialog opens with the export's name as
    /// the suggested folder name, and the import runs once that new, empty workspace is open. An
    /// export that does not parse shows in the error banner and creates nothing.
    pub(crate) fn import_postman(
        &mut self,
        kind: ImportKind,
        json: String,
        source: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.workspace.is_some() {
            self.import_into_workspace(kind, &json, &source, window, cx);
            return;
        }
        let name = match kind {
            ImportKind::Collection => import::postman_collection_name(&json),
            ImportKind::Environment => import::postman_environment_name(&json),
        };
        match name {
            Ok(name) => {
                let pending = PendingImport { kind, json, source };
                new_workspace::open_new_workspace_dialog(
                    cx.weak_entity(),
                    NewWorkspacePurpose::Import(pending),
                    Some(name),
                    window,
                    cx,
                );
            }
            Err(error) => {
                crate::state::workspace_log::log_workspace_error(
                    &format!("import {}", source.display()),
                    &error,
                );
                self.workspace_error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    /// Imports `json` into the open workspace, refreshes the sidebar and shows the import report
    /// in an in-app dialog, or a failure in the usual error banner.
    pub(crate) fn import_into_workspace(
        &mut self,
        kind: ImportKind,
        json: &str,
        source: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.state.workspace.as_mut() else {
            return;
        };
        let report = match kind {
            ImportKind::Collection => import::import_postman_collection(workspace, json),
            ImportKind::Environment => import::import_postman_environment(workspace, json),
        };
        match report {
            Ok(report) => {
                log::info!(
                    "imported {} as a Postman {}: {} files created, {} warnings",
                    source.display(),
                    match kind {
                        ImportKind::Collection => "collection",
                        ImportKind::Environment => "environment",
                    },
                    report.created_files.len(),
                    report.warnings.len(),
                );
                for warning in &report.warnings {
                    log::debug!("import warning: {warning}");
                }
                self.workspace_error = None;
                self.refresh_tree(cx);
                // Both kinds can write an environment: a collection's own variables become one.
                self.refresh_env_rows();
                show_import_report_dialog(kind, report, window, cx);
            }
            Err(error) => {
                crate::state::workspace_log::log_workspace_error(
                    &format!("import {}", source.display()),
                    &error,
                );
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }
}

/// Shows the created files and warnings of an import as an in-app alert dialog.
fn show_import_report_dialog(
    kind: ImportKind,
    report: ImportReport,
    window: &mut Window,
    cx: &mut App,
) {
    let title = match kind {
        ImportKind::Collection => t!("shell.import.collection_done"),
        ImportKind::Environment => t!("shell.import.environment_done"),
    }
    .into_owned();
    let mut description = locale::plural("shell.import.files_created", report.created_files.len());
    if !report.warnings.is_empty() {
        description.push_str("\n\n");
        description.push_str(&t!("shell.import.warnings"));
        for warning in &report.warnings {
            description.push_str("\n- ");
            description.push_str(warning);
        }
    }
    window.open_alert_dialog(cx, move |alert, _, _| {
        alert.title(title.clone()).description(description.clone())
    });
}
