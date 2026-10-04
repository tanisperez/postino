//! The activity rail (GitHub #64): a 48 px column left of the sidebar with one button per
//! section (Collections, Environments, Load tests), and the dispatcher that renders the active
//! section's panel. The state (which section, collapsed or not) is `state::navigation`; the
//! panels are `views/sidebar.rs`, `views/env_panel.rs` and `views/load_panel.rs`.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::state::navigation::NavSection;
use crate::theme::PaletteExt;
use crate::theme::metrics::{RADIUS_MD, RAIL_BUTTON_SIZE, RAIL_WIDTH};

use super::root::AppView;

/// The Lucide icon of a rail button.
fn section_icon(section: NavSection) -> gpui_kit::assets::IconName {
    use gpui_kit::assets::IconName;
    match section {
        NavSection::Collections => IconName::FolderTree,
        NavSection::Environments => IconName::Variable,
        NavSection::LoadTests => IconName::Gauge,
    }
}

/// The tooltip (and accessible name) of a rail button.
fn section_label(section: NavSection) -> String {
    match section {
        NavSection::Collections => t!("navigation.rail.collections").into_owned(),
        NavSection::Environments => t!("navigation.rail.environments").into_owned(),
        NavSection::LoadTests => t!("navigation.rail.load_tests").into_owned(),
    }
}

impl AppView {
    /// Renders the activity rail: a button per [`NavSection`], the active one tinted. Clicking
    /// goes through [`AppView::click_rail`].
    pub(crate) fn render_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();

        v_flex()
            .flex_none()
            .w(px(RAIL_WIDTH))
            .h_full()
            .items_center()
            .gap(px(4.0))
            .pt(px(8.0))
            .bg(palette.bg)
            .border_r_1()
            .border_color(palette.border)
            .children(
                NavSection::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, section)| {
                        let active = self.nav.is_active(section);
                        let tooltip = SharedString::from(section_label(section));
                        let weak = weak.clone();
                        div()
                            .id(("rail-button", index))
                            .flex_none()
                            .size(px(RAIL_BUTTON_SIZE))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(RADIUS_MD))
                            .cursor_pointer()
                            .when(active, |button| {
                                button
                                    .bg(palette.accent_subtle)
                                    .text_color(palette.accent_text)
                            })
                            .when(!active, |button| {
                                button
                                    .text_color(palette.fg_muted)
                                    .hover(|style| style.bg(palette.hover))
                                    .active(|style| style.bg(palette.pressed))
                            })
                            .child(Icon::new(section_icon(section)).with_size(px(17.0)))
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |view, cx| view.click_rail(section, cx));
                            })
                    }),
            )
            .into_any_element()
    }

    /// Handles a click on a rail button (`NavState::click`): switches section, collapses the
    /// sidebar on the active one, or expands it when collapsed. Opening the Environments panel
    /// reloads its rows, since the files may have changed since they were last read.
    pub(crate) fn click_rail(&mut self, section: NavSection, cx: &mut Context<Self>) {
        self.nav.click(section);
        log::debug!(
            "rail: {:?}, sidebar {}",
            self.nav.section(),
            if self.nav.sidebar_visible() {
                "visible"
            } else {
                "collapsed"
            }
        );
        if self.nav.sidebar_visible() && self.nav.section() == NavSection::Environments {
            self.refresh_env_rows();
        }
        if self.nav.section() != NavSection::Environments {
            self.new_env_input = None;
        }
        cx.notify();
    }

    /// Renders the sidebar's content for the active section.
    pub(crate) fn render_side_panel(
        &mut self,
        weak: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.nav.section() {
            NavSection::Collections => self.render_sidebar(weak, cx),
            NavSection::Environments => self.render_env_panel(weak, cx),
            NavSection::LoadTests => self.render_load_panel(weak, cx),
        }
    }
}
