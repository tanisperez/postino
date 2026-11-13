//! Colors for the readable output: on a terminal only, never with `--no-color` or `NO_COLOR`
//! (<https://no-color.org>).

use std::io::IsTerminal;

/// Whether to print ANSI colors on one stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    color: bool,
}

impl Style {
    /// No colors at all.
    #[cfg(test)]
    pub const PLAIN: Style = Style { color: false };

    /// Colors for stdout, unless turned off.
    pub fn stdout(no_color: bool) -> Self {
        Self::detect(no_color, std::io::stdout().is_terminal())
    }

    /// Colors for stderr, unless turned off.
    pub fn stderr(no_color: bool) -> Self {
        Self::detect(no_color, std::io::stderr().is_terminal())
    }

    fn detect(no_color: bool, terminal: bool) -> Self {
        let env_off = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        Style {
            color: terminal && !no_color && !env_off,
        }
    }

    fn paint(self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Passed.
    pub fn green(self, text: &str) -> String {
        self.paint("32", text)
    }

    /// Failed.
    pub fn red(self, text: &str) -> String {
        self.paint("31", text)
    }

    /// A warning.
    pub fn yellow(self, text: &str) -> String {
        self.paint("33", text)
    }

    /// Secondary details: timings, sizes.
    pub fn dim(self, text: &str) -> String {
        self.paint("2", text)
    }

    /// An HTTP status in the color of its class.
    pub fn status(self, status: u16) -> String {
        let text = status.to_string();
        match status {
            200..=299 => self.green(&text),
            300..=399 => self.paint("36", &text),
            400..=499 => self.yellow(&text),
            _ => self.red(&text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prints_the_text_as_is() {
        assert_eq!(Style::PLAIN.green("ok"), "ok");
        assert_eq!(Style::PLAIN.status(404), "404");
    }

    #[test]
    fn colors_wrap_the_text() {
        let style = Style { color: true };
        assert_eq!(style.red("x"), "\x1b[31mx\x1b[0m");
        assert_eq!(style.status(201), "\x1b[32m201\x1b[0m");
    }

    #[test]
    fn no_color_or_no_terminal_turns_colors_off() {
        assert_eq!(Style::detect(true, true), Style::PLAIN);
        assert_eq!(Style::detect(false, false), Style::PLAIN);
    }
}
