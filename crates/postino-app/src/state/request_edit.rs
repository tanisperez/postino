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

/// What the body kinds a request is not showing held, while its tab is open, so switching the
/// body type and back restores them instead of starting empty. Never saved: only the body kind
/// selected when saving is written to the file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyStash {
    /// The raw text of the last JSON, Text or XML body.
    text: Option<String>,
    /// The fields of the last Form body.
    form: Option<Vec<KeyValue>>,
}

/// Switches `request.body` to `kind`, putting what the current body holds in `stash` and
/// restoring what `stash` holds for `kind`.
///
/// JSON, Text and XML share one raw text, since they are all plain strings under the hood:
/// switching from JSON to Text (for example, while writing a body that is not valid JSON yet)
/// keeps what was typed. Form fields cannot be recovered from raw text nor the other way round,
/// so each keeps its own content in `stash`: switching from Form to JSON and back brings the
/// fields back, and from JSON to None and back brings the text back.
pub fn set_body_kind(request: &mut Request, kind: BodyKind, stash: &mut BodyStash) {
    match std::mem::take(&mut request.body) {
        Body::Json(text) | Body::Text(text) | Body::Xml(text) => stash.text = Some(text),
        Body::Form(rows) => stash.form = Some(rows),
        Body::None => {}
    }
    request.body = match kind {
        BodyKind::None => Body::None,
        BodyKind::Json => Body::Json(stash.text.take().unwrap_or_default()),
        BodyKind::Text => Body::Text(stash.text.take().unwrap_or_default()),
        BodyKind::Xml => Body::Xml(stash.text.take().unwrap_or_default()),
        BodyKind::Form => Body::Form(stash.form.take().unwrap_or_default()),
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

/// Pretty-prints `text` as JSON with `indent` spaces per level (the `json_indent` setting), for
/// the request editor's "Format" button. The same formatter as the response's Pretty view, so
/// every token stays as it was typed (key order, number spellings, escapes). `None` if `text` is
/// not valid JSON (a body with `{{ }}` markers outside strings, for example), in which case the
/// caller leaves the body unchanged.
pub fn format_json_body(text: &str, indent: usize) -> Option<String> {
    super::json_pretty::pretty_print_json(text.as_bytes(), indent)
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
        set_body_kind(&mut request, BodyKind::Xml, &mut BodyStash::default());
        assert_eq!(request.body, Body::Xml("{}".to_string()));
    }

    #[test]
    fn set_body_kind_to_form_starts_empty_from_a_raw_body() {
        let mut request = Request {
            body: Body::Json("{}".to_string()),
            ..Request::default()
        };
        set_body_kind(&mut request, BodyKind::Form, &mut BodyStash::default());
        assert_eq!(request.body, Body::Form(Vec::new()));
    }

    #[test]
    fn set_body_kind_to_form_and_back_keeps_form_rows() {
        let mut request = Request {
            body: Body::Form(vec![kv("a", "1")]),
            ..Request::default()
        };
        set_body_kind(&mut request, BodyKind::Form, &mut BodyStash::default());
        assert_eq!(request.body, Body::Form(vec![kv("a", "1")]));
    }

    #[test]
    fn set_body_kind_to_none_drops_content() {
        let mut request = Request {
            body: Body::Text("hello".to_string()),
            ..Request::default()
        };
        set_body_kind(&mut request, BodyKind::None, &mut BodyStash::default());
        assert_eq!(request.body, Body::None);
    }

    #[test]
    fn switching_from_form_and_back_restores_the_fields() {
        let mut request = Request {
            body: Body::Form(vec![kv("a", "1"), kv("b", "2")]),
            ..Request::default()
        };
        let mut stash = BodyStash::default();
        set_body_kind(&mut request, BodyKind::Json, &mut stash);
        assert_eq!(request.body, Body::Json(String::new()));
        set_body_kind(&mut request, BodyKind::Form, &mut stash);
        assert_eq!(request.body, Body::Form(vec![kv("a", "1"), kv("b", "2")]));
    }

    #[test]
    fn switching_to_form_and_back_restores_the_text() {
        let mut request = Request {
            body: Body::Json("{\"a\": 1}".to_string()),
            ..Request::default()
        };
        let mut stash = BodyStash::default();
        set_body_kind(&mut request, BodyKind::Form, &mut stash);
        set_body_kind(&mut request, BodyKind::Text, &mut stash);
        assert_eq!(request.body, Body::Text("{\"a\": 1}".to_string()));
    }

    #[test]
    fn switching_to_none_and_back_restores_the_text() {
        let mut request = Request {
            body: Body::Xml("<a/>".to_string()),
            ..Request::default()
        };
        let mut stash = BodyStash::default();
        set_body_kind(&mut request, BodyKind::None, &mut stash);
        assert_eq!(request.body, Body::None);
        set_body_kind(&mut request, BodyKind::Xml, &mut stash);
        assert_eq!(request.body, Body::Xml("<a/>".to_string()));
    }

    #[test]
    fn the_stash_follows_the_latest_edit() {
        let mut request = Request {
            body: Body::Form(vec![kv("a", "1")]),
            ..Request::default()
        };
        let mut stash = BodyStash::default();
        set_body_kind(&mut request, BodyKind::Json, &mut stash);
        set_body_kind(&mut request, BodyKind::Form, &mut stash);
        request.body = Body::Form(vec![kv("b", "2")]);
        set_body_kind(&mut request, BodyKind::None, &mut stash);
        set_body_kind(&mut request, BodyKind::Form, &mut stash);
        assert_eq!(request.body, Body::Form(vec![kv("b", "2")]));
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
            format_json_body(r#"{"a":1,"b":[1,2]}"#, 2),
            Some("{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2\n  ]\n}".to_string())
        );
    }

    #[test]
    fn format_json_body_uses_the_given_indentation_and_keeps_tokens() {
        assert_eq!(
            format_json_body(r#"{"z":1.0e3,"a":"\u00e9"}"#, 4),
            Some("{\n    \"z\": 1.0e3,\n    \"a\": \"\\u00e9\"\n}".to_string())
        );
    }

    #[test]
    fn format_json_body_rejects_invalid_json() {
        assert_eq!(format_json_body("not json", 2), None);
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
}
