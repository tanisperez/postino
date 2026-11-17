//! The tooltip and the right-click entries of the body editor's `{{ }}` markers: the value of the
//! marker under the pointer, and Copy value and Edit in environment on the one clicked (#266).
//!
//! Both read the markers `views/request_editor.rs` computed when the body last changed, so
//! hovering or opening the menu never rescans the text.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::{SharedString, WeakEntity};
use telaio_ui::components::MenuEntries;

use postino_core::VariableSpan;

use crate::state::variable_hint::{VariableContext, hint};
use crate::views::components::variable_line::{tooltip_text, variable_entries};
use crate::views::request_editor::define_variable_click;
use crate::views::root::AppView;

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

    /// The right-click entries of the marker at byte `offset`, empty off the markers. Edit in
    /// environment goes through `app`, as a click on a chip does.
    pub fn variable_entries(&self, offset: usize, app: &WeakEntity<AppView>) -> MenuEntries {
        let Some(span) = self
            .spans
            .iter()
            .find(|span| span.range.start <= offset && offset < span.range.end)
        else {
            return MenuEntries::new();
        };
        let on_edit = Rc::new(define_variable_click(app, &self.context));
        variable_entries(span, &self.context, false, Some(on_edit))
    }
}
