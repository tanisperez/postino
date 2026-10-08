//! The "New workspace" dialog (GitHub #88): a name, a location with a "Change..." button that
//! opens the native folder picker, and the full path that will be created. Create makes the
//! folder with `postino_workspace::create_workspace`, with the example request and environment,
//! and opens it as the workspace. Opened from the workspace switcher menu and the command
//! palette, and by a Postman import started without a workspace (`views/import_menu.rs`, GitHub
//! #89), which gets an empty folder and imports into it instead.
//!
//! An in-app dialog, never a native "save" one: those are meant for files and behave differently
//! between platforms when they create a folder.

use std::path::{Path, PathBuf};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_workspace::{
    EXAMPLE_ENVIRONMENT, EXAMPLE_REQUEST_ID, NewWorkspaceContent, WorkspaceError, create_workspace,
};

use crate::state;
use crate::state::workspace_log::log_workspace_error;
use crate::theme::PaletteExt;
use crate::theme::metrics::RADIUS_MD;
use crate::views::components::text_field::FIELD_HEIGHT;
use crate::views::components::{
    InlineMessage, InlineMessageKind, PrimaryButton, SecondaryButton, text_field,
};

use super::import_menu::PendingImport;
use super::root::AppView;

/// Width of the dialog.
const DIALOG_WIDTH: f32 = 480.0;

/// Longest path, in characters, the dialog shows. Longer ones get their middle elided, so the
/// folder being created stays visible at the end.
const PATH_MAX_CHARS: usize = 46;

/// What the new workspace is for.
pub(crate) enum NewWorkspacePurpose {
    /// A fresh start: the example request and environment.
    Example,
    /// An empty folder that receives this Postman export once it is open.
    Import(PendingImport),
}

impl NewWorkspacePurpose {
    /// What `create_workspace` writes for this purpose.
    fn content(&self) -> NewWorkspaceContent {
        match self {
            NewWorkspacePurpose::Example => NewWorkspaceContent::Example,
            NewWorkspacePurpose::Import(_) => NewWorkspaceContent::Empty,
        }
    }
}

/// The dialog's state. Owned by the dialog's builder closures, so it ends with the dialog.
struct NewWorkspaceForm {
    /// The folder name the user types.
    name: Entity<InputState>,
    /// The folder the new workspace goes in, the Documents folder until "Change..." picks
    /// another one.
    location: PathBuf,
    /// Why the last Create failed, until the name or the location changes.
    error: Option<String>,
    /// What the workspace is for, taken once it has been created.
    purpose: Option<NewWorkspacePurpose>,
}

/// Opens the dialog for `purpose`, with `suggested_name` already typed (the export's name for an
/// import).
pub(crate) fn open_new_workspace_dialog(
    view: WeakEntity<AppView>,
    purpose: NewWorkspacePurpose,
    suggested_name: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let name = cx.new(|cx| {
        let state =
            InputState::new(window, cx).placeholder(t!("shell.new_workspace.name_placeholder"));
        match suggested_name {
            Some(suggested) => state.default_value(suggested),
            None => state,
        }
    });
    let form = cx.new(|_| NewWorkspaceForm {
        name: name.clone(),
        location: state::new_workspace::default_location_for_user(),
        error: None,
        purpose: Some(purpose),
    });
    {
        // Weak, or the subscription would keep the form, and through it the input, alive forever.
        let form = form.downgrade();
        window
            .subscribe(&name, cx, move |_, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let _ = form.update(cx, |form, cx| {
                        if form.error.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            })
            .detach();
    }
    window.open_dialog(cx, move |dialog, _, cx| {
        let content_form = form.clone();
        let can_create = target_of(&form, cx).is_some();
        let create_form = form.clone();
        let create_view = view.clone();
        let enter_form = form.clone();
        let enter_view = view.clone();
        dialog
            .title(t!("shell.new_workspace.title"))
            .w(px(DIALOG_WIDTH))
            .content(move |content, _, cx| content.child(render_form(&content_form, cx)))
            // Enter anywhere in the dialog, the name field included, is the dialog's own Confirm
            // action. Returning `false` keeps the dialog open to show why Create failed.
            .on_ok(move |_, window, cx| confirm(enter_view.clone(), &enter_form, window, cx))
            .footer(
                h_flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        SecondaryButton::new("new-workspace-cancel", t!("common.cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        PrimaryButton::new("new-workspace-create", t!("common.create"))
                            .disabled(!can_create)
                            .on_click(move |_, window, cx| {
                                if confirm(create_view.clone(), &create_form, window, cx) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
    });
    // Focus only after `open_dialog`, which captures the focused handle to restore on close
    // (see `views/command_palette.rs`). Selecting a suggested name makes typing replace it.
    name.update(cx, |state, cx| {
        state.focus(window, cx);
        state.select_all(window, cx);
    });
}

/// The folder the form's name creates in its location, `None` while the name is blank.
fn target_of(form: &Entity<NewWorkspaceForm>, cx: &App) -> Option<PathBuf> {
    let form = form.read(cx);
    state::new_workspace::target(&form.location, &form.name.read(cx).value())
}

/// The dialog's body: a short note, the name field, the location with its "Change..." button,
/// the path that will be created and the last error.
fn render_form(form: &Entity<NewWorkspaceForm>, cx: &App) -> AnyElement {
    let palette = cx.palette();
    let mono_font = cx.theme().mono_font_family.clone();
    let home = dirs::home_dir();
    let target = target_of(form, cx);
    let state = form.read(cx);
    let shown = |path: &Path| {
        state::format::elide_path(
            &state::format::shorten_path(path, home.as_deref()),
            PATH_MAX_CHARS,
        )
    };
    let location = shown(&state.location);
    let change_form = form.downgrade();
    let note = match &state.purpose {
        Some(NewWorkspacePurpose::Import(pending)) => {
            let file = pending.source.file_name().map_or_else(
                || pending.source.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            t!("shell.new_workspace.import_note", file = file)
        }
        _ => t!("shell.new_workspace.example_note"),
    };

    let label = |text: SharedString| {
        div()
            .text_size(px(12.0))
            .text_color(palette.fg_muted)
            .child(text)
    };

    v_flex()
        .gap(px(14.0))
        .child(div().text_color(palette.fg_muted).child(note))
        .child(
            v_flex()
                .gap(px(6.0))
                .child(label(t!("shell.new_workspace.name").into()))
                .child(text_field(&state.name, cx)),
        )
        .child(
            v_flex()
                .gap(px(6.0))
                .child(label(t!("shell.new_workspace.location").into()))
                .child(
                    h_flex()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(FIELD_HEIGHT))
                                .flex()
                                .items_center()
                                .px(px(10.0))
                                .rounded(px(RADIUS_MD))
                                .bg(palette.surface)
                                .border_1()
                                .border_color(palette.border)
                                .font_family(mono_font.clone())
                                .text_size(px(12.0))
                                .text_color(palette.fg_muted)
                                .child(div().truncate().child(location)),
                        )
                        .child(
                            SecondaryButton::new(
                                "new-workspace-change",
                                t!("shell.new_workspace.change"),
                            )
                            .height(FIELD_HEIGHT)
                            .on_click(move |_, _, cx| pick_location(change_form.clone(), cx)),
                        ),
                ),
        )
        .children(target.map(|target| {
            h_flex()
                .gap(px(6.0))
                .text_size(px(12.0))
                .text_color(palette.fg_subtle)
                .child(div().flex_none().child(t!("shell.new_workspace.target")))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(mono_font)
                        .child(shown(&target)),
                )
        }))
        .children(
            state
                .error
                .clone()
                .map(|error| InlineMessage::new(InlineMessageKind::Danger, error)),
        )
        .into_any_element()
}

/// Opens the native folder picker and, once the user picks a folder, makes it the location.
fn pick_location(form: WeakEntity<NewWorkspaceForm>, cx: &mut App) {
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
        let Some(location) = paths.pop() else {
            return;
        };
        let _ = form.update(cx, |form, cx| {
            form.location = location;
            form.error = None;
            cx.notify();
        });
    })
    .detach();
}

/// Creates the folder and opens it. Returns whether the dialog can close: a target that cannot
/// be created keeps it open with the reason.
fn confirm(
    view: WeakEntity<AppView>,
    form: &Entity<NewWorkspaceForm>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some(target) = target_of(form, cx) else {
        return false;
    };
    let Some(content) = form
        .read(cx)
        .purpose
        .as_ref()
        .map(NewWorkspacePurpose::content)
    else {
        return false;
    };
    if let Err(error) = create_workspace(&target, content) {
        log_workspace_error("create workspace", &error);
        // The path is already on screen, in the "Creates" line.
        let message = match error {
            WorkspaceError::NotEmpty(_) => t!("shell.new_workspace.not_empty"),
            error => t!("shell.new_workspace.failed", error = error.to_string()),
        };
        form.update(cx, |form, cx| {
            form.error = Some(message.into_owned());
            cx.notify();
        });
        return false;
    }
    if let Some(purpose) = form.update(cx, |form, _| form.purpose.take()) {
        let _ = view.update(cx, |view, cx| {
            view.open_new_workspace(target, purpose, window, cx);
        });
    }
    true
}

impl AppView {
    /// Opens the workspace the dialog just created. For the example, then selects its
    /// environment and opens its request, so the first Send works without editing anything; for
    /// an import, runs the import into it.
    fn open_new_workspace(
        &mut self,
        root: PathBuf,
        purpose: NewWorkspacePurpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::info!("created workspace {}", root.display());
        self.open_workspace_at(&root, window, cx);
        match purpose {
            NewWorkspacePurpose::Example => {
                self.when_workspace_opens(window, cx, |view, _, cx| {
                    view.select_environment(Some(EXAMPLE_ENVIRONMENT.to_string()), cx);
                    view.open_request(EXAMPLE_REQUEST_ID.to_string(), cx);
                });
            }
            NewWorkspacePurpose::Import(pending) => {
                self.when_workspace_opens(window, cx, move |view, window, cx| {
                    view.import_into_workspace(
                        pending.kind,
                        &pending.json,
                        &pending.source,
                        window,
                        cx,
                    );
                });
            }
        }
    }
}
