//! The tooltip of the body editor: the value of the `{{ }}` marker under the pointer.
//!
//! The editor asks for a tooltip when the pointer moves over its text. The answer comes from the
//! markers `views/request_editor.rs` computed when the body last changed, so hovering never
//! rescans the text.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::SharedString;

use postino_core::VariableSpan;

use crate::state::variable_hint::{VariableContext, hint};
use crate::views::components::variable_line::tooltip_text;

/// The markers of the body and how they resolve, shared between the request editor (which
/// refreshes them) and the editor's tooltip (which reads them).
#[derive(Default)]
pub struct BodyMarkers {
    pub spans: Vec<VariableSpan>,
    pub context: Rc<VariableContext>,
}

impl BodyMarkers {
    /// The marker at byte `offset` and its tooltip: the value, where it comes from, and whether
    /// it is hidden. `None` off the markers or for a marker with nothing to tell.
    pub fn tooltip_at(&self, offset: usize) -> Option<(Range<usize>, SharedString)> {
        let span = self
            .spans
            .iter()
            .find(|span| span.range.contains(&offset))?;
        let hint = hint(&self.context, span, false)?;
        Some((span.range.clone(), tooltip_text(&hint).into()))
    }
}
