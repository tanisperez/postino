//! The response viewer panel: status, time and size, the Body/Headers/Tests/Console tabs, and a
//! warnings strip for missing variables and script errors (`plans/mvp.md`, Phase 9).
//!
//! Unlike the request editor, nothing here needs a persistent `gpui` entity: every field is
//! read-only, rendered straight from the [`postino_runner::RunResult`] of the last send, so this
//! module only reads [`AppView`] state, it never mutates it during a render.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::theme::Theme;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use postino_core::{ConsoleLevel, TestResult};
use postino_runner::{FailedStage, RunResult};

use crate::state::response_render::{self, StatusClass};
use crate::state::ui_tabs::ResponseTab;

use super::root::AppView;

impl AppView {
    /// Renders the active tab's response, a "sending" spinner while it is in flight, or a
    /// placeholder when nothing has been sent yet.
    pub(crate) fn render_response_view(&self, cx: &Context<Self>) -> AnyElement {
        let Some(tab) = self.state.tabs.active() else {
            return placeholder(cx, "Send a request to see its response here.");
        };

        if self.is_sending(&tab.id) {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Spinner::new().large())
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Sending..."),
                )
                .into_any_element();
        }

        let Some(result) = self.responses.get(&tab.id) else {
            return placeholder(cx, "Send a request to see its response here.");
        };

        v_flex()
            .size_full()
            .child(render_status_row(result, cx))
            .children(render_warnings_strip(result, cx))
            .child(self.render_response_tab_bar(cx))
            .child(
                div()
                    .id("response-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .child(self.render_response_tab_content(result, cx)),
            )
            .into_any_element()
    }

    /// Renders the Body/Headers/Tests/Console tab bar.
    fn render_response_tab_bar(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let active = self.active_response_tab;
        let selected_index = ResponseTab::ALL
            .iter()
            .position(|tab| *tab == active)
            .unwrap_or(0);
        let mut bar = TabBar::new("response-tabs").selected_index(selected_index);
        for tab in ResponseTab::ALL {
            let select_weak = weak.clone();
            bar = bar.child(Tab::new().label(tab.label()).on_click(move |_, _, cx| {
                let _ = select_weak.update(cx, |view, cx| {
                    view.active_response_tab = tab;
                    cx.notify();
                });
            }));
        }
        bar.into_any_element()
    }

    /// Renders the content of whichever response tab is active.
    fn render_response_tab_content(&self, result: &RunResult, cx: &Context<Self>) -> AnyElement {
        match self.active_response_tab {
            ResponseTab::Body => self.render_response_body_tab(result, cx),
            ResponseTab::Headers => render_headers_tab(result, cx),
            ResponseTab::Tests => render_tests_tab(result, cx),
            ResponseTab::Console => render_console_tab(result, cx),
        }
    }

    /// Renders the response body: a Raw/Pretty toggle (Pretty is disabled when the body is not
    /// valid JSON) plus the body text in a monospace block.
    fn render_response_body_tab(&self, result: &RunResult, cx: &Context<Self>) -> AnyElement {
        let Some(response) = &result.response else {
            return div()
                .text_color(cx.theme().muted_foreground)
                .child("No response body.")
                .into_any_element();
        };

        let pretty = response_render::pretty_print_json(&response.body);
        let show_raw = self.response_raw || pretty.is_none();
        let text = if show_raw {
            response_render::body_as_text(&response.body)
        } else {
            // `pretty` is `Some` here since `show_raw` is only `false` when it is.
            pretty.clone().unwrap_or_default()
        };

        let weak = cx.weak_entity();
        let toggle = Button::new("body-raw-toggle")
            .ghost()
            .xsmall()
            .label(if show_raw { "Raw" } else { "Pretty" })
            .disabled(pretty.is_none())
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.response_raw = !view.response_raw;
                    cx.notify();
                });
            });

        let theme = cx.theme();
        v_flex()
            .gap_2()
            .child(toggle)
            .child(
                div()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(theme.mono_font_size)
                    .child(text),
            )
            .into_any_element()
    }
}

/// Renders the status/time/size row above the tabs.
fn render_status_row(result: &RunResult, cx: &Context<AppView>) -> AnyElement {
    let theme = cx.theme();
    match &result.response {
        Some(response) => {
            let class = response_render::status_class(response.status);
            let (bg, fg, class_label) = match class {
                StatusClass::Success => (theme.success, theme.success_foreground, "Success"),
                StatusClass::Redirect => (theme.info, theme.info_foreground, "Redirect"),
                StatusClass::ClientError => {
                    (theme.warning, theme.warning_foreground, "Client Error")
                }
                StatusClass::ServerError => (theme.danger, theme.danger_foreground, "Server Error"),
                StatusClass::Other => (theme.muted, theme.muted_foreground, "Other"),
            };
            h_flex()
                .gap_3()
                .items_center()
                .p_2()
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded(theme.radius)
                        .bg(bg)
                        .text_color(fg)
                        .child(format!("{} {}", response.status, class_label)),
                )
                .child(
                    div()
                        .text_sm()
                        .child(response_render::format_duration(response.time)),
                )
                .child(
                    div()
                        .text_sm()
                        .child(response_render::format_size(response.size)),
                )
                .into_any_element()
        }
        None => h_flex()
            .gap_2()
            .items_center()
            .p_2()
            .child(Icon::new(IconName::TriangleAlert).small())
            .child(div().text_color(theme.danger).child("No response"))
            .into_any_element(),
    }
}

/// Renders the warnings strip for unresolved `{{ }}` markers and the failed pipeline stage, if
/// any. `None` when there is nothing to show.
fn render_warnings_strip(result: &RunResult, cx: &Context<AppView>) -> Option<AnyElement> {
    if result.warnings.is_empty() && result.failed_stage.is_none() {
        return None;
    }
    let theme = cx.theme();
    let (bg, fg) = if result.failed_stage.is_some() {
        (theme.danger, theme.danger_foreground)
    } else {
        (theme.warning, theme.warning_foreground)
    };

    let mut messages: Vec<String> = result.warnings.iter().map(ToString::to_string).collect();
    if let Some(stage) = &result.failed_stage {
        messages.push(stage_message(stage));
    }

    Some(
        v_flex()
            .w_full()
            .px_2()
            .py_1()
            .gap_1()
            .bg(bg)
            .text_color(fg)
            .children(messages.into_iter().map(|message| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(IconName::TriangleAlert).small())
                    .child(div().text_sm().child(message))
            }))
            .into_any_element(),
    )
}

/// A short, user-facing message for a failed pipeline stage.
fn stage_message(stage: &FailedStage) -> String {
    match stage {
        FailedStage::Pre(error) => format!("Pre-request script failed: {error}"),
        FailedStage::Send(error) => format!("Sending failed: {error}"),
        FailedStage::Post(error) => format!("Post-response script failed: {error}"),
    }
}

/// Renders the response headers as `name: value` rows.
fn render_headers_tab(result: &RunResult, cx: &Context<AppView>) -> AnyElement {
    let Some(response) = &result.response else {
        return div()
            .text_color(cx.theme().muted_foreground)
            .child("No response headers.")
            .into_any_element();
    };
    if response.headers.is_empty() {
        return div()
            .text_color(cx.theme().muted_foreground)
            .child("No headers.")
            .into_any_element();
    }
    v_flex()
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

/// Renders the `test()` results: a pass/fail icon, the test name, and its failure message.
fn render_tests_tab(result: &RunResult, cx: &Context<AppView>) -> AnyElement {
    if result.tests.is_empty() {
        return div()
            .text_color(cx.theme().muted_foreground)
            .child("No tests ran.")
            .into_any_element();
    }
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .children(result.tests.iter().map(|test| render_test_row(test, theme)))
        .into_any_element()
}

fn render_test_row(test: &TestResult, theme: &Theme) -> AnyElement {
    let (icon, color) = if test.passed {
        (IconName::Check, theme.success)
    } else {
        (IconName::CircleX, theme.danger)
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
                        .map(|message| div().text_sm().text_color(theme.danger).child(message)),
                ),
        )
        .into_any_element()
}

/// Renders the `console.*` lines, colored by level.
fn render_console_tab(result: &RunResult, cx: &Context<AppView>) -> AnyElement {
    if result.console.is_empty() {
        return div()
            .text_color(cx.theme().muted_foreground)
            .child("No console output.")
            .into_any_element();
    }
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .children(result.console.iter().map(|line| {
            let color = match line.level {
                ConsoleLevel::Log | ConsoleLevel::Info => theme.foreground,
                ConsoleLevel::Warn => theme.warning,
                ConsoleLevel::Error => theme.danger,
            };
            div()
                .font_family(theme.mono_font_family.clone())
                .text_size(theme.mono_font_size)
                .text_color(color)
                .child(line.text.clone())
        }))
        .into_any_element()
}

/// Renders a centered, muted placeholder message filling the panel.
fn placeholder(cx: &Context<AppView>, message: &str) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(message.to_string()),
        )
        .into_any_element()
}
