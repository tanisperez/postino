//! The in-app updater's glue (#45): the single delayed check at startup, the manual check from
//! the command palette, the restart confirmation and the installation. The decisions and the
//! blocking work live in `state::update`; everything blocking runs on the background executor.
//! Nothing here repeats or polls, so an idle window stays idle.

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_update::Platform;
use rust_i18n::t;

use crate::state::update::{
    self, CheckOutcome, InstallOutcome, ReadyUpdate, STARTUP_DELAY, updater_enabled,
};

use super::root::AppView;

impl AppView {
    /// Schedules the one automatic check, [`STARTUP_DELAY`] after startup. A no-op unless the
    /// build has an updater and the setting is on. Failures are only logged.
    pub(crate) fn schedule_startup_update_check(&mut self, cx: &mut Context<Self>) {
        let setting = self.state.settings.check_updates;
        if !update::should_check_at_startup(updater_enabled(), setting) {
            return;
        }
        let Some(platform) = Platform::current() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STARTUP_DELAY).await;
            // The setting may have been turned off during the delay.
            let start = this
                .update(cx, |view, cx| {
                    let start =
                        view.state.settings.check_updates && view.state.update.begin_check();
                    if start {
                        cx.notify();
                    }
                    start
                })
                .unwrap_or(false);
            if !start {
                return;
            }
            let result = cx
                .background_spawn(async move { update::check_and_download(platform) })
                .await;
            // One repaint per change, for the status bar and an open Settings, "About".
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(outcome) => view.state.update.finish_check(&outcome),
                    Err(error) => {
                        view.state.update.fail_check();
                        log::warn!("update check failed: {error}");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The command palette's "Check for updates": runs the check whatever the setting says and
    /// reports the result in a notification.
    pub(crate) fn check_for_updates_manually(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(platform) = Platform::current().filter(|_| updater_enabled()) else {
            return;
        };
        if let Some(ready) = self.state.update.ready() {
            let text = t!("update.already_ready", version = ready.version.as_str());
            window.push_notification(Notification::info(text.into_owned()), cx);
            return;
        }
        if !self.state.update.begin_check() {
            return;
        }
        window.push_notification(Notification::info(t!("update.checking").into_owned()), cx);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { update::check_and_download(platform) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                let notification = match result {
                    Ok(outcome) => {
                        view.state.update.finish_check(&outcome);
                        match outcome {
                            CheckOutcome::UpToDate => Notification::success(
                                t!("update.up_to_date", version = update::CURRENT_VERSION)
                                    .into_owned(),
                            ),
                            CheckOutcome::Ready(ready) => Notification::info(
                                t!("update.found", version = ready.version.as_str()).into_owned(),
                            ),
                        }
                    }
                    Err(error) => {
                        view.state.update.fail_check();
                        log::warn!("update check failed: {error}");
                        Notification::error(
                            t!("update.check_failed", error = error.to_string()).into_owned(),
                        )
                    }
                };
                window.push_notification(notification, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The status bar's "restart" click: confirms first when a tab has unsaved changes, then
    /// installs. Nothing installs without this click.
    pub(crate) fn request_update_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.update.ready().is_none() {
            return;
        }
        if !update::needs_restart_confirmation(&self.state.tabs) {
            self.install_update(window, cx);
            return;
        }
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            alert
                .title(t!("update.confirm.title"))
                .description(t!("update.confirm.text"))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t!("update.confirm.restart"))
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(t!("common.cancel"))
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.install_update(window, cx));
                    true
                })
        });
    }

    /// Installs the ready update on the background executor, then quits (the installer or the
    /// swap script takes over) or explains why it could not.
    fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(platform) = Platform::current() else {
            return;
        };
        let Some(ready) = self.state.update.begin_install() else {
            return;
        };
        log::info!("installing update {}", ready.version);
        let exe = std::env::current_exe().unwrap_or_default();
        cx.spawn_in(window, async move |this, cx| {
            let task_ready = ready.clone();
            let outcome = cx
                .background_spawn(async move { update::start_install(platform, &task_ready, &exe) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_install(outcome, &ready, window, cx);
            });
        })
        .detach();
    }

    fn finish_install(
        &mut self,
        outcome: InstallOutcome,
        ready: &ReadyUpdate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            InstallOutcome::Quit => cx.quit(),
            InstallOutcome::Fallback => {
                self.state.update.fail_install();
                log::warn!("update {} cannot be installed in place", ready.version);
                cx.open_url(&ready.notes_url);
                window.push_notification(
                    Notification::warning(t!("update.fallback").into_owned()),
                    cx,
                );
            }
            InstallOutcome::Failed(error) => {
                self.state.update.fail_install();
                log::warn!("installing update {} failed: {error}", ready.version);
                window.push_notification(
                    Notification::error(t!("update.install_failed", error = error).into_owned()),
                    cx,
                );
            }
        }
    }
}
