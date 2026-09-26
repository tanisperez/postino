//! Small formatting helpers for numbers and paths shown in the UI: human-readable sizes and
//! durations, home-relative paths, and relative day labels (`plans/ui-redesign.md`, section 2.3
//! point 2 "timings, sizes, hints" and section 1 "Recent workspaces").

use std::path::Path;
use std::time::Duration;

/// Number of seconds in a day, used by [`relative_day`] to compare UTC day boundaries.
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// Formats a byte count as a human-readable size, for example `"512 B"`, `"1.2 KB"`, `"3.4 MB"`.
/// Uses binary units (1 KB = 1024 B).
#[allow(dead_code)] // wired by the response viewer and load test dashboard of later phases
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes_f < MB {
        format!("{:.1} KB", bytes_f / KB)
    } else if bytes_f < GB {
        format!("{:.1} MB", bytes_f / MB)
    } else {
        format!("{:.1} GB", bytes_f / GB)
    }
}

/// Formats a duration as a human-readable time, for example `"142 ms"` below one second, `"1.4
/// s"` at or above it.
#[allow(dead_code)] // wired by the response viewer and load test dashboard of later phases
pub fn human_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        format!("{millis} ms")
    } else {
        format!("{:.1} s", duration.as_secs_f64())
    }
}

/// Shortens `path` to start with `~` when it is inside `home`, for example `/home/tanis/dev`
/// with home `/home/tanis` becomes `~/dev`. Returns the path unchanged (as a string) if it is
/// not inside `home`, or if `home` is `None`.
#[allow(dead_code)] // wired by the title bar and sidebar footer of phase 4
pub fn shorten_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_string(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

/// A relative label for a day, comparing UTC day boundaries: `"today"`, `"yesterday"`, or `"N
/// days ago"` for anything older. A `then` on the same UTC day as `now`, or in the future, is
/// also `"today"`.
#[allow(dead_code)] // wired by the load test run history of phase 8
pub fn relative_day(then_unix_seconds: i64, now_unix_seconds: i64) -> String {
    let day_difference = now_unix_seconds.div_euclid(SECONDS_PER_DAY)
        - then_unix_seconds.div_euclid(SECONDS_PER_DAY);
    if day_difference <= 0 {
        "today".to_string()
    } else if day_difference == 1 {
        "yesterday".to_string()
    } else {
        format!("{day_difference} days ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn human_size_formats_each_unit() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1229), "1.2 KB");
        assert_eq!(human_size(3_460_000), "3.3 MB");
        assert_eq!(human_size(2_147_483_648), "2.0 GB");
    }

    #[test]
    fn human_duration_switches_units_at_one_second() {
        assert_eq!(human_duration(Duration::from_millis(142)), "142 ms");
        assert_eq!(human_duration(Duration::from_millis(999)), "999 ms");
        assert_eq!(human_duration(Duration::from_millis(1400)), "1.4 s");
    }

    #[test]
    fn shorten_path_replaces_the_home_prefix() {
        assert_eq!(
            shorten_path(
                Path::new("/home/tanis/dev/postino"),
                Some(Path::new("/home/tanis"))
            ),
            "~/dev/postino"
        );
    }

    #[test]
    fn shorten_path_of_home_itself_is_a_bare_tilde() {
        assert_eq!(
            shorten_path(Path::new("/home/tanis"), Some(Path::new("/home/tanis"))),
            "~"
        );
    }

    #[test]
    fn shorten_path_outside_home_is_unchanged() {
        assert_eq!(
            shorten_path(Path::new("/var/log"), Some(Path::new("/home/tanis"))),
            "/var/log"
        );
    }

    #[test]
    fn shorten_path_without_a_home_is_unchanged() {
        assert_eq!(shorten_path(Path::new("/var/log"), None), "/var/log");
    }

    #[test]
    fn relative_day_labels() {
        let now = 1_000_000i64;
        let day = SECONDS_PER_DAY;
        assert_eq!(relative_day(now, now), "today");
        assert_eq!(relative_day(now - day, now), "yesterday");
        assert_eq!(relative_day(now - 3 * day, now), "3 days ago");
        assert_eq!(relative_day(now + day, now), "today");
    }
}
