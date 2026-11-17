//! The not-focused rendering of a text field that holds `{{ }}` markers: plain text segments and
//! [`VariableChip`]s, each chip with a tooltip showing the variable's current value.
//!
//! A single-line `Input` cannot style byte ranges (see `docs/gpui-notes.md`, "Variable chips"), so
//! [`UrlBar`](super::url_bar::UrlBar) and [`VariableField`] show this row while the field is not
//! focused and swap to the live `Input` when it is. The body code editor does not need it: it
//! styles the markers in place with text decorations (`views/request_editor.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::{VariableKind, VariableSpan, variable_spans};

use super::HoverInput;
use super::IconButton;
use super::no_input_menu;
use super::variable_chip::VariableChip;
use crate::state::variable_hint::{Hint, Origin, VariableContext, copyable_value, hint};
use crate::theme::PaletteExt;
use crate::theme::metrics::{INPUT_PADDING_X, RADIUS_MD, VARIABLE_FIELD_HEIGHT};
use telaio_ui::components::{EditActions, edit_entries, input_edit_state};

use crate::actions::{RedoRequestEdit, UndoRequestEdit};
use telaio_ui::components::{MenuArea, MenuEntries, MenuItem};

thread_local! {
    /// The chip whose right-click menu was opened last, until the pointer enters it again after
    /// the menu: its tooltip stays hidden meanwhile. One at a time, as there is one menu at a time.
    static MENU_CHIP: RefCell<Option<ElementId>> = const { RefCell::new(None) };
}

/// A text-carrying handler (chip click), factored out because clippy's `type_complexity` flags
/// the inline form.
pub type TextHandler = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// A handler without arguments (the eye button of a masked field), factored out for the same
/// reason as [`TextHandler`].
pub type ActionHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// The side of the eye button of a masked field.
const MASK_TOGGLE_SIZE: f32 = 20.0;

/// The lines of the tooltip for `hint`: `name = value`, where the value comes from, and a note
/// when it is hidden.
pub fn tooltip_lines(hint: &Hint) -> Vec<String> {
    let mut lines = Vec::new();
    match hint.value.as_deref() {
        Some("") => lines.push(format!("{} = {}", hint.name, t!("request.variable.empty"))),
        Some(value) => lines.push(format!("{} = {value}", hint.name)),
        None => lines.push(hint.name.clone()),
    }
    lines.push(
        match &hint.origin {
            Some(Origin::Environment(name)) => {
                t!("request.variable.origin_environment", name = name)
            }
            Some(Origin::Session) => t!("request.variable.origin_session"),
            None => t!("request.variable.script_defined"),
        }
        .into_owned(),
    );
    if hint.masked {
        lines.push(t!("request.variable.masked").into_owned());
    }
    lines
}

/// The right-click entries of the `{{variable}}` `span` (#266): **Copy value**, disabled while
/// the variable has no value or its value is hidden (`mask_all`, a sensitive name), and **Edit in
/// environment**, which calls `on_edit` with its name, disabled for a variable the session or a
/// script sets. Nothing for a template function.
pub fn variable_entries(
    span: &VariableSpan,
    context: &VariableContext,
    mask_all: bool,
    on_edit: Option<TextHandler>,
) -> MenuEntries {
    if span.kind != VariableKind::Variable {
        return MenuEntries::new();
    }
    let value = copyable_value(context, span, mask_all);
    let copy = MenuItem::handler(t!("request.variable.copy_value"), {
        let value = value.clone();
        move |_, cx| {
            if let Some(value) = &value {
                cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
            }
        }
    })
    .disabled(value.is_none());
    let name = span.name.clone();
    let editable = on_edit.is_some() && context.opens_environment(&name);
    let edit = MenuItem::handler(
        t!("request.variable.edit_in_environment"),
        move |window, cx| {
            if let Some(on_edit) = &on_edit {
                on_edit(name.clone(), window, cx);
            }
        },
    )
    .disabled(!editable);
    MenuEntries::new().item(copy).item(edit)
}

/// Whether the active request tab can undo and redo, read when a field's menu opens: the request
/// editor's fields undo the whole request, as Ctrl+Z does there (#109, #264).
pub type RequestHistory = Rc<dyn Fn(&App) -> (bool, bool)>;

/// The right-click menu box of a focused field holding `{{ }}` markers: the standard edit entries
/// and, when the cursor is on a variable (a right click moves it there), its
/// [`variable_entries`]. With `history`, Undo and Redo are the request's (the same actions as
/// Ctrl+Z, with their keys shown) and enabled from it, not the field's own.
pub fn variable_menu_area(
    state: &Entity<InputState>,
    context: Rc<VariableContext>,
    mask_all: bool,
    on_edit: Option<TextHandler>,
    history: Option<RequestHistory>,
) -> MenuArea {
    let input = state.clone();
    MenuArea::new(
        ("telaio-edit-menu", state.entity_id()),
        move |menu, window, cx| {
            let mut edit = input_edit_state(&input, cx);
            let mut actions = EditActions::input();
            if let Some(history) = &history {
                let (can_undo, can_redo) = history(cx);
                edit.can_undo = Some(can_undo);
                edit.can_redo = Some(can_redo);
                actions.undo = Box::new(UndoRequestEdit);
                actions.redo = Box::new(RedoRequestEdit);
            }
            let entries = edit_entries(edit, actions);
            let state = input.read(cx);
            let cursor = state.cursor();
            let text = state.value();
            let span = variable_spans(&text)
                .into_iter()
                .find(|span| span.range.start <= cursor && cursor <= span.range.end);
            let entries = match span {
                Some(span) => {
                    entries.section(variable_entries(&span, &context, mask_all, on_edit.clone()))
                }
                None => entries,
            };
            let focus = state.focus_handle(cx);
            entries.render(menu.action_context(focus), window, cx)
        },
    )
}

/// The tooltip text for `hint`, one line per entry of [`tooltip_lines`].
pub fn tooltip_text(hint: &Hint) -> String {
    tooltip_lines(hint).join("\n")
}

/// The children of a line showing `text`: the plain segments between `spans` and one chip per
/// span. A chip is defined or in red according to `context`, shows a tooltip when defined, and
/// calls `on_chip_click` with the variable's name when clicked. `mask_all` hides every value in
/// the tooltips (a variable inside a sensitive header). `id` keeps the chips' element ids apart
/// from other lines on screen.
pub fn marker_children(
    id: &SharedString,
    text: &str,
    spans: &[VariableSpan],
    context: &Rc<VariableContext>,
    mask_all: bool,
    on_chip_click: Option<&TextHandler>,
) -> Vec<AnyElement> {
    let mut children = Vec::new();
    let mut cursor = 0;
    for span in spans {
        if span.range.start > cursor {
            children.push(
                text[cursor..span.range.start]
                    .to_string()
                    .into_any_element(),
            );
        }
        let defined = context.is_defined(span);
        let marker_text = text[span.range.clone()].to_string();
        let name = span.name.clone();
        let chip_handler = on_chip_click.cloned();
        let mut chip = div()
            .id((id.clone(), span.range.start))
            .cursor_pointer()
            .child(VariableChip::new(marker_text, defined));
        let chip_key = ElementId::from((id.clone(), span.range.start));
        if let Some(hint) = hint(context, span, mask_all) {
            let tooltip = SharedString::from(tooltip_text(&hint));
            let key = chip_key.clone();
            chip = chip.tooltip(move |window, cx| {
                // Not over the chip's own right-click menu (#266).
                if MENU_CHIP.with_borrow(|open| open.as_ref() == Some(&key)) {
                    return cx.new(|_| EmptyView).into();
                }
                Tooltip::new(tooltip.clone()).build(window, cx)
            });
        }
        // Only a variable chip calls the handler, not a template function call. A handler that
        // acts on the click stops its propagation, or the line around the chip also takes it and
        // focuses its input.
        if let Some(handler) = on_chip_click
            && span.kind == VariableKind::Variable
        {
            let handler = handler.clone();
            chip = chip.on_click(move |_, window, cx| handler(name.clone(), window, cx));
        }
        // A right click on a variable chip opens its entries (#266), where a click acts on it.
        // gpui hides the chip's tooltip on the click but shows it again while the pointer stays on
        // the chip, over the menu: the chip keeps it hidden until the pointer enters it again.
        let chip = match chip_handler {
            Some(handler) if span.kind == VariableKind::Variable => {
                let key = chip_key.clone();
                let chip = chip
                    .on_mouse_down(MouseButton::Right, move |_, _, _| {
                        MENU_CHIP.set(Some(key.clone()));
                    })
                    // The open menu covers the chip, which counts as leaving it; entering it
                    // again happens once the menu is closed.
                    .on_hover(move |hovered, _, _| {
                        if *hovered {
                            MENU_CHIP.with_borrow_mut(|open| {
                                if open.as_ref() == Some(&chip_key) {
                                    *open = None;
                                }
                            });
                        }
                    });
                let menu_span = span.clone();
                let menu_context = context.clone();
                MenuArea::new(
                    (id.clone(), span.range.start + 1),
                    move |menu, window, cx| {
                        variable_entries(&menu_span, &menu_context, mask_all, Some(handler.clone()))
                            .render(menu, window, cx)
                    },
                )
                .child(chip)
                .into_any_element()
            }
            _ => chip.into_any_element(),
        };
        children.push(chip);
        cursor = span.range.end;
    }
    if cursor < text.len() {
        children.push(text[cursor..].to_string().into_any_element());
    }
    children
}

/// `text` with every character outside its `{{ }}` markers replaced by a bullet.
fn mask_literals(text: &str) -> String {
    let mut masked = String::with_capacity(text.len());
    let mut cursor = 0;
    for span in variable_spans(text) {
        masked.extend(text[cursor..span.range.start].chars().map(|_| '\u{2022}'));
        masked.push_str(&text[span.range.clone()]);
        cursor = span.range.end;
    }
    masked.extend(text[cursor..].chars().map(|_| '\u{2022}'));
    masked
}

/// A single-line field of the request editor (a Params, Headers or Form cell, an Auth field): the
/// live `Input` while focused or while its text has no marker, a row of chips otherwise.
#[derive(IntoElement)]
pub struct VariableField {
    id: SharedString,
    input_state: Entity<InputState>,
    text: SharedString,
    context: Rc<VariableContext>,
    mask_all: bool,
    masked: bool,
    on_chip_click: Option<TextHandler>,
    on_mask_toggle: Option<ActionHandler>,
    history: Option<RequestHistory>,
}

impl VariableField {
    /// A field for `input_state`, whose current content is `text`. `id` must be unique on screen.
    pub fn new(
        id: impl Into<SharedString>,
        input_state: Entity<InputState>,
        text: impl Into<SharedString>,
        context: Rc<VariableContext>,
    ) -> Self {
        Self {
            id: id.into(),
            input_state,
            text: text.into(),
            context,
            mask_all: false,
            masked: false,
            on_chip_click: None,
            on_mask_toggle: None,
            history: None,
        }
    }

    /// Hides the text around the markers behind bullets while not focused, for a secret. The
    /// input itself must be masked too (`InputState::masked`).
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// Hides the value of every variable in the tooltips, for the value of a sensitive header.
    pub fn mask_all(mut self, mask_all: bool) -> Self {
        self.mask_all = mask_all;
        self
    }

    /// Adds an eye button at the end of the field, which calls `handler` to switch between the
    /// masked and the plain text. The handler changes both [`Self::masked`] and the input's own
    /// masking.
    pub fn on_mask_toggle(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_mask_toggle = Some(Rc::new(handler));
        self
    }

    /// Sets the handler for clicking a chip, called with the variable's name.
    pub fn on_chip_click(
        mut self,
        handler: impl Fn(String, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_chip_click = Some(Rc::new(handler));
        self
    }

    /// Makes the menu's Undo and Redo the request's, see [`variable_menu_area`].
    pub fn history(mut self, history: RequestHistory) -> Self {
        self.history = Some(history);
        self
    }
}

impl RenderOnce for VariableField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // A field whose right-click menu is open keeps its selection focus and stays the field: the
        // menu is anchored to it and would close if the field turned back into its line of chips.
        let focused = {
            let state = self.input_state.read(cx);
            state.focus_handle(cx).is_focused(window) || state.has_selection_focus(window, cx)
        };
        let text = if self.masked {
            SharedString::from(mask_literals(&self.text))
        } else {
            self.text.clone()
        };
        let spans = if focused || !text.contains("{{") {
            Vec::new()
        } else {
            variable_spans(&text)
        };
        let mask_toggle = self.on_mask_toggle.map(|handler| {
            let (icon, tooltip) = if self.masked {
                (IconName::Eye, t!("request.auth.show_secret"))
            } else {
                (IconName::EyeOff, t!("request.auth.hide_secret"))
            };
            IconButton::new(SharedString::from(format!("{}-mask", self.id)), icon)
                .box_size(MASK_TOGGLE_SIZE)
                .tooltip(tooltip.into_owned())
                .on_click(move |_, window, cx| {
                    // The chips row around the button focuses the input on a click.
                    cx.stop_propagation();
                    handler(window, cx);
                })
        });
        if spans.is_empty() {
            let accent = cx.palette().accent;
            let input = Input::new(&self.input_state)
                .context_menu(no_input_menu())
                .px(px(INPUT_PADDING_X))
                .py_0()
                // The same height as the row of chips, smaller than the default 32 px so it stays
                // clear of the table's row lines.
                .h(px(VARIABLE_FIELD_HEIGHT))
                .w_full()
                // The default 3 px focus ring spills over the table rows around the cell,
                // so a focused cell only gets the accent border.
                .focus_ring(false)
                .when(focused, |input| input.border_color(accent))
                .when_some(mask_toggle, |input, toggle| input.suffix(toggle));
            return variable_menu_area(
                &self.input_state,
                self.context.clone(),
                self.mask_all || self.masked,
                self.on_chip_click.clone(),
                self.history.clone(),
            )
            .w_full()
            .child(HoverInput::new(
                format!("{}-input", self.id),
                &self.input_state,
                input,
            ))
            .into_any_element();
        }

        let input_state = self.input_state.clone();
        let palette = cx.palette();
        let border_strong = palette.border_strong;
        let children = marker_children(
            &self.id,
            &text,
            &spans,
            &self.context,
            self.mask_all,
            self.on_chip_click.as_ref(),
        );
        h_flex()
            .id(self.id.clone())
            .w_full()
            .h(px(VARIABLE_FIELD_HEIGHT))
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_text()
            .px(px(INPUT_PADDING_X))
            .border_1()
            .border_color(palette.border)
            .hover(move |style| style.border_color(border_strong))
            .rounded(px(RADIUS_MD))
            .on_click(move |_, window, cx| {
                // `character: u32::MAX` clamps to the end of the line, which also focuses the
                // input and so swaps this row for it on the next render.
                let end = gpui_kit::base::input::Position::new(0, u32::MAX);
                input_state.update(cx, |state, cx| state.set_cursor_position(end, window, cx));
            })
            .map(|row| match mask_toggle {
                // The button stays at the end while the chips are clipped before it.
                Some(toggle) => row
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .items_center()
                            .children(children),
                    )
                    .child(div().flex_shrink_0().child(toggle)),
                None => row.children(children),
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{TextHandler, mask_literals, variable_entries};
    use crate::state::variable_hint::VariableContext;
    use postino_core::{KeyValue, variable_spans};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;
    use std::rc::Rc;
    use telaio_ui::components::MenuEntry;

    /// `(label, enabled)` of each entry of `span`'s menu.
    fn entries(text: &str, on_edit: Option<TextHandler>) -> Vec<(String, bool)> {
        let context = VariableContext::new(
            Some("dev".to_string()),
            &[
                KeyValue::new("baseUrl", "https://x"),
                KeyValue::new("authorization", "Bearer t"),
            ],
            &[KeyValue::new("region", "eu")],
            HashSet::new(),
        );
        let span = variable_spans(text).remove(0);
        variable_entries(&span, &context, false, on_edit)
            .visible()
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item(item) => Some((item.label().to_string(), !item.is_disabled())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn variable_entries_follow_what_the_variable_allows() {
        let edit: Option<TextHandler> = Some(Rc::new(|_, _, _| {}));
        let both = |copy, edit| {
            vec![
                ("Copy value".to_string(), copy),
                ("Edit in environment".to_string(), edit),
            ]
        };
        assert_eq!(entries("{{baseUrl}}", edit.clone()), both(true, true));
        // Unknown: nothing to copy, but it can be defined.
        assert_eq!(entries("{{missing}}", edit.clone()), both(false, true));
        // A hidden value, and one the session sets.
        assert_eq!(
            entries("{{authorization}}", edit.clone()),
            both(false, true)
        );
        assert_eq!(entries("{{region}}", edit.clone()), both(true, false));
        // Without a way to edit, and for a template function.
        assert_eq!(entries("{{baseUrl}}", None), both(true, false));
        assert!(entries("{{ uuid() }}", edit).is_empty());
    }

    #[test]
    fn mask_literals_keeps_only_the_markers() {
        assert_eq!(mask_literals("pässw"), "\u{2022}".repeat(5));
        assert_eq!(
            mask_literals("a{{ secret }}bc"),
            "\u{2022}{{ secret }}\u{2022}\u{2022}"
        );
        assert_eq!(mask_literals("{{x}}"), "{{x}}");
    }
}
