//! [`SectionLabel`]: the uppercase section heading used for "COLLECTIONS" and every
//! gallery section title (`plans/ui-redesign.md` phase 3).
//!
//! Custom element: a plain styled `div`, since this is just fixed typography with no
//! interactivity.
//!
//! Deviation: the design's 0.06em letter-spacing ("tracking") has no equivalent in gpui's text
//! style (`gpui-pre-0.3.6/src/style.rs` has no letter-spacing field), so this renders without
//! it; see this phase's report.

use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::theme::PaletteExt;

/// An uppercase, 11px/600 heading, colored `fg_subtle` (section titles such as
/// the sidebar's "COLLECTIONS").
#[derive(IntoElement)]
pub struct SectionLabel {
    text: SharedString,
}

impl SectionLabel {
    /// A label showing `text`, uppercased regardless of the case it is given in.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into() }
    }
}

impl RenderOnce for SectionLabel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        div()
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.fg_subtle)
            .child(self.text.to_string().to_uppercase())
    }
}
