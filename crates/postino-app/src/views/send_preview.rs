//! The "Will be sent" panel (the design's "Empty States" page, 2a): before the first send of a
//! request, the response pane shows it resolved with the active environment, what goes with it
//! and where each variable comes from, so an undefined variable shows up before sending. The
//! content is `state::send_preview`, rebuilt only when the request, the environment or the
//! session overrides change ([`SendPreviewCache`]), never on every frame.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::Environment;
use postino_runner::SessionEnv;

use crate::state::navigation::NavSection;
use crate::state::send_preview::{
    self, AuthSummary, BodySummary, PreviewSource, PreviewValue, SendPreview,
};
use crate::state::variable_hint::MASK;
use crate::theme::PaletteExt;
use crate::theme::metrics::{RADIUS_LG, RADIUS_MD};
use crate::theme::palette::PostinoColors;
use crate::views::components::SecondaryButton;
use telaio_ui::components::ScrollbarOverlay;

use super::root::AppView;

/// Widest the card grows, as in the design.
const CARD_MAX_WIDTH: f32 = 640.0;
/// Height of a summary chip.
const CHIP_HEIGHT: f32 = 22.0;

/// What the preview was built from: the tab and its request's revision, the loaded environment
/// (by identity, a reload makes a new one) and the session overrides.
struct PreviewKey {
    tab_id: String,
    revision: u64,
    active: Option<String>,
    environment: Arc<Environment>,
    local_names: Arc<HashSet<String>>,
    session: SessionEnv,
}

/// The last preview built, with what it was built from.
#[derive(Default)]
pub(crate) struct SendPreviewCache {
    built: Option<(PreviewKey, Rc<SendPreview>)>,
    /// The scroll of the panel, for a short response pane.
    scroll: ScrollHandle,
}

impl AppView {
    /// The preview of the request tab `tab_id`, from the cache when nothing it depends on
    /// changed. `None` for a tab that is not a request.
    fn send_preview(&mut self, tab_id: &str) -> Option<Rc<SendPreview>> {
        let tab = self.state.tabs.active().filter(|tab| tab.id == tab_id)?;
        let request = tab.request()?;
        let state = &self.state;
        if let Some((key, preview)) = &self.send_preview.built
            && key.tab_id == tab_id
            && key.revision == tab.revision
            && key.active == state.active_environment
            && Arc::ptr_eq(&key.environment, &state.environment)
            && Arc::ptr_eq(&key.local_names, &state.environment_local_names)
            && key.session == state.session_env
        {
            return Some(preview.clone());
        }
        let started = std::time::Instant::now();
        let preview = Rc::new(send_preview::build(
            request,
            state.active_environment.as_deref(),
            &state.environment,
            &state.environment_local_names,
            &state.session_env,
        ));
        log::debug!(
            "send preview of {tab_id} rebuilt in {} us",
            started.elapsed().as_micros()
        );
        let key = PreviewKey {
            tab_id: tab_id.to_string(),
            revision: tab.revision,
            active: state.active_environment.clone(),
            environment: state.environment.clone(),
            local_names: state.environment_local_names.clone(),
            session: state.session_env.clone(),
        };
        self.send_preview.built = Some((key, preview.clone()));
        Some(preview)
    }

    /// The "Will be sent" panel of the request tab `tab_id`, for the response pane before its
    /// first send. `None` for a tab that is not a request.
    pub(crate) fn render_send_preview(
        &mut self,
        tab_id: &str,
        send_key: &'static str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let preview = self.send_preview(tab_id)?;
        let scroll = self.send_preview.scroll.clone();
        let card = render_card(&preview, send_key, self, cx);
        Some(
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("send-preview")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&scroll)
                        .child(
                            v_flex()
                                .min_h_full()
                                .items_center()
                                .justify_center()
                                .p(px(16.0))
                                .child(card),
                        ),
                )
                .child(ScrollbarOverlay::new(&scroll))
                .into_any_element(),
        )
    }
}

/// The card: the header with the send key, the request line, the summary, the variables and,
/// when some are undefined, the danger banner.
fn render_card(
    preview: &SendPreview,
    send_key: &'static str,
    view: &AppView,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let palette = cx.palette();
    let mono = cx.theme().mono_font_family.clone();
    let undefined = preview.undefined();
    let border = if undefined.is_empty() {
        palette.border
    } else {
        palette.danger.opacity(0.45)
    };

    let url_color = if preview.url_has_undefined {
        palette.danger
    } else {
        palette.fg
    };
    let request_line = div().font_family(mono.clone()).text_size(px(13.0)).child(
        h_flex()
            .gap(px(10.0))
            .items_start()
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.method_color(&preview.method))
                    .child(preview.method.to_string()),
            )
            .child(
                div()
                    .min_w_0()
                    .text_color(url_color)
                    .child(preview.url.clone()),
            ),
    );

    let mut body = v_flex()
        .px(px(16.0))
        .py(px(14.0))
        .gap(px(12.0))
        .child(request_line)
        .child(render_summary(preview, view, cx));
    if !preview.variables.is_empty() {
        body = body.child(render_variables(preview, &mono, cx));
    }
    if !undefined.is_empty() {
        body = body.child(render_undefined_banner(preview, undefined, cx));
    }

    v_flex()
        .w_full()
        .max_w(px(CARD_MAX_WIDTH))
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(border)
        .bg(palette.surface)
        .child(render_header(send_key, &mono, cx))
        .child(body)
        .into_any_element()
}

/// "Will be sent" and "Press Ctrl+Enter to send".
fn render_header(send_key: &'static str, mono: &SharedString, cx: &App) -> AnyElement {
    let palette = cx.palette();
    // The sentence around the key, in the user's language: the key goes where `%{key}` is.
    let sentence = t!("response.preview.send_hint", key = "\u{1}").into_owned();
    let (before, after) = sentence
        .split_once('\u{1}')
        .unwrap_or((sentence.as_str(), ""));
    h_flex()
        .gap(px(10.0))
        .px(px(16.0))
        .py(px(12.0))
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.fg_subtle)
                .child(t!("response.preview.title")),
        )
        .child(div().flex_1())
        .child(
            h_flex()
                .gap(px(6.0))
                .text_size(px(12.0))
                .text_color(palette.fg_subtle)
                .when(!before.trim().is_empty(), |row| {
                    row.child(before.trim().to_string())
                })
                .child(
                    div()
                        .font_family(mono.clone())
                        .text_size(px(11.0))
                        .text_color(palette.fg_muted)
                        .px(px(6.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(palette.border_strong)
                        .child(send_key),
                )
                .when(!after.trim().is_empty(), |row| {
                    row.child(after.trim().to_string())
                }),
        )
        .into_any_element()
}

/// A rounded summary chip.
fn chip(cx: &App) -> Div {
    let palette = cx.palette();
    h_flex()
        .h(px(CHIP_HEIGHT))
        .gap(px(6.0))
        .px(px(8.0))
        .rounded(px(CHIP_HEIGHT / 2.0))
        .bg(palette.raised)
        .text_size(px(12.0))
        .text_color(palette.fg_muted)
}

/// The environment, headers, authorization, body and scripts.
fn render_summary(preview: &SendPreview, view: &AppView, cx: &App) -> AnyElement {
    let palette = cx.palette();
    let environment = match &preview.environment {
        Some(name) => chip(cx)
            .text_color(palette.fg)
            .child(
                div()
                    .size(px(6.0))
                    .rounded_full()
                    .bg(palette.env_color(view.state.env_colors.color(name))),
            )
            .child(name.clone()),
        None => chip(cx).child(t!("common.no_environment")),
    };
    let headers = if preview.headers == 0 {
        t!("response.preview.no_headers").into_owned()
    } else {
        crate::state::locale::plural("response.preview.headers", preview.headers)
    };
    let auth = match &preview.auth {
        AuthSummary::None => t!("response.preview.no_auth").into_owned(),
        AuthSummary::Basic => auth_label(&t!("request.auth.basic")),
        AuthSummary::Bearer => auth_label(&t!("request.auth.bearer")),
        AuthSummary::ApiKey => auth_label(&t!("request.auth.api_key")),
        AuthSummary::OAuth2(profile) if profile.is_empty() => {
            auth_label(&t!("request.auth.oauth2"))
        }
        AuthSummary::OAuth2(profile) => {
            auth_label(&format!("{} ({profile})", t!("request.auth.oauth2")))
        }
    };
    let body = match preview.body {
        BodySummary::None => t!("response.preview.no_body").into_owned(),
        BodySummary::Json => body_label("JSON"),
        BodySummary::Xml => body_label("XML"),
        BodySummary::Text => body_label(&t!("request.body.text")),
        BodySummary::Form => body_label(&t!("request.body.form_urlencoded")),
        BodySummary::Multipart => body_label(&t!("request.body.form")),
    };
    let scripts = match (preview.pre_script, preview.post_script) {
        (false, false) => t!("response.preview.no_scripts"),
        (true, false) => t!("response.preview.pre_script"),
        (false, true) => t!("response.preview.post_script"),
        (true, true) => t!("response.preview.both_scripts"),
    };
    h_flex()
        .flex_wrap()
        .gap(px(6.0))
        .child(environment)
        .child(chip(cx).child(headers))
        .child(chip(cx).child(auth))
        .child(chip(cx).child(body))
        .child(chip(cx).child(scripts))
        .into_any_element()
}

fn auth_label(kind: &str) -> String {
    t!("response.preview.auth", kind = kind).into_owned()
}

fn body_label(kind: &str) -> String {
    t!("response.preview.body", kind = kind).into_owned()
}

/// One row per variable: its name, its value and where it comes from.
fn render_variables(preview: &SendPreview, mono: &SharedString, cx: &App) -> AnyElement {
    let palette = cx.palette();
    let string_color = palette.syntax_color("string");
    let rows = preview.variables.iter().map(|variable| {
        let (value, color) = match &variable.value {
            PreviewValue::Value(value) => (value.clone(), string_color),
            PreviewValue::Masked => (MASK.to_string(), palette.fg_muted),
            PreviewValue::SetByScript => (
                t!("response.preview.value_script").into_owned(),
                palette.fg_muted,
            ),
            PreviewValue::Undefined => (
                t!("response.preview.value_undefined").into_owned(),
                palette.danger,
            ),
        };
        let source = match &variable.source {
            PreviewSource::File(file) => file.clone(),
            PreviewSource::Session => t!("response.preview.source_session").into_owned(),
            PreviewSource::PreScript => t!("response.preview.source_script").into_owned(),
            PreviewSource::NotInFiles { base, local } => t!(
                "response.preview.source_missing",
                base = base,
                local = local
            )
            .into_owned(),
            PreviewSource::NoEnvironment => {
                t!("response.preview.source_no_environment").into_owned()
            }
        };
        h_flex()
            .gap(px(10.0))
            .font_family(mono.clone())
            .text_size(px(12.0))
            .child(
                div()
                    .flex_none()
                    .text_color(palette.accent_text)
                    .child(variable.name.clone()),
            )
            .child(
                Icon::new(IconName::ArrowRight)
                    .size(px(12.0))
                    .text_color(palette.fg_subtle),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color)
                    .child(value),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(cx.theme().font_family.clone())
                    .text_color(palette.fg_subtle)
                    .child(source),
            )
    });
    v_flex()
        .gap(px(4.0))
        .pt(px(10.0))
        .border_t_1()
        .border_color(palette.raised)
        .child(
            div()
                .pb(px(2.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.fg_subtle)
                .child(t!("response.preview.variables")),
        )
        .children(rows)
        .into_any_element()
}

/// "baseUrl is not defined in local", with "Define in local" and "Change environment", or, with
/// no environment active, "Choose environment".
fn render_undefined_banner(
    preview: &SendPreview,
    undefined: Vec<String>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let palette = cx.palette();
    let names = undefined.join(", ");
    let one = undefined.len() == 1;
    let text = match (&preview.environment, one) {
        (Some(environment), true) => {
            t!(
                "response.preview.undefined.one",
                name = names,
                environment = environment
            )
        }
        (Some(environment), false) => {
            t!(
                "response.preview.undefined.other",
                names = names,
                environment = environment
            )
        }
        (None, true) => t!("response.preview.undefined_no_env.one", name = names),
        (None, false) => t!("response.preview.undefined_no_env.other", names = names),
    };
    let define = {
        let weak = cx.weak_entity();
        let label = match &preview.environment {
            Some(environment) => {
                t!("response.preview.define_in", environment = environment).into_owned()
            }
            None => t!("response.preview.choose_environment").into_owned(),
        };
        SecondaryButton::new("send-preview-define", label)
            .height(26.0)
            .on_click(move |_, window, cx| {
                let names = undefined.clone();
                let _ = weak.update(cx, |view, cx| view.define_variables(names, window, cx));
            })
    };
    let change = preview.environment.is_some().then(|| {
        let weak = cx.weak_entity();
        SecondaryButton::new(
            "send-preview-change-environment",
            t!("response.preview.change_environment"),
        )
        .height(26.0)
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |view, cx| {
                if !view.nav.is_active(NavSection::Environments) {
                    view.click_rail(NavSection::Environments, cx);
                }
            });
        })
    });
    h_flex()
        .flex_wrap()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(9.0))
        .rounded(px(RADIUS_MD))
        .bg(palette.danger_subtle)
        .text_color(palette.fg)
        .child(
            Icon::new(IconName::CircleAlert)
                .size(px(15.0))
                .text_color(palette.danger),
        )
        .child(div().flex_1().min_w(px(160.0)).child(text))
        .child(define)
        .children(change)
        .into_any_element()
}
