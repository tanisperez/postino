//! [`PrimaryButton`], [`SecondaryButton`], [`GhostButton`] and [`DangerButton`]: the four button
//! kinds of `Components.dc.html`'s "Buttons" swatch (`plans/ui-redesign.md` phase 3).
//!
//! All four wrap gpui-kit's [`Button`], built on its `ghost` variant (transparent background, no
//! border edges drawn) with background, text color and (for `SecondaryButton`) border pinned
//! explicitly to [`crate::theme::Palette`] values through `Styled`. This sidesteps a real gap in
//! gpui-component's own variants: its built-in `Danger` (and `Custom`) background formula always
//! mixes the given color 20% toward transparent before painting
//! (`gpui-component-0.6.6/src/button/button.rs`'s `ButtonVariant::bg_color`,
//! `Self::Custom(colors) => colors.color.mix_oklab(cx.theme().transparent, 0.2)`), with no public
//! way to turn that off, so a plain `.danger()`/`.custom(..)` cannot render the flat colors the
//! design calls for. Starting from `.ghost()` and overriding through `Styled` avoids that formula
//! entirely and keeps every color traceable to the palette.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::theme::PaletteExt;
use crate::theme::metrics::{CONTROL_HEIGHT, RADIUS_MD};

/// Opacity applied to a disabled button (`plans/ui-redesign.md` phase 3, "disabled at 45%
/// opacity").
const DISABLED_OPACITY: f32 = 0.45;

/// Builder state shared by the four button kinds in this file. Not public: each kind exposes its
/// own constructor and forwards its builder methods to this.
struct ButtonSpec {
    id: ElementId,
    label: SharedString,
    key_hint: Option<SharedString>,
    icon: Option<Icon>,
    height: f32,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

/// A boxed click handler, factored out because clippy's `type_complexity` flags the inline form.
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

impl ButtonSpec {
    fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            key_hint: None,
            icon: None,
            height: CONTROL_HEIGHT,
            disabled: false,
            on_click: None,
        }
    }

    /// Renders `self` as a gpui-kit [`Button`] with `bg`/`fg` pinned, and `border` drawn
    /// (`border_strong`-weight, [`SecondaryButton`]'s look) when given.
    fn render(self, bg: Hsla, fg: Hsla, border: Option<Hsla>, cx: &mut App) -> Button {
        let key_hint_font = cx.theme().mono_font_family.clone();
        let mut button = Button::new(self.id)
            .ghost()
            .h(px(self.height))
            .rounded(px(RADIUS_MD))
            .bg(bg)
            .text_color(fg)
            .font_weight(FontWeight::MEDIUM)
            .disabled(self.disabled)
            .when(self.disabled, |button| button.opacity(DISABLED_OPACITY))
            .when_some(self.icon, |button, icon| button.icon(icon))
            .label(self.label);
        if let Some(border) = border {
            button = button.border_1().border_color(border);
        }
        if let Some(hint) = self.key_hint {
            button = button.child(
                div()
                    .font_family(key_hint_font)
                    .text_size(px(10.5))
                    .opacity(0.75)
                    .child(hint),
            );
        }
        if let Some(handler) = self.on_click {
            button = button.on_click(move |event, window, cx| handler(event, window, cx));
        }
        button
    }
}

/// The Send button and other primary actions: `accent` background, `accent_fg` text.
#[derive(IntoElement)]
pub struct PrimaryButton {
    spec: ButtonSpec,
}

impl PrimaryButton {
    /// A primary button labeled `label`.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            spec: ButtonSpec::new(id, label),
        }
    }

    /// Sets a trailing mono key hint (for example `"Ctrl \u{21b5}"`).
    pub fn key_hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.spec.key_hint = Some(hint.into());
        self
    }

    /// Sets the button's height (defaults to [`CONTROL_HEIGHT`]).
    pub fn height(mut self, height: f32) -> Self {
        self.spec.height = height;
        self
    }

    /// Disables the button (45% opacity, no click).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.spec.disabled = disabled;
        self
    }

    /// Sets the click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.spec.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for PrimaryButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        self.spec
            .render(palette.accent, palette.accent_fg, None, cx)
    }
}

/// A secondary action: `raised` background, `border_strong` border, default text color.
#[derive(IntoElement)]
pub struct SecondaryButton {
    spec: ButtonSpec,
}

impl SecondaryButton {
    /// A secondary button labeled `label`.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            spec: ButtonSpec::new(id, label),
        }
    }

    /// Sets the button's height (defaults to [`CONTROL_HEIGHT`]).
    #[allow(dead_code)] // no caller outside the components gallery needs this yet
    pub fn height(mut self, height: f32) -> Self {
        self.spec.height = height;
        self
    }

    /// Disables the button (45% opacity, no click).
    #[allow(dead_code)] // no caller outside the components gallery needs this yet
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.spec.disabled = disabled;
        self
    }

    /// Sets the click handler.
    #[allow(dead_code)] // no caller outside the components gallery needs this yet
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.spec.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SecondaryButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        self.spec
            .render(palette.raised, palette.fg, Some(palette.border_strong), cx)
    }
}

/// A low-emphasis action: transparent background, `fg_muted` text, optional icon (`Components
/// .dc.html`'s "Ghost" swatch, for example the "Format"/"Import" buttons).
#[derive(IntoElement)]
pub struct GhostButton {
    spec: ButtonSpec,
}

impl GhostButton {
    /// A ghost button labeled `label`.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            spec: ButtonSpec::new(id, label),
        }
    }

    /// Sets a leading icon.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.spec.icon = Some(icon.into());
        self
    }

    /// Sets the button's height (defaults to [`CONTROL_HEIGHT`]).
    #[allow(dead_code)] // no caller needs a non-default height yet
    pub fn height(mut self, height: f32) -> Self {
        self.spec.height = height;
        self
    }

    /// Disables the button (45% opacity, no click).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.spec.disabled = disabled;
        self
    }

    /// Sets the click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.spec.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for GhostButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        self.spec
            .render(palette.bg.opacity(0.0), palette.fg_muted, None, cx)
    }
}

/// A destructive action: `danger` background, white text.
#[derive(IntoElement)]
pub struct DangerButton {
    spec: ButtonSpec,
}

impl DangerButton {
    /// A danger button labeled `label`.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            spec: ButtonSpec::new(id, label),
        }
    }

    /// Sets the button's height (defaults to [`CONTROL_HEIGHT`]).
    #[allow(dead_code)] // no caller outside the components gallery needs this yet
    pub fn height(mut self, height: f32) -> Self {
        self.spec.height = height;
        self
    }

    /// Disables the button (45% opacity, no click).
    #[allow(dead_code)] // no caller outside the components gallery needs this yet
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.spec.disabled = disabled;
        self
    }

    /// Sets the click handler.
    #[allow(dead_code)] // no caller outside the components gallery needs this yet
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.spec.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for DangerButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        // `accent_fg` is opaque white in both themes (`theme::palette::LIGHT`/`DARK`), which is
        // also the design's literal `color:#fff` for this button, so it is reused here rather
        // than adding a second, redundant "always white" field to `Palette`.
        self.spec
            .render(palette.danger, palette.accent_fg, None, cx)
    }
}
