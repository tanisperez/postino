//! The command line of Postino: the commands that run in the terminal and exit instead of
//! opening the window (#220). No `gpui`, so everything is unit tested without a window.
//!
//! The first argument decides: `-h`/`--help` and `-V`/`--version` print and exit ([`info`]), a
//! subcommand name ([`args::SUBCOMMANDS`]) runs in console mode, and anything else (no
//! argument, a path, an unknown flag) is left to the app, which treats it as the path to open.
//!
//! To add a subcommand:
//! 1. Add its arguments to [`args`]: a variant of [`args::ConsoleCommand`] and its name in
//!    [`args::SUBCOMMANDS`].
//! 2. Create a module with a `command` function returning the exit code ([`exit`]), and call it
//!    from [`run_console`].
//! 3. List it in the help of [`info`] and add its tests next to it.

pub mod args;
mod check;
mod context;
pub mod exit;
mod filter;
pub mod info;
mod oauth;
mod outcome;
mod progress;
mod report;
mod request;
mod run;
mod style;
mod units;

use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use std::sync::LazyLock;

use clap::Parser;
use postino_core::Method;
use telaio_app::AppIdentity;

use args::{Cli, ConsoleCommand};
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

/// The short git commit this build was made from, captured by `build.rs` (or taken from the
/// `POSTINO_COMMIT` environment variable at build time). `None` outside a repository, for example
/// a source tarball.
pub const COMMIT: Option<&str> = option_env!("POSTINO_COMMIT");

/// The suffix of the versions between two releases (`0.4.0-dev`), whose builds show their commit.
const DEV_SUFFIX: &str = "-dev";

/// The version as shown to people: `0.4.0-dev (4e74e2d)` for a development build with a known
/// commit, the bare version otherwise. A release is named by its tag, so it never shows a commit.
/// The updater, the release notes link and anything machine readable use the bare version.
pub fn format_version_label(version: &str, commit: Option<&str>) -> String {
    match commit {
        Some(commit) if version.ends_with(DEV_SUFFIX) => format!("{version} ({commit})"),
        _ => version.to_string(),
    }
}

/// [`format_version_label`] of this build.
pub fn version_label() -> &'static str {
    static LABEL: LazyLock<&'static str> = LazyLock::new(|| {
        let label = format_version_label(IDENTITY.version, COMMIT);
        // `AppIdentity` wants a `&'static str`: leaked once, for the life of the process.
        Box::leak(label.into_boxed_str())
    });
    *LABEL
}

/// [`IDENTITY`] with the version label, for what only displays the version (About, the version
/// line). Pass [`IDENTITY`] itself to everything that needs the bare version.
pub fn display_identity() -> AppIdentity {
    AppIdentity {
        version: version_label(),
        ..IDENTITY
    }
}

/// A command line command that runs without opening the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `-h`/`--help` and `-V`/`--version`.
    Info(Info),
    /// A subcommand, with every argument after the program name.
    Console(Vec<OsString>),
}

impl Command {
    /// The command the arguments (without the program name) ask for, or `None` when Postino
    /// should start normally.
    pub fn parse(args: &[OsString]) -> Option<Self> {
        let first = args.first()?.to_str()?;
        if let Some(info) = Info::parse(first) {
            return Some(Command::Info(info));
        }
        args::SUBCOMMANDS
            .contains(&first)
            .then(|| Command::Console(args.to_vec()))
    }

    /// Runs the command and returns the process exit code.
    pub fn run(self) -> ExitCode {
        let code = match self {
            Command::Info(info) => info.run(),
            Command::Console(args) => run_console(&args),
        };
        ExitCode::from(code)
    }
}

/// Parses and runs a subcommand, returning its exit code.
fn run_console(args: &[OsString]) -> u8 {
    let all = std::iter::once(OsStr::new("postino").to_os_string()).chain(args.iter().cloned());
    let cli = match Cli::try_parse_from(all) {
        Ok(cli) => cli,
        Err(error) => {
            // `--help` of a subcommand is an "error" that prints to stdout.
            let _ = error.print();
            return if error.use_stderr() {
                exit::USAGE
            } else {
                exit::SUCCESS
            };
        }
    };
    match cli.command {
        ConsoleCommand::Run(args) => run::command(&args),
        ConsoleCommand::Check(args) => check::command(&args),
        ConsoleCommand::Get(args) => request::command(Method::Get, &args),
        ConsoleCommand::Post(args) => request::command(Method::Post, &args),
        ConsoleCommand::Put(args) => request::command(Method::Put, &args),
        ConsoleCommand::Patch(args) => request::command(Method::Patch, &args),
        ConsoleCommand::Delete(args) => request::command(Method::Delete, &args),
        ConsoleCommand::Head(args) => request::command(Method::Head, &args),
        ConsoleCommand::Options(args) => request::command(Method::Options, &args),
        ConsoleCommand::Http(http) => {
            let method = http.method.parse().unwrap_or(Method::Get);
            request::command(method, &http.request)
        }
    }
}

/// Handles the command line before anything else is set up. Returns `None` when the arguments
/// (without the program name) are not a command (the app starts normally), otherwise runs it,
/// attaching to the parent console first on Windows, and returns the exit code.
pub fn run_if_cli(args: &[OsString]) -> Option<ExitCode> {
    let command = Command::parse(args)?;
    telaio_app::cli::attach_parent_console();
    Some(command.run())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dev_build_shows_its_commit_next_to_the_version() {
        assert_eq!(
            format_version_label("0.4.0-dev", Some("4e74e2d")),
            "0.4.0-dev (4e74e2d)"
        );
        assert_eq!(format_version_label("0.4.0-dev", None), "0.4.0-dev");
    }

    #[test]
    fn a_release_is_named_by_its_tag_only() {
        assert_eq!(format_version_label("0.4.0", Some("4e74e2d")), "0.4.0");
        assert_eq!(
            format_version_label("0.4.0-rc.1", Some("4e74e2d")),
            "0.4.0-rc.1"
        );
        assert_eq!(format_version_label("0.4.0", None), "0.4.0");
    }

    #[test]
    fn the_display_identity_only_changes_the_version() {
        let display = display_identity();
        assert_eq!(display.version, version_label());
        assert_eq!(display.name, IDENTITY.name);
        assert_eq!(display.repository, IDENTITY.repository);
        assert!(version_label().starts_with(IDENTITY.version));
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_argument_or_a_path_is_not_a_command() {
        assert_eq!(Command::parse(&[]), None);
        assert_eq!(Command::parse(&os(&["/home/me/requests"])), None);
        assert_eq!(Command::parse(&os(&["-x"])), None);
        // A folder named like a subcommand opens with `./run`.
        assert_eq!(Command::parse(&os(&["./run"])), None);
    }

    #[test]
    fn the_info_flags_are_commands() {
        assert_eq!(
            Command::parse(&os(&["--help"])),
            Some(Command::Info(Info::Help))
        );
        assert_eq!(
            Command::parse(&os(&["-V"])),
            Some(Command::Info(Info::Version))
        );
    }

    #[test]
    fn subcommands_run_in_console_mode() {
        let args = os(&["run", "auth/", "--env", "dev"]);
        assert_eq!(Command::parse(&args), Some(Command::Console(args.clone())));
        assert!(matches!(
            Command::parse(&os(&["get", "https://x"])),
            Some(Command::Console(_))
        ));
    }

    #[test]
    fn usage_errors_exit_with_2() {
        assert_eq!(run_console(&os(&["get"])), exit::USAGE);
        assert_eq!(run_console(&os(&["run", "--no-such-flag"])), exit::USAGE);
    }

    #[test]
    fn run_if_cli_ignores_what_is_not_a_command() {
        assert!(run_if_cli(&[]).is_none());
        assert!(run_if_cli(&os(&["orders"])).is_none());
    }
}
