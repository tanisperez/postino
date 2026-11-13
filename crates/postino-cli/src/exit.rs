//! The exit codes of the console commands and the error that carries one.

use std::fmt;

/// Everything passed.
pub const SUCCESS: u8 = 0;
/// A `test()` failed, a script threw, or a token could not be obtained.
pub const FAILED: u8 = 1;
/// The command line is wrong.
pub const USAGE: u8 = 2;
/// A network or TLS failure: nothing came back.
pub const NETWORK: u8 = 3;
/// A file could not be read: a malformed `.postino`, an unknown environment, a missing path.
pub const FILE: u8 = 4;

/// The worse of two exit codes: a file error over a network one over a failure.
pub fn worst(a: u8, b: u8) -> u8 {
    let rank = |code: u8| match code {
        FILE => 4,
        NETWORK => 3,
        USAGE => 2,
        FAILED => 1,
        _ => 0,
    };
    if rank(b) > rank(a) { b } else { a }
}

/// A command that could not run, with its exit code and the message for stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    /// The exit code.
    pub code: u8,
    /// What went wrong, one or more lines without the final newline.
    pub message: String,
}

impl CliError {
    /// A file error (exit code 4).
    pub fn file(message: impl Into<String>) -> Self {
        CliError {
            code: FILE,
            message: message.into(),
        }
    }

    /// A usage error (exit code 2).
    pub fn usage(message: impl Into<String>) -> Self {
        CliError {
            code: USAGE,
            message: message.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_errors_win_over_network_over_failures() {
        assert_eq!(worst(SUCCESS, FAILED), FAILED);
        assert_eq!(worst(NETWORK, FAILED), NETWORK);
        assert_eq!(worst(FAILED, FILE), FILE);
        assert_eq!(worst(FILE, NETWORK), FILE);
        assert_eq!(worst(SUCCESS, SUCCESS), SUCCESS);
    }
}
