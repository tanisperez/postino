//! The app's side of a browser sign-in (#226, design 3c): [`AppSignIn`] hands the prompt to the
//! UI thread, which opens the browser and shows the waiting panel, and passes back a cancel or a
//! pasted redirect URL. The send runs on a background thread and polls it.

use std::sync::Mutex;

use postino_oauth::{CallbackPage, SignInHandler, SignInPoll, SignInPrompt};
use rust_i18n::t;

/// What the waiting panel did since the sign-in started.
#[derive(Debug, Default)]
struct Pending {
    cancelled: bool,
    pasted: Option<String>,
}

/// The [`SignInHandler`] of the app. One per window, shared with every send.
pub struct AppSignIn {
    prompts: async_channel::Sender<SignInPrompt>,
    pending: Mutex<Pending>,
}

impl AppSignIn {
    /// A handler that sends every prompt to `prompts`, read on the UI thread.
    pub fn new(prompts: async_channel::Sender<SignInPrompt>) -> Self {
        AppSignIn {
            prompts,
            pending: Mutex::new(Pending::default()),
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, Pending> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Cancels the sign-in in progress ("Cancel", Esc, or a new send).
    pub fn cancel(&self) {
        self.pending().cancelled = true;
    }

    /// Hands over the URL the browser ended at ("Paste redirect URL").
    pub fn paste(&self, url: String) {
        self.pending().pasted = Some(url);
    }
}

impl SignInHandler for AppSignIn {
    fn open(&self, prompt: &SignInPrompt) {
        *self.pending() = Pending::default();
        if self.prompts.try_send(prompt.clone()).is_err() {
            log::warn!("oauth: the window is gone, cannot open the browser");
            self.cancel();
        }
    }

    fn poll(&self) -> SignInPoll {
        let mut pending = self.pending();
        if pending.cancelled {
            pending.cancelled = false;
            return SignInPoll::Cancelled;
        }
        match pending.pasted.take() {
            Some(url) => SignInPoll::Pasted(url),
            None => SignInPoll::Waiting,
        }
    }

    fn page(&self, profile: &str, error: Option<&str>) -> CallbackPage {
        match error {
            None => CallbackPage {
                title: t!("oauth.page_signed_in", profile = profile).into_owned(),
                body: t!("oauth.page_close_tab").into_owned(),
            },
            Some(error) => CallbackPage {
                title: t!("oauth.page_failed", profile = profile).into_owned(),
                body: error.to_string(),
            },
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn prompt() -> SignInPrompt {
        SignInPrompt {
            profile: "acme-sso".into(),
            environment: "dev".into(),
            url: "https://auth/authorize".into(),
            listening: Some("127.0.0.1:53682".into()),
        }
    }

    #[test]
    fn prompts_reach_the_ui_and_answers_come_back() {
        let (sender, receiver) = async_channel::unbounded();
        let handler = AppSignIn::new(sender);
        handler.cancel();
        // Opening starts a new sign-in: a cancel left from the previous one is forgotten.
        handler.open(&prompt());
        assert_eq!(receiver.try_recv().expect("prompt"), prompt());
        assert_eq!(handler.poll(), SignInPoll::Waiting);
        handler.paste("https://app/cb?code=1".into());
        assert_eq!(
            handler.poll(),
            SignInPoll::Pasted("https://app/cb?code=1".into())
        );
        assert_eq!(handler.poll(), SignInPoll::Waiting);
        handler.cancel();
        assert_eq!(handler.poll(), SignInPoll::Cancelled);
    }

    #[test]
    fn a_closed_window_cancels() {
        let (sender, receiver) = async_channel::unbounded();
        drop(receiver);
        let handler = AppSignIn::new(sender);
        handler.open(&prompt());
        assert_eq!(handler.poll(), SignInPoll::Cancelled);
    }

    #[test]
    fn the_callback_page_is_translated() {
        let (sender, _receiver) = async_channel::unbounded();
        let page = AppSignIn::new(sender).page("acme-sso", None);
        assert_eq!(page.title, "Signed in to acme-sso");
    }
}
