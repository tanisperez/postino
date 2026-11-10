//! The response side of OAuth (design OAuth 3b, 3c, 3d): the panel shown while the user signs
//! in, the "Token renewed" chip, the green "New token" notice, and the error cards.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_oauth::{Force, OAuthStep};
use postino_runner::FailedStage;
use rust_i18n::t;

use crate::state::oauth::{self, ErrorAction, ErrorCard, ErrorTone, TokenNotice};
use crate::state::ui_tabs::{RequestTab, ResponseTab};
use crate::theme::PaletteExt;
use crate::theme::metrics::{OAUTH_BUTTON_HEIGHT, RADIUS_LG, RADIUS_MD};
use crate::views::components::SecondaryButton;

use super::super::root::AppView;
use super::auth_tab::link;
use super::oauth_auth;

impl AppView {
    /// Whether the response panel of `tab_id` shows the sign-in panel.
    pub(crate) fn shows_sign_in(&self, tab_id: &str) -> bool {
        self.oauth.waiting.is_some() && (self.is_sending(tab_id) || self.oauth.is_busy())
    }

    /// The panel shown while the user signs in in the browser (design 3c, step 1).
    pub(crate) fn render_sign_in_panel(&self, cx: &Context<Self>) -> AnyElement {
        let Some(prompt) = self.oauth.waiting.clone() else {
            return div().into_any_element();
        };
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let environment = if prompt.environment.is_empty() {
            t!("oauth.no_environment").into_owned()
        } else {
            prompt.environment.clone()
        };
        let open_url = prompt.url.clone();
        let copy_url = prompt.url.clone();
        let cancel_weak = weak.clone();
        let paste_weak = weak.clone();
        let mut footer = h_flex()
            .gap(px(4.0))
            .items_center()
            .text_size(px(12.0))
            .text_color(palette.fg_subtle);
        if let Some(address) = &prompt.listening {
            footer = footer
                .child(t!("oauth.waiting_on", address = address.as_str()).into_owned())
                .child("·");
        }
        footer = footer
            .child(
                link(
                    "oauth-copy-sign-in",
                    t!("oauth.copy_sign_in_link").into_owned(),
                    &palette,
                )
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                }),
            )
            .child("·")
            .child(
                link(
                    "oauth-paste-redirect",
                    t!("oauth.paste_redirect").into_owned(),
                    &palette,
                )
                .on_click(move |_, window, cx| {
                    let _ = paste_weak.update(cx, |view, cx| {
                        view.open_paste_redirect_dialog(window, cx);
                    });
                }),
            );

        v_flex()
            .id("oauth-sign-in-panel")
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(12.0))
            .p(px(24.0))
            .text_center()
            .child(
                div()
                    .size(px(44.0))
                    .rounded_full()
                    .bg(palette.accent_subtle)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(Lucide::Globe)
                            .with_size(px(20.0))
                            .text_color(palette.accent_text),
                    ),
            )
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(
                        t!("oauth.sign_in_title", profile = prompt.profile.as_str()).into_owned(),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(4.0))
                    .flex_wrap()
                    .justify_center()
                    .text_color(palette.fg_muted)
                    .child(t!("oauth.sign_in_text").into_owned())
                    .child(t!("oauth.environment_word").into_owned())
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.fg)
                            .child(format!("{environment}.")),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.0))
                    .child(
                        SecondaryButton::new("oauth-open-browser", t!("oauth.open_browser_again"))
                            .height(OAUTH_BUTTON_HEIGHT)
                            .on_click(move |_, _, cx| cx.open_url(&open_url)),
                    )
                    .child(
                        crate::views::components::GhostButton::new(
                            "oauth-cancel-sign-in",
                            t!("common.cancel"),
                        )
                        .height(OAUTH_BUTTON_HEIGHT)
                        .on_click(move |_, _, cx| {
                            let _ = cancel_weak.update(cx, |view, cx| view.cancel_sign_in(cx));
                        }),
                    ),
            )
            .child(footer)
            .into_any_element()
    }

    /// The token chip of the response bar (design 3b): "Token renewed", with the profile,
    /// environment and new expiry on hover; a click opens the console at the token request.
    pub(crate) fn render_token_chip(&self, tab_id: &str, cx: &Context<Self>) -> Option<AnyElement> {
        let result = self.responses.get(tab_id)?;
        let run = result.oauth.as_ref()?;
        let notice = oauth::token_notice(&run.profile, &run.environment, run.event?, run.token()?)?;
        let TokenNotice::Renewed { tooltip } = notice else {
            return None;
        };
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let tab = tab_id.to_string();
        Some(
            h_flex()
                .id("oauth-token-chip")
                .h(px(22.0))
                .px_2()
                .gap(px(6.0))
                .items_center()
                .rounded(px(RADIUS_MD - 2.0))
                .bg(palette.accent_subtle)
                .text_color(palette.accent_text)
                .text_size(px(12.0))
                .cursor_pointer()
                .child(Icon::new(Lucide::KeyRound).with_size(px(12.0)))
                .child(t!("oauth.token_renewed").into_owned())
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |view, cx| view.open_console_at_token(&tab, cx));
                })
                .into_any_element(),
        )
    }

    /// Opens the Console of `tab_id` on the token request.
    fn open_console_at_token(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        self.active_response_tab = ResponseTab::Console;
        let generation = self.response_generations.get(tab_id);
        self.console_view.sync(tab_id, generation);
        self.console_view.filter = crate::state::console::ConsoleFilter::All;
        let index = self
            .response_consoles
            .get(tab_id)
            .and_then(|timeline| timeline.token_network);
        self.console_view.open_network(index);
        cx.notify();
    }

    /// The OAuth messages above the response: the green notice of a new token (design 3c, step
    /// 3), the card of a token that could not be obtained, or of a 401 (design 3d).
    pub(crate) fn render_oauth_messages(
        &self,
        tab_id: &str,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(result) = self.responses.get(tab_id) else {
            return Vec::new();
        };
        let Some(run) = result.oauth.as_ref() else {
            return Vec::new();
        };
        let palette = cx.palette();
        let mut messages = Vec::new();
        if let (Some(event), Some(token)) = (run.event, run.token())
            && let Some(TokenNotice::New { text }) =
                oauth::token_notice(&run.profile, &run.environment, event, token)
        {
            messages.push(
                h_flex()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(7.0))
                    .items_center()
                    .rounded(px(RADIUS_MD - 1.0))
                    .bg(palette.success_subtle)
                    .text_color(palette.fg)
                    .text_size(px(12.0))
                    .child(
                        Icon::new(Lucide::KeyRound)
                            .with_size(px(12.0))
                            .text_color(palette.success),
                    )
                    .child(text)
                    .into_any_element(),
            );
        }
        if matches!(result.failed_stage, Some(FailedStage::Auth))
            && let Some(error) = result.oauth_error()
        {
            let exchange = run.steps.iter().rev().find_map(|step| match step {
                OAuthStep::Exchange(exchange) => Some(oauth::exchange_line(
                    &exchange.request.method.to_string(),
                    &exchange.request.url,
                    exchange.response.as_ref().map(|response| response.status),
                )),
                _ => None,
            });
            let card = oauth::error_card(&run.profile, &run.environment, error, exchange);
            messages.push(self.render_error_card(tab_id, card, cx));
        }
        let generation = self.response_generations.get(tab_id);
        if let Some(response) = &result.response
            && response.status == 401
            && !self
                .oauth
                .dismissed
                .contains(&(tab_id.to_string(), generation))
        {
            let header = response
                .headers
                .iter()
                .find(|header| header.name.eq_ignore_ascii_case("www-authenticate"))
                .map(|header| header.value.as_str());
            messages.push(self.render_error_card(tab_id, oauth::unauthorized_card(header), cx));
        }
        messages
    }

    /// One error card (design 3d).
    fn render_error_card(&self, tab_id: &str, card: ErrorCard, cx: &Context<Self>) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let (bg, icon, icon_color) = match card.tone {
            ErrorTone::Danger => (palette.danger_subtle, Lucide::CircleAlert, palette.danger),
            ErrorTone::Warning => (
                palette.warning_subtle,
                Lucide::TriangleAlert,
                palette.warning,
            ),
            ErrorTone::Info => (palette.info_subtle, Lucide::ShieldAlert, palette.info),
        };
        let mut buttons = h_flex().gap(px(8.0));
        for (index, action) in card.actions.into_iter().enumerate() {
            let action_weak = weak.clone();
            let tab = tab_id.to_string();
            let button = h_flex()
                .id(("oauth-error-action", index))
                .h(px(OAUTH_BUTTON_HEIGHT))
                .px(px(12.0))
                .items_center()
                .rounded(px(RADIUS_MD - 1.0))
                .cursor_pointer()
                .when(index == 0, |button| {
                    button
                        .bg(palette.border)
                        .hover(|style| style.bg(palette.border_strong))
                })
                .when(index > 0, |button| {
                    button
                        .border_1()
                        .border_color(palette.border)
                        .text_color(palette.fg_muted)
                        .hover(|style| style.bg(palette.hover))
                })
                .child(action.label())
                .on_click(move |_, window, cx| {
                    let _ = action_weak.update(cx, |view, cx| {
                        view.run_error_action(&tab, action, window, cx);
                    });
                });
            buttons = buttons.child(button);
        }
        v_flex()
            .gap(px(10.0))
            .p(px(16.0))
            .rounded(px(RADIUS_LG))
            .bg(palette.bg)
            .border_1()
            .border_color(palette.border)
            .child(
                h_flex()
                    .gap(px(10.0))
                    .items_start()
                    .px(px(12.0))
                    .py(px(10.0))
                    .rounded(px(RADIUS_MD))
                    .bg(bg)
                    .child(
                        div()
                            .mt(px(1.0))
                            .child(Icon::new(icon).with_size(px(15.0)).text_color(icon_color)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap(px(4.0))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(card.title))
                            .child(div().child(card.message)),
                    ),
            )
            .when(!card.detail.is_empty(), |content| {
                content.child(
                    div()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(11.5))
                        .text_color(palette.fg_subtle)
                        .child(card.detail),
                )
            })
            .child(buttons)
            .into_any_element()
    }

    /// Runs the button of an error card.
    fn run_error_action(
        &mut self,
        tab_id: &str,
        action: ErrorAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ErrorAction::EditProfile => {
                self.active_request_tab = RequestTab::Auth;
                if !self.state.settings.oauth_profile_settings_open {
                    self.toggle_profile_settings(cx);
                }
                cx.notify();
            }
            ErrorAction::OpenInConsole => self.open_console_at_token(tab_id, cx),
            ErrorAction::CopyRedirectUrl => {
                let url = self
                    .state
                    .tabs
                    .active()
                    .and_then(|tab| tab.request())
                    .and_then(|request| oauth_auth(&request.auth))
                    .and_then(|(name, _)| self.state.oauth_profile(tab_id, name))
                    .map(|profile| match profile.redirect {
                        postino_core::OAuthRedirect::Loopback => "http://127.0.0.1".to_string(),
                        _ => profile.redirect_url,
                    })
                    .unwrap_or_else(|| "http://127.0.0.1".to_string());
                cx.write_to_clipboard(ClipboardItem::new_string(url));
            }
            ErrorAction::NewTokenAndResend => self.resend_with_new_token(tab_id, cx),
            ErrorAction::Dismiss => {
                let generation = self.response_generations.get(tab_id);
                self.oauth
                    .dismissed
                    .insert((tab_id.to_string(), generation));
                cx.notify();
            }
            ErrorAction::Retry => {
                let _ = window;
                self.send_active_tab(cx);
            }
        }
    }

    /// "Get new token and resend" (design 3d): a 401 never retries on its own; this renews the
    /// token, even a valid one, and sends again.
    fn resend_with_new_token(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        self.send_with_force(tab_id, Force::Refresh, cx);
    }
}
