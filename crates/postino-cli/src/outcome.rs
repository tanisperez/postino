//! What a [`RunResult`] means for the console: passed or not, the exit code it asks for and the
//! error to print.

use std::time::Duration;

use postino_oauth::OAuthError;
use postino_runner::{FailedStage, RunResult};

use crate::exit;

/// The verdict of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// [`exit::SUCCESS`], [`exit::FAILED`] or [`exit::NETWORK`].
    pub code: u8,
    /// Why the request did not run to the end, when it did not.
    pub error: Option<String>,
    /// How many `test()` calls passed.
    pub passed_tests: usize,
    /// How many `test()` calls ran.
    pub total_tests: usize,
}

/// The verdict of `result`.
pub fn outcome(result: &RunResult) -> Outcome {
    let total_tests = result.tests.len();
    let passed_tests = result.tests.iter().filter(|test| test.passed).count();
    let (code, error) = match &result.failed_stage {
        None if passed_tests < total_tests => (exit::FAILED, None),
        None => (exit::SUCCESS, None),
        Some(FailedStage::Auth) => {
            let error = result.oauth_error();
            let code = match error {
                Some(OAuthError::Send(_)) => exit::NETWORK,
                _ => exit::FAILED,
            };
            let message = crate::oauth::headless_message(result).unwrap_or_else(|| {
                let profile = result.oauth.as_ref().map_or("", |run| run.profile.as_str());
                match error {
                    Some(error) => format!("OAuth token of \"{profile}\": {error}"),
                    None => FailedStage::Auth.to_string(),
                }
            });
            (code, Some(message))
        }
        Some(stage @ FailedStage::Send(_)) => (exit::NETWORK, Some(stage.to_string())),
        Some(stage) => (exit::FAILED, Some(stage.to_string())),
    };
    Outcome {
        code,
        error,
        passed_tests,
        total_tests,
    }
}

/// How long the request took: the response time, or the whole run when nothing came back.
pub fn elapsed(result: &RunResult) -> Duration {
    match &result.response {
        Some(response) => response.time,
        None => result
            .finished_at
            .duration_since(result.started_at)
            .unwrap_or_default(),
    }
}

/// `84 ms`, `1.2 s`.
pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if duration.is_zero() {
        "0 ms".to_string()
    } else if millis == 0 {
        "<1 ms".to_string()
    } else if millis < 1000 {
        format!("{millis} ms")
    } else {
        format!("{:.1} s", duration.as_secs_f64())
    }
}

/// `512 B`, `1.2 kB`, `3.4 MB`: decimal units, like the app.
pub fn format_size(bytes: u64) -> String {
    // Display only: the precision lost converting to f64 does not show.
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64;
    match bytes {
        0..1_000 => format!("{bytes} B"),
        1_000..1_000_000 => format!("{:.1} kB", value / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} MB", value / 1e6),
        _ => format!("{:.1} GB", value / 1e9),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn durations_and_sizes_read_well() {
        assert_eq!(format_duration(Duration::from_millis(84)), "84 ms");
        assert_eq!(format_duration(Duration::from_micros(300)), "<1 ms");
        assert_eq!(format_duration(Duration::from_millis(1240)), "1.2 s");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1_250), "1.2 kB");
        assert_eq!(format_size(3_400_000), "3.4 MB");
    }
}
