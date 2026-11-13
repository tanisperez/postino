//! The browser sign-in of the authorization code grant from the console: the system browser
//! opens on the authorization URL and the loopback listener waits for the redirect, as in the
//! app. With no display it cannot happen, and the run fails with [`headless_message`].

use std::process::{Command, Stdio};

use postino_oauth::{OAuthError, SignInHandler, SignInPoll, SignInPrompt};
use postino_runner::RunResult;

/// The [`SignInHandler`] of the command line.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsoleSignIn;

impl SignInHandler for ConsoleSignIn {
    fn available(&self) -> bool {
        has_display()
    }

    fn open(&self, prompt: &SignInPrompt) {
        eprintln!(
            "Signing in to the OAuth profile \"{}\" in the browser. If it does not open, visit:\n  {}",
            prompt.profile, prompt.url
        );
        if let Err(error) = open_browser(&prompt.url) {
            eprintln!("Could not open the browser: {error}");
        }
    }

    /// Nothing to poll: the console only waits for the redirect, Ctrl+C cancels.
    fn poll(&self) -> SignInPoll {
        SignInPoll::Waiting
    }
}

/// Whether a browser can be shown. On Linux and the BSDs, a graphical session must exist.
fn has_display() -> bool {
    if cfg!(any(windows, target_vendor = "apple")) {
        return true;
    }
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// Opens `url` in the default browser, without waiting for it.
fn open_browser(url: &str) -> std::io::Result<()> {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    } else if cfg!(target_vendor = "apple") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// The message of a run whose token needs a browser that the console cannot show, or `None`.
pub fn headless_message(result: &RunResult) -> Option<String> {
    let run = result
        .oauth
        .iter()
        .chain(result.oauth_more.iter())
        .find(|run| matches!(run.result, Err(OAuthError::SignInRequired)))?;
    Some(format!(
        "the OAuth profile \"{}\" uses the authorization code grant, which signs in in a \
         browser, and there is no display here. Without a display, use a profile with the \
         client_credentials or password grant",
        run.profile
    ))
}
