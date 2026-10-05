//! The keyboard shortcuts cheat sheet: a modal listing every shortcut by area, opened with `F1`
//! (`Cmd+Shift+/` on macOS), the command palette action or the status bar's keyboard icon. Closed
//! with the `x`, Escape, or a click on the backdrop, all handled by gpui-component's `Dialog`.
//! The data is `state::shortcuts`, built once when the modal opens.

use std::rc::Rc;

use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::state::shortcuts::{self, ShortcutGroup};
use crate::theme::metrics::RADIUS_SM;
use crate::theme::{Palette, PaletteExt};

use super::components::{IconButton, SectionLabel};
use super::root::AppView;
use super::settings::settings_scrollbar;

/// Width of the modal.
const MODAL_WIDTH: f32 = 520.0;
/// Height of the modal on a tall enough window.
const MODAL_HEIGHT: f32 = 560.0;
/// Least vertical margin kept between the modal and the window edges.
const MODAL_MARGIN_MIN: f32 = 16.0;
/// Height of the header strip.
const HEADER_HEIGHT: f32 = 52.0;

impl AppView {
    /// Opens the cheat sheet. A no-op when a dialog is already open, so `F1` never stacks a
    /// second modal on top (same guard as `open_settings`).
    pub(crate) fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let groups = Rc::new(shortcuts::groups());
        window.open_dialog(cx, move |dialog, window, cx| {
            let palette = cx.palette();
            let groups = groups.clone();
            // Centered vertically by hand, like the Settings modal: `Dialog` otherwise lets its
            // own `max_h` win over `h` on a short window.
            let viewport_height: f32 = window.viewport_size().height.into();
            let margin_top = ((viewport_height - MODAL_HEIGHT) / 2.0).max(MODAL_MARGIN_MIN);
            dialog
                .w(px(MODAL_WIDTH))
                .h(px(MODAL_HEIGHT))
                .margin_top(px(margin_top))
                .p_0()
                .border_0()
                .bg(palette.overlay)
                .close_button(false)
                .content(move |content, window, cx| {
                    let scroll_handle = window
                        .use_keyed_state("shortcuts-body-scroll", cx, |_, _| {
                            ScrollHandle::default()
                        })
                        .read(cx)
                        .clone();
                    content.min_h_0().child(render_sheet(
                        &groups,
                        &scroll_handle,
                        &cx.palette(),
                        cx,
                    ))
                })
        });
    }
}

/// The modal's content: the header and the scrollable list of groups.
fn render_sheet(
    groups: &[ShortcutGroup],
    scroll_handle: &ScrollHandle,
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let mono_font = cx.theme().mono_font_family.clone();
    v_flex()
        .size_full()
        .min_h_0()
        .child(render_header(palette))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    v_flex()
                        .id("shortcuts-body")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(scroll_handle)
                        .p(px(20.0))
                        .gap(px(22.0))
                        .children(
                            groups
                                .iter()
                                .map(|group| render_group(group, palette, &mono_font)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(settings_scrollbar(scroll_handle, palette)),
                ),
        )
        .into_any_element()
}

/// The title and the close button.
fn render_header(palette: &Palette) -> AnyElement {
    h_flex()
        .h(px(HEADER_HEIGHT))
        .flex_none()
        .items_center()
        .justify_between()
        .px(px(20.0))
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(t!("shortcuts.title")),
        )
        .child(
            IconButton::new("shortcuts-close", IconName::Close)
                .large()
                .tooltip(t!("common.close"))
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                }),
        )
        .into_any_element()
}

/// One group: its uppercase title and a row per shortcut.
fn render_group(group: &ShortcutGroup, palette: &Palette, mono_font: &SharedString) -> AnyElement {
    v_flex()
        .gap(px(6.0))
        .child(SectionLabel::new(group.title.clone()))
        .children(group.shortcuts.iter().map(|shortcut| {
            h_flex()
                .items_center()
                .justify_between()
                .gap(px(16.0))
                .py(px(5.0))
                .child(div().min_w_0().child(shortcut.label.clone()))
                .child(
                    h_flex().flex_none().gap(px(4.0)).children(
                        shortcut
                            .keys
                            .iter()
                            .map(|key| render_key(key, palette, mono_font)),
                    ),
                )
        }))
        .into_any_element()
}

/// One key as a small chip.
fn render_key(key: &str, palette: &Palette, mono_font: &SharedString) -> AnyElement {
    div()
        .min_w(px(22.0))
        .h(px(22.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .bg(palette.raised)
        .border_1()
        .border_color(palette.border_strong)
        .font_family(mono_font.clone())
        .text_size(px(11.0))
        .text_color(palette.fg_muted)
        .child(key.to_string())
        .into_any_element()
}
