//! The Define variable dialog (`plans/ui-redesign.md` phase 7 item 3): opened from a response
//! pane "Define" warning action (`views/response_view.rs`) or a click on a danger chip in the
//! URL (`views/request_editor.rs`, through `UrlBar::on_chip_click`). Asks for the variable's
//! value and which environment to write it to, then calls
//! [`postino_workspace::Workspace::set_environment_var`]. Reuses `views/settings.rs`'s dialog
//! patterns (a plain `Dialog` with custom content, a font-picker-style dropdown for the
//! environment select).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::state::define_variable;
use crate::theme::metrics::{CONTROL_HEIGHT, RADIUS_MD};
use crate::theme::{Palette, PaletteExt};

use super::root::AppView;

/// Width of the dialog.
const DIALOG_WIDTH: f32 = 420.0;

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
}

impl AppView {
    /// Opens the Define variable dialog for `name` (`plans/ui-redesign.md` phase 7 item 3). A
    /// no-op when a dialog is already open, or when there is no workspace to write into.
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

        let name_input = cx.new(|cx| InputState::new(window, cx));
        name_input.update(cx, |state, cx| {
            state.set_value(name.clone(), window, cx);
        });
        let value_input = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let new_environment_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Environment name"));

        self.define_variable = Some(DefineVariableState {
            name_input,
            value_input,
            new_environment_input,
            environment,
            creating_environment,
            store_local: define_variable::looks_sensitive(&name),
        });
        cx.notify();

        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            // `open_dialog`'s builder is `Fn`, not `FnOnce`, so every closure below clones its
            // own copy of `weak` from this outer, persistently-captured one, rather than moving
            // it: a `move` closure that consumed a captured variable outright would make this
            // outer closure only callable once, which the `Fn` bound rejects.
            let content_weak = weak.clone();
            let ok_weak = weak.clone();
            let close_weak = weak.clone();
            dialog
                .title("Define variable")
                .w(px(DIALOG_WIDTH))
                .button_props(DialogButtonProps::default().ok_text("Save"))
                .content(move |content, window, cx| {
                    content.min_h_0().child(render_define_variable_body(
                        content_weak.clone(),
                        window,
                        cx,
                    ))
                })
                .on_ok(move |_, _, cx| {
                    let _ = ok_weak.update(cx, |view, cx| view.save_define_variable(cx));
                    true
                })
                .on_close(move |_, _, cx| {
                    let _ = close_weak.update(cx, |view, _cx| view.define_variable = None);
                })
        });
    }

    /// Writes the dialog's current name/value/environment/local choice with
    /// [`postino_workspace::Workspace::set_environment_var`], makes the target environment the
    /// active one if none was active, and clears the error banner. Silently does nothing if the
    /// name or the target environment is empty, matching `views/sidebar.rs`'s new
    /// request/folder dialogs.
    fn save_define_variable(&mut self, cx: &mut Context<Self>) {
        let Some(state) = &self.define_variable else {
            return;
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
        let store_local = state.store_local;
        if name.is_empty() || target_env.is_empty() {
            return;
        }
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        match workspace.set_environment_var(&target_env, &name, &value, store_local) {
            Ok(()) => {
                self.workspace_error = None;
                if self.state.active_environment.is_none() {
                    self.state.active_environment = Some(target_env);
                }
            }
            Err(error) => {
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }
}

/// Renders the dialog's content: variable name, value, environment select (and, while creating
/// one, its name), and the "Store in .local.env" switch.
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
        )
    };
    let environments = view
        .read(cx)
        .state
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.list_environments().ok())
        .unwrap_or_default();
    let palette = cx.palette();

    let switch_weak = weak.clone();

    v_flex()
        .gap(px(14.0))
        .child(labeled_field("Variable", Input::new(&name_input).w_full()))
        .child(labeled_field("Value", Input::new(&value_input).w_full()))
        .child(labeled_field(
            "Environment",
            render_environment_select(
                weak.clone(),
                &environments,
                creating_environment,
                environment.clone(),
                &palette,
            ),
        ))
        .children(creating_environment.then(|| {
            labeled_field(
                "New environment name",
                Input::new(&new_environment_input)
                    .w_full()
                    .into_any_element(),
            )
        }))
        .child(
            Switch::new("define-variable-store-local")
                .checked(store_local)
                .label("Store in .local.env (not versioned)")
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
fn labeled_field(label: &'static str, control: impl IntoElement) -> AnyElement {
    v_flex()
        .gap(px(6.0))
        .child(div().text_size(px(12.0)).child(label))
        .child(control)
        .into_any_element()
}

/// The environment select: a font-picker-styled trigger (`views/settings.rs`'s
/// `render_font_picker`) showing the current pick, opening a dropdown of every existing
/// environment plus "New environment...".
fn render_environment_select(
    weak: WeakEntity<AppView>,
    environments: &[String],
    creating_environment: bool,
    environment: Option<String>,
    palette: &Palette,
) -> AnyElement {
    let label = if creating_environment {
        "New environment...".to_string()
    } else {
        environment.clone().unwrap_or_default()
    };

    let trigger = Button::new("define-variable-environment")
        .ghost()
        .w_full()
        .h(px(CONTROL_HEIGHT))
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .text_color(palette.fg)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(div().child(label))
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .small()
                        .text_color(palette.fg_subtle),
                ),
        );

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
                PopupMenuItem::new("New environment...")
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
