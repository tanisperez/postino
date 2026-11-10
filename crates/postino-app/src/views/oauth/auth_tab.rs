//! The OAuth 2.0 type of the Auth tab (design OAuth 3a): the profile picker and its menu, the
//! Profile settings button, Scope, the token card and the "Sends" line, and the first use empty
//! state (3a·2, step 3).

use std::time::SystemTime;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_core::{OAuthGrant, OAuthProfile, Request};
use postino_oauth::{Force, StoreKind};
use rust_i18n::t;

use crate::state::oauth::{self, CardState};
use crate::theme::metrics::{
    INPUT_PADDING_X, OAUTH_BUTTON_HEIGHT, OAUTH_CARD_LABEL_WIDTH, OAUTH_LABEL_WIDTH,
    OAUTH_MENU_WIDTH, OAUTH_PICKER_HEIGHT, RADIUS_LG, RADIUS_MD,
};
use crate::theme::{PaletteExt, PostinoColors};
use crate::views::components::{
    GhostButton, HoverInput, InlineMessage, InlineMessageKind, NeutralButtonExt, PrimaryButton,
    SecondaryButton, SegmentedControl, SegmentedItem, edit_menu, rich_menu_width,
};

use super::super::root::AppView;
use super::oauth_auth;

/// Height of a row of the profile menu (design 3a·2, step 2).
const MENU_ROW_HEIGHT: f32 = 40.0;

impl AppView {
    /// The OAuth 2.0 part of the Auth tab, under the type selector.
    pub(crate) fn render_oauth_section(
        &self,
        tab_id: &str,
        request: &Request,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some((name, _scope)) = oauth_auth(&request.auth) else {
            return div().into_any_element();
        };
        let profiles = self.oauth_profiles_for(tab_id);
        if profiles.is_empty() {
            return self.render_oauth_empty(tab_id, cx);
        }
        let profile = profiles
            .iter()
            .find(|profile| profile.name == name)
            .cloned();
        let palette = cx.palette();
        let open = self.state.settings.oauth_profile_settings_open && profile.is_some();

        let mut content = v_flex().gap(px(14.0));
        content = content.child(
            h_flex()
                .gap_3()
                .items_center()
                .child(label(t!("oauth.profile").into_owned(), &palette))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(self.render_profile_picker(tab_id, name, &profiles, cx)),
                )
                .when(profile.is_some(), |row| {
                    row.child(render_settings_toggle(open, cx))
                }),
        );
        if let Some(profile) = profile.as_ref() {
            if open {
                content = content.child(self.render_profile_panel(tab_id, profile, cx));
            }
            content = content.child(self.render_scope_row(profile, cx));
            content = content.child(self.render_token_card(tab_id, request, profile, cx));
            content = content.child(self.render_sends_line(request, profile, cx));
        } else {
            content = content.child(InlineMessage::new(
                InlineMessageKind::Danger,
                t!("oauth.unknown_profile", profile = name).into_owned(),
            ));
        }
        if let Some(error) = &self.oauth.card_error {
            content = content.child(InlineMessage::new(InlineMessageKind::Danger, error.clone()));
        }
        content.into_any_element()
    }

    /// The profile picker: the key, the name, its grant, and the menu (design 3a·2, step 2).
    fn render_profile_picker(
        &self,
        tab_id: &str,
        current: &str,
        profiles: &[OAuthProfile],
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let selected = profiles.iter().find(|profile| profile.name == current);
        let grant = selected.map(oauth::grant_label).unwrap_or_default();
        let missing = selected.is_none();
        let tree = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| workspace.tree().to_vec())
            .unwrap_or_default();
        let rows: Vec<(String, String, usize)> = profiles
            .iter()
            .map(|profile| {
                (
                    profile.name.clone(),
                    oauth::grant_label(profile),
                    oauth::usage_count(&tree, &profile.name),
                )
            })
            .collect();
        let current = current.to_string();
        let tab_id = tab_id.to_string();

        Button::new("oauth-profile-picker")
            .neutral_bordered(cx, palette.raised, palette.border_strong, palette.fg)
            .h(px(OAUTH_PICKER_HEIGHT))
            .w_full()
            .px(px(INPUT_PADDING_X))
            .rounded(px(RADIUS_MD - 1.0))
            .border_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(Lucide::KeyRound)
                            .with_size(px(13.0))
                            .text_color(palette.accent_text),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .when(missing, |name| name.text_color(palette.danger))
                            .child(if current.is_empty() {
                                t!("oauth.choose_profile").into_owned()
                            } else {
                                current.clone()
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(palette.fg_subtle)
                            .child(if missing && !current.is_empty() {
                                t!("oauth.profile_not_found").into_owned()
                            } else {
                                grant
                            }),
                    ),
            )
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                // Many profiles scroll inside the menu instead of running off the window.
                let mut menu = rich_menu_width(menu, OAUTH_MENU_WIDTH)
                    .scrollable(true)
                    .max_h(px(MENU_ROW_HEIGHT * 8.0 + 80.0));
                for (name, grant, uses) in &rows {
                    let active = *name == current;
                    let (name, grant) = (name.clone(), grant.clone());
                    let uses = crate::state::locale::plural("oauth.request_count", *uses);
                    let (accent, subtle) = (palette.accent_text, palette.fg_subtle);
                    let pick_weak = weak.clone();
                    let pick_tab = tab_id.clone();
                    let pick_name = name.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            h_flex()
                                .h(px(MENU_ROW_HEIGHT))
                                .w_full()
                                .items_center()
                                .gap(px(10.0))
                                .px_2()
                                .child(div().w(px(14.0)).flex_none().when(active, |cell| {
                                    cell.child(
                                        Icon::new(Lucide::Check)
                                            .with_size(px(13.0))
                                            .text_color(accent),
                                    )
                                }))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(px(1.0))
                                        .child(
                                            div()
                                                .truncate()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(name.clone()),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(11.5))
                                                .text_color(subtle)
                                                .child(grant.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(12.0))
                                        .text_color(subtle)
                                        .child(uses.clone()),
                                )
                        })
                        .on_click(move |_, _, cx| {
                            let _ = pick_weak.update(cx, |view, cx| {
                                view.select_oauth_profile(&pick_tab, &pick_name, cx);
                            });
                        }),
                    );
                }
                menu = menu.separator();
                let (accent, muted) = (palette.accent_text, palette.fg_muted);
                // Rows drawn here with their own icon column, like the profiles above: an item
                // with a menu icon would make the menu indent every row.
                let action_row = move |icon: Lucide, text: String, color: Hsla| {
                    move |_: &mut Window, _: &mut App| {
                        h_flex()
                            .h(px(32.0))
                            .w_full()
                            .items_center()
                            .gap(px(10.0))
                            .px_2()
                            .text_color(color)
                            .child(
                                div()
                                    .w(px(14.0))
                                    .flex_none()
                                    .child(Icon::new(icon).with_size(px(13.0))),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(text.clone()))
                    }
                };
                let new_weak = weak.clone();
                let new_tab = tab_id.clone();
                menu = menu.item(
                    PopupMenuItem::element(action_row(
                        Lucide::Plus,
                        t!("oauth.new_profile").into_owned(),
                        accent,
                    ))
                    .on_click(move |_, window, cx| {
                        let _ = new_weak.update(cx, |view, cx| {
                            view.open_new_profile_dialog(&new_tab, None, window, cx);
                        });
                    }),
                );
                if !current.is_empty() && rows.iter().any(|(name, _, _)| *name == current) {
                    let from_weak = weak.clone();
                    let from_tab = tab_id.clone();
                    let from = current.clone();
                    menu = menu.item(
                        PopupMenuItem::element(action_row(
                            Lucide::Copy,
                            t!("oauth.new_profile_from", profile = current.as_str()).into_owned(),
                            muted,
                        ))
                        .on_click(move |_, window, cx| {
                            let _ = from_weak.update(cx, |view, cx| {
                                view.open_new_profile_dialog(
                                    &from_tab,
                                    Some(from.clone()),
                                    window,
                                    cx,
                                );
                            });
                        }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// The Scope row: the request's override, or the profile's scope shown muted with "from
    /// profile".
    fn render_scope_row(&self, profile: &OAuthProfile, cx: &Context<Self>) -> AnyElement {
        let palette = cx.palette();
        let input = self
            .request_editor
            .auth
            .iter()
            .find(|(field, _)| *field == crate::state::auth_edit::AuthField::Scope)
            .map(|(_, input)| input.clone());
        let Some(input) = input else {
            return div().into_any_element();
        };
        let empty = input.read(cx).value().trim().is_empty();
        let ui_font = cx.theme().font_family.clone();
        let from_profile = (empty && !profile.scope.trim().is_empty()).then(|| {
            div()
                .flex_none()
                .font_family(ui_font)
                .text_size(px(11.5))
                .text_color(palette.fg_subtle)
                .child(t!("oauth.from_profile").into_owned())
        });
        let field = HoverInput::new(
            "oauth-scope-input",
            &input,
            Input::new(&input)
                .context_menu(edit_menu(&input, cx))
                .px(px(INPUT_PADDING_X))
                .py_0()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(12.0))
                .when(empty, |input| input.bg(palette.surface))
                .when_some(from_profile, |input, suffix| input.suffix(suffix)),
        );
        h_flex()
            .gap_3()
            .items_center()
            .child(label(t!("oauth.scope").into_owned(), &palette))
            .child(div().flex_1().min_w_0().child(field))
            .into_any_element()
    }

    /// The token card (design 3a): the active environment's token of the profile.
    fn render_token_card(
        &self,
        tab_id: &str,
        request: &Request,
        profile: &OAuthProfile,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let scope = oauth_auth(&request.auth)
            .map(|(_, scope)| scope)
            .unwrap_or("");
        let key = self.oauth_key(profile, scope);
        let token = self.oauth.token(&key).flatten();
        let environment = self.state.active_environment.clone().unwrap_or_default();
        let now = SystemTime::now();
        let card = oauth::token_card(
            profile,
            &environment,
            token,
            self.oauth.store_kind.unwrap_or(StoreKind::System),
            now,
        );
        let busy = self.oauth.is_busy();
        let (dot, border, head_bg) = match card.state {
            CardState::Valid => (palette.success, palette.border, palette.surface),
            CardState::Expired => (
                palette.warning,
                palette.warning.opacity(0.35),
                palette.warning_subtle.opacity(0.75),
            ),
            CardState::None => (palette.fg_subtle, palette.border, palette.surface),
        };

        let env_chip = h_flex()
            .flex_none()
            .h(px(22.0))
            .px_2()
            .gap(px(6.0))
            .items_center()
            .rounded(px(11.0))
            .bg(palette.raised)
            .text_size(px(12.0))
            .child(
                div()
                    .size(px(6.0))
                    .rounded_full()
                    .bg(if environment.is_empty() {
                        palette.fg_subtle
                    } else {
                        palette.env_color(self.state.env_colors.color(&environment))
                    }),
            )
            .child(if environment.is_empty() {
                t!("common.no_environment").into_owned()
            } else {
                environment.clone()
            });

        let head = h_flex()
            .gap(px(10.0))
            .px(px(14.0))
            .py(px(12.0))
            .items_center()
            .rounded_t(px(RADIUS_LG - 1.0))
            .bg(head_bg)
            .child(div().flex_none().size(px(8.0)).rounded_full().bg(dot))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(card.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(palette.fg_muted)
                            .child(card.subtitle.clone()),
                    ),
            )
            .child(env_chip);

        let details = token.map(|token| {
            let access = token.access_token.clone();
            let mono = cx.theme().mono_font_family.clone();
            let claims = oauth::card_claims(token, now);
            let all_claims = oauth::all_claims_text(token);
            let mut claim_row = h_flex()
                .flex_wrap()
                .gap(px(4.0))
                .font_family(mono.clone())
                .text_size(px(12.0))
                .text_color(palette.fg);
            for (index, claim) in claims.iter().enumerate() {
                if index > 0 {
                    claim_row = claim_row.child(div().text_color(palette.fg_subtle).child("·"));
                }
                claim_row = claim_row.child(claim.name.clone()).child(
                    div()
                        .text_color(if claim.time {
                            palette.warning
                        } else {
                            palette.syntax_color("string")
                        })
                        .child(claim.value.clone()),
                );
            }
            let has_claims = !claims.is_empty();
            let claims_weak = weak.clone();
            let ui_font = cx.theme().font_family.clone();
            claim_row = claim_row.child(
                link(
                    "oauth-all-claims",
                    t!("oauth.all_claims").into_owned(),
                    &palette,
                )
                .ml(px(8.0))
                .font_family(ui_font)
                .on_click(move |_, window, cx| {
                    let text = all_claims.clone();
                    let _ = claims_weak.update(cx, |view, cx| {
                        view.open_all_claims_dialog(text, window, cx);
                    });
                }),
            );
            let row = |name: String, value: AnyElement| {
                h_flex()
                    .items_start()
                    .child(
                        div()
                            .w(px(OAUTH_CARD_LABEL_WIDTH))
                            .flex_none()
                            .text_color(palette.fg_subtle)
                            .child(name),
                    )
                    .child(div().flex_1().min_w_0().child(value))
            };
            v_flex()
                .gap(px(6.0))
                .px(px(14.0))
                .py(px(12.0))
                .border_t_1()
                .border_color(palette.border)
                .text_size(px(12.5))
                .child(row(
                    t!("oauth.access_token").into_owned(),
                    h_flex()
                        .gap(px(8.0))
                        .child(
                            div()
                                .font_family(mono.clone())
                                .text_size(px(12.0))
                                .text_color(palette.fg)
                                .child(oauth::abbreviate(&access, 32)),
                        )
                        .child(
                            link("oauth-copy-token", t!("common.copy").into_owned(), &palette)
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        access.clone(),
                                    ));
                                }),
                        )
                        .into_any_element(),
                ))
                .child(row(
                    t!("oauth.obtained").into_owned(),
                    div()
                        .text_color(palette.fg)
                        .child(oauth::obtained_line(token))
                        .into_any_element(),
                ))
                .when(has_claims, |rows| {
                    rows.child(row(
                        t!("oauth.claims").into_owned(),
                        claim_row.into_any_element(),
                    ))
                })
        });

        let primary_force = match card.state {
            CardState::None => Force::Grant,
            CardState::Valid | CardState::Expired => Force::Refresh,
        };
        let primary_icon = if card.state == CardState::None {
            Lucide::LogIn
        } else {
            Lucide::RefreshCw
        };
        let primary_tab = tab_id.to_string();
        let primary_weak = weak.clone();
        let on_primary = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = primary_weak.update(cx, |view, cx| {
                view.run_oauth_action(&primary_tab, primary_force, cx);
            });
        };
        let primary = if card.state == CardState::Valid {
            SecondaryButton::new("oauth-card-primary", card.primary.clone())
                .icon(Icon::new(primary_icon))
                .height(OAUTH_BUTTON_HEIGHT)
                .disabled(busy)
                .on_click(on_primary)
                .into_any_element()
        } else {
            PrimaryButton::new("oauth-card-primary", card.primary.clone())
                .icon(Icon::new(primary_icon))
                .height(OAUTH_BUTTON_HEIGHT)
                .disabled(busy)
                .on_click(on_primary)
                .into_any_element()
        };
        let mut buttons = h_flex()
            .gap(px(8.0))
            .px(px(14.0))
            .py(px(10.0))
            .items_center()
            .border_t_1()
            .border_color(palette.border)
            .child(primary);
        if token.is_some() {
            let again_tab = tab_id.to_string();
            let again_weak = weak.clone();
            let clear_weak = weak.clone();
            let clear_key = key.clone();
            buttons = buttons
                .child(
                    SecondaryButton::new("oauth-card-again", card.again.clone())
                        .icon(Icon::new(Lucide::LogIn))
                        .height(OAUTH_BUTTON_HEIGHT)
                        .disabled(busy)
                        .on_click(move |_, _, cx| {
                            let _ = again_weak.update(cx, |view, cx| {
                                view.run_oauth_action(&again_tab, Force::Grant, cx);
                            });
                        }),
                )
                .child(
                    GhostButton::new("oauth-card-clear", t!("oauth.clear"))
                        .icon(Icon::new(Lucide::Trash))
                        .height(OAUTH_BUTTON_HEIGHT)
                        .disabled(busy)
                        .on_click(move |_, _, cx| {
                            let key = clear_key.clone();
                            let _ =
                                clear_weak.update(cx, |view, cx| view.clear_oauth_token(key, cx));
                        }),
                );
        }
        buttons = buttons.child(div().flex_1()).child(
            div()
                .text_size(px(12.0))
                .text_color(palette.fg_subtle)
                .child(card.foot.clone()),
        );

        v_flex()
            .mt(px(4.0))
            .border_1()
            .border_color(border)
            .rounded(px(RADIUS_LG))
            .bg(palette.surface)
            .child(head)
            .children(details)
            .child(buttons)
            .into_any_element()
    }

    /// "→ Sends Authorization: Bearer eyJhbGciOi…" under the card.
    fn render_sends_line(
        &self,
        request: &Request,
        profile: &OAuthProfile,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let scope = oauth_auth(&request.auth)
            .map(|(_, scope)| scope)
            .unwrap_or("");
        let key = self.oauth_key(profile, scope);
        let card = oauth::token_card(
            profile,
            self.state.active_environment.as_deref().unwrap_or_default(),
            self.oauth.token(&key).flatten(),
            StoreKind::System,
            SystemTime::now(),
        );
        h_flex()
            .gap(px(8.0))
            .items_center()
            .text_size(px(12.0))
            .text_color(palette.fg_subtle)
            .child(Icon::new(Lucide::ArrowRight).with_size(px(12.0)))
            .child(t!("oauth.sends").into_owned())
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(palette.fg_muted)
                    .child(card.sends),
            )
            .into_any_element()
    }

    /// The first use (design 3a·2, step 3): no profile in the workspace, and the create form.
    fn render_oauth_empty(&self, tab_id: &str, cx: &Context<Self>) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let create_weak = weak.clone();
        let empty = v_flex()
            .items_center()
            .gap(px(8.0))
            .p(px(16.0))
            .border_1()
            .border_dashed()
            .border_color(palette.border_strong)
            .rounded(px(RADIUS_LG))
            .text_center()
            .child(
                Icon::new(Lucide::KeyRound)
                    .with_size(px(20.0))
                    .text_color(palette.fg_subtle),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("oauth.empty_title").into_owned()),
            )
            .child(
                div()
                    .text_color(palette.fg_muted)
                    .child(t!("oauth.empty_text").into_owned()),
            )
            .child(
                PrimaryButton::new("oauth-create-profile", t!("oauth.create_profile"))
                    .icon(Icon::new(Lucide::Plus))
                    .height(30.0)
                    .on_click(move |_, window, cx| {
                        let _ = create_weak.update(cx, |view, cx| {
                            view.start_create_profile(window, cx);
                        });
                    }),
            );
        let mut column = v_flex().gap(px(12.0)).child(empty);
        if let Some(grant) = self.oauth.creating {
            column =
                column.child(self.render_create_form(cx.weak_entity(), tab_id, grant, true, cx));
        }
        column.into_any_element()
    }

    /// The create form: Name and Grant type, the hint about variables, and Create. `inline` is
    /// the empty state's copy, framed like the design; the dialog shows it bare.
    pub(crate) fn render_create_form(
        &self,
        weak: WeakEntity<AppView>,
        tab_id: &str,
        grant: OAuthGrant,
        inline: bool,
        cx: &App,
    ) -> AnyElement {
        let palette = cx.palette();
        let Some(input) = self.oauth.inputs.create_name.clone() else {
            return div().into_any_element();
        };
        let mut selector = SegmentedControl::new("oauth-create-grant");
        for option in OAuthGrant::ALL {
            let pick_weak = weak.clone();
            selector = selector.item(
                SegmentedItem::new(oauth::grant_option_label(option))
                    .selected(option == grant)
                    .on_click(move |_, cx| {
                        let _ = pick_weak.update(cx, |view, cx| {
                            view.oauth.creating = Some(option);
                            cx.notify();
                        });
                    }),
            );
        }
        let name = input.read(cx).value().to_string();
        let problem = (!name.trim().is_empty())
            .then(|| oauth::name_problem(&name, &self.state.oauth_profiles, None))
            .flatten();
        let create_weak = weak.clone();
        let create_tab = tab_id.to_string();
        let form = v_flex()
            .gap(px(10.0))
            .when(inline, |form| {
                form.p(px(14.0))
                    .border_1()
                    .border_color(palette.border_strong)
                    .rounded(px(RADIUS_LG))
                    .bg(palette.surface)
            })
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .w(px(90.0))
                            .flex_none()
                            .text_color(palette.fg_muted)
                            .child(t!("oauth.name").into_owned()),
                    )
                    .child(
                        div().flex_1().child(HoverInput::new(
                            "oauth-create-name",
                            &input,
                            Input::new(&input)
                                .context_menu(edit_menu(&input, cx))
                                .px(px(INPUT_PADDING_X))
                                .py_0()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_size(px(12.0)),
                        )),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .w(px(90.0))
                            .flex_none()
                            .text_color(palette.fg_muted)
                            .child(t!("oauth.grant_type").into_owned()),
                    )
                    .child(selector),
            )
            .children(problem.clone().map(|problem| {
                div()
                    .text_size(px(12.0))
                    .text_color(palette.danger)
                    .child(problem)
            }))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(palette.fg_subtle)
                    .child(t!("oauth.create_hint").into_owned()),
            );
        if !inline {
            return form.into_any_element();
        }
        form.child(
            h_flex().justify_end().child(
                PrimaryButton::new("oauth-create-confirm", t!("oauth.create"))
                    .height(OAUTH_BUTTON_HEIGHT)
                    .disabled(name.trim().is_empty() || problem.is_some())
                    .on_click(move |_, _, cx| {
                        let _ = create_weak.update(cx, |view, cx| {
                            view.confirm_create_profile(&create_tab, None, cx);
                        });
                    }),
            ),
        )
        .into_any_element()
    }
}

/// A field label of the OAuth 2.0 Auth tab.
fn label(text: String, palette: &crate::theme::Palette) -> Div {
    div()
        .w(px(OAUTH_LABEL_WIDTH))
        .flex_none()
        .text_color(palette.fg_muted)
        .child(text)
}

/// "Profile settings": unfolds the profile under the picker (design 3a·2).
fn render_settings_toggle(open: bool, cx: &Context<AppView>) -> AnyElement {
    let palette = cx.palette();
    let weak = cx.weak_entity();
    h_flex()
        .id("oauth-profile-settings")
        .flex_none()
        .h(px(OAUTH_PICKER_HEIGHT))
        .px(px(10.0))
        .gap(px(6.0))
        .items_center()
        .rounded(px(RADIUS_MD - 1.0))
        .text_color(palette.accent_text)
        .cursor_pointer()
        .when(open, |button| button.bg(palette.accent_subtle))
        .when(!open, |button| {
            button.hover(|style| style.bg(palette.hover))
        })
        .child(Icon::new(Lucide::SlidersHorizontal).with_size(px(13.0)))
        .child(t!("oauth.profile_settings").into_owned())
        .child(
            Icon::new(if open {
                Lucide::ChevronUp
            } else {
                Lucide::ChevronDown
            })
            .with_size(px(13.0)),
        )
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |view, cx| view.toggle_profile_settings(cx));
        })
        .into_any_element()
}

/// A text link in the accent color, like the design's "Copy" and "All claims".
pub(crate) fn link(
    id: impl Into<ElementId>,
    text: String,
    palette: &crate::theme::Palette,
) -> Stateful<Div> {
    let (color, hover) = (palette.accent_text, palette.fg);
    div()
        .id(id)
        .flex_none()
        .cursor_pointer()
        .text_color(color)
        .hover(move |style| style.text_color(hover))
        .child(text)
}
