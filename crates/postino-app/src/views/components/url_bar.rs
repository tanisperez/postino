//! [`UrlBar`]: the method selector plus URL field joined in one bordered box
//! (`plans/ui-redesign.md` phase 3, design reference `Main A.dc.html`).
//!
//! Follows the fallback `plans/ui-redesign-spikes.md` section 3 settled on for variable chips in
//! a single-line input: gpui-component's `Input`/`InputState` cannot style byte ranges (only its
//! multi-line, language-aware `Editor` can), so `UrlBar` renders a row of plain text and
//! [`VariableChip`] spans while the field is not focused, and swaps to a live `Input` bound to
//! the caller's own `Entity<InputState>` once it is. Wiring `UrlBar` into the real request
//! editor (creating that entity, subscribing to its change events, calling
//! `postino_runner::preview` on every edit) is phase 5's job; this component only renders and
//! forwards clicks.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use postino_core::{Method, VariableSpan};

use super::variable_chip::VariableChip;
use crate::theme::PaletteExt;
use crate::theme::metrics::{RADIUS_MD, URL_BAR_HEIGHT};

/// The standard methods offered by the selector, before the trailing "Custom..." entry.
const STANDARD_METHODS: [Method; 7] = [
    Method::Get,
    Method::Post,
    Method::Put,
    Method::Patch,
    Method::Delete,
    Method::Head,
    Method::Options,
];

/// The method selector joined to a URL field, with variable chips styled per
/// `plans/ui-redesign.md` section 2.1: `accent` for a defined `{{var}}`, `danger` with a wavy
/// underline for an unknown one.
#[derive(IntoElement)]
pub struct UrlBar {
    method: Method,
    text: SharedString,
    /// Every `{{ }}` marker in `text` (`postino_core::variable_spans`), for styling.
    spans: Vec<VariableSpan>,
    /// Names, among `spans`, that [`postino_runner::preview`] reported as unknown. A name not in
    /// this set (and not a function call) renders as defined.
    unknown_names: HashSet<String>,
    input_state: Entity<InputState>,
    on_method_change: Option<MethodHandler>,
    on_chip_click: Option<TextHandler>,
    /// Fired when the URL text changes. `UrlBar` is a `RenderOnce` (no `Context<Self>`), so it
    /// cannot subscribe to `input_state`'s own change events itself; the owning view does that
    /// subscription (as `views/request_editor.rs` already does for its other inputs) and calls
    /// this same callback from there. Kept on the builder so the contract lives in one place.
    #[allow(dead_code)] // read by phase 5, once UrlBar is wired into the live request editor
    on_text_change: Option<TextHandler>,
}

/// A method change handler, factored out because clippy's `type_complexity` flags the inline
/// form.
type MethodHandler = Rc<dyn Fn(Method, &mut Window, &mut App)>;
/// A text-carrying handler (chip click, text change), see [`MethodHandler`].
type TextHandler = Rc<dyn Fn(String, &mut Window, &mut App)>;

impl UrlBar {
    /// A bar showing `method` and `text`, with `spans` (`postino_core::variable_spans(text)`)
    /// and `unknown_names` (the names `postino_runner::preview` reported as unknown) driving
    /// chip colors. `input_state` backs the live `Input` shown once the bar is focused.
    pub fn new(
        method: Method,
        text: impl Into<SharedString>,
        spans: Vec<VariableSpan>,
        unknown_names: HashSet<String>,
        input_state: Entity<InputState>,
    ) -> Self {
        Self {
            method,
            text: text.into(),
            spans,
            unknown_names,
            input_state,
            on_method_change: None,
            on_chip_click: None,
            on_text_change: None,
        }
    }

    /// Sets the handler for picking a different method.
    #[allow(dead_code)] // wired by phase 5, once UrlBar backs the real request editor
    pub fn on_method_change(
        mut self,
        handler: impl Fn(Method, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_method_change = Some(Rc::new(handler));
        self
    }

    /// Sets the handler for clicking a variable chip, called with the variable's name.
    #[allow(dead_code)] // wired by phase 5, once UrlBar backs the real request editor
    pub fn on_chip_click(
        mut self,
        handler: impl Fn(String, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_chip_click = Some(Rc::new(handler));
        self
    }

    /// Sets the URL text change handler (see the field's own doc comment for how it is
    /// actually invoked).
    #[allow(dead_code)] // wired by phase 5, once UrlBar backs the real request editor
    pub fn on_text_change(
        mut self,
        handler: impl Fn(String, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_text_change = Some(Rc::new(handler));
        self
    }

    /// Renders the method selector: the method's label in its own color, a dropdown of the
    /// standard methods plus "Custom...".
    fn render_method(&self, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();
        let color = palette.method_color(&self.method);
        let label = self.method.to_string();
        let handler = self.on_method_change.clone();

        Button::new("url-bar-method")
            .ghost()
            .h(px(URL_BAR_HEIGHT))
            .px_3()
            .border_r_1()
            .border_color(palette.border)
            .dropdown_caret(true)
            .child(
                div()
                    .font_family(mono_font)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(12.0))
                    .text_color(color)
                    .child(label),
            )
            .dropdown_menu(move |mut menu, _, _| {
                for method in STANDARD_METHODS {
                    let handler = handler.clone();
                    menu = menu.item(PopupMenuItem::new(method.to_string()).on_click(
                        move |_, window, cx| {
                            if let Some(handler) = &handler {
                                handler(method.clone(), window, cx);
                            }
                        },
                    ));
                }
                menu = menu.separator();
                let handler = handler.clone();
                menu = menu.item(
                    PopupMenuItem::new("Custom...").on_click(move |_, window, cx| {
                        if let Some(handler) = &handler {
                            handler(Method::Custom(String::new()), window, cx);
                        }
                    }),
                );
                menu
            })
    }

    /// Renders the not-focused URL line: plain text segments and [`VariableChip`]s, each chip
    /// wrapped in its own clickable `div` (see this module's doc comment for why, instead of a
    /// single [`gpui::StyledText`] with highlight runs, which cannot carry a per-range click
    /// handler on its own).
    fn render_line(&self, cx: &mut App) -> AnyElement {
        let mono_font = cx.theme().mono_font_family.clone();
        let mut row = h_flex()
            .flex_1()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(mono_font)
            .text_size(px(12.5))
            .px_2();
        let mut cursor = 0;
        for span in &self.spans {
            if span.range.start > cursor {
                row = row.child(self.text[cursor..span.range.start].to_string());
            }
            let defined = span.kind == postino_core::VariableKind::FunctionCall
                || !self.unknown_names.contains(&span.name);
            let marker_text = self.text[span.range.clone()].to_string();
            let name = span.name.clone();
            let handler = self.on_chip_click.clone();
            let mut chip = div()
                .id(("url-bar-chip", span.range.start))
                .cursor_pointer()
                .child(VariableChip::new(marker_text, defined));
            if let Some(handler) = handler {
                chip = chip.on_click(move |_, window, cx| handler(name.clone(), window, cx));
            }
            row = row.child(chip);
            cursor = span.range.end;
        }
        if cursor < self.text.len() {
            row = row.child(self.text[cursor..].to_string());
        }
        row.into_any_element()
    }
}

impl RenderOnce for UrlBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        let focused = self
            .input_state
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);

        let field: AnyElement = if focused {
            Input::new(&self.input_state)
                .h(px(URL_BAR_HEIGHT))
                .bordered(false)
                .into_any_element()
        } else {
            self.render_line(cx)
        };

        h_flex()
            .h(px(URL_BAR_HEIGHT))
            .items_center()
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(palette.border_strong)
            .bg(palette.raised)
            .overflow_hidden()
            .child(self.render_method(cx))
            .child(field)
    }
}
