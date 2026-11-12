//! The OAuth dialogs and profile commands: create ("Create profile", "New profile…", "New profile
//! from…", "Duplicate"), rename, delete, "Used by N requests", "Move to .local.env", "All
//! claims" and "Paste redirect URL" (design OAuth 3a·2 and 3c).

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_core::{OAuthGrant, OAuthProfile};
use postino_workspace::{EnvChange, EnvLayer, ProjectWrite};
use rust_i18n::t;

use crate::state::oauth::{self, ProfileFieldKind};
use crate::state::workspace_log::log_workspace_error;
use crate::theme::metrics::{INPUT_PADDING_X, RADIUS_MD};
use crate::theme::{PaletteExt, PostinoColors};
use crate::views::components::{
    ConfirmDialog, HoverInput, MODAL_INNER_RADIUS, ModalPlacement, PrimaryButton, SecondaryButton,
    edit_menu, framed_dialog, modal_body, modal_footer, modal_header_with_icon,
    open_confirm_dialog,
};

use super::super::root::AppView;

/// Width of the OAuth dialogs.
const DIALOG_WIDTH: f32 = 520.0;

/// A handler shared by Enter and the confirm button of a dialog.
type Submit = Rc<dyn Fn(&mut Window, &mut App)>;

impl AppView {
    /// Shows the create form of the empty state, with its name field focused.
    pub(crate) fn start_create_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.create_name_input(String::new(), window, cx);
        self.oauth.creating = Some(OAuthGrant::AuthorizationCode);
        let tab = self.state.tabs.active().map(|tab| tab.id.clone());
        // The callback gets the view itself: updating it again through a weak handle from
        // here would re-enter it.
        cx.subscribe(&input, move |view, _, event: &InputEvent, cx| match event {
            InputEvent::PressEnter { .. } => {
                if let Some(tab) = tab.clone() {
                    view.confirm_create_profile(&tab, None, cx);
                }
            }
            InputEvent::Change => cx.notify(),
            _ => {}
        })
        .detach();
        input.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// A new name input for the create form, kept in [`super::OAuthInputs::create_name`].
    fn create_name_input(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("acme-sso"));
        input.update(cx, |state, cx| state.set_value(text, window, cx));
        self.oauth.inputs.create_name = Some(input.clone());
        input
    }

    /// Creates the profile of the create form, or a copy of `from` with the form's name, selects
    /// it for the request of `tab_id` and opens Profile settings. Returns whether it was created.
    pub(crate) fn confirm_create_profile(
        &mut self,
        tab_id: &str,
        from: Option<&str>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(input) = self.oauth.inputs.create_name.clone() else {
            return false;
        };
        let name = input.read(cx).value().trim().to_string();
        if oauth::name_problem(&name, &self.state.oauth_profiles, None).is_some() {
            return false;
        }
        let profile = match from.and_then(|from| self.state.oauth_profile(tab_id, from)) {
            Some(mut copy) => {
                copy.name = name.clone();
                copy
            }
            None => OAuthProfile::new(
                name.clone(),
                self.oauth.creating.unwrap_or(OAuthGrant::AuthorizationCode),
            ),
        };
        let Some(workspace) = self.state.workspace.as_ref() else {
            return false;
        };
        if let Err(error) = workspace.update_project(ProjectWrite::Create, |config| {
            config.set_oauth_profile(&profile)
        }) {
            log_workspace_error(&format!("create OAuth profile {name:?}"), &error);
            self.workspace_error = Some(error.to_string());
            cx.notify();
            return false;
        }
        log::info!("oauth: profile {name} created");
        self.state.reload_oauth_profiles();
        self.oauth.creating = None;
        self.oauth.inputs.create_name = None;
        if !self.state.settings.oauth_profile_settings_open {
            self.toggle_profile_settings(cx);
        }
        self.select_oauth_profile(tab_id, &name, cx);
        true
    }

    /// "New profile…" (no `from`), "New profile from acme-sso…" and "Duplicate" (`from`): the
    /// create form in a dialog.
    pub(crate) fn open_new_profile_dialog(
        &mut self,
        tab_id: &str,
        from: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let suggested = from
            .as_deref()
            .map(|from| oauth::copy_name(from, &self.state.oauth_profiles))
            .unwrap_or_default();
        let input = self.create_name_input(suggested, window, cx);
        self.oauth.creating = Some(OAuthGrant::AuthorizationCode);
        let weak = cx.weak_entity();
        let tab = tab_id.to_string();
        let submit: Submit = {
            let weak = weak.clone();
            let from = from.clone();
            Rc::new(move |window, cx| {
                let created = weak
                    .update(cx, |view, cx| {
                        view.confirm_create_profile(&tab, from.as_deref(), cx)
                    })
                    .unwrap_or(false);
                if created {
                    window.close_dialog(cx);
                }
            })
        };
        subscribe_enter(&input, submit.clone(), window, cx);
        let title = match &from {
            Some(from) => t!("oauth.new_profile_from_title", profile = from.as_str()),
            None => t!("oauth.new_profile_title"),
        }
        .into_owned();
        let tab = tab_id.to_string();
        let copying = from.is_some();
        window.open_dialog(cx, move |dialog, window, _| {
            let weak = weak.clone();
            let submit = submit.clone();
            let title = title.clone();
            let tab = tab.clone();
            framed_dialog(
                dialog,
                DIALOG_WIDTH,
                None,
                ModalPlacement::DEFAULT,
                window,
                move |_, cx| {
                    let submit = submit.clone();
                    let Some(view) = weak.upgrade() else {
                        return div().into_any_element();
                    };
                    let view = view.read(cx);
                    let grant = view.oauth.creating.unwrap_or(OAuthGrant::AuthorizationCode);
                    let mut form = view.render_create_form(weak.clone(), &tab, grant, false, cx);
                    if copying {
                        form = name_only_form(view, cx);
                    }
                    let name = view
                        .oauth
                        .inputs
                        .create_name
                        .as_ref()
                        .map(|input| input.read(cx).value().to_string())
                        .unwrap_or_default();
                    let blocked =
                        oauth::name_problem(&name, &view.state.oauth_profiles, None).is_some();
                    v_flex()
                        .child(modal_header_with_icon(
                            Lucide::KeyRound,
                            cx.palette().accent_text,
                            title.clone(),
                            cx,
                        ))
                        .child(modal_body().child(form))
                        .child(
                            modal_footer(cx)
                                .child(
                                    SecondaryButton::new("oauth-new-cancel", t!("common.cancel"))
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    PrimaryButton::new("oauth-new-create", t!("oauth.create"))
                                        .disabled(blocked)
                                        .on_click(move |_, window, cx| submit(window, cx)),
                                ),
                        )
                        .into_any_element()
                },
            )
        });
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
    }

    /// The pencil next to the profile name: renames it now, in `project.toml` and in every
    /// request that uses it.
    pub(crate) fn open_rename_profile_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |state, cx| state.set_value(name.clone(), window, cx));
        let weak = cx.weak_entity();
        let submit: Submit = {
            let input = input.clone();
            let weak = weak.clone();
            let old = name.clone();
            Rc::new(move |window, cx| {
                let new = input.read(cx).value().trim().to_string();
                let renamed = weak
                    .update(cx, |view, cx| view.rename_oauth_profile(&old, &new, cx))
                    .unwrap_or(false);
                if renamed {
                    window.close_dialog(cx);
                }
            })
        };
        subscribe_enter(&input, submit.clone(), window, cx);
        cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let dialog_input = input.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            let input = dialog_input.clone();
            let submit = submit.clone();
            let weak = weak.clone();
            let old = name.clone();
            framed_dialog(
                dialog,
                DIALOG_WIDTH,
                None,
                ModalPlacement::DEFAULT,
                window,
                move |_, cx| {
                    let submit = submit.clone();
                    let palette = cx.palette();
                    let new = input.read(cx).value().trim().to_string();
                    let (problem, uses) = weak
                        .upgrade()
                        .map(|view| {
                            let view = view.read(cx);
                            let problem = (new != old)
                                .then(|| {
                                    oauth::name_problem(&new, &view.state.oauth_profiles, None)
                                })
                                .flatten();
                            let uses = view
                                .state
                                .workspace
                                .as_ref()
                                .map(|workspace| oauth::usage_count(workspace.tree(), &old))
                                .unwrap_or(0);
                            (problem, uses)
                        })
                        .unwrap_or((None, 0));
                    v_flex()
                        .child(modal_header_with_icon(
                            Lucide::Pencil,
                            cx.palette().accent_text,
                            t!("oauth.rename_profile"),
                            cx,
                        ))
                        .child(
                            modal_body()
                                .gap(px(10.0))
                                .child(name_field("oauth-rename-input", &input, cx))
                                .children(problem.clone().map(|problem| {
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(palette.danger)
                                        .child(problem)
                                }))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(palette.fg_muted)
                                        .child(crate::state::locale::plural(
                                            "oauth.rename_note",
                                            uses,
                                        )),
                                ),
                        )
                        .child(
                            modal_footer(cx)
                                .child(
                                    SecondaryButton::new(
                                        "oauth-rename-cancel",
                                        t!("common.cancel"),
                                    )
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    PrimaryButton::new("oauth-rename-ok", t!("common.rename"))
                                        .disabled(problem.is_some() || new.is_empty() || new == old)
                                        .on_click(move |_, window, cx| submit(window, cx)),
                                ),
                        )
                        .into_any_element()
                },
            )
        });
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
    }

    /// Renames the profile `old` to `new`: the table in `project.toml`, the `profile =` line of
    /// every request that uses it, the open tabs and their drafts. Returns whether it renamed.
    fn rename_oauth_profile(&mut self, old: &str, new: &str, cx: &mut Context<Self>) -> bool {
        if old == new || oauth::name_problem(new, &self.state.oauth_profiles, None).is_some() {
            return false;
        }
        let Some(workspace) = self.state.workspace.as_mut() else {
            return false;
        };
        if let Err(error) = workspace.update_project(ProjectWrite::Create, |config| {
            config.rename_oauth_profile(old, new)
        }) {
            log_workspace_error(&format!("rename OAuth profile {old:?}"), &error);
            self.workspace_error = Some(error.to_string());
            cx.notify();
            return false;
        }
        let users = oauth::profile_users(workspace.tree(), old);
        let mut failed = None;
        for user in &users {
            let result = workspace.load_request(&user.id).and_then(|mut request| {
                crate::state::auth_edit::set_oauth_profile(&mut request.auth, new);
                workspace.save_request(&user.id, &request)
            });
            if let Err(error) = result {
                log_workspace_error(&format!("rename OAuth profile in {}", user.id), &error);
                failed = Some(error.to_string());
            }
        }
        for index in 0..self.state.tabs.open_tabs().len() {
            if let Some(tab) = self.state.tabs.get_mut(index) {
                if let Some(request) = tab.request_mut()
                    && oauth::profile_of(&request.auth) == Some(old)
                {
                    crate::state::auth_edit::set_oauth_profile(&mut request.auth, new);
                }
                if let Some(draft) = tab.profile_draft.as_mut()
                    && draft.name == old
                {
                    draft.name = new.to_string();
                }
            }
        }
        log::info!("oauth: profile renamed, {} requests updated", users.len());
        self.state.reload_oauth_profiles();
        self.oauth.inputs.invalidate();
        self.workspace_error = failed;
        self.refresh_tree(cx);
        cx.notify();
        true
    }

    /// "Used by N requests": the list, each one opens its request.
    pub(crate) fn open_profile_users_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let users = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| oauth::profile_users(workspace.tree(), &name))
            .unwrap_or_default();
        let weak = cx.weak_entity();
        let title = crate::state::locale::plural("oauth.used_by", users.len());
        window.open_dialog(cx, move |dialog, window, _| {
            let users = users.clone();
            let weak = weak.clone();
            let title = title.clone();
            framed_dialog(
                dialog,
                DIALOG_WIDTH,
                None,
                ModalPlacement::DEFAULT,
                window,
                move |_, cx| {
                    v_flex()
                        .child(modal_header_with_icon(
                            Lucide::Users,
                            cx.palette().accent_text,
                            title.clone(),
                            cx,
                        ))
                        .child(modal_body().child(request_list(&users, Some(&weak), cx)))
                        .child(
                            modal_footer(cx).child(
                                SecondaryButton::new("oauth-users-close", t!("common.close"))
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            ),
                        )
                        .into_any_element()
                },
            )
        });
    }

    /// "Delete…": asks with the list of requests that use the profile, which keep naming it and
    /// show it in red until another one is chosen.
    pub(crate) fn open_delete_profile_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let users = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| oauth::profile_users(workspace.tree(), &name))
            .unwrap_or_default();
        let mut text = if users.is_empty() {
            t!("oauth.delete_text_unused", profile = name.as_str()).into_owned()
        } else {
            crate::state::locale::plural("oauth.delete_text", users.len())
                .replace("%{profile}", &name)
        };
        for user in users.iter().take(12) {
            let method = user
                .method
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            text.push_str(&format!("\n{method} {}", user.id));
        }
        if users.len() > 12 {
            text.push_str(&format!("\n… +{}", users.len() - 12));
        }
        let weak = cx.weak_entity();
        open_confirm_dialog(
            window,
            cx,
            ConfirmDialog {
                title: t!("oauth.delete_title", profile = name.as_str()).into(),
                text: text.into(),
                confirm_label: t!("common.delete").into(),
                destructive: true,
                icon: None,
                show_cancel: true,
                on_confirm: Rc::new(move |_, cx| {
                    let name = name.clone();
                    let _ = weak.update(cx, |view, cx| view.delete_oauth_profile(&name, cx));
                }),
            },
        );
    }

    /// Removes the profile `name` from `project.toml`, and its unsaved drafts.
    fn delete_oauth_profile(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        if let Err(error) = workspace.update_project(ProjectWrite::IfExists, |config| {
            config.remove_oauth_profile(name)
        }) {
            log_workspace_error(&format!("delete OAuth profile {name:?}"), &error);
            self.workspace_error = Some(error.to_string());
            cx.notify();
            return;
        }
        for index in 0..self.state.tabs.open_tabs().len() {
            if let Some(tab) = self.state.tabs.get_mut(index)
                && tab
                    .profile_draft
                    .as_ref()
                    .is_some_and(|draft| draft.name == name)
            {
                tab.profile_draft = None;
                tab.dirty = tab
                    .history
                    .as_ref()
                    .is_some_and(crate::state::edit_history::EditHistory::is_dirty);
            }
        }
        log::info!("oauth: profile {name} deleted");
        self.state.reload_oauth_profiles();
        self.oauth.inputs.invalidate();
        cx.notify();
    }

    /// "Move to .local.env": the secret becomes a variable of the active environment's
    /// `.local.env`, and the field references it.
    pub(crate) fn open_move_to_local_dialog(
        &mut self,
        tab_id: String,
        profile: String,
        field: ProfileFieldKind,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(environment) = self.state.active_environment.clone() else {
            self.workspace_error = Some(t!("oauth.move_needs_environment").into_owned());
            cx.notify();
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        let suggested = oauth::suggested_variable(&profile, field);
        input.update(cx, |state, cx| state.set_value(suggested, window, cx));
        let weak = cx.weak_entity();
        let submit: Submit = {
            let input = input.clone();
            let weak = weak.clone();
            let environment = environment.clone();
            Rc::new(move |window, cx| {
                let variable = input.read(cx).value().trim().to_string();
                if variable.is_empty() {
                    return;
                }
                let moved = weak
                    .update(cx, |view, cx| {
                        view.move_secret_to_local(
                            &tab_id,
                            &profile,
                            field,
                            &environment,
                            &variable,
                            &value,
                            cx,
                        )
                    })
                    .unwrap_or(false);
                if moved {
                    window.close_dialog(cx);
                }
            })
        };
        subscribe_enter(&input, submit.clone(), window, cx);
        let dialog_input = input.clone();
        let file = format!("environments/{environment}.local.env");
        window.open_dialog(cx, move |dialog, window, _| {
            let input = dialog_input.clone();
            let submit = submit.clone();
            let file = file.clone();
            framed_dialog(
                dialog,
                DIALOG_WIDTH,
                None,
                ModalPlacement::DEFAULT,
                window,
                move |_, cx| {
                    let submit = submit.clone();
                    let palette = cx.palette();
                    v_flex()
                        .child(modal_header_with_icon(
                            Lucide::FolderInput,
                            cx.palette().accent_text,
                            t!("oauth.move_title"),
                            cx,
                        ))
                        .child(
                            modal_body()
                                .gap(px(10.0))
                                .child(div().text_color(palette.fg_muted).child(
                                    t!("oauth.move_text", file = file.as_str()).into_owned(),
                                ))
                                .child(name_field("oauth-move-input", &input, cx)),
                        )
                        .child(
                            modal_footer(cx)
                                .child(
                                    SecondaryButton::new("oauth-move-cancel", t!("common.cancel"))
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    PrimaryButton::new("oauth-move-ok", t!("oauth.move_confirm"))
                                        .on_click(move |_, window, cx| submit(window, cx)),
                                ),
                        )
                        .into_any_element()
                },
            )
        });
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
    }

    /// Writes `value` as `variable` in the `.local.env` of `environment` and makes the profile
    /// field reference it. Returns whether it worked.
    #[allow(clippy::too_many_arguments)]
    fn move_secret_to_local(
        &mut self,
        tab_id: &str,
        profile: &str,
        field: ProfileFieldKind,
        environment: &str,
        variable: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return false;
        };
        let change = EnvChange::Set {
            layer: EnvLayer::Local,
            key: variable.to_string(),
            value: value.to_string(),
        };
        if let Err(error) = workspace.save_environment(environment, &[change]) {
            log_workspace_error(&format!("move secret to {environment}.local.env"), &error);
            self.workspace_error = Some(error.to_string());
            cx.notify();
            return false;
        }
        log::info!("oauth: secret of {profile} moved to {environment}.local.env");
        let reference = format!("{{{{{variable}}}}}");
        self.edit_oauth_profile(tab_id, profile, cx, |profile| field.set(profile, reference));
        self.state.reload_environment();
        self.oauth.layers_for = None;
        self.oauth.inputs.invalidate();
        self.refresh_environments();
        cx.notify();
        true
    }

    /// "All claims": every claim of the token as JSON, with the times in local time.
    pub(crate) fn open_all_claims_dialog(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.open_dialog(cx, move |dialog, window, _| {
            let text = text.clone();
            framed_dialog(
                dialog,
                DIALOG_WIDTH + 80.0,
                None,
                ModalPlacement::DEFAULT,
                window,
                move |_, cx| {
                    let palette = cx.palette();
                    let copy = text.clone();
                    v_flex()
                        .child(modal_header_with_icon(
                            Lucide::ScrollText,
                            cx.palette().accent_text,
                            t!("oauth.all_claims"),
                            cx,
                        ))
                        .child(
                            modal_body().child(
                                div()
                                    .id("oauth-claims-text")
                                    .max_h(px(420.0))
                                    .overflow_y_scroll()
                                    .p(px(12.0))
                                    .rounded(px(RADIUS_MD))
                                    .bg(palette.bg)
                                    .border_1()
                                    .border_color(palette.border)
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_size(px(12.0))
                                    .whitespace_normal()
                                    .children(
                                        text.lines().map(|line| {
                                            div().min_h(px(18.0)).child(line.to_string())
                                        }),
                                    ),
                            ),
                        )
                        .child(
                            modal_footer(cx)
                                .child(
                                    SecondaryButton::new("oauth-claims-copy", t!("common.copy"))
                                        .icon(Icon::new(Lucide::Copy))
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.clone(),
                                            ));
                                        }),
                                )
                                .child(
                                    PrimaryButton::new("oauth-claims-close", t!("common.close"))
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                ),
                        )
                        .into_any_element()
                },
            )
        });
    }

    /// "Paste redirect URL" (design 3c): for providers that do not redirect to 127.0.0.1, the URL
    /// the browser ended at, with the code.
    pub(crate) fn open_paste_redirect_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("https://app.acme.dev/oauth/callback?code=…")
        });
        let weak = cx.weak_entity();
        let submit: Submit = {
            let input = input.clone();
            let weak = weak.clone();
            Rc::new(move |window, cx| {
                let url = input.read(cx).value().trim().to_string();
                if url.is_empty() {
                    return;
                }
                let _ = weak.update(cx, |view, _| view.oauth.sign_in.paste(url));
                window.close_dialog(cx);
            })
        };
        subscribe_enter(&input, submit.clone(), window, cx);
        let dialog_input = input.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            let input = dialog_input.clone();
            let submit = submit.clone();
            framed_dialog(
                dialog,
                DIALOG_WIDTH + 80.0,
                None,
                ModalPlacement::DEFAULT,
                window,
                move |_, cx| {
                    let submit = submit.clone();
                    let palette = cx.palette();
                    v_flex()
                        .child(modal_header_with_icon(
                            Lucide::ClipboardPaste,
                            cx.palette().accent_text,
                            t!("oauth.paste_title"),
                            cx,
                        ))
                        .child(
                            modal_body()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .text_color(palette.fg_muted)
                                        .child(t!("oauth.paste_text").into_owned()),
                                )
                                .child(name_field("oauth-paste-input", &input, cx)),
                        )
                        .child(
                            modal_footer(cx)
                                .child(
                                    SecondaryButton::new("oauth-paste-cancel", t!("common.cancel"))
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    PrimaryButton::new("oauth-paste-ok", t!("oauth.paste_confirm"))
                                        .on_click(move |_, window, cx| submit(window, cx)),
                                ),
                        )
                        .into_any_element()
                },
            )
        });
        input.update(cx, |state, cx| state.focus(window, cx));
    }
}

/// Calls `submit` on Enter in `input`. The subscription belongs to the window, not to the view:
/// `submit` updates the view, which a view's own subscription could not do again.
fn subscribe_enter(
    input: &Entity<InputState>,
    submit: Submit,
    window: &mut Window,
    cx: &mut Context<AppView>,
) {
    window
        .subscribe(input, cx, move |_, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                submit(window, cx);
            }
        })
        .detach();
}

/// A single line mono input of a dialog.
fn name_field(id: &'static str, input: &Entity<InputState>, cx: &App) -> AnyElement {
    HoverInput::new(
        id,
        input,
        Input::new(input)
            .context_menu(edit_menu(input, cx))
            .px(px(INPUT_PADDING_X))
            .py_0()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(12.0)),
    )
    .into_any_element()
}

/// The Name field alone, for a copy: the grant comes from the profile copied.
fn name_only_form(view: &AppView, cx: &App) -> AnyElement {
    let palette = cx.palette();
    let Some(input) = view.oauth.inputs.create_name.clone() else {
        return div().into_any_element();
    };
    v_flex()
        .gap(px(10.0))
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
                    div()
                        .flex_1()
                        .child(name_field("oauth-copy-name", &input, cx)),
                ),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(palette.fg_subtle)
                .child(t!("oauth.copy_hint").into_owned()),
        )
        .into_any_element()
}

/// The requests of a profile, each opening its tab when `weak` is given.
fn request_list(
    users: &[oauth::ProfileUser],
    weak: Option<&WeakEntity<AppView>>,
    cx: &App,
) -> AnyElement {
    let palette = cx.palette();
    if users.is_empty() {
        return div()
            .text_color(palette.fg_muted)
            .child(t!("oauth.used_by_none").into_owned())
            .into_any_element();
    }
    let mono = cx.theme().mono_font_family.clone();
    let mut list = v_flex()
        .id("oauth-users-list")
        .max_h(px(360.0))
        .overflow_y_scroll()
        .rounded(px(MODAL_INNER_RADIUS));
    for (index, user) in users.iter().enumerate() {
        let id = user.id.clone();
        let weak = weak.cloned();
        let method = user.method.clone().unwrap_or_default();
        list = list.child(
            h_flex()
                .id(("oauth-user", index))
                .h(px(30.0))
                .px_2()
                .gap(px(10.0))
                .items_center()
                .rounded(px(RADIUS_MD - 2.0))
                .cursor_pointer()
                .hover(|style| style.bg(palette.hover))
                .child(
                    div()
                        .w(px(46.0))
                        .flex_none()
                        .font_family(mono.clone())
                        .text_size(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.method_color(&method))
                        .child(method.to_string()),
                )
                .child(div().flex_none().child(user.name.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(user.id.clone()),
                )
                .on_click(move |_, window, cx| {
                    let Some(weak) = weak.clone() else {
                        return;
                    };
                    window.close_dialog(cx);
                    let id = id.clone();
                    let _ = weak.update(cx, |view, cx| view.open_request(id, cx));
                }),
        );
    }
    list.into_any_element()
}
