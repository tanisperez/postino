//! The Code snippet dialog: opened from the "Code" ghost button at the right of the request
//! editor's inner tabs row (`views/request_editor.rs`). Shows the active tab's request, resolved
//! against the current environment (`postino_runner::preview`, never running a script or sending),
//! as a copyable cURL, `fetch`, Python or Postino CLI snippet (`postino_format::snippet::render_snippet`).
//! Unresolved `{{name}}` markers are kept literally, exactly as `render_snippet` already leaves
//! them.
//!
//! The snippet shows in a read-only code editor, highlighted with the editors' syntax colors, at
//! one fixed height for every language so the dialog does not jump when the language changes.
//! While the dialog itself has the focus, Left and Right change the language and Ctrl+C (Cmd+C on
//! macOS) copies the whole snippet; in the editor they move the caret and copy the selection.

use std::time::Duration;

use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::ResolvedRequest;
use postino_format::snippet::{SnippetLanguage, render_snippet};
use postino_runner::{Preview, preview};

use crate::theme::PaletteExt;
use crate::theme::metrics::RADIUS_MD;
use telaio_editor::{EditorDisplay, EditorState, EditorView, Language};
use telaio_ui::components::SecondaryButton;

use super::components::{
    ModalPlacement, SegmentedControl, SegmentedItem, framed_dialog, modal_body,
    modal_header_with_icon, modal_hints,
};
use super::root::AppView;

/// Width of the dialog.
const DIALOG_WIDTH: f32 = 640.0;
/// Height of the code editor, the same for every language.
const CODE_HEIGHT: f32 = 300.0;
/// How long the Copy button says "Copied" after a copy.
const COPIED_FEEDBACK: Duration = Duration::from_millis(1500);

/// Every language the dialog offers, in the order the `SegmentedControl` shows them, with their
/// display labels and the editor's highlight language.
const LANGUAGES: [(SnippetLanguage, &str, Language); 4] = [
    (SnippetLanguage::Curl, "cURL", Language::Shell),
    (SnippetLanguage::JsFetch, "fetch", Language::JavaScript),
    (SnippetLanguage::PythonRequests, "Python", Language::Python),
    (SnippetLanguage::PostinoCli, "Postino CLI", Language::Shell),
];

impl AppView {
    /// Opens the Code snippet dialog for the active tab's request. A no-op when a dialog is
    /// already open, or when no tab is open (nothing to show).
    pub(crate) fn open_snippet_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        self.state.reload_environment();
        let Some(preview) = self.snippet_preview() else {
            return;
        };
        self.snippet_language = SnippetLanguage::Curl;
        self.snippet_editor = None;
        self.snippet_copied = false;
        cx.notify();

        let resolved = preview.resolved;
        let weak = cx.weak_entity();
        let focus = cx.focus_handle();
        let dialog_focus = focus.clone();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let weak = weak.clone();
            let resolved = resolved.clone();
            let focus = dialog_focus.clone();
            framed_dialog(
                dialog,
                DIALOG_WIDTH,
                None,
                ModalPlacement::DEFAULT,
                window,
                move |_window, cx| {
                    let key_weak = weak.clone();
                    let key_resolved = resolved.clone();
                    v_flex()
                        .track_focus(&focus)
                        .on_key_down(move |event, _window, cx| {
                            on_dialog_key(&key_weak, &key_resolved, event, cx);
                        })
                        .child(modal_header_with_icon(
                            gpui_kit::assets::IconName::CodeXml,
                            cx.palette().accent_text,
                            t!("common.code"),
                            cx,
                        ))
                        .child(modal_body().child(render_snippet_body(weak.clone(), &resolved, cx)))
                        .child(modal_hints(
                            [
                                (copy_key_hint(), t!("request.snippet.hint_copy")),
                                ("← →".to_string(), t!("request.snippet.hint_language")),
                                ("Esc".to_string(), t!("shell.palette.hint_close")),
                            ],
                            cx,
                        ))
                        .into_any_element()
                },
            )
        });
        // After the dialog took the focus for itself, so the keys above work at once.
        window.defer(cx, move |window, cx| window.focus(&focus, cx));
    }

    /// The preview of the active tab's request for its snippet: an OAuth 2.0 auth becomes the
    /// token kept for the active environment, or a placeholder when there is none (#269). Never
    /// asks for a token.
    fn snippet_preview(&self) -> Option<Preview> {
        let tab = self.state.tabs.active()?;
        let environment = self.active_environment();
        let request = self
            .oauth_config(&tab.id)
            .with_snippet_token(tab.request()?, &environment.name);
        Some(preview(&request, &environment, &self.state.session_env))
    }

    /// Copies the snippet of `resolved` in the language shown, and lets the Copy button say
    /// "Copied" for a moment.
    fn copy_snippet(&mut self, resolved: &ResolvedRequest, cx: &mut Context<Self>) {
        let text = render_snippet(resolved, self.snippet_language);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.snippet_copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FEEDBACK).await;
            let _ = this.update(cx, |view, cx| {
                if view.snippet_copied {
                    view.snippet_copied = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// `"Ctrl C"` (`"Cmd C"` on macOS): the key cap of the copy hint.
fn copy_key_hint() -> String {
    if cfg!(target_os = "macos") {
        "Cmd C".to_string()
    } else {
        "Ctrl C".to_string()
    }
}

/// The keys of the dialog while it has the focus (not the editor): Left and Right change the
/// language, Ctrl+C (Cmd+C on macOS) copies the whole snippet.
fn on_dialog_key(
    weak: &WeakEntity<AppView>,
    resolved: &ResolvedRequest,
    event: &KeyDownEvent,
    cx: &mut App,
) {
    let keystroke = &event.keystroke;
    let modifiers = keystroke.modifiers;
    let copy_modifier = if cfg!(target_os = "macos") {
        modifiers.platform && !modifiers.control
    } else {
        modifiers.control && !modifiers.platform
    };
    let plain = !modifiers.control && !modifiers.alt && !modifiers.platform && !modifiers.shift;
    let step: isize = match keystroke.key.as_str() {
        "left" if plain => -1,
        "right" if plain => 1,
        "c" if copy_modifier && !modifiers.alt && !modifiers.shift => {
            cx.stop_propagation();
            let resolved = resolved.clone();
            let _ = weak.update(cx, |view, cx| view.copy_snippet(&resolved, cx));
            return;
        }
        _ => return,
    };
    cx.stop_propagation();
    let _ = weak.update(cx, |view, cx| {
        let current = LANGUAGES
            .iter()
            .position(|(language, _, _)| *language == view.snippet_language)
            .unwrap_or(0) as isize;
        let next = (current + step).rem_euclid(LANGUAGES.len() as isize) as usize;
        view.snippet_language = LANGUAGES[next].0;
        cx.notify();
    });
}

/// Renders the language switch, the Copy button and the code view.
fn render_snippet_body(
    weak: WeakEntity<AppView>,
    resolved: &ResolvedRequest,
    cx: &mut App,
) -> AnyElement {
    let Some(view) = weak.upgrade() else {
        return div().into_any_element();
    };
    let palette = cx.palette();
    let language = view.read(cx).snippet_language;
    let copied = view.read(cx).snippet_copied;
    // The editor is rebuilt only when the language changes, never on a plain render.
    let editor = view.update(cx, |view, cx| match &view.snippet_editor {
        Some((shown, editor)) if *shown == language => editor.clone(),
        _ => {
            let highlight = LANGUAGES
                .iter()
                .find(|(candidate, _, _)| *candidate == language)
                .map_or(Language::PlainText, |(_, _, highlight)| *highlight);
            let text = render_snippet(resolved, language);
            let editor = cx.new(|cx| {
                let mut editor = EditorView::new(EditorState::with_text(&text, highlight), cx);
                editor.set_read_only(true, cx);
                editor.set_display(
                    EditorDisplay {
                        minimap: false,
                        current_line: false,
                        folding: false,
                        find: false,
                        goto_line: false,
                        ..EditorDisplay::default()
                    },
                    cx,
                );
                editor
            });
            view.snippet_editor = Some((language, editor.clone()));
            editor
        }
    });

    let mut selector = SegmentedControl::new("snippet-language");
    for (candidate, label, _) in LANGUAGES {
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

    let copy_weak = weak.clone();
    let copy_resolved = resolved.clone();
    let copy_label = if copied {
        t!("request.snippet.copied")
    } else {
        t!("common.copy")
    };
    let copy = SecondaryButton::new("snippet-copy", copy_label)
        .icon(if copied {
            IconName::Check
        } else {
            IconName::Copy
        })
        .on_click(move |_, _, cx| {
            let _ = copy_weak.update(cx, |view, cx| view.copy_snippet(&copy_resolved, cx));
        });

    v_flex()
        .gap(px(12.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(10.0))
                .child(selector)
                .child(div().flex_1())
                .child(copy),
        )
        .child(
            div()
                .h(px(CODE_HEIGHT))
                .border_1()
                .border_color(palette.border)
                .rounded(px(RADIUS_MD))
                .bg(palette.raised)
                .overflow_hidden()
                .child(editor),
        )
        .into_any_element()
}
