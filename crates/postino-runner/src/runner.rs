//! [`Runner`], the request execution pipeline (see `docs/architecture.md`): build the variable
//! scope, run the pre script, interpolate, send the request, run the post script, and collect a
//! complete [`RunResult`] for the UI.

use std::sync::Arc;
use std::time::Instant;

use postino_core::{Environment, KeyValue, Request, VarScope};
use postino_http::SendOptions;
use postino_script::{PostContext, PreContext, ScriptEngine};

use crate::resolve::resolve;
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
}

impl Runner {
    /// Creates a runner that uses `engine` for `::: pre`/`::: post` scripts and sends requests
    /// with `options`.
    #[must_use]
    pub fn new(engine: Arc<dyn ScriptEngine>, options: SendOptions) -> Self {
        Self { engine, options }
    }

    /// Runs `request` against `environment`, applying any environment changes the scripts make
    /// to `session_env`.
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
        let mut console = Vec::new();
        let mut tests = Vec::new();

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

        // 2. Interpolate into a ResolvedRequest. Never fatal: unresolved markers become warnings.
        let scope = VarScope {
            request_vars: &request_vars,
            session_env: session_env.as_slice(),
            environment: &environment.variables,
        };
        let started = Instant::now();
        let (resolved, warnings) = resolve(&working_request, &scope);
        log::trace!(
            "interpolated in {} us, {} warnings",
            started.elapsed().as_micros(),
            warnings.len()
        );

        // 3. Send.
        let response = match postino_http::send(&resolved, &self.options) {
            Ok(response) => response,
            Err(error) => {
                return RunResult {
                    resolved_request: Some(resolved),
                    response: None,
                    tests,
                    console,
                    warnings,
                    failed_stage: Some(FailedStage::Send(error)),
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
        }
    }
}
