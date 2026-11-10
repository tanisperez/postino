//! [`Runner`], the request execution pipeline (see `docs/architecture.md`): build the variable
//! scope, run the pre script, interpolate, send the request, run the post script, and collect a
//! complete [`RunResult`] for the UI.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use postino_core::{
    ApiKeyLocation, Auth, Environment, KeyValue, OAuthProfile, Request, ResolvedRequest, Response,
    TokenPlacement, VarScope,
};
use postino_http::{Client, HttpError, SendOptions};
use postino_oauth::{
    Force, MemoryStore, NoSignIn, OAuthEnv, OAuthError, OAuthRun, SignInHandler, TokenStore,
    ensure_token, token_key,
};
use postino_script::{PostContext, PreContext, ScriptEngine};

use crate::resolve::{MultipartFiles, resolve};
use crate::result::{FailedStage, RunResult};
use crate::script_bridge::{
    apply_script_request, resolved_to_script_request, to_script_request, to_script_response,
};
use crate::session_env::SessionEnv;

/// Runs a [`Request`] end to end: pre script, interpolation, send, post script.
///
/// A `Runner` is cheap to keep around: it only holds the [`ScriptEngine`] implementation and the
/// [`SendOptions`] every request is sent with. Create one per application (or one per test) and
/// call [`Runner::run`] for every request.
pub struct Runner {
    engine: Arc<dyn ScriptEngine>,
    options: SendOptions,
    /// Where the relative paths of multipart files start: the workspace.
    base_dir: Option<PathBuf>,
    /// The workspace's OAuth 2.0 profiles and where their tokens live.
    oauth: OAuthConfig,
}

/// What [`Runner`] needs to send a request whose auth is an OAuth 2.0 profile (#225).
#[derive(Clone)]
pub struct OAuthConfig {
    /// The workspace id of the token keys, see [`postino_oauth::workspace_id`].
    pub workspace: String,
    /// The profiles of the workspace's `project.toml`.
    pub profiles: Arc<Vec<OAuthProfile>>,
    /// Where tokens are kept.
    pub store: Arc<dyn TokenStore>,
    /// The browser sign-in of the authorization code grant.
    pub sign_in: Arc<dyn SignInHandler>,
}

impl Default for OAuthConfig {
    /// No profiles, tokens in memory, no browser sign-in.
    fn default() -> Self {
        OAuthConfig {
            workspace: String::new(),
            profiles: Arc::new(Vec::new()),
            store: Arc::new(MemoryStore::default()),
            sign_in: Arc::new(NoSignIn),
        }
    }
}

impl OAuthConfig {
    /// The profile named `name`.
    pub fn profile(&self, name: &str) -> Option<&OAuthProfile> {
        self.profiles.iter().find(|profile| profile.name == name)
    }
}

/// The auth that carries `token` the way `profile` sends it, so the usual rules apply (an
/// enabled header or query entry of the same name wins).
fn token_auth(profile: &OAuthProfile, token: &str) -> Auth {
    match profile.token_placement {
        TokenPlacement::Header => Auth::ApiKey {
            name: "Authorization".to_string(),
            value: if profile.token_prefix.trim().is_empty() {
                token.to_string()
            } else {
                format!("{} {token}", profile.token_prefix.trim())
            },
            location: ApiKeyLocation::Header,
        },
        TokenPlacement::Query => Auth::ApiKey {
            name: profile.token_param.clone(),
            value: token.to_string(),
            location: ApiKeyLocation::Query,
        },
    }
}

impl Runner {
    /// Creates a runner that uses `engine` for `::: pre`/`::: post` scripts and sends requests
    /// with `options`.
    #[must_use]
    pub fn new(engine: Arc<dyn ScriptEngine>, options: SendOptions) -> Self {
        Self {
            engine,
            options,
            base_dir: None,
            oauth: OAuthConfig::default(),
        }
    }

    /// Gives the runner the workspace's OAuth 2.0 profiles and token store.
    #[must_use]
    pub fn with_oauth(mut self, oauth: OAuthConfig) -> Self {
        self.oauth = oauth;
        self
    }

    /// Gets the token of the profile `name` for `environment`, as Send does, or with `force` as
    /// the buttons of the Auth tab do. `scope` overrides the profile's when not empty.
    pub fn oauth_token(
        &self,
        name: &str,
        scope: &str,
        environment: &Environment,
        session_env: &SessionEnv,
        force: Force,
    ) -> OAuthRun {
        let vars = VarScope {
            request_vars: &[],
            session_env: session_env.as_slice(),
            environment: &environment.variables,
        };
        match self.oauth.profile(name) {
            Some(profile) => {
                let env = OAuthEnv {
                    workspace: &self.oauth.workspace,
                    environment: &environment.name,
                    store: self.oauth.store.as_ref(),
                    options: &self.options,
                    sign_in: self.oauth.sign_in.as_ref(),
                };
                ensure_token(&env, profile, scope, &vars, force)
            }
            None => {
                let missing = OAuthProfile::new(name, Default::default());
                OAuthRun {
                    profile: name.to_string(),
                    environment: environment.name.clone(),
                    key: token_key(&self.oauth.workspace, &environment.name, &missing, scope),
                    steps: Vec::new(),
                    event: None,
                    result: Err(OAuthError::UnknownProfile(name.to_string())),
                }
            }
        }
    }

    /// Makes the relative paths of multipart files start at `base_dir`, the workspace folder.
    #[must_use]
    pub fn with_base_dir(mut self, base_dir: Option<PathBuf>) -> Self {
        self.base_dir = base_dir;
        self
    }

    /// Runs `request` against `environment`, applying any environment changes the scripts make
    /// to `session_env`. The request is sent on a new connection, closed afterwards.
    ///
    /// This never fails: every kind of failure (a pre script exception, a send error, a post
    /// script exception) is reported inside the returned [`RunResult`] instead of a top-level
    /// `Result` (see [`RunResult::failed_stage`]), alongside whatever was collected before the
    /// failure.
    pub fn run(
        &self,
        request: &Request,
        environment: &Environment,
        session_env: &mut SessionEnv,
    ) -> RunResult {
        self.execute(request, environment, session_env, |resolved| {
            postino_http::send(resolved, &self.options)
        })
    }

    /// A [`Client`] that sends with this runner's options, for [`Self::run_with`].
    #[must_use]
    pub fn client(&self) -> Client {
        Client::new(self.options.clone())
    }

    /// [`Self::run`], sending the request through `client`, which reuses its open connections
    /// (keep-alive). `client` should come from [`Self::client`], so it has the same options.
    pub fn run_with(
        &self,
        request: &Request,
        environment: &Environment,
        session_env: &mut SessionEnv,
        client: &Client,
    ) -> RunResult {
        self.execute(request, environment, session_env, |resolved| {
            client.send(resolved)
        })
    }

    /// The pipeline behind [`Self::run`] and [`Self::run_with`], which only differ in `send`.
    fn execute(
        &self,
        request: &Request,
        environment: &Environment,
        session_env: &mut SessionEnv,
        send: impl FnOnce(&ResolvedRequest) -> Result<Response, HttpError>,
    ) -> RunResult {
        let started_at = SystemTime::now();
        let mut console = Vec::new();
        let mut tests = Vec::new();

        // 0. The OAuth 2.0 token, before the pre script: scripts have no network, and the token
        // only depends on the environment.
        let mut token_auth_value = None;
        let mut oauth = None;
        if let Auth::OAuth2 { profile, scope } = &request.auth {
            let run = self.oauth_token(profile, scope, environment, session_env, Force::None);
            match (&run.result, self.oauth.profile(profile)) {
                (Ok(token), Some(profile)) => {
                    token_auth_value = Some(token_auth(profile, &token.access_token));
                    oauth = Some(run);
                }
                _ => {
                    return RunResult {
                        resolved_request: None,
                        response: None,
                        tests,
                        console,
                        warnings: Vec::new(),
                        failed_stage: Some(FailedStage::Auth),
                        started_at,
                        sent_at: None,
                        finished_at: SystemTime::now(),
                        oauth: Some(run),
                    };
                }
            }
        }

        // 1. Pre script, skipped if empty.
        let mut working_request = request.clone();
        let mut request_vars: Vec<KeyValue> = Vec::new();
        if !request.pre_script.trim().is_empty() {
            let ctx = PreContext {
                request: to_script_request(&working_request),
                vars: Vec::new(),
                env: session_env.merged_with(environment),
            };
            let started = Instant::now();
            let outcome = self.engine.run_pre(&request.pre_script, ctx);
            log::trace!(
                "pre-request script ran in {} us",
                started.elapsed().as_micros()
            );
            match outcome {
                Err(error) => {
                    // An uncaught exception in a pre script aborts the run: nothing is sent.
                    return RunResult {
                        resolved_request: None,
                        response: None,
                        tests,
                        console,
                        warnings: Vec::new(),
                        failed_stage: Some(FailedStage::Pre(error)),
                        started_at,
                        sent_at: None,
                        finished_at: SystemTime::now(),
                        oauth,
                    };
                }
                Ok(outcome) => {
                    working_request = apply_script_request(&working_request, outcome.request);
                    request_vars = outcome.vars;
                    session_env.apply(&outcome.env_changes);
                    console.extend(outcome.console);
                    tests.extend(outcome.tests);
                }
            }
        }

        if let Some(auth) = token_auth_value {
            working_request.auth = auth;
        }

        // 2. Interpolate into a ResolvedRequest. Never fatal: unresolved markers become warnings.
        let scope = VarScope {
            request_vars: &request_vars,
            session_env: session_env.as_slice(),
            environment: &environment.variables,
        };
        let started = Instant::now();
        let (resolved, warnings, file_error) = resolve(
            &working_request,
            &scope,
            MultipartFiles::Read(self.base_dir.as_deref()),
        );
        log::trace!(
            "interpolated in {} us, {} warnings",
            started.elapsed().as_micros(),
            warnings.len()
        );

        // 3. Send.
        let sent_at = SystemTime::now();
        let sent = match file_error {
            Some(error) => Err(HttpError::InvalidRequest(error)),
            None => send(&resolved),
        };
        let response = match sent {
            Ok(response) => response,
            Err(error) => {
                return RunResult {
                    resolved_request: Some(resolved),
                    response: None,
                    tests,
                    console,
                    warnings,
                    failed_stage: Some(FailedStage::Send(error)),
                    started_at,
                    sent_at: Some(sent_at),
                    finished_at: SystemTime::now(),
                    oauth,
                };
            }
        };

        // 4. Post script, skipped if empty.
        if !request.post_script.trim().is_empty() {
            let ctx = PostContext {
                request: resolved_to_script_request(&resolved),
                response: to_script_response(&response),
                vars: request_vars,
                env: session_env.merged_with(environment),
            };
            let started = Instant::now();
            let outcome = self.engine.run_post(&request.post_script, ctx);
            log::trace!(
                "post-response script ran in {} us",
                started.elapsed().as_micros()
            );
            match outcome {
                Err(error) => {
                    // An engine-level failure (timeout, memory limit, ...): unlike a plain
                    // script exception, there is no PostOutcome to read console lines or tests
                    // from.
                    return RunResult {
                        resolved_request: Some(resolved),
                        response: Some(response),
                        tests,
                        console,
                        warnings,
                        failed_stage: Some(FailedStage::Post(error)),
                        started_at,
                        sent_at: Some(sent_at),
                        finished_at: SystemTime::now(),
                        oauth,
                    };
                }
                Ok(outcome) => {
                    session_env.apply(&outcome.env_changes);
                    console.extend(outcome.console);
                    tests.extend(outcome.tests);
                    if let Some(script_error) = outcome.script_error {
                        // A plain uncaught exception: the response is still shown, but the
                        // failure is reported alongside whatever ran before it was thrown.
                        return RunResult {
                            resolved_request: Some(resolved),
                            response: Some(response),
                            tests,
                            console,
                            warnings,
                            failed_stage: Some(FailedStage::Post(script_error)),
                            started_at,
                            sent_at: Some(sent_at),
                            finished_at: SystemTime::now(),
                            oauth,
                        };
                    }
                }
            }
        }

        RunResult {
            resolved_request: Some(resolved),
            response: Some(response),
            tests,
            console,
            warnings,
            failed_stage: None,
            started_at,
            sent_at: Some(sent_at),
            finished_at: SystemTime::now(),
            oauth,
        }
    }
}
