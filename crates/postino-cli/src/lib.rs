//! The command line of Postino: the commands that run in the terminal and exit instead of
//! opening the window. No `gpui`, so everything is unit tested without a window.
//!
//! Only the first argument is read; anything that is not a command (no argument, a path, an
//! unknown flag) is left to the app, which treats it as the path to open.
//!
//! To add a command:
//! 1. Create a module with its type, a `parse` from the first argument, a `run` returning an
//!    exit code and its help text (see [`info`]).
//! 2. Register it in one place: a variant in [`Command`], a line in [`Command::parse`] and one
//!    in [`Command::run`], and mention it in the help of [`info`].
//! 3. Add its tests next to it.

pub mod info;

use std::ffi::OsStr;
use std::process::ExitCode;

use telaio_app::AppIdentity;

pub use info::Info;

/// Who Postino is, for every piece of `telaio-app` the app and the command line use: the name,
/// the lowercase name of folders and files, the application id (Wayland `app_id`, X11 `WM_CLASS`,
/// Windows AppUserModelID, the same as `codes.tanis.postino.desktop` and the Windows installer
/// shortcut), the version, the environment variable prefix and the repository.
pub const IDENTITY: AppIdentity = AppIdentity {
    name: "Postino",
    slug: "postino",
    app_id: "codes.tanis.postino",
    version: env!("CARGO_PKG_VERSION"),
    env_prefix: "POSTINO",
    repository: "https://github.com/tanisperez/postino",
};

/// A command line command that runs without opening the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// `-h`/`--help` and `-V`/`--version`.
    Info(Info),
}

impl Command {
    /// The command asked by the first argument, or `None` when Postino should start normally.
    pub fn parse(first_arg: Option<&OsStr>) -> Option<Self> {
        let arg = first_arg?.to_str()?;
        Info::parse(arg).map(Command::Info)
    }

    /// Runs the command and returns the process exit code.
    pub fn run(self) -> ExitCode {
        let code = match self {
            Command::Info(info) => info.run(),
        };
        ExitCode::from(code)
    }
}

/// Handles the command line before anything else is set up. Returns `None` when the first
/// argument is not a command (the app starts normally), otherwise runs it, attaching to the
/// parent console first on Windows, and returns the exit code.
pub fn run_if_cli(first_arg: Option<&OsStr>) -> Option<ExitCode> {
    let command = Command::parse(first_arg)?;
    telaio_app::cli::attach_parent_console();
    Some(command.run())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_argument_or_a_path_is_not_a_command() {
        assert_eq!(Command::parse(None), None);
        assert_eq!(Command::parse(Some(OsStr::new("/home/me/requests"))), None);
        assert_eq!(Command::parse(Some(OsStr::new("-x"))), None);
    }

    #[test]
    fn the_info_flags_are_commands() {
        assert_eq!(
            Command::parse(Some(OsStr::new("--help"))),
            Some(Command::Info(Info::Help))
        );
        assert_eq!(
            Command::parse(Some(OsStr::new("-V"))),
            Some(Command::Info(Info::Version))
        );
    }

    #[test]
    fn run_if_cli_ignores_what_is_not_a_command() {
        assert!(run_if_cli(None).is_none());
        assert!(run_if_cli(Some(OsStr::new("orders"))).is_none());
    }
}
