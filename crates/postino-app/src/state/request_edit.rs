//! Pure editing operations on a [`Request`], used by the request editor view
//! (`views/request_editor.rs`).
//!
//! Kept free of `gpui` types so every rule here (what a button does to the data) is unit-tested
//! directly, without a window. The view's job is only to turn a click or a keystroke into a call
//! into this module, then mark the tab dirty and re-render.

use postino_core::{Body, KeyValue, Request};
use rust_i18n::t;

/// The kind of body a request has, mirroring [`Body`] without its content. Used by the body type
/// selector, which needs to list and compare kinds without carrying the (possibly large) body
/// text around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyKind {
    /// No body.
    #[default]
    None,
    /// A JSON body.
    Json,
    /// A plain text body.
    Text,
    /// An XML body.
    Xml,
    /// A `application/x-www-form-urlencoded` body.
    Form,
}

impl BodyKind {
    /// The kind of an existing [`Body`].
    pub fn of(body: &Body) -> Self {
        match body {
            Body::None => BodyKind::None,
            Body::Json(_) => BodyKind::Json,
            Body::Text(_) => BodyKind::Text,
            Body::Xml(_) => BodyKind::Xml,
            Body::Form(_) => BodyKind::Form,
        }
    }

    /// Every kind, in the order the body type selector lists them.
    pub const ALL: [BodyKind; 5] = [
        BodyKind::None,
        BodyKind::Json,
        BodyKind::Text,
        BodyKind::Xml,
        BodyKind::Form,
    ];

    /// The label shown in the body type selector.
    pub fn label(self) -> String {
        match self {
            BodyKind::None => t!("request.body.none").into_owned(),
            BodyKind::Json => "JSON".to_string(),
            BodyKind::Text => t!("request.body.text").into_owned(),
            BodyKind::Xml => "XML".to_string(),
            BodyKind::Form => t!("request.body.form_urlencoded").into_owned(),
        }
    }
}

/// Switches `request.body` to `kind`.
///
/// The raw text is preserved across the JSON, Text and XML kinds, since they are all plain
/// strings under the hood: switching from JSON to Text (for example, while writing a body that
/// is not valid JSON yet) does not lose what was typed. Switching to or from Form necessarily
/// starts empty (or restores whatever Form content there was, if switching back to Form without
/// having changed it), since a list of fields cannot be recovered from raw text.
pub fn set_body_kind(request: &mut Request, kind: BodyKind) {
    let existing_text = match &request.body {
        Body::Json(text) | Body::Text(text) | Body::Xml(text) => text.clone(),
        Body::None | Body::Form(_) => String::new(),
    };
    request.body = match kind {
        BodyKind::None => Body::None,
        BodyKind::Json => Body::Json(existing_text),
        BodyKind::Text => Body::Text(existing_text),
        BodyKind::Xml => Body::Xml(existing_text),
        BodyKind::Form => match &request.body {
            Body::Form(rows) => Body::Form(rows.clone()),
            _ => Body::Form(Vec::new()),
        },
    };
}

/// Replaces the raw text of a JSON, Text or XML body, keeping its current kind. Does nothing if
/// the body is currently `None` or `Form` (the body type selector must be used to switch kind
/// first).
pub fn set_body_text(request: &mut Request, text: String) {
    match &mut request.body {
        Body::Json(existing) | Body::Text(existing) | Body::Xml(existing) => *existing = text,
        Body::None | Body::Form(_) => {}
    }
}

/// Whether `token` is a valid custom HTTP method for the `.postino` file format: non-empty and
/// without a lowercase ASCII letter, the same rule `postino_format`'s parser enforces on a request
/// line's method token (`crates/postino-format/src/format.rs`'s `InvalidMethod` check). Not
/// imported from there directly: that check is private to the parser, and duplicating one boolean
/// condition here is simpler than exposing it as a new public API only this inline method editor
/// needs. Used by the request editor's "Custom..." method input to decide whether to commit an
/// edit.
pub fn is_valid_custom_method_token(token: &str) -> bool {
    !token.is_empty() && !token.chars().any(|c| c.is_ascii_lowercase())
}

/// Pretty-prints `text` as JSON with two-space indentation, for the request editor's "Format"
/// button. `None` if `text` is not valid JSON, in which case the caller leaves the body
/// unchanged (the button is only enabled for a JSON body, so this is a defensive fallback, not
/// an expected path).
pub fn format_json_body(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// Appends a new, empty, enabled row (used for the Params, Headers and Form key-value tables).
pub fn add_row(rows: &mut Vec<KeyValue>) {
    rows.push(KeyValue::new("", ""));
}

/// Removes the row at `index`. Does nothing if `index` is out of range.
pub fn remove_row(rows: &mut Vec<KeyValue>, index: usize) {
    if index < rows.len() {
        rows.remove(index);
    }
}

/// Rebuilds `request.url`'s query string from `request.query`, keeping everything before the
/// first `?` unchanged. Only enabled rows with a non-empty key are included, in table order.
///
/// This is the one direction of the Params/URL sync: the Params
/// table is the source of truth, and editing it rewrites the URL bar. Editing the URL bar
/// directly does not rewrite the table back, on purpose: the URL is free text and may contain a
/// `{{ }}` marker spanning what would otherwise be several parameters (for example
/// `?{{extraParams}}`), which a naive "split on `&`" parser would mangle. Keeping the sync
/// one-directional means typing in the URL bar is never fought by the app.
pub fn sync_url_from_query(request: &mut Request) {
    let base = request.url.split('?').next().unwrap_or("").to_string();
    let pairs: Vec<String> = request
        .query
        .iter()
        .filter(|row| row.enabled && !row.key.is_empty())
        .map(|row| format!("{}={}", row.key, row.value))
        .collect();
    request.url = if pairs.is_empty() {
        base
    } else {
        format!("{base}?{}", pairs.join("&"))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn kv(key: &str, value: &str) -> KeyValue {
        KeyValue::new(key, value)
    }

    #[test]
    fn body_kind_of_matches_every_variant() {
        assert_eq!(BodyKind::of(&Body::None), BodyKind::None);
        assert_eq!(BodyKind::of(&Body::Json(String::new())), BodyKind::Json);
        assert_eq!(BodyKind::of(&Body::Text(String::new())), BodyKind::Text);
        assert_eq!(BodyKind::of(&Body::Xml(String::new())), BodyKind::Xml);
        assert_eq!(BodyKind::of(&Body::Form(Vec::new())), BodyKind::Form);
    }

    #[test]
    fn set_body_kind_preserves_text_across_raw_kinds() {
        let mut request = Request {
            body: Body::Json("{}".to_string()),
            ..Request::default()
        };
        set_body_kind(&mut request, BodyKind::Xml);
        assert_eq!(request.body, Body::Xml("{}".to_string()));
    }

    #[test]
    fn set_body_kind_to_form_starts_empty_from_a_raw_body() {
        let mut request = Request {
            body: Body::Json("{}".to_string()),
            ..Request::default()
        };
        set_body_kind(&mut request, BodyKind::Form);
        assert_eq!(request.body, Body::Form(Vec::new()));
    }

    #[test]
    fn set_body_kind_to_form_and_back_keeps_form_rows() {
        let mut request = Request {
            body: Body::Form(vec![kv("a", "1")]),
            ..Request::default()
        };
        set_body_kind(&mut request, BodyKind::Form);
        assert_eq!(request.body, Body::Form(vec![kv("a", "1")]));
    }

    #[test]
    fn set_body_kind_to_none_drops_content() {
        let mut request = Request {
            body: Body::Text("hello".to_string()),
            ..Request::default()
        };
        set_body_kind(&mut request, BodyKind::None);
        assert_eq!(request.body, Body::None);
    }

    #[test]
    fn set_body_text_updates_a_raw_body() {
        let mut request = Request {
            body: Body::Json("{}".to_string()),
            ..Request::default()
        };
        set_body_text(&mut request, "{\"a\":1}".to_string());
        assert_eq!(request.body, Body::Json("{\"a\":1}".to_string()));
    }

    #[test]
    fn set_body_text_does_nothing_for_none_or_form() {
        let mut request = Request::default();
        set_body_text(&mut request, "ignored".to_string());
        assert_eq!(request.body, Body::None);
    }

    #[test]
    fn format_json_body_pretty_prints_valid_json() {
        assert_eq!(
            format_json_body(r#"{"a":1,"b":[1,2]}"#),
            Some("{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2\n  ]\n}".to_string())
        );
    }

    #[test]
    fn format_json_body_rejects_invalid_json() {
        assert_eq!(format_json_body("not json"), None);
    }

    #[test]
    fn is_valid_custom_method_token_accepts_an_uppercase_token() {
        assert!(is_valid_custom_method_token("PURGE"));
        assert!(is_valid_custom_method_token("X-CUSTOM2"));
    }

    #[test]
    fn is_valid_custom_method_token_rejects_empty() {
        assert!(!is_valid_custom_method_token(""));
    }

    #[test]
    fn is_valid_custom_method_token_rejects_lowercase() {
        assert!(!is_valid_custom_method_token("purge"));
        assert!(!is_valid_custom_method_token("Purge"));
    }

    #[test]
    fn add_row_appends_an_empty_enabled_row() {
        let mut rows = vec![kv("a", "1")];
        add_row(&mut rows);
        assert_eq!(rows, vec![kv("a", "1"), KeyValue::new("", "")]);
        assert!(rows[1].enabled);
    }

    #[test]
    fn remove_row_removes_the_given_index() {
        let mut rows = vec![kv("a", "1"), kv("b", "2")];
        remove_row(&mut rows, 0);
        assert_eq!(rows, vec![kv("b", "2")]);
    }

    #[test]
    fn remove_row_out_of_range_does_nothing() {
        let mut rows = vec![kv("a", "1")];
        remove_row(&mut rows, 5);
        assert_eq!(rows, vec![kv("a", "1")]);
    }

    #[test]
    fn sync_url_from_query_appends_enabled_rows() {
        let mut request = Request {
            url: "https://example.com/users".to_string(),
            query: vec![kv("page", "1"), kv("limit", "20")],
            ..Request::default()
        };
        sync_url_from_query(&mut request);
        assert_eq!(request.url, "https://example.com/users?page=1&limit=20");
    }

    #[test]
    fn sync_url_from_query_skips_disabled_and_empty_key_rows() {
        let mut disabled = kv("verbose", "true");
        disabled.enabled = false;
        let mut request = Request {
            url: "https://example.com/users".to_string(),
            query: vec![disabled, kv("", "ignored"), kv("page", "1")],
            ..Request::default()
        };
        sync_url_from_query(&mut request);
        assert_eq!(request.url, "https://example.com/users?page=1");
    }

    #[test]
    fn sync_url_from_query_replaces_an_existing_query_string() {
        let mut request = Request {
            url: "https://example.com/users?old=1".to_string(),
            query: vec![kv("page", "2")],
            ..Request::default()
        };
        sync_url_from_query(&mut request);
        assert_eq!(request.url, "https://example.com/users?page=2");
    }

    #[test]
    fn sync_url_from_query_with_no_rows_strips_the_query_string() {
        let mut request = Request {
            url: "https://example.com/users?old=1".to_string(),
            query: Vec::new(),
            ..Request::default()
        };
        sync_url_from_query(&mut request);
        assert_eq!(request.url, "https://example.com/users");
    }
}
