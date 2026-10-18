//! The command line options that print something and exit instead of opening the window
//! (GitHub #136): `-h`/`--help` and `-V`/`--version`. Pure logic, unit tested without a window.
//! The output stays in English, like log messages.

use std::ffi::OsStr;

/// The command line guide `--help` links to.
pub const DOCS_URL: &str = "https://postino.tanis.codes/docs/command-line.html";

/// What a command line option asks for instead of opening the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    /// Print the usage summary and exit.
    Help,
    /// Print the version and exit.
    Version,
}

impl CliAction {
    /// The text to print to stdout, ending in a newline.
    pub fn output(self) -> String {
        match self {
            CliAction::Help => help_text(),
            CliAction::Version => format!("{}\n", version_line()),
        }
    }
}

/// The action asked by the first command line argument, or `None` when Postino should start
/// normally (no argument, or a path). Only the first argument is read, like the path.
pub fn parse(first_arg: Option<&OsStr>) -> Option<CliAction> {
    match first_arg?.to_str()? {
        "-h" | "--help" => Some(CliAction::Help),
        "-V" | "--version" => Some(CliAction::Version),
        _ => None,
    }
}

/// `Postino X.Y.Z`.
fn version_line() -> String {
    format!("Postino {}", env!("CARGO_PKG_VERSION"))
}

/// The `--help` text.
fn help_text() -> String {
    format!(
        "{version}
A lightweight, native HTTP client. Requests are plain local files.

Usage: postino [OPTIONS] [PATH]

Arguments:
  [PATH]  A workspace folder, or a .postino file to open in a tab.
          Without it, Postino opens the last workspace used.

Options:
  -h, --help     Print this help and exit
  -V, --version  Print the version and exit

Environment variables:
  POSTINO_LOG       Log level for this run: off, error, warn, info, debug or trace
  POSTINO_ENV       Environment to activate at startup, a file name under environments/
  POSTINO_AUTOSEND  Request to open and send right away, such as auth/login.postino

Documentation: {DOCS_URL}
",
        version = version_line(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(arg: &str) -> Option<CliAction> {
        parse(Some(OsStr::new(arg)))
    }

    #[test]
    fn help_flags_ask_for_help() {
        assert_eq!(parse_str("-h"), Some(CliAction::Help));
        assert_eq!(parse_str("--help"), Some(CliAction::Help));
    }

    #[test]
    fn version_flags_ask_for_the_version() {
        assert_eq!(parse_str("-V"), Some(CliAction::Version));
        assert_eq!(parse_str("--version"), Some(CliAction::Version));
    }

    #[test]
    fn no_argument_or_a_path_starts_normally() {
        assert_eq!(parse(None), None);
        assert_eq!(parse_str("/home/me/requests"), None);
        assert_eq!(parse_str("orders/create.postino"), None);
        assert_eq!(parse_str("-v"), None);
        assert_eq!(parse_str("--HELP"), None);
    }

    #[test]
    fn the_version_output_is_the_name_and_the_package_version() {
        assert_eq!(
            CliAction::Version.output(),
            format!("Postino {}\n", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn the_help_lists_every_option_and_variable() {
        let help = CliAction::Help.output();
        assert!(help.starts_with(&version_line()));
        for needle in [
            "Usage: postino [OPTIONS] [PATH]",
            "-h, --help",
            "-V, --version",
            "POSTINO_LOG",
            "POSTINO_ENV",
            "POSTINO_AUTOSEND",
            DOCS_URL,
        ] {
            assert!(help.contains(needle), "missing {needle:?}");
        }
        assert!(help.ends_with('\n'));
        assert!(!help.contains('\u{2014}'), "em dash in the help");
    }
}
