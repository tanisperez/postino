//! The status bar at the bottom of the main window (`plans/ui-redesign.md` section 2.3 point 6):
//! "Local only" with a lock icon, the active tab's id, a spacer, "Unsaved changes" when the
//! active tab is dirty, and "UTF-8".

use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::theme::PaletteExt;
use crate::theme::metrics::STATUS_BAR_HEIGHT;

use super::root::AppView;

impl AppView {
    /// Renders the status bar.
    pub(crate) fn render_status_bar(&self, cx: &Context<Self>) -> AnyElement {
        let palette = cx.palette();
        let active_tab = self.state.tabs.active();

        h_flex()
            .flex_none()
            .h(px(STATUS_BAR_HEIGHT))
            .items_center()
            .gap(px(16.0))
            .px_3()
            .bg(palette.surface)
            .border_t_1()
            .border_color(palette.border)
            .text_size(px(11.5))
            .text_color(palette.fg_subtle)
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(Icon::new(gpui_kit::assets::IconName::Lock).small())
                    .child("Local only"),
            )
            .children(active_tab.map(|tab| div().child(tab.id.clone())))
            .child(div().flex_1())
            .children(
                active_tab
                    .filter(|tab| tab.dirty)
                    .map(|_| div().child("Unsaved changes")),
            )
            .child(div().child("UTF-8"))
            .into_any_element()
    }
}
