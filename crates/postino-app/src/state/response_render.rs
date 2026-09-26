//! Pure formatting helpers for the response viewer (`views/response_view.rs`): pretty-printing
//! the body and formatting size and time. Status coloring now comes from
//! [`crate::theme::Palette::status_colors`] (`plans/ui-redesign.md` phase 5), which classifies a
//! status code the same way this module used to.
//!
//! Kept free of `gpui` types so every rule is unit-tested directly.

use std::time::Duration;

use postino_core::TemplateWarning;

/// The standard reason phrase for a status code (`"OK"` for 200, `"Not Found"` for 404, ...),
/// for the response status badge label ("200 OK", "404 Not Found", `plans/ui-redesign.md` phase
/// 5, reviewer fix item 2). `postino_core::Response` carries no reason phrase from the wire
/// (`ureq`/the `http` crate normalize away the raw status line), so this is always the standard
/// phrase for the code; a code this table does not cover returns `None`, and the caller then
/// shows the bare code, as before.
pub fn reason_phrase(status: u16) -> Option<&'static str> {
    Some(match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => return None,
    })
}

/// One row for the response pane's warnings strip (`plans/ui-redesign.md` phase 5, reviewer fix
/// items 4 and 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningGroup {
    /// A single unknown variable.
    UnknownVariable(String),
    /// More than one unknown variable in the same run: every name, deduplicated, in order of
    /// first appearance. Shown as one row instead of one row per variable.
    UnknownVariables(Vec<String>),
    /// A function-call error's own message.
    Function(String),
}

/// Groups a run's [`TemplateWarning`]s into display rows: every unknown variable name,
/// deduplicated regardless of how many times or where it appeared, becomes a single
/// [`WarningGroup::UnknownVariable`] or [`WarningGroup::UnknownVariables`] row (singular only
/// when there is exactly one), listed before one [`WarningGroup::Function`] row per
/// function-call error, in order of appearance.
pub fn group_warnings(warnings: &[TemplateWarning]) -> Vec<WarningGroup> {
    let mut unknown_names = Vec::new();
    let mut function_groups = Vec::new();
    for warning in warnings {
        match warning {
            TemplateWarning::UnknownVariable(name) => {
                if !unknown_names.contains(name) {
                    unknown_names.push(name.clone());
                }
            }
            TemplateWarning::Function(_) => {
                function_groups.push(WarningGroup::Function(warning.to_string()));
            }
        }
    }
    let unknown_group = match unknown_names.len() {
        0 => None,
        1 => Some(WarningGroup::UnknownVariable(unknown_names.remove(0))),
        _ => Some(WarningGroup::UnknownVariables(unknown_names)),
    };
    unknown_group.into_iter().chain(function_groups).collect()
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

    #[test]
    fn reason_phrase_covers_common_codes() {
        assert_eq!(reason_phrase(200), Some("OK"));
        assert_eq!(reason_phrase(404), Some("Not Found"));
        assert_eq!(reason_phrase(500), Some("Internal Server Error"));
    }

    #[test]
    fn reason_phrase_is_none_for_an_uncovered_code() {
        assert_eq!(reason_phrase(299), None);
    }

    #[test]
    fn group_warnings_is_empty_for_no_warnings() {
        assert_eq!(group_warnings(&[]), Vec::new());
    }

    #[test]
    fn group_warnings_singular_form_for_one_unknown_variable() {
        let warnings = vec![TemplateWarning::UnknownVariable("token".to_string())];
        assert_eq!(
            group_warnings(&warnings),
            vec![WarningGroup::UnknownVariable("token".to_string())]
        );
    }

    #[test]
    fn group_warnings_plural_form_for_several_unknown_variables() {
        let warnings = vec![
            TemplateWarning::UnknownVariable("token".to_string()),
            TemplateWarning::UnknownVariable("userId".to_string()),
        ];
        assert_eq!(
            group_warnings(&warnings),
            vec![WarningGroup::UnknownVariables(vec![
                "token".to_string(),
                "userId".to_string()
            ])]
        );
    }

    #[test]
    fn group_warnings_deduplicates_repeated_names() {
        let warnings = vec![
            TemplateWarning::UnknownVariable("token".to_string()),
            TemplateWarning::UnknownVariable("token".to_string()),
        ];
        assert_eq!(
            group_warnings(&warnings),
            vec![WarningGroup::UnknownVariable("token".to_string())]
        );
    }

    #[test]
    fn group_warnings_lists_unknown_variables_before_function_errors() {
        use postino_core::VarScope;
        let scope = VarScope {
            request_vars: &[],
            session_env: &[],
            environment: &[],
        };
        let warnings = vec![
            TemplateWarning::UnknownVariable("token".to_string()),
            postino_core::interpolate("{{ noSuchFunction() }}", &scope).warnings[0].clone(),
        ];
        let groups = group_warnings(&warnings);
        assert_eq!(
            groups[0],
            WarningGroup::UnknownVariable("token".to_string())
        );
        assert!(matches!(groups[1], WarningGroup::Function(_)));
    }
}
