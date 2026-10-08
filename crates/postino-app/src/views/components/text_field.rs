//! [`text_field`]: a single-line `Input` for dialogs and forms, with the app's edit menu and a
//! line height that leaves room around the glyphs.
//!
//! The selection highlight of an `Input` is as tall as its line. gpui-component's default line
//! (1.25 rem) nearly fills a short box, so a selection touched both borders. A taller line, the
//! same one the URL bar and the environment rows use, gives the highlight room above and below.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::edit_menu::edit_menu;

/// Line height of a [`text_field`], and so the height of its selection highlight.
const FIELD_LINE_HEIGHT: f32 = 22.0;
/// Height of a [`text_field`] box: the line plus 6 px of breathing room on each side. Controls
/// that sit next to a field, such as a button, use it too so both line up.
pub(crate) const FIELD_HEIGHT: f32 = 36.0;

/// Builds the field for `state`, with the app's edit menu. The height is set on a wrapper: the
/// `Input` itself ignores a fixed height in a dialog's content.
pub fn text_field(state: &Entity<InputState>, cx: &App) -> Div {
    div().w_full().h(px(FIELD_HEIGHT)).child(
        Input::new(state)
            .context_menu(edit_menu(state, cx))
            .h_full()
            .py_0()
            .line_height(px(FIELD_LINE_HEIGHT)),
    )
}
