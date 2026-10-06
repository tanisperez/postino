//! The status bar at the bottom of the main window: the workspace path with a `hard-drive` icon
//! and, inside a git repository, the branch with a `git-branch` icon in the accent color, a spacer,
//! "Unsaved changes" when the active tab is dirty, and "UTF-8".

use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_workspace::git_branch;
use rust_i18n::t;

use crate::state;
use crate::state::update::ReadyUpdate;
use crate::theme::metrics::{STATUS_BAR_BUTTON, STATUS_BAR_HEIGHT};
use crate::theme::{Palette, PaletteExt};

use super::components::IconButton;
use super::root::AppView;

impl AppView {
    /// Renders the status bar.
    pub(crate) fn render_status_bar(&self, cx: &Context<Self>) -> AnyElement {
        let palette = cx.palette();
        let active_tab = self.state.tabs.active();
        let weak = cx.weak_entity();

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
            .children(self.state.workspace.as_ref().map(|workspace| {
                let path_label =
                    state::format::shorten_path(workspace.root(), dirs::home_dir().as_deref());
                let branch = git_branch(workspace.root());
                h_flex()
                    .items_center()
                    .gap(px(16.0))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(Icon::new(gpui_kit::assets::IconName::HardDrive).small())
                            .child(path_label),
                    )
                    .children(branch.map(|branch| {
                        h_flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_color(palette.accent_text)
                            .child(Icon::new(gpui_kit::assets::IconName::GitBranch).small())
                            .child(branch)
                    }))
            }))
            .child(div().flex_1())
            .children(
                self.state
                    .update
                    .ready()
                    .map(|ready| render_update_ready(weak.clone(), ready, &palette)),
            )
            .children(
                active_tab
                    .filter(|tab| tab.dirty)
                    .map(|_| div().child(t!("shell.status_bar.unsaved"))),
            )
            .child(div().child("UTF-8"))
            .child(
                IconButton::new("status-bar-shortcuts", gpui_kit::assets::IconName::Keyboard)
                    .box_size(STATUS_BAR_BUTTON)
                    .tooltip(t!(
                        "shortcuts.tooltip",
                        keys = state::shortcuts::open_hint()
                    ))
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| view.open_shortcuts(window, cx));
                    }),
            )
            .into_any_element()
    }
}

/// The discreet "update ready" item: the restart click and the release notes link. Built from
/// the stored update, no work beyond a few small clones.
fn render_update_ready(
    weak: WeakEntity<AppView>,
    ready: &ReadyUpdate,
    palette: &Palette,
) -> impl IntoElement {
    let notes_url = ready.notes_url.clone();
    h_flex()
        .items_center()
        .gap(px(12.0))
        .child(
            h_flex()
                .id("status-bar-update-restart")
                .items_center()
                .gap(px(6.0))
                .cursor_pointer()
                .text_color(palette.accent_text)
                .hover(|style| style.underline())
                .child(Icon::new(gpui_kit::assets::IconName::RefreshCw).small())
                .child(t!("update.status.ready", version = ready.version.as_str()))
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.request_update_restart(window, cx));
                }),
        )
        .child(
            div()
                .id("status-bar-update-notes")
                .cursor_pointer()
                .hover(|style| style.underline())
                .child(t!("update.status.notes"))
                .on_click(move |_, _, cx| cx.open_url(&notes_url)),
        )
}
