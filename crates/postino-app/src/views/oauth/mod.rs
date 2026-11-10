//! OAuth 2.0 in the window (#225, #226, design page "OAuth"): the OAuth 2.0 type of the Auth
//! tab (profile picker, Scope, Profile settings, token card), the dialogs that create, rename and
//! delete profiles, and the response side (sign-in panel, notices, error cards).
//!
//! The rules live in `state::oauth`; tokens are read from the store on the background executor
//! and kept in [`OAuthUi`]'s cache, so render never touches the system keychain.

mod auth_tab;
mod dialogs;
mod panel;
mod response;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use postino_core::{Auth, OAuthProfile};
use postino_oauth::{Force, MemoryStore, SignInPrompt, TokenKey, TokenSet, TokenStore};
use postino_runner::{OAuthConfig, Runner};

use crate::state::oauth::ProfileFieldKind;
use crate::state::oauth_sign_in::AppSignIn;

use super::root::AppView;

/// The OAuth parts of [`AppView`].
pub(crate) struct OAuthUi {
    /// Where tokens are kept, in memory, shared with every send.
    pub(crate) store: Arc<dyn TokenStore>,
    /// The browser sign-in, shared with every send.
    pub(crate) sign_in: Arc<AppSignIn>,
    /// The sign-in in progress, shown in the response panel (design 3c).
    pub(crate) waiting: Option<SignInPrompt>,
    /// The stored token of each key, as last read. `None` inside means no token.
    tokens: HashMap<String, Option<TokenSet>>,
    /// The inputs of the Profile settings panel and the create form.
    pub(crate) inputs: OAuthInputs,
    /// The grant chosen in the create form ("Create profile"), `Some` while it shows.
    pub(crate) creating: Option<postino_core::OAuthGrant>,
    /// A card button's token request in flight.
    action: Option<Task<()>>,
    /// Why the last card button failed, under the card.
    pub(crate) card_error: Option<String>,
    /// Responses whose 401 card was dismissed, by tab id and response generation.
    pub(crate) dismissed: HashSet<(String, u64)>,
    /// The workspace id of the token keys, computed when a workspace opens.
    pub(crate) workspace_id: String,
    /// Whether the Advanced part of Profile settings is unfolded. Not remembered.
    pub(crate) advanced_open: bool,
    /// The environment whose files [`Self::layers`] holds, for the hints of Profile settings.
    pub(crate) layers_for: Option<String>,
    /// Both files of that environment, read in the background.
    pub(crate) layers: Option<postino_workspace::EnvLayers>,
    /// The placeholder last given to the Scope input: the profile's scope.
    pub(crate) scope_placeholder: Option<String>,
}

/// The inputs of the Profile settings panel, rebuilt when the profile, its grant or where the
/// token goes change, and the create form's name.
#[derive(Default)]
pub(crate) struct OAuthInputs {
    built_for: Option<String>,
    pub(crate) fields: Vec<(ProfileFieldKind, Entity<InputState>)>,
    /// The key and value inputs of every extra parameter, and a blank last row.
    pub(crate) params: Vec<(Entity<InputState>, Entity<InputState>)>,
    pub(crate) create_name: Option<Entity<InputState>>,
}

impl OAuthInputs {
    /// Makes the next sync rebuild every input, after something other than typing changed the
    /// profile (a variable moved to `.local.env`, a grant switched).
    pub(crate) fn invalidate(&mut self) {
        self.built_for = None;
    }
}

impl OAuthUi {
    /// The OAuth state of a new window, with no token: they live in memory only (#228).
    pub(crate) fn new(prompts: async_channel::Sender<SignInPrompt>) -> Self {
        // Postino 0.10.0 kept tokens in this file when the system had no credential store.
        if let Some(folder) = crate::state::config::config_dir() {
            let _ = std::fs::remove_file(folder.join("oauth-tokens.json"));
        }
        OAuthUi {
            store: Arc::new(MemoryStore::default()),
            sign_in: Arc::new(AppSignIn::new(prompts)),
            waiting: None,
            tokens: HashMap::new(),
            inputs: OAuthInputs::default(),
            creating: None,
            action: None,
            card_error: None,
            dismissed: HashSet::new(),
            workspace_id: String::new(),
            advanced_open: false,
            layers_for: None,
            layers: None,
            scope_placeholder: None,
        }
    }

    /// The stored token of `key`, `None` while it has not been read yet.
    pub(crate) fn token(&self, key: &TokenKey) -> Option<Option<&TokenSet>> {
        self.tokens.get(&key.to_uri()).map(Option::as_ref)
    }

    /// Forgets every token read, for a new workspace.
    pub(crate) fn reset(&mut self) {
        self.tokens.clear();
        self.waiting = None;
        self.card_error = None;
        self.creating = None;
        self.inputs = OAuthInputs::default();
        self.layers_for = None;
        self.layers = None;
    }

    /// Whether a card button is running.
    pub(crate) fn is_busy(&self) -> bool {
        self.action.is_some()
    }
}

/// The profile and scope override of `auth`, when it is OAuth 2.0.
pub(crate) fn oauth_auth(auth: &Auth) -> Option<(&str, &str)> {
    match auth {
        Auth::OAuth2 { profile, scope } => Some((profile, scope)),
        _ => None,
    }
}

impl AppView {
    /// Starts reading the sign-in prompts of the sends: each one opens the browser and the
    /// waiting panel. Called once by [`AppView::new`].
    pub(crate) fn listen_for_sign_in(
        &mut self,
        prompts: async_channel::Receiver<SignInPrompt>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Ok(prompt) = prompts.recv().await {
                let alive = this.update(cx, |view, cx| {
                    cx.open_url(&prompt.url);
                    view.oauth.waiting = Some(prompt);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// The workspace's profiles as the tab `tab_id` sees them: its unsaved draft in place of the
    /// saved profile.
    pub(crate) fn oauth_profiles_for(&self, tab_id: &str) -> Vec<OAuthProfile> {
        let mut profiles: Vec<OAuthProfile> = self.state.oauth_profiles.as_ref().clone();
        if let Some(draft) = self
            .state
            .tabs
            .index_of(tab_id)
            .and_then(|index| self.state.tabs.get(index))
            .and_then(|tab| tab.profile_draft.clone())
        {
            match profiles
                .iter_mut()
                .find(|profile| profile.name == draft.name)
            {
                Some(profile) => *profile = draft,
                None => profiles.push(draft),
            }
        }
        profiles
    }

    /// What the runner of a send from `tab_id` needs for OAuth.
    pub(crate) fn oauth_config(&self, tab_id: &str) -> OAuthConfig {
        OAuthConfig {
            workspace: self.oauth.workspace_id.clone(),
            profiles: Arc::new(self.oauth_profiles_for(tab_id)),
            store: self.oauth.store.clone(),
            sign_in: self.oauth.sign_in.clone(),
        }
    }

    /// Computes the workspace id of the token keys, when a workspace opens.
    pub(crate) fn on_oauth_workspace_opened(&mut self) {
        self.oauth.reset();
        self.oauth.workspace_id = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| postino_oauth::workspace_id(workspace.root()))
            .unwrap_or_default();
    }

    /// The key of the token the tab `tab_id` sends with `profile`.
    pub(crate) fn oauth_key(&self, profile: &OAuthProfile, scope: &str) -> TokenKey {
        postino_oauth::token_key(
            &self.oauth.workspace_id,
            self.state.active_environment.as_deref().unwrap_or_default(),
            profile,
            scope,
        )
    }

    /// Reads the token of `key` from the store, once: a load test or another tab's send may
    /// have left one there.
    pub(crate) fn load_oauth_token(&mut self, key: &TokenKey) {
        let uri = key.to_uri();
        if !self.oauth.tokens.contains_key(&uri) {
            let token = self.oauth.store.get(key);
            self.oauth.tokens.insert(uri, token);
        }
    }

    /// Records the token a send or a card button left under `key`.
    pub(crate) fn remember_oauth_token(&mut self, key: &TokenKey, token: Option<TokenSet>) {
        self.oauth.tokens.insert(key.to_uri(), token);
    }

    /// Edits the profile `name` as the tab `tab_id` sees it. The change stays in the tab's draft,
    /// marking it dirty, until Ctrl+S writes it to `project.toml` (design 3a·2).
    pub(crate) fn edit_oauth_profile(
        &mut self,
        tab_id: &str,
        name: &str,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut OAuthProfile),
    ) {
        let Some(mut profile) = self.state.oauth_profile(tab_id, name) else {
            return;
        };
        let saved = self
            .state
            .oauth_profiles
            .iter()
            .find(|profile| profile.name == name)
            .cloned();
        edit(&mut profile);
        let Some(index) = self.state.tabs.index_of(tab_id) else {
            return;
        };
        let Some(tab) = self.state.tabs.get_mut(index) else {
            return;
        };
        tab.profile_draft = (saved.as_ref() != Some(&profile)).then_some(profile);
        let request_dirty = tab
            .history
            .as_ref()
            .is_some_and(crate::state::edit_history::EditHistory::is_dirty);
        tab.dirty = request_dirty || tab.profile_draft.is_some();
        cx.notify();
    }

    /// Writes the tab's profile draft to `project.toml`, as part of Ctrl+S. Returns whether it
    /// was saved (or there was nothing to save).
    pub(crate) fn save_profile_draft(&mut self, index: usize) -> bool {
        let Some(draft) = self
            .state
            .tabs
            .get(index)
            .and_then(|tab| tab.profile_draft.clone())
        else {
            return true;
        };
        let Some(workspace) = self.state.workspace.as_ref() else {
            return false;
        };
        let result = workspace.update_project(postino_workspace::ProjectWrite::Create, |config| {
            config.set_oauth_profile(&draft);
        });
        match result {
            Ok(()) => {
                log::info!("oauth: profile {} saved", draft.name);
                self.state.reload_oauth_profiles();
                true
            }
            Err(error) => {
                log::warn!("oauth: cannot save profile {}: {error}", draft.name);
                self.workspace_error = Some(error.to_string());
                false
            }
        }
    }

    /// Makes the request of `tab_id` use the profile `name`.
    pub(crate) fn select_oauth_profile(
        &mut self,
        tab_id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        let name = name.to_string();
        self.edit_active_request(tab_id, cx, |request| {
            crate::state::auth_edit::set_oauth_profile(&mut request.auth, &name);
        });
        self.oauth.inputs.invalidate();
        self.oauth.card_error = None;
    }

    /// Opens or folds Profile settings, remembered in the settings.
    pub(crate) fn toggle_profile_settings(&mut self, cx: &mut Context<Self>) {
        self.state.settings.oauth_profile_settings_open =
            !self.state.settings.oauth_profile_settings_open;
        crate::state::settings::save_settings(&self.state.settings);
        cx.notify();
    }

    /// Runs a card button (design 3a): "Refresh now", "Sign in now", "Get token now", "Sign in
    /// again", "Get new token". The sign-in, when needed, shows in the response panel.
    pub(crate) fn run_oauth_action(&mut self, tab_id: &str, force: Force, cx: &mut Context<Self>) {
        let Some((profile_name, scope)) = self
            .state
            .tabs
            .index_of(tab_id)
            .and_then(|index| self.state.tabs.get(index))
            .and_then(|tab| tab.request())
            .and_then(|request| oauth_auth(&request.auth))
            .map(|(profile, scope)| (profile.to_string(), scope.to_string()))
        else {
            return;
        };
        self.state.reload_environment();
        let environment = self.active_environment();
        let session = self.state.session_env.clone();
        let runner = Runner::new(self.script_engine.clone(), self.send_options.clone())
            .with_oauth(self.oauth_config(tab_id));
        self.oauth.card_error = None;
        let started = Instant::now();
        let task = cx.spawn(async move |this, cx| {
            let run = cx
                .background_spawn(async move {
                    runner.oauth_token(&profile_name, &scope, &environment, &session, force)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                log::debug!(
                    "oauth: card action finished in {} ms",
                    started.elapsed().as_millis()
                );
                view.oauth.action = None;
                view.oauth.waiting = None;
                match &run.result {
                    Ok(token) => view.remember_oauth_token(&run.key, Some(token.clone())),
                    Err(error) => {
                        view.oauth.card_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        });
        self.oauth.action = Some(task);
        cx.notify();
    }

    /// "Clear": forgets the token of `key`.
    pub(crate) fn clear_oauth_token(&mut self, key: TokenKey, cx: &mut Context<Self>) {
        self.oauth.store.remove(&key);
        log::info!("oauth: token cleared");
        self.remember_oauth_token(&key, None);
        cx.notify();
    }

    /// Cancels the sign-in in progress: "Cancel" or Esc in the waiting panel.
    pub(crate) fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        if self.oauth.waiting.take().is_some() {
            self.oauth.sign_in.cancel();
            log::info!("oauth: sign-in cancelled");
            cx.notify();
        }
    }
}
