//! The "Import" dropdown in the title bar: Postman collection and environment import, with the
//! result shown in an in-app dialog (`plans/mvp.md`, Phase 9). The actual import call goes
//! through `state::import`, the same functions its own unit tests exercise.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use postino_workspace::{ImportReport, WorkspaceError};

use crate::state::import;
use crate::theme::metrics::ENV_PILL_HEIGHT;

use super::root::AppView;

/// Which kind of Postman export a picked file is, chosen by which menu item was clicked.
#[derive(Debug, Clone, Copy)]
enum ImportKind {
    /// A `*.postman_collection.json` export.
    Collection,
    /// A `*.postman_environment.json` export.
    Environment,
}

impl AppView {
    /// Renders the "Import" dropdown button: disabled until a workspace is open, since both
    /// imports need somewhere to write files into.
    pub(crate) fn render_import_menu(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let has_workspace = self.state.workspace.is_some();
        let collection_weak = weak.clone();
        let environment_weak = weak;
        Button::new("import-menu")
            .ghost()
            .h(px(ENV_PILL_HEIGHT))
            .icon(Icon::new(gpui_kit::assets::IconName::Download).small())
            .label("Import")
            .disabled(!has_workspace)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                let weak = collection_weak.clone();
                menu = menu.item(PopupMenuItem::new("Postman collection...").on_click(
                    move |_, window, cx| {
                        pick_and_import(weak.clone(), ImportKind::Collection, window, cx);
                    },
                ));
                let weak = environment_weak.clone();
                menu = menu.item(PopupMenuItem::new("Postman environment...").on_click(
                    move |_, window, cx| {
                        pick_and_import(weak.clone(), ImportKind::Environment, window, cx);
                    },
                ));
                menu
            })
            .into_any_element()
    }
}

/// Opens a native file picker for a Postman export, imports the chosen file into the open
/// workspace, refreshes the sidebar, and shows the import report in an in-app dialog (or a
/// failure in the usual error banner).
fn pick_and_import(view: WeakEntity<AppView>, kind: ImportKind, window: &mut Window, cx: &mut App) {
    // Captured now, while a `Window` is at hand: the dialog showing the report is opened later,
    // from inside an async block that only has an `AsyncApp` and must look the window back up.
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
                let _ = view.update(cx, |view, cx| {
                    view.workspace_error =
                        Some(format!("could not read {}: {error}", path.display()));
                    cx.notify();
                });
                return;
            }
        };

        let outcome: Option<Result<ImportReport, WorkspaceError>> = view
            .update(cx, |view, cx| {
                let workspace = view.state.workspace.as_mut()?;
                let report = match kind {
                    ImportKind::Collection => import::import_postman_collection(workspace, &json),
                    ImportKind::Environment => import::import_postman_environment(workspace, &json),
                };
                if report.is_ok() {
                    view.workspace_error = None;
                    view.refresh_tree(cx);
                }
                cx.notify();
                Some(report)
            })
            .ok()
            .flatten();

        match outcome {
            Some(Ok(report)) => {
                let _ = cx.update_window(window_handle, move |_, window, cx| {
                    show_import_report_dialog(kind, report, window, cx);
                });
            }
            Some(Err(error)) => {
                let _ = view.update(cx, |view, cx| {
                    view.workspace_error = Some(error.to_string());
                    cx.notify();
                });
            }
            // No workspace open: cannot happen, the "Import" button is disabled without one.
            None => {}
        }
    })
    .detach();
}

/// Shows the created files and warnings of an import as an in-app alert dialog.
fn show_import_report_dialog(
    kind: ImportKind,
    report: ImportReport,
    window: &mut Window,
    cx: &mut App,
) {
    let title = match kind {
        ImportKind::Collection => "Postman collection imported",
        ImportKind::Environment => "Postman environment imported",
    };
    let mut description = format!("{} file(s) created.", report.created_files.len());
    if !report.warnings.is_empty() {
        description.push_str("\n\nWarnings:");
        for warning in &report.warnings {
            description.push_str("\n- ");
            description.push_str(warning);
        }
    }
    window.open_alert_dialog(cx, move |alert, _, _| {
        alert.title(title).description(description.clone())
    });
}
