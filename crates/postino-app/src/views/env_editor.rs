//! The environment editor tab (GitHub #65): a header with the environment's name and actions, the
//! two files it is stored in, a toolbar (filter, file scope, secrets toggle, add), a table of the
//! variables with inline inputs, and the resolution order. The rows and every decision about
//! them live in `state::env_edit`; this file only draws them and forwards the edits. The input
//! entities are created once per tab (and once per added row), never per frame, and resynced
//! only by [`AppView::reload_env_tab`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::KeyValue;
use postino_workspace::EnvLayer;

use crate::state::env_color::env_color;
use crate::state::env_edit::{EnvEditTab, EnvScope, EnvValidationError, EnvVarRow, RowNote};
use crate::state::number::format_integer;
use crate::state::workspace_log::log_workspace_error;
use crate::theme::metrics::{RADIUS_MD, RADIUS_SM};
use crate::theme::{Palette, PaletteExt};
use crate::views::components::{
    GhostButton, IconButton, InlineMessage, InlineMessageKind, PrimaryButton, SecondaryButton,
    SegmentedControl, SegmentedItem, edit_menu,
};

use super::root::AppView;

/// Width of the Name column.
const NAME_COLUMN_WIDTH: f32 = 200.0;
/// Width of the Stored in column.
const STORED_COLUMN_WIDTH: f32 = 190.0;
/// Width of the trash column.
const TRASH_COLUMN_WIDTH: f32 = 30.0;
/// Height of the toolbar and header controls.
const CONTROL_HEIGHT: f32 = 28.0;

/// Line height of the row inputs, which is also the height of their selection highlight (same as
/// the URL bar's). Use `row_line_height`, not this, to size an input.
const ROW_LINE_HEIGHT: f32 = 22.0;

/// The whole-pixel line height closest to `ROW_LINE_HEIGHT` that a row input can hold without
/// scrolling. gpui rounds a line height to whole logical pixels and snaps an input's box to
/// device pixels, so at a fractional scale (1.2: 22 px is 26.4 device pixels, snapped to 26) the
/// box ends up shorter than the line and the input scrolls the missing fraction when focused,
/// then snaps back. That is the text bobbing on click.
fn row_line_height(scale_factor: f32) -> Pixels {
    let fits = |line: f32| (line * scale_factor).round() >= line * scale_factor;
    let line = (0..=4)
        .flat_map(|step| [ROW_LINE_HEIGHT + step as f32, ROW_LINE_HEIGHT - step as f32])
        .find(|&line| fits(line))
        .unwrap_or(ROW_LINE_HEIGHT);
    px(line)
}

/// A shared dialog submit handler.
type SubmitHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Which input of a row an edit comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvField {
    Name,
    Value,
}

/// The two inputs of one row.
struct EnvRowInputs {
    name: Entity<InputState>,
    value: Entity<InputState>,
}

/// The `gpui` entities of one environment editor tab.
pub(crate) struct EnvEditorEntities {
    filter: Entity<InputState>,
    rows: HashMap<u64, EnvRowInputs>,
    /// The table's scroll position, so a row added at the end can be brought into view.
    scroll: ScrollHandle,
}

impl AppView {
    /// Opens the editor tab of the environment `name`, or activates it if it is already open.
    pub(crate) fn open_environment_tab(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        if let Some(index) = self
            .state
            .tabs
            .index_of(&crate::state::env_edit::tab_id(&name))
        {
            self.state.tabs.set_active(index);
            cx.notify();
            return;
        }
        match workspace.load_environment_layers(&name) {
            Ok(layers) => {
                log::debug!("opened environment tab {name}");
                self.state
                    .tabs
                    .open_environment(EnvEditTab::load(&name, &layers));
                self.workspace_error = None;
            }
            Err(error) => {
                log_workspace_error(&format!("open environment {name:?}"), &error);
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    /// Tells the open tab of `name`, if any, that `key` was set to `value` in `layer` on disk by
    /// something else (the Define variable dialog), keeping its unsaved edits. Its inputs are
    /// rebuilt from the rows on the next render.
    pub(crate) fn sync_env_tab_after_set(
        &mut self,
        name: &str,
        layer: EnvLayer,
        key: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        let tab_id = crate::state::env_edit::tab_id(name);
        self.edit_env(&tab_id, cx, |edit| {
            edit.apply_saved_set(layer, key, value);
            true
        });
        self.env_editors.remove(&tab_id);
    }

    /// Opens the editor tab of the environment `name` with a new row for `key` holding `value`
    /// in `layer`, and focuses its value. Used by the Define variable dialog's "Open in editor".
    pub(crate) fn open_env_tab_with_variable(
        &mut self,
        name: String,
        key: &str,
        value: &str,
        layer: EnvLayer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab_id = crate::state::env_edit::tab_id(&name);
        self.open_environment_tab(name, cx);
        if self.state.tabs.active().is_none_or(|tab| tab.id != tab_id) {
            return;
        }
        self.ensure_env_entities(&tab_id, window, cx);
        let mut row = None;
        self.edit_env(&tab_id, cx, |edit| {
            let id = edit.add_variable();
            edit.set_name(id, key);
            edit.set_value(id, value);
            edit.set_layer(id, layer);
            row = edit
                .rows
                .iter()
                .find(|row| row.id == id)
                .map(|row| (id, edit.is_masked(row)));
            true
        });
        let Some((id, masked)) = row else {
            return;
        };
        let inputs = build_row_inputs(&tab_id, id, key, value, masked, window, cx);
        let value_input = inputs.value.clone();
        if let Some(entities) = self.env_editors.get_mut(&tab_id) {
            entities
                .filter
                .update(cx, |state, cx| state.set_value(String::new(), window, cx));
            entities.rows.insert(id, inputs);
            entities.scroll.scroll_to_bottom();
        }
        value_input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Creates the entities of the tab `tab_id` the first time it is shown.
    fn ensure_env_entities(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.env_editors.contains_key(tab_id) {
            return;
        }
        let Some(edit) = self
            .state
            .tabs
            .index_of(tab_id)
            .and_then(|index| self.state.tabs.get(index))
            .and_then(|tab| tab.environment())
        else {
            return;
        };
        let filter_text = edit.filter.clone();
        let rows: Vec<(u64, String, String, bool)> = edit
            .rows
            .iter()
            .map(|row| {
                (
                    row.id,
                    row.name.clone(),
                    row.value.clone(),
                    edit.is_masked(row),
                )
            })
            .collect();

        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("environments.filter_placeholder"))
        });
        filter.update(cx, |state, cx| state.set_value(filter_text, window, cx));
        {
            let tab_id = tab_id.to_string();
            cx.subscribe(&filter, move |view, entity, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = entity.read(cx).value().to_string();
                    view.set_env_filter(&tab_id, &text, cx);
                }
            })
            .detach();
        }
        let mut entities = EnvEditorEntities {
            filter,
            rows: HashMap::new(),
            scroll: ScrollHandle::new(),
        };
        for (id, name, value, masked) in rows {
            let inputs = build_row_inputs(tab_id, id, &name, &value, masked, window, cx);
            entities.rows.insert(id, inputs);
        }
        self.env_editors.insert(tab_id.to_string(), entities);
    }

    /// Re-applies the translated placeholders of every editor's inputs after a language change.
    pub(crate) fn relocalize_env_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for entities in self.env_editors.values() {
            entities.filter.update(cx, |input, cx| {
                input.set_placeholder(t!("environments.filter_placeholder"), window, cx);
            });
            for inputs in entities.rows.values() {
                inputs.name.update(cx, |input, cx| {
                    input.set_placeholder(t!("environments.name_placeholder"), window, cx);
                });
                inputs.value.update(cx, |input, cx| {
                    input.set_placeholder(t!("common.value"), window, cx);
                });
            }
        }
    }

    /// Applies `f` to the editor state of the tab `tab_id` and, when it reports a change, syncs
    /// the tab's dirty flag and repaints. The one place every edit funnels through.
    fn edit_env(
        &mut self,
        tab_id: &str,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut EnvEditTab) -> bool,
    ) {
        let Some(index) = self.state.tabs.index_of(tab_id) else {
            return;
        };
        let Some(edit) = self
            .state
            .tabs
            .get_mut(index)
            .and_then(|tab| tab.environment_mut())
        else {
            return;
        };
        if f(edit) {
            let dirty = edit.is_dirty();
            self.state.tabs.set_dirty(index, dirty);
            cx.notify();
        }
    }

    fn edit_env_row(
        &mut self,
        tab_id: &str,
        row_id: u64,
        field: EnvField,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        self.edit_env(tab_id, cx, |edit| match field {
            EnvField::Name => edit.set_name(row_id, text),
            EnvField::Value => edit.set_value(row_id, text),
        });
    }

    fn set_env_filter(&mut self, tab_id: &str, text: &str, cx: &mut Context<Self>) {
        let Some(edit) = self
            .state
            .tabs
            .index_of(tab_id)
            .and_then(|index| self.state.tabs.get_mut(index))
            .and_then(|tab| tab.environment_mut())
        else {
            return;
        };
        edit.set_filter(text);
        cx.notify();
    }

    /// Appends an empty row stored in `.env` to the active environment tab and focuses its name.
    fn add_env_variable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab_id) = self
            .state
            .tabs
            .active()
            .filter(|tab| tab.environment().is_some())
            .map(|tab| tab.id.clone())
        else {
            return;
        };
        self.ensure_env_entities(&tab_id, window, cx);
        let mut new_id = None;
        self.edit_env(&tab_id, cx, |edit| {
            new_id = Some(edit.add_variable());
            true
        });
        let Some(id) = new_id else {
            return;
        };
        let inputs = build_row_inputs(&tab_id, id, "", "", false, window, cx);
        let name_input = inputs.name.clone();
        if let Some(entities) = self.env_editors.get_mut(&tab_id) {
            entities
                .filter
                .update(cx, |state, cx| state.set_value(String::new(), window, cx));
            entities.rows.insert(id, inputs);
            entities.scroll.scroll_to_bottom();
        }
        name_input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn remove_env_row(&mut self, tab_id: &str, row_id: u64, cx: &mut Context<Self>) {
        self.edit_env(tab_id, cx, |edit| {
            edit.remove(row_id);
            true
        });
        if let Some(entities) = self.env_editors.get_mut(tab_id) {
            entities.rows.remove(&row_id);
        }
    }

    /// Moves a row to the other file and updates its mask.
    fn move_env_row(
        &mut self,
        tab_id: &str,
        row_id: u64,
        layer: EnvLayer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_env(tab_id, cx, |edit| {
            edit.set_layer(row_id, layer);
            true
        });
        self.apply_env_masks(tab_id, window, cx);
    }

    fn toggle_env_secrets(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self
            .state
            .tabs
            .index_of(tab_id)
            .and_then(|index| self.state.tabs.get_mut(index))
            .and_then(|tab| tab.environment_mut())
        else {
            return;
        };
        edit.toggle_secrets();
        self.apply_env_masks(tab_id, window, cx);
        cx.notify();
    }

    /// Sets the masked flag of every value input to what the editor state says.
    fn apply_env_masks(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self
            .state
            .tabs
            .index_of(tab_id)
            .and_then(|index| self.state.tabs.get(index))
            .and_then(|tab| tab.environment())
        else {
            return;
        };
        let Some(entities) = self.env_editors.get(tab_id) else {
            return;
        };
        for row in &edit.rows {
            if let Some(inputs) = entities.rows.get(&row.id) {
                let masked = edit.is_masked(row);
                inputs
                    .value
                    .update(cx, |input, cx| input.set_masked(masked, window, cx));
            }
        }
    }

    /// Saves the environment tab at `index`: validates, writes both files keeping everything the
    /// edits did not touch, then marks the tab clean and refreshes the panel.
    pub(crate) fn save_env_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(edit) = self
            .state
            .tabs
            .get_mut(index)
            .and_then(|tab| tab.environment_mut())
        else {
            return;
        };
        if let Err(error) = edit.validate() {
            edit.error = Some(error);
            cx.notify();
            return;
        }
        let name = edit.name.clone();
        let changes = edit.changes();
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        match workspace.save_environment(&name, &changes) {
            Ok(()) => {
                log::debug!("saved environment {name}: {} changes", changes.len());
                if let Some(edit) = self
                    .state
                    .tabs
                    .get_mut(index)
                    .and_then(|tab| tab.environment_mut())
                {
                    edit.mark_saved();
                }
                self.state.tabs.mark_saved(index);
                self.workspace_error = None;
                // Requests read the active environment from disk when they run, so the next
                // send already sees the saved values.
                self.refresh_env_rows();
            }
            Err(error) => {
                log_workspace_error(&format!("save environment {name:?}"), &error);
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    /// Closes the tab at `index`, asking first when it is an environment tab with unsaved edits.
    pub(crate) fn request_close_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.state.tabs.get(index) else {
            return;
        };
        let Some(edit) = tab.environment().filter(|_| tab.dirty) else {
            self.close_tab(index, cx);
            return;
        };
        let name = edit.name.clone();
        let tab_id = tab.id.clone();
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            let tab_id = tab_id.clone();
            alert
                .title(t!("environments.discard.title"))
                .description(t!("environments.discard.text", name = name.as_str()))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t!("environments.discard.confirm"))
                        .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                        .cancel_text(t!("common.cancel"))
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        if let Some(index) = view.state.tabs.index_of(&tab_id) {
                            view.close_tab(index, cx);
                        }
                    });
                    true
                })
        });
    }

    /// Renames the environment `old` to `new` on disk and everywhere the app remembers it.
    pub(crate) fn rename_environment(&mut self, old: String, new: String, cx: &mut Context<Self>) {
        if old == new {
            return;
        }
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        match workspace.rename_environment(&old, &new) {
            Ok(()) => {
                log::debug!("renamed environment {old} to {new}");
                let root = workspace.root().to_path_buf();
                self.state.tabs.rename_environment(&old, &new);
                if let Some(entities) = self
                    .env_editors
                    .remove(&crate::state::env_edit::tab_id(&old))
                {
                    self.env_editors
                        .insert(crate::state::env_edit::tab_id(&new), entities);
                }
                // The remembered environment is rewritten even when it is not the active one.
                if self.state.active_environment.as_deref() == Some(old.as_str()) {
                    self.state.active_environment = Some(new.clone());
                    crate::state::config::record_environment(&root, Some(&new));
                } else if crate::state::config::last_environment(&root).as_deref()
                    == Some(old.as_str())
                {
                    crate::state::config::record_environment(&root, Some(&new));
                }
                self.workspace_error = None;
                self.refresh_env_rows();
            }
            Err(error) => {
                log_workspace_error(&format!("rename environment {old:?}"), &error);
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    /// Deletes both files of the environment `name`, closes its tab and deactivates it.
    pub(crate) fn delete_environment(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        match workspace.delete_environment(&name) {
            Ok(()) => {
                log::debug!("deleted environment {name}");
                let tab_id = crate::state::env_edit::tab_id(&name);
                if let Some(index) = self.state.tabs.index_of(&tab_id) {
                    self.close_tab(index, cx);
                }
                if self.state.active_environment.as_deref() == Some(name.as_str()) {
                    self.select_environment(None, cx);
                }
                self.workspace_error = None;
                self.refresh_env_rows();
            }
            Err(error) => {
                log_workspace_error(&format!("delete environment {name:?}"), &error);
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    fn open_env_rename_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("navigation.env.name_placeholder"))
        });
        input.update(cx, |state, cx| state.set_value(name.clone(), window, cx));
        let weak = cx.weak_entity();
        // Renames and closes the dialog; shared by Enter in the input and the Rename button.
        let submit: SubmitHandler = {
            let input = input.clone();
            let weak = weak.clone();
            let old = name.clone();
            Rc::new(move |window, cx| {
                let new = input.read(cx).value().trim().to_string();
                if new.is_empty() || new == old {
                    return;
                }
                window.close_dialog(cx);
                let old = old.clone();
                let _ = weak.update(cx, |view, cx| view.rename_environment(old, new, cx));
            })
        };
        {
            let submit = submit.clone();
            cx.subscribe_in(
                &input,
                window,
                move |_, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        submit(window, cx);
                    }
                },
            )
            .detach();
        }
        let dialog_input = input.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let input_for_content = dialog_input.clone();
            let submit = submit.clone();
            dialog
                .title(t!("environments.rename.title"))
                .content(move |content, _, cx| {
                    content.child(
                        Input::new(&input_for_content)
                            .context_menu(edit_menu(&input_for_content, cx))
                            .py_0(),
                    )
                })
                .footer(
                    h_flex()
                        .justify_end()
                        .gap(px(8.0))
                        .child(
                            SecondaryButton::new("env-rename-cancel", t!("common.cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            PrimaryButton::new("env-rename-ok", t!("common.rename"))
                                .on_click(move |_, window, cx| submit(window, cx)),
                        ),
                )
        });
        // Focus only after `open_dialog`, which captures the focused handle to restore on close
        // (see `views/command_palette.rs`). Selecting the whole name makes typing replace it.
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
    }

    fn open_env_delete_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            let target = name.clone();
            alert
                .title(t!("environments.delete.title"))
                .description(t!("environments.delete.text", name = name.as_str()))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t!("common.delete"))
                        .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                        .cancel_text(t!("common.cancel"))
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    let target = target.clone();
                    let _ = weak.update(cx, |view, cx| view.delete_environment(target, cx));
                    true
                })
        });
    }

    /// Renders the editor of the active environment tab.
    pub(crate) fn render_env_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab_id) = self.state.tabs.active().map(|tab| tab.id.clone()) else {
            return div().into_any_element();
        };
        self.ensure_env_entities(&tab_id, window, cx);
        let (Some(edit), Some(entities)) = (
            self.state.tabs.active().and_then(|tab| tab.environment()),
            self.env_editors.get(&tab_id),
        ) else {
            return div().into_any_element();
        };
        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();
        let weak = cx.weak_entity();
        let is_active = self.state.active_environment.as_deref() == Some(edit.name.as_str());
        let reveal_path = self.state.workspace.as_ref().map(|workspace| {
            let layer = if edit.base_exists {
                EnvLayer::Base
            } else {
                EnvLayer::Local
            };
            workspace.environment_path(&edit.name, layer)
        });
        let session: Vec<KeyValue> = if is_active {
            self.state.session_env.as_slice().to_vec()
        } else {
            Vec::new()
        };

        v_flex()
            .size_full()
            .bg(palette.bg)
            .child(render_header(
                edit,
                reveal_path,
                is_active,
                &weak,
                &palette,
                cx,
            ))
            .child(render_toolbar(
                edit,
                entities,
                session.len(),
                &tab_id,
                &weak,
                &palette,
                cx,
            ))
            .children(edit.error.as_ref().map(|error| {
                div().px(px(20.0)).pt(px(10.0)).child(InlineMessage::new(
                    InlineMessageKind::Danger,
                    error_text(error),
                ))
            }))
            .child(
                div()
                    .id("env-table-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&entities.scroll)
                    .px(px(20.0))
                    .py(px(12.0))
                    .child(render_table(
                        edit,
                        entities,
                        &session,
                        &tab_id,
                        &weak,
                        &palette,
                        &mono_font,
                        row_line_height(window.scale_factor()),
                        cx,
                    )),
            )
            .child(render_footer(&palette))
            .into_any_element()
    }
}

/// The translated text of a validation error.
fn error_text(error: &EnvValidationError) -> String {
    match error {
        EnvValidationError::EmptyName => t!("environments.error.empty_name").into_owned(),
        EnvValidationError::InvalidName(name) => {
            t!("environments.error.invalid_name", name = name.as_str()).into_owned()
        }
        EnvValidationError::InvalidValue(name) => {
            t!("environments.error.invalid_value", name = name.as_str()).into_owned()
        }
        EnvValidationError::Duplicate(name) => {
            t!("environments.error.duplicate", name = name.as_str()).into_owned()
        }
    }
}

/// Builds the name and value inputs of one row, prefilled, and subscribes both to write typed
/// changes back into the tab. The value is set before subscribing so it does not count as an
/// edit.
fn build_row_inputs(
    tab_id: &str,
    row_id: u64,
    name: &str,
    value: &str,
    masked: bool,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> EnvRowInputs {
    let name_input =
        cx.new(|cx| InputState::new(window, cx).placeholder(t!("environments.name_placeholder")));
    name_input.update(cx, |state, cx| {
        state.set_value(name.to_string(), window, cx)
    });
    let value_input = cx.new(|cx| InputState::new(window, cx).placeholder(t!("common.value")));
    value_input.update(cx, |state, cx| {
        state.set_value(value.to_string(), window, cx);
        state.set_masked(masked, window, cx);
    });
    for (input, field) in [
        (&name_input, EnvField::Name),
        (&value_input, EnvField::Value),
    ] {
        let tab_id = tab_id.to_string();
        cx.subscribe(input, move |view, entity, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = entity.read(cx).value().to_string();
                view.edit_env_row(&tab_id, row_id, field, &text, cx);
            }
        })
        .detach();
    }
    EnvRowInputs {
        name: name_input,
        value: value_input,
    }
}

/// The header: color dot, name, "Use this environment", the "..." menu and the two file chips.
fn render_header(
    edit: &EnvEditTab,
    reveal_path: Option<PathBuf>,
    is_active: bool,
    weak: &WeakEntity<AppView>,
    palette: &Palette,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let name = edit.name.clone();
    let use_label = if is_active {
        t!("environments.active_env")
    } else {
        t!("environments.use")
    };
    let use_button = SecondaryButton::new("env-use", use_label)
        .icon(IconName::Check)
        .height(CONTROL_HEIGHT)
        .disabled(is_active)
        .on_click({
            let weak = weak.clone();
            let name = name.clone();
            move |_, _, cx| {
                let name = name.clone();
                let _ = weak.update(cx, |view, cx| view.select_environment(Some(name), cx));
            }
        });

    let more = Button::new("env-more")
        .ghost()
        .icon(Icon::new(IconName::Ellipsis))
        .w(px(CONTROL_HEIGHT))
        .h(px(CONTROL_HEIGHT))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .text_color(palette.fg_muted)
        .tooltip(t!("environments.more_tooltip"))
        .dropdown_menu({
            let weak = weak.clone();
            let name = name.clone();
            move |menu, _, _| {
                let rename = {
                    let weak = weak.clone();
                    let name = name.clone();
                    PopupMenuItem::new(t!("common.rename")).on_click(move |_, window, cx| {
                        let name = name.clone();
                        let _ = weak
                            .update(cx, |view, cx| view.open_env_rename_dialog(name, window, cx));
                    })
                };
                let reveal = {
                    let path = reveal_path.clone();
                    PopupMenuItem::new(t!("environments.menu.reveal")).on_click(move |_, _, cx| {
                        if let Some(path) = &path {
                            cx.reveal_path(path);
                        }
                    })
                };
                let delete = {
                    let weak = weak.clone();
                    let name = name.clone();
                    PopupMenuItem::new(t!("common.delete")).on_click(move |_, window, cx| {
                        let name = name.clone();
                        let _ = weak
                            .update(cx, |view, cx| view.open_env_delete_dialog(name, window, cx));
                    })
                };
                menu.item(rename).item(reveal).separator().item(delete)
            }
        });

    v_flex()
        .gap(px(10.0))
        .px(px(20.0))
        .pt(px(16.0))
        .pb(px(12.0))
        .border_b_1()
        .border_color(palette.border)
        .child(
            h_flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_none()
                        .size(px(9.0))
                        .rounded_full()
                        .bg(palette.env_color(env_color(&name))),
                )
                .child(
                    div()
                        .text_size(px(17.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.fg)
                        .child(name.clone()),
                )
                .child(div().flex_1())
                .child(use_button)
                .child(more),
        )
        .child(
            h_flex()
                .gap(px(8.0))
                .flex_wrap()
                .child(file_chip(
                    IconName::File,
                    format!("environments/{name}.env"),
                    t!("environments.file.versioned").into_owned(),
                    true,
                    palette,
                    cx,
                ))
                .child(file_chip(
                    IconName::Lock,
                    format!("environments/{name}.local.env"),
                    if edit.local_exists {
                        t!("environments.file.gitignored").into_owned()
                    } else {
                        t!("environments.file.not_created").into_owned()
                    },
                    edit.local_exists,
                    palette,
                    cx,
                )),
        )
        .into_any_element()
}

/// One file chip of the header: icon, mono path and a muted note. Muted when the file does not
/// exist.
fn file_chip(
    icon: IconName,
    path: String,
    note: String,
    exists: bool,
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let mono_font = cx.theme().mono_font_family.clone();
    h_flex()
        .items_center()
        .gap(px(6.0))
        .h(px(22.0))
        .px(px(8.0))
        .rounded(px(RADIUS_SM))
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .text_color(if exists {
            palette.fg_muted
        } else {
            palette.fg_subtle
        })
        .child(Icon::new(icon).with_size(px(11.0)))
        .child(div().font_family(mono_font).text_size(px(11.5)).child(path))
        .child(
            div()
                .text_size(px(11.5))
                .text_color(palette.fg_subtle)
                .child(format!("\u{b7} {note}")),
        )
        .into_any_element()
}

/// The toolbar: filter, scope control, secrets toggle and "Add variable".
fn render_toolbar(
    edit: &EnvEditTab,
    entities: &EnvEditorEntities,
    session_count: usize,
    tab_id: &str,
    weak: &WeakEntity<AppView>,
    palette: &Palette,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let scope_item = |label: String, scope: EnvScope| {
        let weak = weak.clone();
        let tab_id = tab_id.to_string();
        SegmentedItem::new(label)
            .selected(edit.scope == scope)
            .on_click(move |_, cx| {
                let tab_id = tab_id.clone();
                let _ = weak.update(cx, |view, cx| {
                    view.edit_env(&tab_id, cx, |edit| {
                        edit.set_scope(scope);
                        true
                    });
                });
            })
    };
    let all_label = t!(
        "environments.scope.all",
        count = format_integer((edit.count_all() + session_count) as u64).as_str()
    )
    .into_owned();
    let secrets_label = if edit.show_secrets {
        t!("environments.hide_secrets")
    } else {
        t!("environments.show_secrets")
    };
    let secrets_icon = if edit.show_secrets {
        IconName::EyeOff
    } else {
        IconName::Eye
    };

    h_flex()
        .items_center()
        .gap(px(8.0))
        .px(px(20.0))
        .pt(px(12.0))
        .child(
            div().w(px(220.0)).child(
                Input::new(&entities.filter)
                    .context_menu(edit_menu(&entities.filter, cx))
                    .h(px(CONTROL_HEIGHT))
                    .py_0()
                    .prefix(
                        Icon::new(IconName::ListFilter)
                            .small()
                            .text_color(palette.fg_subtle),
                    )
                    .cleanable(true),
            ),
        )
        .child(
            SegmentedControl::new("env-scope")
                .item(scope_item(all_label, EnvScope::All))
                .item(scope_item(".env".to_string(), EnvScope::Base))
                .item(scope_item(".local.env".to_string(), EnvScope::Local)),
        )
        .child(div().flex_1())
        .child(
            GhostButton::new("env-secrets", secrets_label)
                .icon(secrets_icon)
                .height(CONTROL_HEIGHT)
                .on_click({
                    let weak = weak.clone();
                    let tab_id = tab_id.to_string();
                    move |_, window, cx| {
                        let tab_id = tab_id.clone();
                        let _ = weak
                            .update(cx, |view, cx| view.toggle_env_secrets(&tab_id, window, cx));
                    }
                }),
        )
        .child(
            PrimaryButton::new("env-add", t!("environments.add"))
                .icon(IconName::Plus)
                .height(CONTROL_HEIGHT)
                .on_click({
                    let weak = weak.clone();
                    move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| view.add_env_variable(window, cx));
                    }
                }),
        )
        .into_any_element()
}

/// The variables table: header, one row per visible variable, then the session rows.
#[allow(clippy::too_many_arguments)]
fn render_table(
    edit: &EnvEditTab,
    entities: &EnvEditorEntities,
    session: &[KeyValue],
    tab_id: &str,
    weak: &WeakEntity<AppView>,
    palette: &Palette,
    mono_font: &SharedString,
    line_height: Pixels,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let header_cell = |text: String| {
        div()
            .px(px(12.0))
            .text_size(px(11.0))
            .text_color(palette.fg_subtle)
            .child(text.to_uppercase())
    };
    let header = h_flex()
        .h(px(30.0))
        .items_center()
        .bg(palette.surface)
        .border_b_1()
        .border_color(palette.border)
        .child(
            header_cell(t!("environments.col.name").into_owned())
                .flex_none()
                .w(px(NAME_COLUMN_WIDTH)),
        )
        .child(
            header_cell(t!("common.value").into_owned())
                .flex_1()
                .min_w_0(),
        )
        .child(
            header_cell(t!("environments.col.stored_in").into_owned())
                .flex_none()
                .w(px(STORED_COLUMN_WIDTH)),
        )
        .child(div().flex_none().w(px(TRASH_COLUMN_WIDTH)));

    let session_rows = edit.visible_session(session);
    let total = edit.visible().len() + session_rows.len();
    let mut table = v_flex()
        .border_1()
        .border_color(palette.border)
        .rounded(px(RADIUS_MD))
        .overflow_hidden()
        .child(header);

    for (position, &index) in edit.visible().iter().enumerate() {
        let row = &edit.rows[index];
        let Some(inputs) = entities.rows.get(&row.id) else {
            continue;
        };
        let last = position + 1 == total;
        table = table.child(file_row(
            edit,
            row,
            inputs,
            last,
            tab_id,
            weak,
            palette,
            mono_font,
            line_height,
            cx,
        ));
    }
    for (offset, variable) in session_rows.iter().enumerate() {
        let last = edit.visible().len() + offset + 1 == total;
        table = table.child(session_row(variable, last, palette, mono_font));
    }
    if total == 0 {
        let text = if edit.filter.trim().is_empty() {
            t!("environments.empty")
        } else {
            t!("environments.empty_filter")
        };
        table = table.child(
            div()
                .px(px(12.0))
                .py(px(14.0))
                .text_size(px(12.5))
                .text_color(palette.fg_subtle)
                .child(text),
        );
    }
    table.into_any_element()
}

/// A small button drawn as a "stored in" chip: icon and mono file name on a tinted background.
/// Opens a menu when given one with `dropdown_menu`.
#[allow(clippy::too_many_arguments)]
fn chip_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: String,
    bg: Hsla,
    fg: Hsla,
    palette: &Palette,
    mono_font: &SharedString,
    cx: &App,
) -> Button {
    let custom = ButtonCustomVariant::new(cx)
        .color(bg)
        .foreground(fg)
        .hover(palette.hover)
        .active(palette.pressed);
    Button::new(id)
        .custom(custom)
        .h(px(20.0))
        .px(px(7.0))
        .rounded(px(5.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(5.0))
                .child(Icon::new(icon).with_size(px(10.0)))
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_family(mono_font.clone())
                        .child(label),
                ),
        )
}

/// The background and text colors of a "stored in" chip.
fn chip_colors(layer: Option<EnvLayer>, palette: &Palette) -> (Hsla, Hsla) {
    match layer {
        Some(EnvLayer::Base) => (palette.raised, palette.fg_muted),
        Some(EnvLayer::Local) => (palette.warning_subtle, palette.warning),
        None => (palette.accent_subtle, palette.accent_text),
    }
}

/// One editable row of a file.
#[allow(clippy::too_many_arguments)]
fn file_row(
    edit: &EnvEditTab,
    row: &EnvVarRow,
    inputs: &EnvRowInputs,
    last: bool,
    tab_id: &str,
    weak: &WeakEntity<AppView>,
    palette: &Palette,
    mono_font: &SharedString,
    line_height: Pixels,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let row_id = row.id;
    let note = edit.note(row).map(|note| match note {
        RowNote::OverridesBase => t!(
            "environments.note.overrides",
            file = format!("{}.env", edit.name).as_str()
        )
        .into_owned(),
    });
    let (chip_bg, chip_fg) = chip_colors(Some(row.layer), palette);
    let (chip_icon, chip_file) = match row.layer {
        EnvLayer::Base => (IconName::File, format!("{}.env", edit.name)),
        EnvLayer::Local => (IconName::Lock, format!("{}.local.env", edit.name)),
    };
    let current = row.layer;
    let picker = chip_button(
        ("env-stored", row_id as usize),
        chip_icon,
        chip_file,
        chip_bg,
        chip_fg,
        palette,
        mono_font,
        cx,
    )
    .tooltip(t!("environments.stored_in_tooltip"))
    .dropdown_menu({
        let weak = weak.clone();
        let tab_id = tab_id.to_string();
        let name = edit.name.clone();
        move |mut menu, _, _| {
            for (layer, file) in [
                (EnvLayer::Base, format!("{name}.env")),
                (EnvLayer::Local, format!("{name}.local.env")),
            ] {
                let weak = weak.clone();
                let tab_id = tab_id.clone();
                menu = menu.item(PopupMenuItem::new(file).checked(current == layer).on_click(
                    move |_, window, cx| {
                        let tab_id = tab_id.clone();
                        let _ = weak.update(cx, |view, cx| {
                            view.move_env_row(&tab_id, row_id, layer, window, cx)
                        });
                    },
                ));
            }
            menu
        }
    });

    h_flex()
        .min_h(px(34.0))
        .items_center()
        .when(!last, |row| row.border_b_1().border_color(palette.border))
        .child(
            div()
                .flex_none()
                .w(px(NAME_COLUMN_WIDTH))
                .px(px(4.0))
                .font_family(mono_font.clone())
                .text_size(px(12.5))
                .text_color(palette.accent_text)
                .child(
                    Input::new(&inputs.name)
                        .context_menu(edit_menu(&inputs.name, cx))
                        .py_0()
                        .line_height(line_height)
                        .appearance(false)
                        .bordered(false),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .px(px(4.0))
                .py(px(3.0))
                .child(
                    div()
                        .font_family(mono_font.clone())
                        .text_size(px(12.5))
                        .child(
                            Input::new(&inputs.value)
                                .context_menu(edit_menu(&inputs.value, cx))
                                .py_0()
                                .line_height(line_height)
                                .appearance(false)
                                .bordered(false),
                        ),
                )
                .children(note.map(|note| {
                    div()
                        .px(px(8.0))
                        .text_size(px(11.0))
                        .text_color(palette.fg_subtle)
                        .child(note)
                })),
        )
        .child(
            div()
                .flex_none()
                .w(px(STORED_COLUMN_WIDTH))
                .px(px(12.0))
                .child(picker),
        )
        .child(
            div()
                .flex_none()
                .w(px(TRASH_COLUMN_WIDTH))
                .flex()
                .justify_center()
                .child(
                    IconButton::new(("env-remove", row_id as usize), IconName::Trash)
                        .tooltip(t!("environments.remove_tooltip"))
                        .on_click({
                            let weak = weak.clone();
                            let tab_id = tab_id.to_string();
                            move |_, _, cx| {
                                let tab_id = tab_id.clone();
                                let _ = weak.update(cx, |view, cx| {
                                    view.remove_env_row(&tab_id, row_id, cx)
                                });
                            }
                        }),
                ),
        )
        .into_any_element()
}

/// One read only session row: a variable a script set with `env.set` during this session.
fn session_row(
    variable: &KeyValue,
    last: bool,
    palette: &Palette,
    mono_font: &SharedString,
) -> AnyElement {
    let (chip_bg, chip_fg) = chip_colors(None, palette);
    h_flex()
        .min_h(px(34.0))
        .items_center()
        .bg(palette.accent_subtle.opacity(0.5))
        .when(!last, |row| row.border_b_1().border_color(palette.border))
        .child(
            div()
                .flex_none()
                .w(px(NAME_COLUMN_WIDTH))
                .px(px(12.0))
                .truncate()
                .font_family(mono_font.clone())
                .text_size(px(12.5))
                .text_color(palette.accent_text)
                .child(variable.key.clone()),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .px(px(12.0))
                .py(px(6.0))
                .gap(px(2.0))
                .child(
                    div()
                        .truncate()
                        .font_family(mono_font.clone())
                        .text_size(px(12.5))
                        .text_color(palette.fg)
                        .child(variable.value.clone()),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(palette.fg_subtle)
                        .child(t!("environments.note.session")),
                ),
        )
        .child(
            div()
                .flex_none()
                .w(px(STORED_COLUMN_WIDTH))
                .px(px(12.0))
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(5.0))
                        .h(px(20.0))
                        .px(px(7.0))
                        .rounded(px(5.0))
                        .w_auto()
                        .bg(chip_bg)
                        .text_color(chip_fg)
                        .text_size(px(11.0))
                        .font_family(mono_font.clone())
                        .child(Icon::new(IconName::Zap).with_size(px(10.0)))
                        .child(t!("environments.source.session")),
                ),
        )
        .child(div().flex_none().w(px(TRASH_COLUMN_WIDTH)))
        .into_any_element()
}

/// The footer: the resolution order of a variable.
fn render_footer(palette: &Palette) -> AnyElement {
    h_flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .px(px(20.0))
        .py(px(10.0))
        .border_t_1()
        .border_color(palette.border)
        .text_size(px(12.0))
        .text_color(palette.fg_subtle)
        .child(Icon::new(IconName::Layers).with_size(px(13.0)))
        .child(t!("environments.resolution.label"))
        .child(
            div()
                .text_color(palette.fg_muted)
                .child(t!("environments.resolution.order")),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{ROW_LINE_HEIGHT, row_line_height};

    #[test]
    fn row_line_height_fills_whole_device_pixels() {
        for scale in [1.0, 1.1, 1.2, 1.25, 1.5, 1.75, 2.0, 3.0] {
            let line: f32 = row_line_height(scale).into();
            assert!(
                (line * scale).round() >= line * scale,
                "scale {scale}: {line}"
            );
            assert!(
                (line - ROW_LINE_HEIGHT).abs() <= 4.0,
                "scale {scale}: {line}"
            );
        }
        assert_eq!(f32::from(row_line_height(1.0)), ROW_LINE_HEIGHT);
    }
}
