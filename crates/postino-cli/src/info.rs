//! `-h`/`--help` and `-V`/`--version` (GitHub #136): print something and exit. Pure logic, unit
//! tested without a window. The output stays in English, like log messages.
//!
//! This is also the template for a future command module: a type with `parse`, `run` and its
//! help text, registered in [`crate::Command`].

use std::io::Write;

use crate::common;

/// The command line guide `--help` links to.
pub const DOCS_URL: &str = "https://postino.tanis.codes/docs/command-line.html";

const APP_NAME: &str = "Postino";
const TAGLINE: &str = "A lightweight, native HTTP client. Requests are plain local files.";

/// What an informational option asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Info {
    /// Print the usage summary and exit.
    Help,
    /// Print the version and exit.
    Version,
}

impl Info {
    /// The command asked by the first argument, or `None` when it is not one of ours.
    pub fn parse(first_arg: &str) -> Option<Self> {
        match first_arg {
            "-h" | "--help" => Some(Info::Help),
            "-V" | "--version" => Some(Info::Version),
            _ => None,
        }
    }

    /// The text to print to stdout, ending in a newline.
    pub fn output(self) -> String {
        match self {
            Info::Help => help_text(),
            Info::Version => format!("{}\n", version_line()),
        }
    }

    /// Prints the output to stdout and returns the exit code.
    pub fn run(self) -> u8 {
        // A closed stdout (`postino --help | head -1`) is not worth a panic.
        let _ = std::io::stdout().write_all(self.output().as_bytes());
        common::EXIT_SUCCESS
    }
}

/// `Postino X.Y.Z`.
fn version_line() -> String {
    common::version_line(APP_NAME, env!("CARGO_PKG_VERSION"))
}

/// The `--help` text.
fn help_text() -> String {
    format!(
        "{header}
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

{footer}",
        header = common::help_header(APP_NAME, env!("CARGO_PKG_VERSION"), TAGLINE),
        footer = common::help_footer(DOCS_URL),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_flags_ask_for_help() {
        assert_eq!(Info::parse("-h"), Some(Info::Help));
        assert_eq!(Info::parse("--help"), Some(Info::Help));
    }

    #[test]
    fn version_flags_ask_for_the_version() {
        assert_eq!(Info::parse("-V"), Some(Info::Version));
        assert_eq!(Info::parse("--version"), Some(Info::Version));
    }

    #[test]
    fn a_path_or_another_flag_is_not_ours() {
        assert_eq!(Info::parse("/home/me/requests"), None);
        assert_eq!(Info::parse("orders/create.postino"), None);
        assert_eq!(Info::parse("-v"), None);
        assert_eq!(Info::parse("--HELP"), None);
    }

    #[test]
    fn the_version_output_is_the_name_and_the_package_version() {
        assert_eq!(
            Info::Version.output(),
            format!("Postino {}\n", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn the_help_lists_every_option_and_variable() {
        let help = Info::Help.output();
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
