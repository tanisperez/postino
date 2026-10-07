//! The response viewer panel: status, time and size, the Body/Headers/Tests/Console tabs, and a
//! warnings strip for missing variables and script errors.
//!
//! The response body is the one field with a persistent `gpui` entity of its own (a read-only
//! [`EditorState`], see [`ResponseEditorEntities`]), so it can show line numbers and a working
//! find bar; every other field is read-only, rendered straight from the
//! [`postino_runner::RunResult`] of the last send.

use std::sync::Arc;

use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::{ConsoleLevel, TestResult};
use postino_runner::{FailedStage, HttpError, RunResult};

use crate::state::locale;
use crate::state::response_render;
use crate::state::settings::max_response_label;
use crate::state::ui_tabs::ResponseTab;
use crate::theme::PaletteExt;
use crate::views::components::{
    IconButton, InlineMessage, InlineMessageKind, SegmentedControl, SegmentedItem, StatusBadge,
    StatusState, UnderlineTabItem, UnderlineTabs, edit_menu,
};

use super::root::AppView;

/// The response body's key hint shown in the "not sent yet" empty state: `Cmd \u{21b5}` on
/// macOS, `Ctrl \u{21b5}` elsewhere (same local-constant pattern as `views/request_editor.rs`'s
/// `SEND_KEY_HINT`, matching `main.rs`'s key bindings).
#[cfg(target_os = "macos")]
const SEND_KEY_HINT: &str = "Cmd+\u{21b5}";
#[cfg(not(target_os = "macos"))]
const SEND_KEY_HINT: &str = "Ctrl+\u{21b5}";

/// The live, read-only `EditorState` behind the response body view: a code editor, not a plain
/// block of text, so it gets the design's line numbers and, wired in
/// [`AppView::render_response_body_tab`], a working find bar (`EditorState::open_search`, which
/// works on a readonly editor same as a writable one). Rebuilt when the active tab, the raw/pretty
/// toggle or the JSON syntax highlighting changes; the shown text is unconditionally resynced
/// afterwards, the same pattern `RequestEditorEntities` uses for its own editors.
#[derive(Default)]
pub(crate) struct ResponseEditorEntities {
    built_for: Option<(String, bool)>,
    json: bool,
    /// The `(tab id, response generation, raw)` whose text the editor currently holds, so a
    /// resync is decided by comparing this cheap key and never the (possibly huge) text.
    synced: Option<(String, u64, bool)>,
    editor: Option<Entity<EditorState>>,
    texts: response_render::BodyTextCache,
}

impl ResponseEditorEntities {
    /// Rebuilds the editor if `tab_id`, `raw` or `json` differ from what it was last built for,
    /// then resyncs its text to `text` when the `(tab_id, generation, raw)` key changed.
    fn sync(
        &mut self,
        tab_id: &str,
        generation: u64,
        text: &Arc<str>,
        raw: bool,
        window: &mut Window,
        cx: &mut Context<AppView>,
    ) {
        // Raw bodies are never highlighted; only the pretty view is JSON.
        let json = !raw;
        let key = (tab_id.to_string(), raw);
        if self.built_for.as_ref() != Some(&key) || self.json != json {
            let editor = cx.new(|cx| {
                let state = EditorState::new(window, cx);
                if json { state.language("json") } else { state }
            });
            editor.update(cx, |state, cx| state.set_readonly(true, cx));
            self.editor = Some(editor);
            self.built_for = Some(key);
            self.json = json;
            self.synced = None;
        }
        let synced = (tab_id.to_string(), generation, raw);
        if self.synced.as_ref() != Some(&synced)
            && let Some(editor) = &self.editor
        {
            let started = std::time::Instant::now();
            let new_value = SharedString::from(text.clone());
            editor.update(cx, |state, cx| state.set_value(new_value, window, cx));
            log::debug!(
                "response editor of {tab_id} resynced with {} bytes in {} us",
                text.len(),
                started.elapsed().as_micros()
            );
            self.synced = Some(synced);
        }
    }
}

impl AppView {
    /// Renders the active tab's response, a "sending" spinner while it is in flight, or a
    /// placeholder when nothing has been sent yet.
    pub(crate) fn render_response_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab) = self.state.tabs.active() else {
            return placeholder(cx, &t!("response.placeholder.not_sent"), None);
        };
        let tab_id = tab.id.clone();

        if self.is_sending(&tab_id) {
            let palette = cx.palette();
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Spinner::new().large())
                .child(
                    div()
                        .text_color(palette.fg_muted)
                        .child(t!("common.sending")),
                )
                .into_any_element();
        }

        if !self.responses.contains_key(&tab_id) {
            return placeholder(
                cx,
                &t!("response.placeholder.not_sent"),
                Some(SEND_KEY_HINT),
            );
        }
        // Each of these reads `self.responses` for as long as it needs to, but no longer: by the
        // time `render_response_tab_content` runs, neither still borrows `self`, so it is free
        // to take `&mut self` for the Body tab's read-only editor entity.
        let warnings = self.render_warnings_strip(&tab_id, cx);
        let tab_bar = self.render_response_tab_bar(&tab_id, cx);
        // The Body tab's read-only `Editor` scrolls its own content and needs a real, determinate
        // height to fill: an ancestor `overflow_y_scroll()` container instead measures its child's
        // intrinsic height, which collapses a `flex_1` editor to a couple of lines.
        // Headers/Tests/Console are plain lists with no scrolling of their own, so they still need
        // it here.
        let content = div().id("response-content").flex_1().min_h_0();
        let content = if self.active_response_tab == ResponseTab::Body {
            content
        } else {
            content.overflow_y_scroll()
        };

        v_flex()
            .size_full()
            .child(tab_bar)
            .children(warnings)
            .child(content.child(self.render_response_tab_content(&tab_id, window, cx)))
            .into_any_element()
    }

    /// Renders the status badge, time and size, shown as the tab bar's trailing suffix
    /// (on the right of the same 36 px
    /// bordered row as the tabs, not a separate row above them). The badge's label includes the
    /// reason phrase (`state::response_render::reason_phrase`).
    fn render_status_suffix(&self, tab_id: &str, cx: &Context<Self>) -> AnyElement {
        let palette = cx.palette();
        // `render_response_view` already checked this tab has a result before calling here.
        let Some(result) = self.responses.get(tab_id) else {
            return div().into_any_element();
        };
        let state = match &result.response {
            Some(response) => StatusState::Code(response.status),
            None => StatusState::NotSent,
        };
        let mut badge = StatusBadge::new(state.clone());
        if let StatusState::Code(code) = state {
            let label = match response_render::reason_phrase(code) {
                Some(reason) => format!("{code} {reason}"),
                None => code.to_string(),
            };
            badge = badge.label(label);
        }

        let mut row = h_flex().items_center().gap(px(14.0)).child(badge);
        if let Some(response) = &result.response {
            row = row
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(palette.fg_muted)
                        .child(response_render::format_duration(response.time)),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(palette.fg_muted)
                        .child(response_render::format_size(response.size)),
                );
        }
        row.into_any_element()
    }

    /// Renders the Body/Headers/Tests/Console tab bar, with the header count, the tests pass/fail
    /// count colored `success`/`danger`, and the status badge/time/size on the right.
    fn render_response_tab_bar(&self, tab_id: &str, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let palette = cx.palette();
        let active = self.active_response_tab;
        let Some(result) = self.responses.get(tab_id) else {
            return div().into_any_element();
        };
        let header_count = result.response.as_ref().map_or(0, |r| r.headers.len());
        let test_count = result.tests.len();
        let passed_count = result.tests.iter().filter(|test| test.passed).count();

        let mut bar = UnderlineTabs::new("response-tabs")
            .height(crate::theme::metrics::RESPONSE_TAB_BAR_HEIGHT);
        for tab in ResponseTab::ALL {
            let select_weak = weak.clone();
            let mut item = UnderlineTabItem::new(tab.label()).selected(tab == active);
            match tab {
                ResponseTab::Headers if header_count > 0 => {
                    item = item.count(header_count.to_string());
                }
                ResponseTab::Tests if test_count > 0 => {
                    item = item
                        .count(format!("{passed_count}/{test_count}"))
                        .count_color(if passed_count == test_count {
                            palette.success
                        } else {
                            palette.danger
                        });
                }
                _ => {}
            }
            item = item.on_click(move |_, cx| {
                let _ = select_weak.update(cx, |view, cx| {
                    view.active_response_tab = tab;
                    cx.notify();
                });
            });
            bar = bar.item(item);
        }
        bar = bar.suffix(self.render_status_suffix(tab_id, cx));
        div().px(px(14.0)).child(bar).into_any_element()
    }

    /// Renders the content of whichever response tab is active.
    fn render_response_tab_content(
        &mut self,
        tab_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.active_response_tab == ResponseTab::Body {
            return self.render_response_body_tab(tab_id, window, cx);
        }
        let Some(result) = self.responses.get(tab_id) else {
            return div().into_any_element();
        };
        match self.active_response_tab {
            ResponseTab::Headers => render_headers_tab(result, cx),
            ResponseTab::Tests => render_tests_tab(result, cx),
            ResponseTab::Console => render_console_tab(result, cx),
            // Handled above, before `result` was even looked up.
            ResponseTab::Body => unreachable!(),
        }
    }

    /// Renders the response body: a Pretty/Raw segmented control plus `copy` and `search` icon
    /// buttons on the right, then the body in a read-only, line-numbered code editor.
    fn render_response_body_tab(
        &mut self,
        tab_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let generation = self.response_generations.get(tab_id);
        // Scoped so the borrow of `self.responses` ends before `self.response_editor` (below)
        // needs a mutable one: `shown` is owned by the time this block ends.
        let shown = {
            let Some(result) = self.responses.get(tab_id) else {
                return div().into_any_element();
            };
            let Some(response) = &result.response else {
                return div()
                    .p_2()
                    .text_color(palette.fg_muted)
                    .child(t!("response.body.empty"))
                    .into_any_element();
            };
            self.response_editor
                .texts
                .shown(tab_id, generation, &response.body, self.response_raw)
        };
        let show_raw = shown.raw;
        let text = shown.text;

        self.response_editor
            .sync(tab_id, generation, &text, show_raw, window, cx);

        let weak = cx.weak_entity();
        let pretty_available = shown.pretty_available;
        let mut toggle = SegmentedControl::new("body-raw-toggle").item(
            SegmentedItem::new(t!("response.body.pretty"))
                .selected(!show_raw)
                .on_click({
                    let weak = weak.clone();
                    move |_, cx| {
                        if pretty_available {
                            let _ = weak.update(cx, |view, cx| {
                                view.response_raw = false;
                                cx.notify();
                            });
                        }
                    }
                }),
        );
        toggle = toggle.item(
            SegmentedItem::new(t!("response.body.raw"))
                .selected(show_raw)
                .on_click({
                    let weak = weak.clone();
                    move |_, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.response_raw = true;
                            cx.notify();
                        });
                    }
                }),
        );

        let copy_text = text.clone();
        let copy_button = IconButton::new("response-copy", IconName::Copy)
            .tooltip(t!("common.copy"))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.to_string()));
            });

        let search_editor = self.response_editor.editor.clone();
        let search_button = IconButton::new("response-search", IconName::Search)
            .tooltip(t!("common.search"))
            .on_click(move |_, _, cx| {
                if let Some(editor) = &search_editor {
                    editor.update(cx, |state, cx| state.open_search(false, cx));
                }
            });

        let header = h_flex()
            .items_center()
            .justify_between()
            .px(px(14.0))
            .py(px(8.0))
            .child(toggle)
            .child(
                h_flex()
                    .gap(px(2.0))
                    .child(copy_button)
                    .child(search_button),
            );

        // `Editor` needs a concrete height from its own inherent `h()` (pixels or a relative
        // fraction of its parent) to lay out its line-based content; the generic `flex_1()` /
        // `min_h_0()` leaves that unset and the editor sizes to its content instead of the space
        // the wrapping `flex_1`/`min_h_0` div below makes available (see `views/request_editor.rs`'s
        // `render_editor_or_placeholder` doc comment for the `gpui-component` precedent).
        let body: AnyElement = match &self.response_editor.editor {
            Some(editor) => Editor::new(editor)
                .context_menu(edit_menu(editor, cx))
                .h(relative(1.0))
                .into_any_element(),
            None => div().into_any_element(),
        };

        v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    /// Renders the warnings strip above the response body, using [`InlineMessage`]: one warning row
    /// per unknown variable from the live `preview` (before sending and after, since it always
    /// reflects the request's current text, not necessarily what was last sent), one warning row
    /// per function-call error, and one danger row for a failed pipeline stage. `None` when there
    /// is nothing to show.
    ///
    /// Uses the actual [`RunResult::warnings`] of the last send, not the live preview: unlike the
    /// URL chips (which have no real run to look at until the request is sent, and so keep using
    /// the preview plus the `vars.set` heuristic), this method is only ever reached once a result
    /// exists (`render_response_view` returns the "not sent yet" placeholder before it). At that
    /// point the real pipeline already ran the pre script for real, so a variable it set with
    /// `vars.set(...)` is correctly resolved here with no heuristic needed.
    fn render_warnings_strip(&self, tab_id: &str, cx: &Context<Self>) -> Option<AnyElement> {
        let result = self.responses.get(tab_id)?;
        let weak = cx.weak_entity();
        let mut messages: Vec<AnyElement> = response_render::group_warnings(&result.warnings)
            .into_iter()
            .map(|group| match group {
                response_render::WarningGroup::UnknownVariable(name) => {
                    let define_weak = weak.clone();
                    InlineMessage::new(
                        InlineMessageKind::Warning,
                        t!("response.warning.unknown_variable"),
                    )
                    .mono_suffix(name.clone())
                    .action(t!("common.define"), move |window, cx| {
                        let names = vec![name.clone()];
                        let _ = define_weak
                            .update(cx, |view, cx| view.define_variables(names, window, cx));
                    })
                    .into_any_element()
                }
                response_render::WarningGroup::UnknownVariables(names) => {
                    let define_weak = weak.clone();
                    let all_names = names.clone();
                    InlineMessage::new(
                        InlineMessageKind::Warning,
                        t!("response.warning.unknown_variables"),
                    )
                    .mono_suffix(names.join(", "))
                    .action(t!("common.define"), move |window, cx| {
                        let names = all_names.clone();
                        let _ = define_weak
                            .update(cx, |view, cx| view.define_variables(names, window, cx));
                    })
                    .into_any_element()
                }
                response_render::WarningGroup::Function(message) => {
                    InlineMessage::new(InlineMessageKind::Warning, message).into_any_element()
                }
            })
            .collect();

        if let Some(warning) = result
            .response
            .as_ref()
            .and_then(|response| response.tls_warning.clone())
        {
            messages.insert(
                0,
                InlineMessage::new(InlineMessageKind::Warning, warning).into_any_element(),
            );
        }

        if let Some(stage) = &result.failed_stage {
            messages.push(
                InlineMessage::new(InlineMessageKind::Danger, stage_message(stage))
                    .into_any_element(),
            );
        }

        if messages.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .px(px(14.0))
                .pt(px(8.0))
                .gap_1()
                .children(messages)
                .into_any_element(),
        )
    }
}

/// A short, user-facing message for a failed pipeline stage.
fn stage_message(stage: &FailedStage) -> String {
    match stage {
        FailedStage::Pre(error) => {
            t!("response.stage.pre_failed", error = error.to_string()).into_owned()
        }
        FailedStage::Send(HttpError::BodyTooLarge(limit)) => t!(
            "response.stage.body_too_large",
            limit = max_response_label(u32::try_from(limit / 1_000_000).unwrap_or(u32::MAX))
        )
        .into_owned(),
        FailedStage::Send(error) => {
            t!("response.stage.send_failed", error = error.to_string()).into_owned()
        }
        FailedStage::Post(error) => {
            t!("response.stage.post_failed", error = error.to_string()).into_owned()
        }
    }
}

/// Renders the response headers as `name: value` rows.
fn render_headers_tab(result: &RunResult, cx: &Context<AppView>) -> AnyElement {
    let palette = cx.palette();
    let Some(response) = &result.response else {
        return div()
            .p_2()
            .text_color(palette.fg_muted)
            .child(t!("response.headers.no_response"))
            .into_any_element();
    };
    if response.headers.is_empty() {
        return div()
            .p_2()
            .text_color(palette.fg_muted)
            .child(t!("response.headers.empty"))
            .into_any_element();
    }
    v_flex()
        .p_2()
        .gap_1()
        .children(response.headers.iter().map(|header| {
            h_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(header.name.clone()),
                )
                .child(div().text_sm().child(header.value.clone()))
        }))
        .into_any_element()
}

/// Renders the `test()` results: a pass/fail summary strip, then a pass/fail icon, the test name,
/// and its failure message for each test.
fn render_tests_tab(result: &RunResult, cx: &Context<AppView>) -> AnyElement {
    let palette = cx.palette();
    if result.tests.is_empty() {
        return div()
            .p_2()
            .text_color(palette.fg_muted)
            .child(t!("response.tests.empty"))
            .into_any_element();
    }
    let total = result.tests.len();
    let passed = result.tests.iter().filter(|test| test.passed).count();
    let summary = if passed == total {
        let key = locale::plural_key("response.tests.passed", total);
        InlineMessage::new(
            InlineMessageKind::Success,
            t!(key.as_str(), passed = passed, total = total).into_owned(),
        )
    } else {
        let key = locale::plural_key("response.tests.failed", total);
        InlineMessage::new(
            InlineMessageKind::Danger,
            t!(key.as_str(), failed = total - passed, total = total).into_owned(),
        )
    };

    v_flex()
        .p_2()
        .gap_2()
        .child(summary)
        .children(
            result
                .tests
                .iter()
                .map(|test| render_test_row(test, &palette)),
        )
        .into_any_element()
}

fn render_test_row(test: &TestResult, palette: &crate::theme::Palette) -> AnyElement {
    let (icon, color) = if test.passed {
        (IconName::Check, palette.success)
    } else {
        (IconName::CircleX, palette.danger)
    };
    h_flex()
        .gap_2()
        .items_start()
        .child(Icon::new(icon).small().text_color(color))
        .child(
            v_flex()
                .child(div().text_sm().child(test.name.clone()))
                .children(
                    test.message
                        .clone()
                        .map(|message| div().text_sm().text_color(palette.danger).child(message)),
                ),
        )
        .into_any_element()
}

/// Renders the `console.*` lines, colored by level.
fn render_console_tab(result: &RunResult, cx: &Context<AppView>) -> AnyElement {
    let palette = cx.palette();
    if result.console.is_empty() {
        return div()
            .p_2()
            .text_color(palette.fg_muted)
            .child(t!("response.console.empty"))
            .into_any_element();
    }
    let mono_font = cx.theme().mono_font_family.clone();
    let mono_size = cx.theme().mono_font_size;
    v_flex()
        .p_2()
        .gap_1()
        .children(result.console.iter().map(|line| {
            let color = match line.level {
                ConsoleLevel::Log | ConsoleLevel::Info => palette.fg,
                ConsoleLevel::Warn => palette.warning,
                ConsoleLevel::Error => palette.danger,
            };
            div()
                .font_family(mono_font.clone())
                .text_size(mono_size)
                .text_color(color)
                .child(line.text.clone())
        }))
        .into_any_element()
}

/// Renders a centered, muted placeholder message filling the panel, with an optional trailing mono
/// key hint.
fn placeholder(cx: &Context<AppView>, message: &str, key_hint: Option<&str>) -> AnyElement {
    let palette = cx.palette();
    let mono_font = cx.theme().mono_font_family.clone();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            div()
                .text_color(palette.fg_muted)
                .child(message.to_string()),
        )
        .children(key_hint.map(|hint| {
            div()
                .font_family(mono_font)
                .text_size(px(10.5))
                .text_color(palette.fg_subtle)
                .child(hint.to_string())
        }))
        .into_any_element()
}
