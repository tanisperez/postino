//! [`IconButton`]: a small icon-only ghost button, in the 24 and 28 px sizes.
//!
//! Wraps gpui-kit's [`Button`] (ghost variant): its click, hover, disabled and tooltip handling
//! already work as is; only the icon color is pinned to [`crate::theme::Palette::fg_muted`]
//! through [`PaletteExt`], since a plain ghost button's default text color is not one of the
//! `ThemeConfigColors` fields the palette populates.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::theme::PaletteExt;
use crate::theme::metrics::ICON_BUTTON_LG;

/// An icon-only ghost button, a 24 or 28 px square.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: Icon,
    large: bool,
    box_size: Option<f32>,
    disabled: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

/// A boxed click handler, factored out because clippy's `type_complexity` flags the inline form.
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

impl IconButton {
    /// A small (24x24) icon button.
    pub fn new(id: impl Into<ElementId>, icon: impl Into<Icon>) -> Self {
        Self {
            id: id.into(),
            icon: icon.into(),
            large: false,
            box_size: None,
            disabled: false,
            tooltip: None,
            on_click: None,
        }
    }

    /// Switches to the large (28x28) size.
    pub fn large(mut self) -> Self {
        self.large = true;
        self
    }

    /// Keeps the small icon but makes the button a `size` x `size` square, for a roomier hover
    /// background.
    pub fn box_size(mut self, size: f32) -> Self {
        self.box_size = Some(size);
        self
    }

    /// Sets a tooltip shown on hover.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Disables the button (45% opacity, no click).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        let size = if self.large {
            gpui_kit::component::Size::Size(px(ICON_BUTTON_LG))
        } else {
            gpui_kit::component::Size::Small
        };

        let mut button = Button::new(self.id)
            .ghost()
            .with_size(size)
            .icon(self.icon)
            .text_color(palette.fg_muted)
            .disabled(self.disabled);
        if let Some(size) = self.box_size {
            button = button.size(px(size));
        }
        if let Some(tooltip) = self.tooltip {
            button = button.tooltip(tooltip);
        }
        if let Some(handler) = self.on_click {
            button = button.on_click(move |event, window, cx| handler(event, window, cx));
        }
        button
    }
}
