//! The pieces every app of the family shares: the `<App> X.Y.Z` version line, the common help
//! header and footer, the standard exit codes and the Windows console attach.
//!
//! This whole module is meant to move to the future `telaio-app` crate, so keep it free of
//! anything specific to Postino: the app name, version and URLs come in as arguments.

/// Exit code of a command that did what it was asked.
pub const EXIT_SUCCESS: u8 = 0;

/// Exit code of a command line that could not be understood (unknown option, missing value).
pub const EXIT_USAGE: u8 = 2;

/// `<App> X.Y.Z`, the first line of `--version` and of `--help`.
pub fn version_line(app_name: &str, version: &str) -> String {
    format!("{app_name} {version}")
}

/// The top of a `--help` text: the version line followed by the one line description.
pub fn help_header(app_name: &str, version: &str, tagline: &str) -> String {
    format!("{}\n{tagline}\n", version_line(app_name, version))
}

/// The bottom of a `--help` text: the link to the command line documentation.
pub fn help_footer(docs_url: &str) -> String {
    format!("Documentation: {docs_url}\n")
}

/// Lets a Windows GUI subsystem binary print to the console it was started from. Without it,
/// stdout goes nowhere. Does nothing when there is no parent console (a double click). The
/// shell does not wait for a GUI program, so the output lands after its prompt.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: takes a plain process id and touches no memory of ours; failure is harmless.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Other platforms keep the terminal's stdout.
#[cfg(not(windows))]
pub fn attach_parent_console() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_is_the_name_and_the_version() {
        assert_eq!(version_line("App", "1.2.3"), "App 1.2.3");
    }

    #[test]
    fn the_help_header_and_footer_end_in_a_newline() {
        assert_eq!(
            help_header("App", "1.2.3", "Does things."),
            "App 1.2.3\nDoes things.\n"
        );
        assert_eq!(
            help_footer("https://x.test"),
            "Documentation: https://x.test\n"
        );
    }

    #[test]
    fn the_exit_codes_are_the_conventional_ones() {
        assert_eq!(EXIT_SUCCESS, 0);
        assert_eq!(EXIT_USAGE, 2);
    }
}
