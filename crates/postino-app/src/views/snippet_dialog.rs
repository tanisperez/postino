//! The Code snippet dialog: opened from the "Code" ghost button at the right of the request
//! editor's inner tabs row (`views/request_editor.rs`). Shows the active tab's request, resolved
//! against the current environment (`postino_runner::preview`, never running a script or sending),
//! as a copyable cURL, `fetch` or Python snippet (`postino_format::snippet::render_snippet`).
//! Unresolved `{{name}}` markers are kept literally, exactly as `render_snippet` already leaves
//! them.

use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::ResolvedRequest;
use postino_format::snippet::{SnippetLanguage, render_snippet};

use crate::theme::PaletteExt;
use crate::theme::metrics::RADIUS_MD;

use super::components::{IconButton, SegmentedControl, SegmentedItem};
use super::root::AppView;

/// Width of the dialog.
const DIALOG_WIDTH: f32 = 560.0;
/// Max height of the scrollable code view.
const CODE_MAX_HEIGHT: f32 = 340.0;

/// Every language the dialog offers, in the order the `SegmentedControl` shows them, with their
/// display labels.
const LANGUAGES: [(SnippetLanguage, &str); 3] = [
    (SnippetLanguage::Curl, "cURL"),
    (SnippetLanguage::JsFetch, "fetch"),
    (SnippetLanguage::PythonRequests, "Python"),
];

impl AppView {
    /// Opens the Code snippet dialog for the active tab's request. A no-op when a dialog is
    /// already open, or when no tab is open (nothing to show).
    pub(crate) fn open_snippet_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(preview) = self.current_preview() else {
            return;
        };
        self.snippet_language = SnippetLanguage::Curl;
        cx.notify();

        let resolved = preview.resolved;
        let weak = cx.weak_entity();
        let copy_weak = weak.clone();
        let copy_resolved = resolved.clone();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let weak = weak.clone();
            let resolved = resolved.clone();
            dialog
                .title(t!("common.code"))
                .w(px(DIALOG_WIDTH))
                .content(move |content, window, cx| {
                    content.min_h_0().child(render_snippet_body(
                        weak.clone(),
                        &resolved,
                        window,
                        cx,
                    ))
                })
                .footer(render_snippet_footer(
                    copy_weak.clone(),
                    copy_resolved.clone(),
                ))
        });
    }
}

/// Renders the language switch and the code view.
fn render_snippet_body(
    weak: WeakEntity<AppView>,
    resolved: &ResolvedRequest,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(view) = weak.upgrade() else {
        return div().into_any_element();
    };
    let language = view.read(cx).snippet_language;
    let palette = cx.palette();
    let mono_font = cx.theme().mono_font_family.clone();
    let mono_size = cx.theme().mono_font_size;
    let text = render_snippet(resolved, language);

    let mut selector = SegmentedControl::new("snippet-language");
    for (candidate, label) in LANGUAGES {
        let select_weak = weak.clone();
        selector = selector.item(
            SegmentedItem::new(label)
                .selected(candidate == language)
                .on_click(move |_, cx| {
                    let _ = select_weak.update(cx, |view, cx| {
                        view.snippet_language = candidate;
                        cx.notify();
                    });
                }),
        );
    }

    v_flex()
        .gap(px(10.0))
        .child(selector)
        .child(
            div()
                .id("snippet-code")
                .max_h(px(CODE_MAX_HEIGHT))
                .overflow_y_scroll()
                .p(px(12.0))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(palette.border)
                .bg(palette.raised)
                .child(
                    v_flex()
                        .font_family(mono_font)
                        .text_size(mono_size)
                        .children(text.lines().map(|line| div().child(line.to_string()))),
                ),
        )
        .into_any_element()
}

/// The footer's "Copy" button: recomputes the snippet for whichever language is currently
/// selected at click time, so it always copies exactly what is on screen.
fn render_snippet_footer(weak: WeakEntity<AppView>, resolved: ResolvedRequest) -> AnyElement {
    h_flex()
        .justify_end()
        .child(
            IconButton::new("snippet-copy", IconName::Copy)
                .tooltip(t!("common.copy"))
                .on_click(move |_, _, cx| {
                    let Some(view) = weak.upgrade() else {
                        return;
                    };
                    let language = view.read(cx).snippet_language;
                    let text = render_snippet(&resolved, language);
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }),
        )
        .into_any_element()
}
