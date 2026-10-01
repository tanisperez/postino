//! Small formatting helpers for numbers and paths shown in the UI: human-readable sizes and
//! durations, home-relative paths, and relative day labels (`plans/ui-redesign.md`, section 2.3
//! point 2 "timings, sizes, hints" and section 1 "Recent workspaces").

use std::path::Path;
use std::time::Duration;

use rust_i18n::t;

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
pub fn shorten_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_string(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

/// The label shown in the open-tabs bar for an open request tab: the file stem (its name without
/// the `.postino` extension), for example `"login"` for `"auth/login.postino"`. The full id is
/// shown in the tab's tooltip instead (`plans/ui-redesign.md` section 2.3 point 3).
pub fn tab_label(id: &str) -> &str {
    let name = id.rsplit('/').next().unwrap_or(id);
    name.strip_suffix(".postino").unwrap_or(name)
}

/// A relative label for a day, comparing UTC day boundaries: `"today"`, `"yesterday"`, or `"N
/// days ago"` for anything older. A `then` on the same UTC day as `now`, or in the future, is
/// also `"today"`.
#[allow(dead_code)] // wired by the load test run history of phase 8
pub fn relative_day(then_unix_seconds: i64, now_unix_seconds: i64) -> String {
    relative_day_in(then_unix_seconds, now_unix_seconds, &rust_i18n::locale())
}

/// [`relative_day`] in an explicit `locale`, so tests need not touch the process wide one.
fn relative_day_in(then_unix_seconds: i64, now_unix_seconds: i64, locale: &str) -> String {
    let day_difference = now_unix_seconds.div_euclid(SECONDS_PER_DAY)
        - then_unix_seconds.div_euclid(SECONDS_PER_DAY);
    if day_difference <= 0 {
        t!("request.format.today", locale = locale).into_owned()
    } else if day_difference == 1 {
        t!("request.format.yesterday", locale = locale).into_owned()
    } else {
        t!(
            "request.format.days_ago",
            locale = locale,
            count = day_difference
        )
        .into_owned()
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
    fn tab_label_is_the_file_stem() {
        assert_eq!(tab_label("auth/login.postino"), "login");
        assert_eq!(tab_label("health.postino"), "health");
    }

    #[test]
    fn relative_day_labels() {
        let now = 1_000_000i64;
        let day = SECONDS_PER_DAY;
        assert_eq!(relative_day_in(now, now, "en"), "today");
        assert_eq!(relative_day_in(now - day, now, "en"), "yesterday");
        assert_eq!(relative_day_in(now - 3 * day, now, "en"), "3 days ago");
        assert_eq!(relative_day_in(now + day, now, "en"), "today");
    }

    #[test]
    fn relative_day_is_translated() {
        let now = 1_000_000i64;
        let day = SECONDS_PER_DAY;
        assert_eq!(relative_day_in(now - day, now, "es"), "ayer");
        assert_eq!(relative_day_in(now - 3 * day, now, "it"), "3 giorni fa");
    }
}
