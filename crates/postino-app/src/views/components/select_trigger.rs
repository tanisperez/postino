//! [`select_trigger`]: a ghost `Button` restyled as a bordered select box, the trigger of a
//! dropdown menu (the selects of Settings, the load test target picker). [`select_label_row`] is
//! its usual content: the current pick on the left, a chevron on the right.
//!
//! The trigger is a `Button` because `DropdownMenu` (`gpui-component`'s trait for opening a
//! `PopupMenu` on click) is only implemented for `Button`
//! (`gpui-component-0.6.6/src/menu/dropdown_menu.rs`, `impl DropdownMenu for Button {}`), not for
//! a plain styled `div`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::theme::Palette;
use crate::theme::metrics::{CONTROL_HEIGHT, RADIUS_MD};

/// The bordered, raised select box, full height and without a width: the caller sets the width,
/// the content and the dropdown.
pub fn select_trigger(id: impl Into<ElementId>, palette: &Palette) -> Button {
    Button::new(id)
        .ghost()
        .h(px(CONTROL_HEIGHT))
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
}

/// `label` on the left and a chevron on the right, to give a [`select_trigger`] as its child.
///
/// Not `Button::label`/`Button::icon`: `Button` lays those out in its own inner row, which
/// hardcodes `justify_center()` on a style field this crate has no builder to reach
/// (`content_style`, `gpui-component-0.6.6/src/button/button.rs:207,289,712`, `pub(crate)`). A
/// single, full-width child of our own sidesteps that: `Button`'s row centers it (a no-op once it
/// fills the width), and this row's `justify_between()` places the label and the chevron.
pub fn select_label_row(label: impl IntoElement, palette: &Palette) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .child(label)
        .child(
            Icon::new(IconName::ChevronsUpDown)
                .small()
                .text_color(palette.fg_subtle),
        )
}
