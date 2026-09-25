//! The request execution pipeline: build the variable scope, run the pre script, interpolate,
//! send the request, run the post script, and collect the result.
//!
//! See `plans/mvp.md`, section 6. The single entry point is [`Runner::run`], which never fails:
//! every kind of failure is reported inside the returned [`RunResult`] instead of a top-level
//! `Result` (see [`RunResult::failed_stage`]).
#![warn(missing_docs)]

mod resolve;
mod result;
mod runner;
mod script_bridge;
mod session_env;

pub use postino_http::SendOptions;
pub use postino_script::ScriptEngine;
pub use result::{FailedStage, RunResult};
pub use runner::Runner;
pub use session_env::SessionEnv;
