//! The Define variable dialog: opened from a response pane "Define" warning action
//! (`views/response_view.rs`) or a click on a danger chip in the URL (`views/request_editor.rs`,
//! through `UrlBar::on_chip_click`). Asks for the variable's value and which environment to write
//! it to, then saves it with [`postino_workspace::Workspace::save_environment`], the same call the
//! environment editor uses, and tells that environment's open tab about it. Under the name it
//! lists the environments that already define it and a similar known name, and "Open in editor"
//! moves the variable to the environment editor instead. Reuses `views/settings.rs`'s dialog
//! patterns (a plain `Dialog` with custom content, a select-styled dropdown).

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_workspace::{EnvChange, EnvLayer};

use crate::state::define_variable::{self, KnownNames};
use crate::theme::{Palette, PaletteExt};
use crate::views::components::{
    GhostButton, PrimaryButton, SecondaryButton, edit_menu, select_label_row, select_trigger,
};

use super::root::AppView;

/// Width of the dialog.
const DIALOG_WIDTH: f32 = 420.0;
/// Height of the hint line under the name.
const HINT_LINE_HEIGHT: f32 = 16.0;

/// The live `gpui` entities and in-progress choices of an open Define variable dialog. `None` on
/// [`AppView`] while the dialog is closed.
pub(crate) struct DefineVariableState {
    /// The variable name, prefilled and editable.
    name_input: Entity<InputState>,
    /// The value to write.
    value_input: Entity<InputState>,
    /// The new environment's name, typed while [`Self::creating_environment`] is set. Always
    /// created (even when not shown yet) so picking "New environment..." never needs a fresh
    /// entity.
    new_environment_input: Entity<InputState>,
    /// The existing environment to write into. Stale once [`Self::creating_environment`] is set.
    environment: Option<String>,
    /// Whether "New environment..." is the current pick, showing
    /// [`Self::new_environment_input`] in place of the existing choice.
    creating_environment: bool,
    /// Whether to write to `<env>.local.env` instead of `<env>.env`.
    store_local: bool,
    /// The variable names of every environment, read when the dialog opened.
    known: KnownNames,
    /// The environments that already define the current name, refreshed as it is typed.
    defined_in: Vec<String>,
    /// A known name close to the current one, refreshed as it is typed.
    similar: Option<String>,
}

impl DefineVariableState {
    /// Recomputes the hints under the name for `name`.
    fn refresh_hints(&mut self, name: &str) {
        self.defined_in = self.known.defined_in(name);
        self.similar = self.known.similar(name);
    }

    /// The file the variable goes to.
    fn layer(&self) -> EnvLayer {
        if self.store_local {
            EnvLayer::Local
        } else {
            EnvLayer::Base
        }
    }
}

impl AppView {
    /// Opens the Define variable dialog for `name`. A no-op when a dialog is already open, or when
    /// there is no workspace to write into.
    pub(crate) fn open_define_variable_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        let environments = workspace.list_environments().unwrap_or_default();
        let environment = define_variable::initial_environment(
            self.state.active_environment.as_deref(),
            &environments,
        );
        let creating_environment = environment.is_none();
        let known = KnownNames::new(
            environments
                .iter()
                .map(|env| {
                    let names = workspace
                        .load_environment(env)
                        .map(|loaded| loaded.variables.into_iter().map(|kv| kv.key).collect())
                        .unwrap_or_default();
                    (env.clone(), names)
                })
                .collect(),
        );

        let name_input = cx.new(|cx| InputState::new(window, cx));
        name_input.update(cx, |state, cx| {
            state.set_value(name.clone(), window, cx);
        });
        cx.subscribe(&name_input, |view, entity, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let name = entity.read(cx).value().trim().to_string();
                if let Some(state) = &mut view.define_variable {
                    state.refresh_hints(&name);
                }
                cx.notify();
            }
        })
        .detach();
        let value_input = cx.new(|cx| InputState::new(window, cx).placeholder(t!("common.value")));
        let new_environment_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("request.define_variable.environment_name_placeholder"))
        });

        let mut state = DefineVariableState {
            name_input,
            value_input,
            new_environment_input,
            environment,
            creating_environment,
            store_local: define_variable::looks_sensitive(&name),
            known,
            defined_in: Vec::new(),
            similar: None,
        };
        state.refresh_hints(&name);
        self.define_variable = Some(state);
        cx.notify();

        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            // `open_dialog`'s builder is `Fn`, not `FnOnce`, so every closure below clones its
            // own copy of `weak` from this outer, persistently-captured one, rather than moving
            // it: a `move` closure that consumed a captured variable outright would make this
            // outer closure only callable once, which the `Fn` bound rejects.
            let content_weak = weak.clone();
            let ok_weak = weak.clone();
            let save_weak = weak.clone();
            let editor_weak = weak.clone();
            let close_weak = weak.clone();
            // "Open in editor" needs an existing environment to open.
            let can_open_editor = weak
                .upgrade()
                .and_then(|view| {
                    view.read(cx)
                        .define_variable
                        .as_ref()
                        .map(|state| !state.creating_environment)
                })
                .unwrap_or(false);
            dialog
                .title(t!("request.define_variable.title"))
                .w(px(DIALOG_WIDTH))
                .content(move |content, window, cx| {
                    content.min_h_0().child(render_define_variable_body(
                        content_weak.clone(),
                        window,
                        cx,
                    ))
                })
                // Enter in an input saves too. The dialog stays open while there is nothing to
                // save yet (an empty name or environment).
                .on_ok(move |_, _, cx| {
                    ok_weak
                        .update(cx, |view, cx| view.save_define_variable(cx))
                        .unwrap_or(true)
                })
                .footer(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .gap(px(8.0))
                        .child(div().when(can_open_editor, |el| {
                            el.child(
                                GhostButton::new(
                                    "define-variable-open-editor",
                                    t!("request.define_variable.open_in_editor"),
                                )
                                .on_click(move |_, window, cx| {
                                    // Read before closing: closing clears the dialog state
                                    // and gives the focus back, so the editor opens after.
                                    let target = editor_weak
                                        .update(cx, |view, cx| view.define_variable_target(cx))
                                        .ok()
                                        .flatten();
                                    window.close_dialog(cx);
                                    let Some((environment, name, value, layer)) = target else {
                                        return;
                                    };
                                    let _ = editor_weak.update(cx, |view, cx| {
                                        view.open_env_tab_with_variable(
                                            environment,
                                            &name,
                                            &value,
                                            layer,
                                            window,
                                            cx,
                                        )
                                    });
                                }),
                            )
                        }))
                        .child(
                            h_flex()
                                .gap(px(8.0))
                                .child(
                                    SecondaryButton::new(
                                        "define-variable-cancel",
                                        t!("common.cancel"),
                                    )
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    PrimaryButton::new("define-variable-save", t!("common.save"))
                                        .on_click(move |_, window, cx| {
                                            let saved = save_weak
                                                .update(cx, |view, cx| {
                                                    view.save_define_variable(cx)
                                                })
                                                .unwrap_or(true);
                                            if saved {
                                                window.close_dialog(cx);
                                            }
                                        }),
                                ),
                        ),
                )
                .on_close(move |_, _, cx| {
                    let _ = close_weak.update(cx, |view, _cx| view.define_variable = None);
                })
        });
    }

    /// Applies the current language to the open dialog's input placeholders, if it is open.
    pub(crate) fn relocalize_define_variable(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = &self.define_variable else {
            return;
        };
        state.value_input.update(cx, |input, cx| {
            input.set_placeholder(t!("common.value"), window, cx);
        });
        state.new_environment_input.update(cx, |input, cx| {
            input.set_placeholder(
                t!("request.define_variable.environment_name_placeholder"),
                window,
                cx,
            );
        });
    }

    /// Writes the dialog's current name/value/environment/local choice with
    /// [`postino_workspace::Workspace::save_environment`], tells that environment's open tab,
    /// makes the target environment the active one if none was active, and clears the error
    /// banner. Does nothing if the name or the target environment is empty, matching
    /// `views/sidebar.rs`'s new request/folder dialogs.
    ///
    /// Returns whether the dialog may close: `false` only when there was nothing to save yet.
    fn save_define_variable(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(state) = &self.define_variable else {
            return true;
        };
        let name = state.name_input.read(cx).value().trim().to_string();
        let value = state.value_input.read(cx).value().to_string();
        let target_env = if state.creating_environment {
            state
                .new_environment_input
                .read(cx)
                .value()
                .trim()
                .to_string()
        } else {
            state.environment.clone().unwrap_or_default()
        };
        let layer = state.layer();
        if name.is_empty() || target_env.is_empty() {
            return false;
        }
        let Some(workspace) = self.state.workspace.as_ref() else {
            return true;
        };
        let change = EnvChange::Set {
            layer,
            key: name.clone(),
            value: value.clone(),
        };
        match workspace.save_environment(&target_env, &[change]) {
            Ok(()) => {
                self.workspace_error = None;
                self.refresh_env_rows();
                self.sync_env_tab_after_set(&target_env, layer, &name, &value, cx);
                if self.state.active_environment.is_none() {
                    self.select_environment(Some(target_env), cx);
                }
            }
            Err(error) => {
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
        true
    }

    /// What "Open in editor" moves to the environment editor: the chosen environment, and the
    /// dialog's name, value and file. `None` while "New environment..." is picked.
    fn define_variable_target(&self, cx: &App) -> Option<(String, String, String, EnvLayer)> {
        let state = self.define_variable.as_ref()?;
        if state.creating_environment {
            return None;
        }
        Some((
            state.environment.clone()?,
            state.name_input.read(cx).value().trim().to_string(),
            state.value_input.read(cx).value().to_string(),
            state.layer(),
        ))
    }
}

/// Renders the dialog's content: variable name and its hints, value, environment select (and,
/// while creating one, its name), and the "Store in .local.env" switch.
fn render_define_variable_body(
    weak: WeakEntity<AppView>,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(view) = weak.upgrade() else {
        return div().into_any_element();
    };
    let (
        name_input,
        value_input,
        new_environment_input,
        creating_environment,
        store_local,
        environment,
        defined_in,
        similar,
    ) = {
        let read = view.read(cx);
        let Some(state) = read.define_variable.as_ref() else {
            return div().into_any_element();
        };
        (
            state.name_input.clone(),
            state.value_input.clone(),
            state.new_environment_input.clone(),
            state.creating_environment,
            state.store_local,
            state.environment.clone(),
            state.defined_in.clone(),
            state.similar.clone(),
        )
    };
    let will_activate = view.read(cx).state.active_environment.is_none();
    let environments = view
        .read(cx)
        .state
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.list_environments().ok())
        .unwrap_or_default();
    let palette = cx.palette();

    let switch_weak = weak.clone();

    let mut name_hints = Vec::new();
    if !defined_in.is_empty() {
        name_hints.push(t!(
            "request.define_variable.defined_in",
            environments = defined_in.join(", ")
        ));
    }
    if let Some(similar) = similar {
        name_hints.push(t!("request.define_variable.similar", name = similar));
    }

    v_flex()
        .gap(px(14.0))
        .child(labeled_field(
            t!("request.define_variable.variable"),
            v_flex()
                .gap(px(6.0))
                .child(
                    Input::new(&name_input)
                        .context_menu(edit_menu(&name_input, cx))
                        .py_0()
                        .w_full(),
                )
                // One line, always reserved, so the fields below do not move as hints come
                // and go while the name is typed.
                .child(
                    div()
                        .h(px(HINT_LINE_HEIGHT))
                        .truncate()
                        .child(field_hint(name_hints.join(" \u{b7} "), &palette)),
                ),
        ))
        .child(labeled_field(
            t!("common.value"),
            Input::new(&value_input)
                .context_menu(edit_menu(&value_input, cx))
                .py_0()
                .w_full(),
        ))
        .child(labeled_field(
            t!("request.define_variable.environment"),
            v_flex()
                .gap(px(6.0))
                .child(render_environment_select(
                    weak.clone(),
                    &environments,
                    creating_environment,
                    environment.clone(),
                    &palette,
                ))
                .when(will_activate, |el| {
                    el.child(field_hint(
                        t!("request.define_variable.will_activate"),
                        &palette,
                    ))
                }),
        ))
        .children(creating_environment.then(|| {
            labeled_field(
                t!("request.define_variable.new_environment_name"),
                Input::new(&new_environment_input)
                    .context_menu(edit_menu(&new_environment_input, cx))
                    .py_0()
                    .w_full()
                    .into_any_element(),
            )
        }))
        .child(
            Switch::new("define-variable-store-local")
                .checked(store_local)
                .label(t!("request.define_variable.store_local").into_owned())
                .on_click(move |checked, _, cx| {
                    let checked = *checked;
                    let _ = switch_weak.update(cx, |view, cx| {
                        if let Some(state) = &mut view.define_variable {
                            state.store_local = checked;
                        }
                        cx.notify();
                    });
                })
                .into_any_element(),
        )
        .into_any_element()
}

/// A label above an arbitrary control, matching `views/settings.rs`'s field layout.
fn labeled_field(label: impl Into<SharedString>, control: impl IntoElement) -> AnyElement {
    v_flex()
        .gap(px(6.0))
        .child(div().text_size(px(12.0)).child(label.into()))
        .child(control)
        .into_any_element()
}

/// A short, subtle line of help under a field.
fn field_hint(text: impl Into<SharedString>, palette: &Palette) -> AnyElement {
    div()
        .text_size(px(12.0))
        .text_color(palette.fg_subtle)
        .child(text.into())
        .into_any_element()
}

/// The environment select: a [`select_trigger`] showing the current pick, opening a dropdown of
/// every existing environment plus "New environment...".
fn render_environment_select(
    weak: WeakEntity<AppView>,
    environments: &[String],
    creating_environment: bool,
    environment: Option<String>,
    palette: &Palette,
) -> AnyElement {
    let label = if creating_environment {
        t!("request.define_variable.new_environment").into_owned()
    } else {
        environment.clone().unwrap_or_default()
    };

    let trigger = select_trigger("define-variable-environment", palette)
        .w_full()
        .text_color(palette.fg)
        .child(select_label_row(div().child(label), palette));

    let environments = environments.to_vec();
    trigger
        .dropdown_menu(move |mut menu, _, _| {
            for name in &environments {
                let target = name.clone();
                let select_weak = weak.clone();
                let checked =
                    !creating_environment && environment.as_deref() == Some(name.as_str());
                menu = menu.item(PopupMenuItem::new(name.clone()).checked(checked).on_click(
                    move |_, _, cx| {
                        let target = target.clone();
                        let _ = select_weak.update(cx, |view, cx| {
                            if let Some(state) = &mut view.define_variable {
                                state.environment = Some(target);
                                state.creating_environment = false;
                            }
                            cx.notify();
                        });
                    },
                ));
            }
            menu = menu.separator();
            let select_weak = weak.clone();
            menu = menu.item(
                PopupMenuItem::new(t!("request.define_variable.new_environment"))
                    .checked(creating_environment)
                    .on_click(move |_, _, cx| {
                        let _ = select_weak.update(cx, |view, cx| {
                            if let Some(state) = &mut view.define_variable {
                                state.creating_environment = true;
                            }
                            cx.notify();
                        });
                    }),
            );
            menu
        })
        .into_any_element()
}
