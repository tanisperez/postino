//! [`ensure_token`]: what Send does before the request leaves (design 2b). A valid stored token
//! is reused; an expired one is refreshed, or the grant repeated when there is no refresh token
//! or the server rejects it; with no token at all, the profile's grant runs, which for the
//! authorization code grant means signing in in the browser.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use postino_core::{OAuthGrant, OAuthProfile, VarScope};
use postino_http::SendOptions;

use crate::browser::{
    CallbackPage, Listener, Pkce, SIGN_IN_TIMEOUT, SignInPoll, authorization_url,
    code_from_redirect, new_state, wait_for_redirect,
};
use crate::error::OAuthError;
use crate::exchange::{TokenExchange, build_request, grant_form, request_token};
use crate::resolve::{ResolvedProfile, resolve_profile};
use crate::store::{TokenKey, TokenStore};
use crate::token::TokenSet;

/// What the browser sign-in shows the user while it waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInPrompt {
    /// The profile name.
    pub profile: String,
    /// The environment name, empty for "No environment".
    pub environment: String,
    /// The authorization URL to open.
    pub url: String,
    /// The address Postino listens on, `None` when the redirect URL has to be pasted.
    pub listening: Option<String>,
}

/// The part of a browser sign-in that needs the user: opening the browser, showing that Postino
/// waits, and reporting a cancel or a pasted URL. Called from the thread that sends.
pub trait SignInHandler: Send + Sync {
    /// Whether a browser sign-in can happen at all. The command line and load tests answer
    /// `false`, so a profile without a stored token fails with [`OAuthError::SignInRequired`].
    fn available(&self) -> bool {
        true
    }
    /// The listener is ready: open `prompt.url` in the browser and show the waiting panel.
    fn open(&self, prompt: &SignInPrompt);
    /// Checked several times a second while waiting.
    fn poll(&self) -> SignInPoll;
    /// The page answered to the browser after the redirect: `error` is `None` on success.
    fn page(&self, profile: &str, error: Option<&str>) -> CallbackPage {
        match error {
            None => CallbackPage {
                title: format!("Signed in to {profile}"),
                body: "You can close this tab and go back to Postino.".to_string(),
            },
            Some(error) => CallbackPage {
                title: format!("Couldn't sign in to {profile}"),
                body: error.to_string(),
            },
        }
    }
}

/// A [`SignInHandler`] for places without a UI: never signs in.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSignIn;

impl SignInHandler for NoSignIn {
    fn available(&self) -> bool {
        false
    }

    fn open(&self, _prompt: &SignInPrompt) {}

    fn poll(&self) -> SignInPoll {
        SignInPoll::Cancelled
    }
}

/// What to do even when a valid token is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Force {
    /// Nothing: reuse a valid token (Send).
    #[default]
    None,
    /// Refresh it now ("Refresh now"), or repeat the grant when it cannot be refreshed.
    Refresh,
    /// Repeat the grant from the start ("Sign in again", "Get token", "Sign in").
    Grant,
}

/// How the token sent came to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenEvent {
    /// A valid stored token was reused.
    Reused,
    /// There was no token: a new one was obtained (the green notice of design 3c).
    New,
    /// The stored token had expired, or the user asked, and it was renewed (the "Token renewed"
    /// chip of design 3b).
    Renewed,
}

/// A line of the token's story, shown in the console as an `oauth` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthNote {
    /// The stored token expired `expired_for` ago (zero: it was about to).
    Expired {
        /// How long ago.
        expired_for: Duration,
        /// Whether it can be refreshed.
        has_refresh: bool,
    },
    /// There was no token for this profile and environment.
    NoToken,
    /// The server rejected the refresh token: the grant is repeated.
    RefreshRejected,
    /// The browser was opened to sign in.
    SigningIn,
    /// A new token was stored.
    NewToken {
        /// When it expires, `None` for never.
        expires_at: Option<SystemTime>,
    },
    /// Getting the token failed.
    Failed {
        /// The error, as [`OAuthError`] prints it.
        message: String,
    },
}

/// One step of [`OAuthRun::steps`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthStep {
    /// A line of the story.
    Note {
        /// When.
        time: SystemTime,
        /// What.
        note: OAuthNote,
    },
    /// A request to the token endpoint.
    Exchange(TokenExchange),
}

/// Everything [`ensure_token`] did, for the response bar, the console and the log.
#[derive(Debug)]
pub struct OAuthRun {
    /// The profile name.
    pub profile: String,
    /// The environment name, empty for "No environment".
    pub environment: String,
    /// Where the token is stored.
    pub key: TokenKey,
    /// The story, in order.
    pub steps: Vec<OAuthStep>,
    /// How the token came to be, `None` when it could not be obtained.
    pub event: Option<TokenEvent>,
    /// The token, or why there is none.
    pub result: Result<TokenSet, OAuthError>,
}

impl OAuthRun {
    /// The token, when there is one.
    pub fn token(&self) -> Option<&TokenSet> {
        self.result.as_ref().ok()
    }
}

/// Where and how [`ensure_token`] gets a token.
pub struct OAuthEnv<'a> {
    /// The workspace id, see [`crate::workspace_id`].
    pub workspace: &'a str,
    /// The active environment's name, empty for "No environment".
    pub environment: &'a str,
    /// Where tokens are kept.
    pub store: &'a dyn TokenStore,
    /// The TLS and proxy options of the token requests, the same as the requests'.
    pub options: &'a SendOptions,
    /// The browser sign-in.
    pub sign_in: &'a dyn SignInHandler,
}

/// The key the token of `profile` is stored under: the scope only counts when a request asks for
/// one that differs from the profile's.
pub fn token_key(
    workspace: &str,
    environment: &str,
    profile: &OAuthProfile,
    scope_override: &str,
) -> TokenKey {
    let scope_override = scope_override.trim();
    TokenKey {
        workspace: workspace.to_string(),
        profile: profile.name.clone(),
        environment: environment.to_string(),
        scope: (!scope_override.is_empty() && scope_override != profile.scope.trim())
            .then(|| scope_override.to_string()),
    }
}

/// One lock per token key, so two sends of the same profile (the workers of a load test) never
/// refresh the same token twice.
fn key_lock(key: &TokenKey) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let locks = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks.entry(key.to_uri()).or_default().clone()
}

/// A duration in log form: `12m`, `45s`, `2h`.
fn short_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 7_200 {
        format!("{}h", seconds / 3_600)
    } else if seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

/// Gets a token for the request: see the module docs. `vars` resolves the profile's markers (the
/// environment and the session overrides; the pre script has not run yet).
pub fn ensure_token(
    env: &OAuthEnv,
    profile: &OAuthProfile,
    scope_override: &str,
    vars: &VarScope,
    force: Force,
) -> OAuthRun {
    let key = token_key(env.workspace, env.environment, profile, scope_override);
    let mut run = OAuthRun {
        profile: profile.name.clone(),
        environment: env.environment.to_string(),
        key: key.clone(),
        steps: Vec::new(),
        event: None,
        result: Err(OAuthError::Cancelled),
    };
    let lock = key_lock(&key);
    let _guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let outcome = obtain(
        env,
        profile,
        scope_override,
        vars,
        force,
        &key,
        &mut run.steps,
    );
    run.result = match outcome {
        Ok((token, event)) => {
            run.event = Some(event);
            Ok(token)
        }
        Err(error) => {
            log::warn!(
                "oauth: no token\n  profile={} env={} error={}",
                profile.name,
                log_environment(env.environment),
                error.code().unwrap_or(error_kind(&error))
            );
            run.steps.push(OAuthStep::Note {
                time: SystemTime::now(),
                note: OAuthNote::Failed {
                    message: error.to_string(),
                },
            });
            Err(error)
        }
    };
    run
}

/// The environment as the log writes it.
fn log_environment(environment: &str) -> &str {
    if environment.is_empty() {
        "-"
    } else {
        environment
    }
}

/// A short name of `error` for the log, which never holds a secret.
fn error_kind(error: &OAuthError) -> &'static str {
    match error {
        OAuthError::UnknownProfile(_) => "unknown_profile",
        OAuthError::MissingField(_) => "missing_field",
        OAuthError::UnresolvedVariable { .. } => "unknown_variable",
        OAuthError::Server { .. } => "server",
        OAuthError::InvalidResponse { .. } => "invalid_response",
        OAuthError::Send(_) => "send_failed",
        OAuthError::SignInRequired => "sign_in_required",
        OAuthError::Cancelled => "cancelled",
        OAuthError::TimedOut => "timed_out",
        OAuthError::Denied { .. } => "denied",
        OAuthError::StateMismatch => "state_mismatch",
        OAuthError::NoCode => "no_code",
        OAuthError::Listen { .. } => "listen_failed",
        OAuthError::InvalidRedirectUrl => "invalid_redirect_url",
        OAuthError::Store(_) => "store_failed",
    }
}

fn obtain(
    env: &OAuthEnv,
    profile: &OAuthProfile,
    scope_override: &str,
    vars: &VarScope,
    force: Force,
    key: &TokenKey,
    steps: &mut Vec<OAuthStep>,
) -> Result<(TokenSet, TokenEvent), OAuthError> {
    let resolved = resolve_profile(profile, scope_override, vars)?;
    let stored = match env.store.get(key) {
        Ok(stored) => stored,
        Err(error) => {
            log::warn!("oauth: cannot read the stored token: {error}");
            None
        }
    };
    let now = SystemTime::now();
    let note = |steps: &mut Vec<OAuthStep>, note: OAuthNote| {
        steps.push(OAuthStep::Note {
            time: SystemTime::now(),
            note,
        });
    };

    if force == Force::None
        && let Some(token) = &stored
        && token.is_valid_at(now)
    {
        return Ok((token.clone(), TokenEvent::Reused));
    }

    let mut exchanges = Vec::new();
    let renewing = stored.is_some();
    match &stored {
        Some(token) if force != Force::Grant => {
            let has_refresh = token.refresh_token.is_some();
            if force == Force::None {
                let expired_for = token.expired_for_at(now).unwrap_or_default();
                log::info!(
                    "oauth: token expired\n  profile={} env={} expired_ago={}{}",
                    profile.name,
                    log_environment(env.environment),
                    short_duration(expired_for),
                    if has_refresh {
                        ""
                    } else {
                        " refresh_token=none"
                    }
                );
                note(
                    steps,
                    OAuthNote::Expired {
                        expired_for,
                        has_refresh,
                    },
                );
            }
            if let Some(refresh_token) = &token.refresh_token {
                let request = build_request(
                    &resolved,
                    grant_form(
                        "refresh_token",
                        &[("refresh_token", refresh_token), ("scope", &resolved.scope)],
                    ),
                );
                let result = request_token(
                    &request,
                    "refresh_token",
                    &resolved.scope,
                    env.options,
                    &mut exchanges,
                );
                steps.extend(exchanges.drain(..).map(OAuthStep::Exchange));
                match result {
                    Ok(mut new) => {
                        // RFC 6749, section 6: the server may keep the old refresh token.
                        if new.refresh_token.is_none() {
                            new.refresh_token = Some(refresh_token.clone());
                        }
                        if new.id_token.is_none() {
                            new.id_token = token.id_token.clone();
                        }
                        store_new(env, profile, key, &new, "refresh_token", steps)?;
                        return Ok((new, TokenEvent::Renewed));
                    }
                    Err(error) if error.is_invalid_grant() => {
                        note(steps, OAuthNote::RefreshRejected);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Some(_) => {}
        None => note(steps, OAuthNote::NoToken),
    }

    let token = run_grant(env, &resolved, steps)?;
    store_new(env, profile, key, &token, profile.grant.as_str(), steps)?;
    let event = if renewing {
        TokenEvent::Renewed
    } else {
        TokenEvent::New
    };
    Ok((token, event))
}

/// Saves a new token and records it in the log and the steps.
fn store_new(
    env: &OAuthEnv,
    profile: &OAuthProfile,
    key: &TokenKey,
    token: &TokenSet,
    grant: &str,
    steps: &mut Vec<OAuthStep>,
) -> Result<(), OAuthError> {
    env.store.set(key, token)?;
    let expires_in = token
        .expires_at
        .map(|expires_at| expires_at.saturating_sub(token.obtained_at).to_string())
        .unwrap_or_else(|| "never".to_string());
    log::info!(
        "oauth: new token\n  profile={} env={} grant={grant} expires_in={expires_in}",
        profile.name,
        log_environment(env.environment)
    );
    steps.push(OAuthStep::Note {
        time: SystemTime::now(),
        note: OAuthNote::NewToken {
            expires_at: token.expires_at_time(),
        },
    });
    Ok(())
}

/// Runs the profile's grant from the start.
fn run_grant(
    env: &OAuthEnv,
    profile: &ResolvedProfile,
    steps: &mut Vec<OAuthStep>,
) -> Result<TokenSet, OAuthError> {
    let mut exchanges = Vec::new();
    let result = match profile.grant {
        OAuthGrant::ClientCredentials => {
            let request = build_request(
                profile,
                grant_form("client_credentials", &[("scope", &profile.scope)]),
            );
            request_token(
                &request,
                "client_credentials",
                &profile.scope,
                env.options,
                &mut exchanges,
            )
        }
        OAuthGrant::Password => {
            let request = build_request(
                profile,
                grant_form(
                    "password",
                    &[
                        ("username", &profile.username),
                        ("password", &profile.password),
                        ("scope", &profile.scope),
                    ],
                ),
            );
            request_token(
                &request,
                "password",
                &profile.scope,
                env.options,
                &mut exchanges,
            )
        }
        OAuthGrant::AuthorizationCode => sign_in(env, profile, steps, &mut exchanges),
    };
    steps.extend(exchanges.into_iter().map(OAuthStep::Exchange));
    result
}

/// The authorization code grant: browser, redirect, code exchange.
fn sign_in(
    env: &OAuthEnv,
    profile: &ResolvedProfile,
    steps: &mut Vec<OAuthStep>,
    exchanges: &mut Vec<TokenExchange>,
) -> Result<TokenSet, OAuthError> {
    if !env.sign_in.available() {
        return Err(OAuthError::SignInRequired);
    }
    let listener = Listener::start(profile.redirect, &profile.redirect_url)?;
    let pkce = profile.pkce.then(Pkce::new);
    let state = new_state();
    let redirect_uri = listener.redirect_uri().to_string();
    let url = authorization_url(profile, &redirect_uri, &state, pkce.as_ref());
    steps.push(OAuthStep::Note {
        time: SystemTime::now(),
        note: OAuthNote::SigningIn,
    });
    log::info!(
        "oauth: browser sign-in\n  profile={} env={}",
        profile.name,
        log_environment(env.environment)
    );
    env.sign_in.open(&SignInPrompt {
        profile: profile.name.clone(),
        environment: env.environment.to_string(),
        url,
        listening: listener.address().map(str::to_string),
    });
    let page = |url: &str| {
        let error = code_from_redirect(url, &state)
            .err()
            .map(|error| error.to_string());
        env.sign_in.page(&profile.name, error.as_deref())
    };
    let redirect = wait_for_redirect(&listener, &|| env.sign_in.poll(), &page, SIGN_IN_TIMEOUT)?;
    drop(listener);
    let code = code_from_redirect(&redirect, &state)?;
    let verifier = pkce
        .as_ref()
        .map(|pkce| pkce.verifier.as_str())
        .unwrap_or("");
    let request = build_request(
        profile,
        grant_form(
            "authorization_code",
            &[
                ("code", &code),
                ("redirect_uri", &redirect_uri),
                ("code_verifier", verifier),
            ],
        ),
    );
    request_token(
        &request,
        "authorization_code",
        &profile.scope,
        env.options,
        exchanges,
    )
}
