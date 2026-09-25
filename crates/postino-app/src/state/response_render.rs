//! Pure formatting helpers for the response viewer (`views/response_view.rs`): pretty-printing
//! the body, formatting size and time, and classifying a status code for coloring.
//!
//! Kept free of `gpui` types so every rule is unit-tested directly.

use std::time::Duration;

/// The class of an HTTP status code, used to color the response viewer's status badge the way
/// most HTTP clients do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusClass {
    /// 2xx: the request succeeded.
    Success,
    /// 3xx: redirection.
    Redirect,
    /// 4xx: the client made a bad request.
    ClientError,
    /// 5xx: the server failed.
    ServerError,
    /// Anything outside the standard 2xx-5xx ranges (1xx, or a value more than three digits
    /// long, which cannot happen for a real HTTP status but the type is a plain `u16`).
    Other,
}

/// Classifies `status` by its leading digit.
pub fn status_class(status: u16) -> StatusClass {
    match status {
        200..=299 => StatusClass::Success,
        300..=399 => StatusClass::Redirect,
        400..=499 => StatusClass::ClientError,
        500..=599 => StatusClass::ServerError,
        _ => StatusClass::Other,
    }
}

/// Attempts to pretty-print `body` as JSON with two-space indentation. Returns `None` if the
/// body is not valid UTF-8 or not valid JSON, in which case the caller falls back to
/// [`body_as_text`].
pub fn pretty_print_json(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// Renders `body` as text for the raw view: valid UTF-8 shown as is, otherwise a short
/// placeholder noting the byte count (Phase 9 has no binary/hex viewer).
pub fn body_as_text(body: &[u8]) -> String {
    match std::str::from_utf8(body) {
        Ok(text) => text.to_string(),
        Err(_) => format!("<{} bytes, not valid UTF-8>", body.len()),
    }
}

/// Formats a byte count for display: bytes under 1000 as is, otherwise kilobytes or megabytes
/// with one decimal place.
pub fn format_size(bytes: usize) -> String {
    let bytes = bytes as f64;
    if bytes < 1000.0 {
        format!("{} B", bytes as u64)
    } else if bytes < 1_000_000.0 {
        format!("{:.1} KB", bytes / 1000.0)
    } else {
        format!("{:.1} MB", bytes / 1_000_000.0)
    }
}

/// Formats a duration for display: whole milliseconds under one second, otherwise seconds with
/// two decimal places.
pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        format!("{millis} ms")
    } else {
        format!("{:.2} s", duration.as_secs_f64())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn status_class_covers_every_standard_range() {
        assert_eq!(status_class(101), StatusClass::Other);
        assert_eq!(status_class(200), StatusClass::Success);
        assert_eq!(status_class(201), StatusClass::Success);
        assert_eq!(status_class(299), StatusClass::Success);
        assert_eq!(status_class(301), StatusClass::Redirect);
        assert_eq!(status_class(404), StatusClass::ClientError);
        assert_eq!(status_class(500), StatusClass::ServerError);
    }

    #[test]
    fn pretty_print_json_formats_valid_json() {
        let result = pretty_print_json(br#"{"a":1,"b":[1,2]}"#).expect("valid JSON");
        assert_eq!(result, "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2\n  ]\n}");
    }

    #[test]
    fn pretty_print_json_rejects_invalid_json() {
        assert_eq!(pretty_print_json(b"not json"), None);
    }

    #[test]
    fn pretty_print_json_rejects_non_utf8() {
        assert_eq!(pretty_print_json(&[0xff, 0xfe]), None);
    }

    #[test]
    fn body_as_text_shows_utf8_as_is() {
        assert_eq!(body_as_text(b"hello"), "hello");
    }

    #[test]
    fn body_as_text_placeholders_non_utf8() {
        assert_eq!(body_as_text(&[0xff, 0xfe]), "<2 bytes, not valid UTF-8>");
    }

    #[test]
    fn format_size_uses_bytes_below_1000() {
        assert_eq!(format_size(42), "42 B");
        assert_eq!(format_size(999), "999 B");
    }

    #[test]
    fn format_size_uses_kilobytes() {
        assert_eq!(format_size(1500), "1.5 KB");
    }

    #[test]
    fn format_size_uses_megabytes() {
        assert_eq!(format_size(2_500_000), "2.5 MB");
    }

    #[test]
    fn format_duration_uses_milliseconds_below_one_second() {
        assert_eq!(format_duration(Duration::from_millis(842)), "842 ms");
    }

    #[test]
    fn format_duration_uses_seconds_at_and_above_one_second() {
        assert_eq!(format_duration(Duration::from_millis(1200)), "1.20 s");
    }
}
