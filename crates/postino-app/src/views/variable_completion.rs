//! The variable autocompletion menu of the request editor's single-line fields (the URL and the
//! values of Params, Headers and Form, see [`CompletionField`]).
//!
//! It opens with `Ctrl+Space`, or by itself while a `{{` marker is being typed, at the start of
//! that marker. The input keeps the focus the whole time: the arrow keys, `Enter`, `Tab` and
//! `Escape` are bound in the `Input` context to the actions below, which act only while the menu
//! is open for the focused input and otherwise let the input's own binding run
//! (`cx.propagate()`). The logic (marker, query, filter, highlighted row) is in
//! `state::variable_completion`.

use std::ops::Range;

use gpui::{anchored, deferred};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::actions::{
    ConfirmVariableCompletion, DismissVariableCompletion, NextVariableCompletion,
    PreviousVariableCompletion, ShowVariableCompletion,
};
use crate::state::variable_completion::{CompletionMenu, filter, marker, trigger_at};
use crate::state::variable_hint::{Hint, completions};
use crate::theme::PaletteExt;
use crate::theme::metrics::{
    MENU_ROW_HEIGHT, RADIUS_LG, RADIUS_SM, VARIABLE_MENU_MAX_ROWS, VARIABLE_MENU_WIDTH,
};

use super::request_editor::CompletionField;
use super::root::AppView;

/// Gap between the field's text line and the menu below it.
const MENU_OFFSET_Y: f32 = 4.0;

/// The open menu.
pub(crate) struct ActiveCompletion {
    field: CompletionField,
    input: Entity<InputState>,
    /// The text of the field the chosen `{{name}}` replaces.
    replace: Range<usize>,
    menu: CompletionMenu,
    /// Shown below the rows: no environment is selected, so its variables are missing.
    no_environment: bool,
    /// Where the menu's top left corner goes, in window coordinates.
    anchor: Point<Pixels>,
    scroll: ScrollHandle,
}

impl AppView {
    /// `Ctrl+Space`: opens the menu in the focused field, or lets the key through elsewhere.
    pub(crate) fn on_show_variable_completion(
        &mut self,
        _: &ShowVariableCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.focused_completion_field(window, cx) {
            Some(field) => self.update_variable_completion(field, true, cx),
            None => cx.propagate(),
        }
    }

    /// The text of `field` changed: opens, filters or closes the menu after what was typed.
    pub(crate) fn on_completion_field_changed(
        &mut self,
        field: CompletionField,
        cx: &mut Context<Self>,
    ) {
        self.update_variable_completion(field, false, cx);
    }

    /// `field` lost the focus: its menu closes.
    pub(crate) fn on_completion_field_blurred(
        &mut self,
        field: CompletionField,
        cx: &mut Context<Self>,
    ) {
        if self
            .variable_completion
            .as_ref()
            .is_some_and(|open| open.field == field)
        {
            self.close_variable_completion(cx);
        }
    }

    /// Closes the menu, if it is open.
    pub(crate) fn close_variable_completion(&mut self, cx: &mut Context<Self>) {
        if self.variable_completion.take().is_some() {
            cx.notify();
        }
    }

    /// Opens or refreshes the menu of `field` for its current text and cursor, or closes it when
    /// the cursor is not in a `{{` marker (and `explicit`, `Ctrl+Space`, is not set) or nothing
    /// matches.
    fn update_variable_completion(
        &mut self,
        field: CompletionField,
        explicit: bool,
        cx: &mut Context<Self>,
    ) {
        let was_open = self
            .variable_completion
            .as_ref()
            .is_some_and(|open| open.field == field);
        let Some(input) = self.completion_input(field) else {
            self.close_variable_completion(cx);
            return;
        };
        let (text, cursor, selection) = {
            let state = input.read(cx);
            (state.value(), state.cursor(), state.selected_range())
        };
        let Some(trigger) = trigger_at(&text, cursor, selection, explicit) else {
            self.close_variable_completion(cx);
            return;
        };
        let context = self.request_editor_variable_context();
        let all = completions(&context, self.completion_masks_values(field));
        let items = filter(&all, &trigger.query);
        let no_environment = !context.has_environment();
        if items.is_empty() && !no_environment {
            self.close_variable_completion(cx);
            return;
        }
        let at = trigger.replace.start..trigger.replace.start;
        let state = input.read(cx);
        let anchor = state
            .range_to_bounds(&at)
            .or_else(|| state.cursor_layout().map(|(bounds, _)| bounds))
            .map_or_else(
                || state.input_bounds().bottom_left(),
                |bounds| bounds.bottom_left(),
            )
            + point(px(0.0), px(MENU_OFFSET_Y));

        // Keep the highlighted variable while the list narrows, if it is still there.
        let mut menu = CompletionMenu::new(items);
        if was_open
            && let Some(previous) = self
                .variable_completion
                .as_ref()
                .and_then(|open| open.menu.selected())
            && let Some(ix) = menu
                .items
                .iter()
                .position(|hint| hint.name == previous.name)
        {
            menu.selected = ix;
        }
        let scroll = self
            .variable_completion
            .take()
            .filter(|_| was_open)
            .map_or_else(ScrollHandle::new, |open| open.scroll);
        scroll.scroll_to_item(menu.selected);
        self.variable_completion = Some(ActiveCompletion {
            field,
            input,
            replace: trigger.replace,
            menu,
            no_environment,
            anchor,
            scroll,
        });
        cx.notify();
    }

    /// The open menu, when its input has the focus.
    fn focused_completion(&mut self, window: &Window, cx: &App) -> Option<&mut ActiveCompletion> {
        self.variable_completion
            .as_mut()
            .filter(|open| open.input.focus_handle(cx).is_focused(window))
    }

    /// `Down` while the menu is open: highlights the next variable.
    pub(crate) fn on_next_variable_completion(
        &mut self,
        _: &NextVariableCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.focused_completion(window, cx) {
            Some(open) => {
                open.menu.select_next();
                open.scroll.scroll_to_item(open.menu.selected);
                cx.notify();
            }
            None => cx.propagate(),
        }
    }

    /// `Up` while the menu is open: highlights the previous variable.
    pub(crate) fn on_previous_variable_completion(
        &mut self,
        _: &PreviousVariableCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.focused_completion(window, cx) {
            Some(open) => {
                open.menu.select_previous();
                open.scroll.scroll_to_item(open.menu.selected);
                cx.notify();
            }
            None => cx.propagate(),
        }
    }

    /// `Enter` or `Tab` while the menu is open: inserts the highlighted variable. With nothing to
    /// insert (only the "no environment" note), closes the menu and lets the key through.
    pub(crate) fn on_confirm_variable_completion(
        &mut self,
        _: &ConfirmVariableCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.focused_completion(window, cx) else {
            cx.propagate();
            return;
        };
        match open.menu.selected().map(|hint| hint.name.clone()) {
            Some(name) => self.insert_variable(name, window, cx),
            None => {
                self.close_variable_completion(cx);
                cx.propagate();
            }
        }
    }

    /// `Escape` while the menu is open: closes it.
    pub(crate) fn on_dismiss_variable_completion(
        &mut self,
        _: &DismissVariableCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focused_completion(window, cx).is_some() {
            self.close_variable_completion(cx);
        } else {
            cx.propagate();
        }
    }

    /// Replaces the marker being typed (or the selection) with `{{name}}`, closes the menu and
    /// reports the edit like typing does, so the request follows it.
    fn insert_variable(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.variable_completion.take() else {
            return;
        };
        open.input.update(cx, |state, cx| {
            state.set_selected_range(open.replace.clone(), cx);
            state.replace(marker(&name), window, cx);
            // `replace` is silent: emit the change the field's subscription writes back.
            cx.emit(InputEvent::Change);
        });
        cx.notify();
    }

    /// The open menu, drawn above everything else at its anchor.
    pub(crate) fn render_variable_completion(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let open = self.variable_completion.as_ref()?;
        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();
        let weak = cx.weak_entity();

        let rows = open.menu.items.iter().enumerate().map(|(ix, hint)| {
            let selected = ix == open.menu.selected;
            let name = hint.name.clone();
            let weak = weak.clone();
            h_flex()
                .id(("variable-completion", ix))
                .h(px(MENU_ROW_HEIGHT))
                .flex_none()
                .items_center()
                .gap_3()
                .px_2()
                .rounded(px(RADIUS_SM))
                .cursor_pointer()
                .when(selected, |this| this.bg(palette.accent_subtle))
                .when(!selected, |this| this.hover(|this| this.bg(palette.hover)))
                // On the press, before the click could take the focus from the input.
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    let name = name.clone();
                    let _ = weak.update(cx, |view, cx| view.insert_variable(name, window, cx));
                })
                .child(
                    div()
                        .flex_none()
                        .font_family(mono_font.clone())
                        .text_sm()
                        .text_color(if selected {
                            palette.accent_text
                        } else {
                            palette.fg
                        })
                        .child(hint.name.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_right()
                        .text_xs()
                        .text_color(palette.fg_subtle)
                        .child(value_label(hint)),
                )
        });

        let menu = v_flex()
            .id("variable-completion-menu")
            .w(px(VARIABLE_MENU_WIDTH))
            .p_1()
            .bg(palette.overlay)
            .border_1()
            .border_color(palette.border)
            .rounded(px(RADIUS_LG))
            .shadow(palette.shadow.clone())
            .on_mouse_down_out(cx.listener(|view, _, _, cx| view.close_variable_completion(cx)))
            .when(!open.menu.items.is_empty(), |this| {
                this.child(
                    v_flex()
                        .id("variable-completion-rows")
                        .max_h(px(MENU_ROW_HEIGHT * VARIABLE_MENU_MAX_ROWS))
                        .overflow_y_scroll()
                        .track_scroll(&open.scroll)
                        .children(rows),
                )
            })
            .when(open.no_environment, |this| {
                this.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(palette.fg_subtle)
                        .child(t!("request.variable.completion_no_environment")),
                )
            });

        Some(
            deferred(
                anchored()
                    .position(open.anchor)
                    .snap_to_window_with_margin(px(8.0))
                    .child(menu),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// What a row shows on the right: the value as in the tooltips (masked or shortened), or where it
/// comes from when the value is not known yet.
fn value_label(hint: &Hint) -> String {
    match hint.value.as_deref() {
        Some("") => t!("request.variable.empty").into_owned(),
        Some(value) => value.to_string(),
        None => t!("request.variable.completion_script").into_owned(),
    }
}
