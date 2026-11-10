//! Profile settings (design OAuth 3a·2, step 1): the profile unfolded inside the Auth tab, between
//! the picker and the token card, with the same controls as the rest of the tab.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_core::{
    ClientAuth, ExtraParam, OAuthGrant, OAuthProfile, OAuthRedirect, ParamTarget, Request,
    TokenPlacement,
};
use postino_oauth::Force;
use rust_i18n::t;

use crate::state::oauth::{self, FieldHint, ProfileFieldKind};
use crate::theme::metrics::{
    OAUTH_ADVANCED_LABEL_WIDTH, OAUTH_BUTTON_HEIGHT, OAUTH_HINT_WIDTH, OAUTH_PANEL_LABEL_WIDTH,
    RADIUS_LG, RADIUS_MD,
};
use crate::theme::{PaletteExt, PostinoColors};
use crate::views::components::{
    GhostButton, HoverInput, NeutralButtonExt, SecondaryButton, SegmentedControl, SegmentedItem,
    VariableField, edit_menu, switch_halo,
};
use crate::views::request_editor::define_variable_click;

use super::super::root::AppView;

/// The options of a segmented control of Advanced: label, whether selected, and what picking it
/// changes in the profile.
type ProfileOptions = Vec<(String, bool, Box<dyn Fn(&mut OAuthProfile)>)>;
use super::auth_tab::link;
use super::oauth_auth;

impl AppView {
    /// Builds the inputs of Profile settings when the request's profile, its grant, where its
    /// token goes or its redirect changed, and keeps Scope's placeholder on the profile's scope.
    /// Called from the request editor's render with the window, never per keystroke work.
    pub(crate) fn sync_oauth_inputs(
        &mut self,
        tab_id: &str,
        request: &Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((name, scope)) = oauth_auth(&request.auth) else {
            return;
        };
        let profile = self.state.oauth_profile(tab_id, name);
        if let Some(profile) = &profile {
            let key = self.oauth_key(profile, scope);
            self.load_oauth_token(&key, cx);
            if let Some((_, input)) = self
                .request_editor
                .auth
                .iter()
                .find(|(field, _)| *field == crate::state::auth_edit::AuthField::Scope)
                && self.oauth.scope_placeholder.as_deref() != Some(profile.scope.as_str())
            {
                let placeholder = profile.scope.clone();
                self.oauth.scope_placeholder = Some(placeholder.clone());
                input.update(cx, |state, cx| {
                    state.set_placeholder(placeholder, window, cx)
                });
            }
            self.load_oauth_layers(cx);
        }
        let show_secrets = self
            .state
            .tabs
            .active()
            .is_some_and(|tab| tab.show_auth_secrets);
        let built_for = profile.as_ref().map(|profile| {
            format!(
                "{tab_id}|{}|{:?}|{:?}|{:?}|{show_secrets}",
                profile.name, profile.grant, profile.token_placement, profile.redirect
            )
        });
        if self.oauth.inputs.built_for == built_for {
            if let Some(profile) = &profile {
                self.push_param_rows(tab_id, profile, window, cx);
            }
            return;
        }
        self.oauth.inputs.built_for = built_for;
        self.oauth.inputs.fields.clear();
        self.oauth.inputs.params.clear();
        let Some(profile) = profile else {
            return;
        };
        let mut kinds: Vec<ProfileFieldKind> = ProfileFieldKind::for_grant(profile.grant).to_vec();
        if profile.grant == OAuthGrant::AuthorizationCode
            && profile.redirect != OAuthRedirect::Loopback
        {
            kinds.push(ProfileFieldKind::RedirectUrl);
        }
        kinds.push(ProfileFieldKind::TokenName);
        for kind in kinds {
            let masked = kind.is_secret() && !show_secrets;
            let placeholder = kind.placeholder(profile.grant);
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(masked)
                    .placeholder(placeholder)
            });
            let text = kind.value(&profile).to_string();
            input.update(cx, |state, cx| state.set_value(text, window, cx));
            let tab = tab_id.to_string();
            let profile_name = profile.name.clone();
            cx.subscribe(&input, move |view, entity, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = entity.read(cx).value().to_string();
                    view.edit_oauth_profile(&tab, &profile_name, cx, |profile| {
                        kind.set(profile, value)
                    });
                }
            })
            .detach();
            self.oauth.inputs.fields.push((kind, input));
        }
        self.push_param_rows(tab_id, &profile, window, cx);
    }

    /// Adds the inputs of the extra parameters that have none yet, and the blank last row.
    fn push_param_rows(
        &mut self,
        tab_id: &str,
        profile: &OAuthProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = profile.extra_params.len() + 1;
        for index in self.oauth.inputs.params.len()..rows {
            let param = profile.extra_params.get(index);
            let key = cx.new(|cx| InputState::new(window, cx).placeholder("key"));
            let value = cx.new(|cx| InputState::new(window, cx).placeholder("value"));
            if let Some(param) = param {
                let (k, v) = (param.key.clone(), param.value.clone());
                key.update(cx, |state, cx| state.set_value(k, window, cx));
                value.update(cx, |state, cx| state.set_value(v, window, cx));
            }
            for (input, is_key) in [(&key, true), (&value, false)] {
                let tab = tab_id.to_string();
                let profile_name = profile.name.clone();
                cx.subscribe(input, move |view, entity, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let text = entity.read(cx).value().to_string();
                    view.edit_oauth_param(&tab, &profile_name, index, is_key, text, cx);
                })
                .detach();
            }
            self.oauth.inputs.params.push((key, value));
        }
    }

    /// Writes the key or value of the extra parameter `index`, adding it when `index` is the
    /// blank last row, which then gets a new blank row of its own.
    fn edit_oauth_param(
        &mut self,
        tab_id: &str,
        name: &str,
        index: usize,
        is_key: bool,
        text: String,
        cx: &mut Context<Self>,
    ) {
        self.edit_oauth_profile(tab_id, name, cx, |profile| {
            if index >= profile.extra_params.len() {
                profile.extra_params.push(ExtraParam::default());
            }
            if let Some(param) = profile.extra_params.get_mut(index) {
                if is_key {
                    param.key = text;
                } else {
                    param.value = text;
                }
            }
        });
        // The row typed in keeps its inputs, and the focus: the next sync only adds a new blank
        // row after it.
    }

    /// Reads both files of the active environment in the background, for the hints of Profile
    /// settings, once per environment.
    fn load_oauth_layers(&mut self, cx: &mut Context<Self>) {
        let environment = self.state.active_environment.clone().unwrap_or_default();
        if self.oauth.layers_for.as_deref() == Some(environment.as_str()) {
            return;
        }
        self.oauth.layers_for = Some(environment.clone());
        self.oauth.layers = None;
        if environment.is_empty() {
            return;
        }
        let Some(root) = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().to_path_buf())
        else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let layers = cx
                .background_spawn({
                    let environment = environment.clone();
                    async move {
                        postino_workspace::Workspace::open(&root)
                            .ok()
                            .and_then(|workspace| {
                                workspace.load_environment_layers(&environment).ok()
                            })
                    }
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.oauth.layers_for.as_deref() == Some(environment.as_str()) {
                    view.oauth.layers = layers;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Profile settings, unfolded (design 3a·2, step 1).
    pub(crate) fn render_profile_panel(
        &self,
        tab_id: &str,
        profile: &OAuthProfile,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let tree = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| workspace.tree().to_vec())
            .unwrap_or_default();
        let uses = oauth::usage_count(&tree, &profile.name);
        let environment = self.state.active_environment.clone().unwrap_or_default();
        let name = profile.name.clone();

        let rename_weak = weak.clone();
        let rename_name = name.clone();
        let users_weak = weak.clone();
        let users_name = name.clone();
        let header = h_flex()
            .gap(px(10.0))
            .px(px(16.0))
            .py(px(12.0))
            .items_center()
            .border_b_1()
            .border_color(palette.border)
            .child(
                Icon::new(Lucide::KeyRound)
                    .with_size(px(15.0))
                    .text_color(palette.accent_text),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(14.0))
                    .child(name.clone()),
            )
            .child(
                div()
                    .id("oauth-rename-profile")
                    .cursor_pointer()
                    .text_color(palette.fg_subtle)
                    .hover(|style| style.text_color(palette.fg))
                    .child(Icon::new(Lucide::Pencil).with_size(px(12.0)))
                    .tooltip(move |window, cx| {
                        Tooltip::new(t!("oauth.rename_profile").into_owned()).build(window, cx)
                    })
                    .on_click(move |_, window, cx| {
                        let name = rename_name.clone();
                        let _ = rename_weak.update(cx, |view, cx| {
                            view.open_rename_profile_dialog(name, window, cx);
                        });
                    }),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .id("oauth-used-by")
                    .gap(px(6.0))
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(palette.accent_text)
                    .hover(|style| style.text_color(palette.fg))
                    .child(Icon::new(Lucide::Link).with_size(px(12.0)))
                    .child(crate::state::locale::plural("oauth.used_by", uses))
                    .on_click(move |_, window, cx| {
                        let name = users_name.clone();
                        let _ = users_weak.update(cx, |view, cx| {
                            view.open_profile_users_dialog(name, window, cx);
                        });
                    }),
            );

        let info = h_flex()
            .gap(px(10.0))
            .mx(px(16.0))
            .mt(px(12.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(RADIUS_MD))
            .bg(palette.info_subtle)
            .items_center()
            .child(
                Icon::new(Lucide::Info)
                    .with_size(px(14.0))
                    .text_color(palette.info),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette.fg)
                    .child(crate::state::locale::plural("oauth.shared_profile", uses)),
            );

        let mut body = v_flex().gap(px(12.0)).px(px(16.0)).py(px(14.0));
        body = body.child(panel_row(
            t!("oauth.grant_type").into_owned(),
            self.grant_selector(tab_id, profile, cx),
            &palette,
        ));
        for (kind, input) in &self.oauth.inputs.fields {
            if matches!(
                kind,
                ProfileFieldKind::RedirectUrl | ProfileFieldKind::TokenName
            ) {
                continue;
            }
            body = body.child(self.render_profile_field(
                tab_id,
                profile,
                *kind,
                input,
                &environment,
                cx,
            ));
        }
        if profile.grant == OAuthGrant::AuthorizationCode {
            body = body.child(self.render_pkce_row(tab_id, profile, cx));
            body = body.child(self.render_redirect_row(tab_id, profile, cx));
        }
        body = body.child(self.render_advanced(tab_id, profile, cx));

        let duplicate_weak = weak.clone();
        let duplicate_tab = tab_id.to_string();
        let duplicate_name = name.clone();
        let delete_weak = weak.clone();
        let delete_name = name.clone();
        let test_weak = weak.clone();
        let test_tab = tab_id.to_string();
        let env_label = if environment.is_empty() {
            t!("oauth.no_environment").into_owned()
        } else {
            environment.clone()
        };
        let test_label = if profile.grant.is_interactive() {
            t!("oauth.test_sign_in", env = env_label.as_str())
        } else {
            t!("oauth.test_get_token", env = env_label.as_str())
        };
        let footer = h_flex()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(10.0))
            .items_center()
            .border_t_1()
            .border_color(palette.border)
            .child(
                GhostButton::new("oauth-duplicate", t!("oauth.duplicate"))
                    .icon(Icon::new(Lucide::Copy))
                    .height(OAUTH_BUTTON_HEIGHT)
                    .on_click(move |_, window, cx| {
                        let _ = duplicate_weak.update(cx, |view, cx| {
                            view.open_new_profile_dialog(
                                &duplicate_tab,
                                Some(duplicate_name.clone()),
                                window,
                                cx,
                            );
                        });
                    }),
            )
            .child(
                Button::new("oauth-delete")
                    .neutral_ghost(cx, palette.danger)
                    .h(px(OAUTH_BUTTON_HEIGHT))
                    .px(px(10.0))
                    .rounded(px(RADIUS_MD - 1.0))
                    .text_color(palette.danger)
                    .child(
                        h_flex()
                            .gap(px(6.0))
                            .items_center()
                            .child(Icon::new(Lucide::Trash).with_size(px(13.0)))
                            .child(t!("oauth.delete").into_owned()),
                    )
                    .on_click(move |_, window, cx| {
                        let name = delete_name.clone();
                        let _ = delete_weak.update(cx, |view, cx| {
                            view.open_delete_profile_dialog(name, window, cx);
                        });
                    }),
            )
            .child(div().flex_1())
            .child(
                SecondaryButton::new("oauth-test", test_label.into_owned())
                    .icon(Icon::new(Lucide::LogIn))
                    .height(OAUTH_BUTTON_HEIGHT)
                    .disabled(self.oauth.is_busy())
                    .on_click(move |_, _, cx| {
                        let _ = test_weak.update(cx, |view, cx| {
                            view.run_oauth_action(&test_tab, Force::Grant, cx);
                        });
                    }),
            );

        v_flex()
            .border_1()
            .border_color(palette.border_strong)
            .rounded(px(RADIUS_LG))
            .bg(palette.surface)
            .child(header)
            .child(info)
            .child(body)
            .child(footer)
            .into_any_element()
    }

    /// The grant type selector of Profile settings.
    fn grant_selector(
        &self,
        tab_id: &str,
        profile: &OAuthProfile,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let mut selector = SegmentedControl::new("oauth-grant");
        for grant in OAuthGrant::ALL {
            let pick_weak = weak.clone();
            let tab = tab_id.to_string();
            let name = profile.name.clone();
            selector = selector.item(
                SegmentedItem::new(oauth::grant_option_label(grant))
                    .selected(grant == profile.grant)
                    .on_click(move |_, cx| {
                        let _ = pick_weak.update(cx, |view, cx| {
                            view.edit_oauth_profile(&tab, &name, cx, |profile| {
                                profile.grant = grant
                            });
                            view.oauth.inputs.invalidate();
                        });
                    }),
            );
        }
        h_flex().child(selector).into_any_element()
    }

    /// One field of Profile settings: label, field with variable chips, and the hint on the right.
    fn render_profile_field(
        &self,
        tab_id: &str,
        profile: &OAuthProfile,
        kind: ProfileFieldKind,
        input: &Entity<InputState>,
        environment: &str,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let text = kind.value(profile).to_string();
        let show_secrets = self
            .state
            .tabs
            .active()
            .is_some_and(|tab| tab.show_auth_secrets);
        let context = self.request_editor.variable_context.clone();
        let mut field = VariableField::new(
            format!("oauth-field-{kind:?}"),
            input.clone(),
            text.clone(),
            context.clone(),
        )
        .mask_all(kind.is_secret())
        .masked(kind.is_secret() && !show_secrets)
        .on_chip_click(define_variable_click(&weak, &context));
        if kind.is_secret() {
            let toggle_weak = weak.clone();
            let tab = tab_id.to_string();
            field = field.on_mask_toggle(move |window, cx| {
                let _ = toggle_weak.update(cx, |view, cx| {
                    view.toggle_auth_secrets(&tab, window, cx);
                    view.oauth.inputs.invalidate();
                });
            });
        }
        let hint = oauth::field_hint(
            kind,
            &text,
            profile.grant,
            environment,
            self.oauth.layers.as_ref(),
        );
        let hint_cell = match hint {
            FieldHint::None => div().into_any_element(),
            FieldHint::File { file, local } => div()
                .truncate()
                .text_color(if local {
                    palette.success
                } else {
                    palette.fg_subtle
                })
                .child(file)
                .into_any_element(),
            FieldHint::PublicClient => div()
                .truncate()
                .text_color(palette.fg_subtle)
                .child(t!("oauth.public_client").into_owned())
                .into_any_element(),
            FieldHint::PlainSecret => {
                let move_weak = weak.clone();
                let tab = tab_id.to_string();
                let name = profile.name.clone();
                let value = text.clone();
                div()
                    .id(SharedString::from(format!("oauth-move-{kind:?}")))
                    .truncate()
                    .cursor_pointer()
                    .text_color(palette.warning)
                    .hover(|style| style.underline())
                    .child(t!("oauth.move_to_local_env").into_owned())
                    .tooltip(move |window, cx| {
                        Tooltip::new(t!("oauth.move_to_local_env_tooltip").into_owned())
                            .build(window, cx)
                    })
                    .on_click(move |_, window, cx| {
                        let (tab, name, value) = (tab.clone(), name.clone(), value.clone());
                        let _ = move_weak.update(cx, |view, cx| {
                            view.open_move_to_local_dialog(tab, name, kind, value, window, cx);
                        });
                    })
                    .into_any_element()
            }
        };
        h_flex()
            .gap_3()
            .items_center()
            .child(panel_label(kind.label(), &palette))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(12.0))
                    .child(field),
            )
            .child(
                div()
                    .w(px(OAUTH_HINT_WIDTH))
                    .flex_none()
                    .text_size(px(11.5))
                    .child(hint_cell),
            )
            .into_any_element()
    }

    /// PKCE: a switch, "SHA-256" and the note.
    fn render_pkce_row(
        &self,
        tab_id: &str,
        profile: &OAuthProfile,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let tab = tab_id.to_string();
        let name = profile.name.clone();
        panel_row(
            "PKCE".to_string(),
            h_flex()
                .gap(px(8.0))
                .items_center()
                .child(switch_halo(
                    "oauth-pkce-halo",
                    Switch::new("oauth-pkce").checked(profile.pkce).on_click(
                        move |checked, _, cx| {
                            let checked = *checked;
                            let _ = weak.update(cx, |view, cx| {
                                view.edit_oauth_profile(&tab, &name, cx, |profile| {
                                    profile.pkce = checked
                                });
                            });
                        },
                    ),
                    &palette,
                ))
                .child("SHA-256")
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(t!("oauth.pkce_note").into_owned()),
                )
                .into_any_element(),
            &palette,
        )
    }

    /// Redirect: Loopback, Fixed URL or Paste manually, the URL with Copy, and the note.
    fn render_redirect_row(
        &self,
        tab_id: &str,
        profile: &OAuthProfile,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let mut selector = SegmentedControl::new("oauth-redirect");
        for (option, text) in [
            (OAuthRedirect::Loopback, t!("oauth.redirect_loopback")),
            (OAuthRedirect::Fixed, t!("oauth.redirect_fixed")),
            (OAuthRedirect::Manual, t!("oauth.redirect_manual")),
        ] {
            let pick_weak = weak.clone();
            let tab = tab_id.to_string();
            let name = profile.name.clone();
            selector = selector.item(
                SegmentedItem::new(text.into_owned())
                    .selected(option == profile.redirect)
                    .on_click(move |_, cx| {
                        let _ = pick_weak.update(cx, |view, cx| {
                            view.edit_oauth_profile(&tab, &name, cx, |profile| {
                                profile.redirect = option;
                                if option == OAuthRedirect::Fixed
                                    && profile.redirect_url.trim().is_empty()
                                {
                                    profile.redirect_url =
                                        "http://localhost:8765/callback".to_string();
                                }
                            });
                            view.oauth.inputs.invalidate();
                        });
                    }),
            );
        }
        let shown = oauth::redirect_shown(profile);
        let copy_value = shown.clone();
        let url_input = self
            .oauth
            .inputs
            .fields
            .iter()
            .find(|(kind, _)| *kind == ProfileFieldKind::RedirectUrl)
            .map(|(_, input)| input.clone());
        let url = match url_input {
            Some(input) => div()
                .flex_1()
                .min_w_0()
                .child(HoverInput::new(
                    "oauth-redirect-url",
                    &input,
                    Input::new(&input)
                        .context_menu(edit_menu(&input, cx))
                        .appearance(false)
                        .px_0()
                        .py_0()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(12.0)),
                ))
                .into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(12.0))
                .child(shown)
                .into_any_element(),
        };
        let column = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(8.0))
            .child(h_flex().child(selector))
            .child(
                h_flex()
                    .gap(px(8.0))
                    .items_center()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(RADIUS_MD - 1.0))
                    .bg(palette.bg)
                    .border_1()
                    .border_color(palette.border)
                    .child(url)
                    .child(
                        link(
                            "oauth-copy-redirect",
                            t!("common.copy").into_owned(),
                            &palette,
                        )
                        .text_size(px(12.0))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_value.clone()));
                        }),
                    ),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(palette.fg_subtle)
                    .child(oauth::redirect_note(profile.redirect)),
            );
        h_flex()
            .gap_3()
            .items_start()
            .child(panel_label(t!("oauth.redirect").into_owned(), &palette).pt(px(5.0)))
            .child(column)
            .into_any_element()
    }

    /// Advanced: client authentication, where the token goes, extra parameters.
    fn render_advanced(
        &self,
        tab_id: &str,
        profile: &OAuthProfile,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let open = self.oauth.advanced_open;
        let toggle_weak = weak.clone();
        let toggle = h_flex()
            .id("oauth-advanced")
            .gap(px(8.0))
            .pt(px(2.0))
            .items_center()
            .cursor_pointer()
            .text_color(palette.fg_muted)
            .child(
                Icon::new(if open {
                    Lucide::ChevronDown
                } else {
                    Lucide::ChevronRight
                })
                .with_size(px(14.0)),
            )
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.fg)
                    .child(t!("oauth.advanced").into_owned()),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(palette.fg_subtle)
                    .child(oauth::advanced_summary(profile)),
            )
            .on_click(move |_, _, cx| {
                let _ = toggle_weak.update(cx, |view, cx| {
                    view.oauth.advanced_open = !view.oauth.advanced_open;
                    cx.notify();
                });
            });
        if !open {
            return toggle.into_any_element();
        }

        let advanced_label = |text: String| {
            div()
                .w(px(OAUTH_ADVANCED_LABEL_WIDTH))
                .flex_none()
                .text_color(palette.fg_muted)
                .child(text)
        };
        let segmented = |id: &'static str, options: ProfileOptions| {
            let mut selector = SegmentedControl::new(id);
            for (text, selected, apply) in options {
                let pick_weak = weak.clone();
                let tab = tab_id.to_string();
                let name = profile.name.clone();
                let apply = std::rc::Rc::new(apply);
                selector = selector.item(SegmentedItem::new(text).selected(selected).on_click(
                    move |_, cx| {
                        let apply = apply.clone();
                        let _ = pick_weak.update(cx, |view, cx| {
                            view.edit_oauth_profile(&tab, &name, cx, |profile| apply(profile));
                            view.oauth.inputs.invalidate();
                        });
                    },
                ));
            }
            selector
        };
        let client = segmented(
            "oauth-client-auth",
            vec![
                (
                    t!("oauth.client_auth_basic").into_owned(),
                    profile.client_auth == ClientAuth::BasicHeader,
                    Box::new(|profile: &mut OAuthProfile| {
                        profile.client_auth = ClientAuth::BasicHeader
                    }),
                ),
                (
                    t!("oauth.client_auth_body").into_owned(),
                    profile.client_auth == ClientAuth::InBody,
                    Box::new(|profile: &mut OAuthProfile| profile.client_auth = ClientAuth::InBody),
                ),
            ],
        );
        let token_in = segmented(
            "oauth-token-in",
            vec![
                (
                    t!("oauth.token_in_header").into_owned(),
                    profile.token_placement == TokenPlacement::Header,
                    Box::new(|profile: &mut OAuthProfile| {
                        profile.token_placement = TokenPlacement::Header
                    }),
                ),
                (
                    t!("oauth.token_in_query").into_owned(),
                    profile.token_placement == TokenPlacement::Query,
                    Box::new(|profile: &mut OAuthProfile| {
                        profile.token_placement = TokenPlacement::Query
                    }),
                ),
            ],
        );
        let token_name = self
            .oauth
            .inputs
            .fields
            .iter()
            .find(|(kind, _)| *kind == ProfileFieldKind::TokenName)
            .map(|(_, input)| input.clone());
        let token_name = token_name.map(|input| {
            h_flex()
                .h(px(30.0))
                .gap(px(8.0))
                .px(px(10.0))
                .items_center()
                .rounded(px(RADIUS_MD - 1.0))
                .bg(palette.raised)
                .border_1()
                .border_color(palette.border_strong)
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(match profile.token_placement {
                            TokenPlacement::Header => t!("oauth.prefix").into_owned(),
                            TokenPlacement::Query => t!("oauth.param_name").into_owned(),
                        }),
                )
                .child(
                    div().w(px(120.0)).child(
                        Input::new(&input)
                            .appearance(false)
                            .px_0()
                            .py_0()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(px(12.0)),
                    ),
                )
        });

        let mono = cx.theme().mono_font_family.clone();
        let mut table = v_flex()
            .flex_1()
            .border_1()
            .border_color(palette.border)
            .rounded(px(RADIUS_MD - 1.0))
            .overflow_hidden()
            .font_family(mono.clone())
            .text_size(px(12.0));
        let rows = self.oauth.inputs.params.len();
        for (index, (key, value)) in self.oauth.inputs.params.iter().enumerate() {
            let target = profile
                .extra_params
                .get(index)
                .map(|param| param.target)
                .unwrap_or_default();
            let target_label = match target {
                ParamTarget::Token => t!("oauth.param_token_request"),
                ParamTarget::Auth => t!("oauth.param_auth_request"),
            }
            .into_owned();
            let menu_weak = weak.clone();
            let tab = tab_id.to_string();
            let name = profile.name.clone();
            let ui_font = cx.theme().font_family.clone();
            let blank = index >= profile.extra_params.len();
            table = table.child(
                h_flex()
                    .h(px(30.0))
                    .px(px(10.0))
                    .gap(px(8.0))
                    .items_center()
                    .when(index + 1 < rows, |row| {
                        row.border_b_1().border_color(palette.border)
                    })
                    .child(
                        div().w(px(110.0)).flex_none().child(
                            Input::new(key)
                                .appearance(false)
                                .px_0()
                                .py_0()
                                .text_size(px(12.0)),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(palette.syntax_color("string"))
                            .child(
                                Input::new(value)
                                    .appearance(false)
                                    .px_0()
                                    .py_0()
                                    .text_size(px(12.0)),
                            ),
                    )
                    .child(
                        Button::new(("oauth-param-target", index))
                            .neutral_ghost(cx, palette.fg_muted)
                            .h(px(24.0))
                            .w(px(120.0))
                            .px(px(6.0))
                            .font_family(ui_font)
                            .text_size(px(12.0))
                            .text_color(if blank {
                                palette.fg_subtle
                            } else {
                                palette.fg_muted
                            })
                            .child(target_label)
                            .dropdown_caret(true)
                            .dropdown_menu(move |menu, _, _| {
                                let mut menu = menu;
                                for (option, text) in [
                                    (ParamTarget::Token, t!("oauth.param_token_request")),
                                    (ParamTarget::Auth, t!("oauth.param_auth_request")),
                                ] {
                                    let pick_weak = menu_weak.clone();
                                    let tab = tab.clone();
                                    let name = name.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(text.into_owned())
                                            .checked(option == target)
                                            .on_click(move |_, _, cx| {
                                                let _ = pick_weak.update(cx, |view, cx| {
                                                    view.edit_oauth_profile(
                                                        &tab,
                                                        &name,
                                                        cx,
                                                        |profile| {
                                                            if index >= profile.extra_params.len() {
                                                                profile
                                                                    .extra_params
                                                                    .push(ExtraParam::default());
                                                            }
                                                            if let Some(param) =
                                                                profile.extra_params.get_mut(index)
                                                            {
                                                                param.target = option;
                                                            }
                                                        },
                                                    );
                                                    view.oauth.inputs.invalidate();
                                                });
                                            }),
                                    );
                                }
                                menu
                            }),
                    ),
            );
        }

        let content = v_flex()
            .gap(px(12.0))
            .pl(px(22.0))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(advanced_label(t!("oauth.client_auth").into_owned()))
                    .child(client),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(advanced_label(t!("oauth.send_token_as").into_owned()))
                    .child(token_in)
                    .children(token_name),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(advanced_label(t!("oauth.extra_params_label").into_owned()).pt(px(6.0)))
                    .child(table),
            );
        v_flex()
            .gap(px(12.0))
            .child(toggle)
            .child(content)
            .into_any_element()
    }
}

/// A label of Profile settings.
fn panel_label(text: String, palette: &crate::theme::Palette) -> Div {
    div()
        .w(px(OAUTH_PANEL_LABEL_WIDTH))
        .flex_none()
        .text_color(palette.fg_muted)
        .child(text)
}

/// A labeled row of Profile settings.
fn panel_row(text: String, content: AnyElement, palette: &crate::theme::Palette) -> AnyElement {
    h_flex()
        .gap_3()
        .items_center()
        .child(panel_label(text, palette))
        .child(content)
        .into_any_element()
}
