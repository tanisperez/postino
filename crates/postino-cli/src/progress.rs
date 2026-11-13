//! The download progress bar of `-o`, drawn on stderr when it is a terminal.

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use crate::outcome::format_size;

/// The width of the bar itself, in cells.
const BAR_WIDTH: usize = 24;
/// The time between two redraws.
const REDRAW: Duration = Duration::from_millis(100);

/// A progress bar that redraws one line of stderr.
pub struct Progress {
    enabled: bool,
    last: Option<Instant>,
    drawn: bool,
}

impl Progress {
    /// A bar that only draws when stderr is a terminal.
    pub fn new() -> Self {
        Progress {
            enabled: std::io::stderr().is_terminal(),
            last: None,
            drawn: false,
        }
    }

    /// Records the bytes written so far and redraws, at most every [`REDRAW`] and at the end.
    pub fn update(&mut self, done: u64, total: Option<u64>) {
        if !self.enabled {
            return;
        }
        let finished = total.is_some_and(|total| done >= total);
        if !finished && self.last.is_some_and(|last| last.elapsed() < REDRAW) {
            return;
        }
        self.last = Some(Instant::now());
        self.drawn = true;
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "\r\x1b[2K{}", line(done, total));
        let _ = err.flush();
    }

    /// Clears the bar, so the summary takes its line.
    pub fn finish(&mut self) {
        if self.drawn {
            let mut err = std::io::stderr().lock();
            let _ = write!(err, "\r\x1b[2K");
            let _ = err.flush();
            self.drawn = false;
        }
    }
}

/// `  42% [██████████░░░░░░░░░░░░░░] 4.2 MB / 10.0 MB`, or only the size without a total.
pub fn line(done: u64, total: Option<u64>) -> String {
    match total.filter(|&total| total > 0) {
        Some(total) => {
            let done = done.min(total);
            // Both are byte counts far below the range where the ratio loses meaning.
            #[allow(clippy::cast_precision_loss)]
            let ratio = done as f64 / total as f64;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let filled = ((ratio * BAR_WIDTH as f64).round() as usize).min(BAR_WIDTH);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let percent = (ratio * 100.0).floor() as u32;
            format!(
                "{percent:>4}% [{}{}] {} / {}",
                "█".repeat(filled),
                "░".repeat(BAR_WIDTH - filled),
                format_size(done),
                format_size(total)
            )
        }
        None => format!("Downloading {}", format_size(done)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_line_shows_the_share_and_the_sizes() {
        assert_eq!(
            line(5_000_000, Some(10_000_000)),
            "  50% [████████████░░░░░░░░░░░░] 5.0 MB / 10.0 MB"
        );
        assert_eq!(
            line(0, Some(10)),
            "   0% [░░░░░░░░░░░░░░░░░░░░░░░░] 0 B / 10 B"
        );
        assert_eq!(line(2_500, None), "Downloading 2.5 kB");
    }
}
