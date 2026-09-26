//! Sending the active tab's request through `postino-runner` on `gpui`'s background executor
//! (`plans/mvp.md`, Phase 9).
//!
//! The pipeline itself (pre script, interpolation, HTTP, post script) is exactly
//! [`postino_runner::Runner::run`]; this module's only job is to run it off the main thread so
//! the UI stays responsive, show a spinner while it is in flight, and allow cancelling it.

use gpui_kit::*;
use postino_core::Environment;
use postino_runner::{Preview, Runner, preview};

use super::root::AppView;

/// A request currently being sent: which tab it belongs to, and the background [`Task`] driving
/// it.
///
/// Dropping the `Task` cancels the pipeline immediately: `gpui-pre`'s `Task` is documented as
/// "If you drop a task it will be cancelled immediately", unlike `.detach()`, which lets it run
/// to completion. [`AppView::cancel_send`] relies on exactly this by replacing this struct (and
/// therefore its `Task`) with `None`, per `plans/mvp.md` Phase 9: "allows cancel (drop the result
/// if cancelled)".
pub(crate) struct SendingTask {
    /// The id of the tab whose request is being sent.
    pub(crate) tab_id: String,
    /// Kept alive only so it is not dropped (and cancelled) while still in flight; never polled
    /// directly, its result is delivered through the `view.update` call inside
    /// [`AppView::send_active_tab`]'s spawned future.
    _task: Task<()>,
}

impl AppView {
    /// The environment to send with: the active environment's variables, loaded from the
    /// workspace, or an empty [`Environment`] for "No environment" (or when no workspace is
    /// open, which should not happen while a tab is open, but is handled the same way rather
    /// than panicking).
    pub(crate) fn active_environment(&self) -> Environment {
        let workspace = match self.state.workspace.as_ref() {
            Some(workspace) => workspace,
            None => return Environment::default(),
        };
        let Some(name) = self.state.active_environment.as_deref() else {
            return Environment::default();
        };
        workspace.load_environment(name).unwrap_or_default()
    }

    /// The active tab's request resolved against the active environment and the session
    /// environment, without running its scripts or sending it (`plans/ui-redesign.md` phase 5
    /// item 2). `None` when no tab is open. Cheap enough to call on every render: it only
    /// interpolates `{{ }}` markers, it never runs a script or opens a socket.
    pub(crate) fn current_preview(&self) -> Option<Preview> {
        let tab = self.state.tabs.active()?;
        let environment = self.active_environment();
        Some(preview(&tab.request, &environment, &self.state.session_env))
    }

    /// Whether `tab_id` currently has a request in flight.
    pub(crate) fn is_sending(&self, tab_id: &str) -> bool {
        self.sending
            .as_ref()
            .is_some_and(|task| task.tab_id == tab_id)
    }

    /// Sends the active tab's request on the background executor. Replacing
    /// [`AppView::sending`] (done here unconditionally) drops any request already in flight,
    /// cancelling it, so starting a new send also acts as "cancel the previous one".
    pub(crate) fn send_active_tab(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.state.tabs.active() else {
            return;
        };
        let tab_id = tab.id.clone();
        let request = tab.request.clone();
        let environment = self.active_environment();
        let engine = self.script_engine.clone();
        let options = self.send_options.clone();
        let mut session_env = self.state.session_env.clone();

        let task = cx.spawn(async move |this, cx| {
            // The actual pipeline (pre script, interpolation, HTTP, post script) runs on a
            // background thread: `Runner::run` blocks on the HTTP call and can run a script for
            // up to 5 seconds, neither of which should stall the UI thread.
            let (result, session_env) = cx
                .background_spawn(async move {
                    let runner = Runner::new(engine, options);
                    let result = runner.run(&request, &environment, &mut session_env);
                    (result, session_env)
                })
                .await;
            // Back on the main thread: fold the script-updated session environment back into
            // the single, per-session `SessionEnv` (`plans/mvp.md`: "one SessionEnv per app
            // session"), record the result, and clear `sending` unless a newer send has already
            // replaced it (which would mean this one was superseded, not cancelled, since a
            // dropped task never reaches this point at all).
            let _ = this.update(cx, |view, cx| {
                view.state.session_env = session_env;
                view.responses.insert(tab_id.clone(), result);
                if view.is_sending(&tab_id) {
                    view.sending = None;
                }
                cx.notify();
            });
        });

        self.sending = Some(SendingTask {
            tab_id: tab.id.clone(),
            _task: task,
        });
        cx.notify();
    }

    /// Cancels the request currently in flight, if any, by dropping its `Task`.
    pub(crate) fn cancel_send(&mut self, cx: &mut Context<Self>) {
        self.sending = None;
        cx.notify();
    }
}
