//! Turning a [`Request`] into a [`ResolvedRequest`] ready to send: interpolating every `{{ }}`
//! marker, keeping only enabled headers, query and form entries, appending the query string to
//! the URL and setting a default `Content-Type` when the body implies one.

use postino_core::{
    Body, KeyValue, Request, ResolvedBody, ResolvedField, ResolvedRequest, TemplateWarning,
    VarScope, functions, interpolate,
};

/// Interpolates `request` into a [`ResolvedRequest`], collecting every [`TemplateWarning`] found
/// along the way. This never fails: an unresolved marker is left as is in the output and reported
/// as a warning instead.
pub(crate) fn resolve(
    request: &Request,
    scope: &VarScope,
) -> (ResolvedRequest, Vec<TemplateWarning>) {
    let mut warnings = Vec::new();

    let interpolated_url = interpolate(&request.url, scope);
    warnings.extend(interpolated_url.warnings);
    let url = append_query(interpolated_url.text, &request.query, scope, &mut warnings);

    let headers = resolve_fields(&request.headers, scope, &mut warnings);
    let body = resolve_body(&request.body, scope, &mut warnings);
    let headers = apply_default_content_type(headers, &request.body);

    let resolved = ResolvedRequest {
        method: request.method.clone(),
        url,
        headers,
        body,
    };
    (resolved, warnings)
}

/// Interpolates the enabled entries of `fields` (both key and value), skipping disabled ones.
fn resolve_fields(
    fields: &[KeyValue],
    scope: &VarScope,
    warnings: &mut Vec<TemplateWarning>,
) -> Vec<ResolvedField> {
    fields
        .iter()
        .filter(|field| field.enabled)
        .map(|field| {
            let name = interpolate(&field.key, scope);
            warnings.extend(name.warnings);
            let value = interpolate(&field.value, scope);
            warnings.extend(value.warnings);
            ResolvedField {
                name: name.text,
                value: value.text,
            }
        })
        .collect()
}

/// Interpolates the enabled `::: query` entries and appends them to `base_url` as a query
/// string, percent-encoding each key and value with [`functions::url_encode`]. Appends with `&`
/// when `base_url` already has a query string (contains `?`), with a leading `?` otherwise.
/// Returns `base_url` unchanged when there are no enabled query entries.
fn append_query(
    base_url: String,
    query: &[KeyValue],
    scope: &VarScope,
    warnings: &mut Vec<TemplateWarning>,
) -> String {
    let pairs: Vec<(String, String)> = query
        .iter()
        .filter(|entry| entry.enabled)
        .map(|entry| {
            let key = interpolate(&entry.key, scope);
            warnings.extend(key.warnings);
            let value = interpolate(&entry.value, scope);
            warnings.extend(value.warnings);
            (key.text, value.text)
        })
        .collect();

    if pairs.is_empty() {
        return base_url;
    }

    let encoded = pairs
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                functions::url_encode(key),
                functions::url_encode(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    let separator = if base_url.contains('?') { '&' } else { '?' };
    format!("{base_url}{separator}{encoded}")
}

/// Interpolates the body and encodes it to the bytes actually sent.
fn resolve_body(
    body: &Body,
    scope: &VarScope,
    warnings: &mut Vec<TemplateWarning>,
) -> ResolvedBody {
    match body {
        Body::None => ResolvedBody::None,
        Body::Json(text) | Body::Text(text) | Body::Xml(text) => {
            let interpolated = interpolate(text, scope);
            warnings.extend(interpolated.warnings);
            ResolvedBody::Bytes(interpolated.text.into_bytes())
        }
        Body::Form(fields) => {
            let pairs: Vec<(String, String)> = fields
                .iter()
                .filter(|field| field.enabled)
                .map(|field| {
                    let key = interpolate(&field.key, scope);
                    warnings.extend(key.warnings);
                    let value = interpolate(&field.value, scope);
                    warnings.extend(value.warnings);
                    (key.text, value.text)
                })
                .collect();
            let encoded = pairs
                .iter()
                .map(|(key, value)| format!("{}={}", form_encode(key), form_encode(value)))
                .collect::<Vec<_>>()
                .join("&");
            ResolvedBody::Bytes(encoded.into_bytes())
        }
    }
}

/// Percent-encodes a string for an `application/x-www-form-urlencoded` body: like
/// [`functions::url_encode`], but a space becomes `+` rather than `%20`, the historical
/// convention every HTTP server expects for this content type.
fn form_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Sets a default `Content-Type` header when the body implies one (`json`, `xml`, `form`) and
/// the user did not already set one (a case-insensitive match against the enabled headers).
fn apply_default_content_type(mut headers: Vec<ResolvedField>, body: &Body) -> Vec<ResolvedField> {
    let Some(default) = default_content_type(body) else {
        return headers;
    };
    let already_set = headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("content-type"));
    if !already_set {
        headers.push(ResolvedField {
            name: "Content-Type".to_string(),
            value: default.to_string(),
        });
    }
    headers
}

/// The default `Content-Type` for a body type, or `None` if it should not get one. Only
/// `json`, `xml` and `form` bodies get a default; `text` does
/// not, since there is no single sensible guess for its content type.
fn default_content_type(body: &Body) -> Option<&'static str> {
    match body {
        Body::Json(_) => Some("application/json"),
        Body::Xml(_) => Some("application/xml"),
        Body::Form(_) => Some("application/x-www-form-urlencoded"),
        Body::Text(_) | Body::None => None,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use postino_core::Method;
    use pretty_assertions::assert_eq;

    fn scope() -> VarScope<'static> {
        VarScope::default()
    }

    fn base_request(url: &str) -> Request {
        Request {
            method: Method::Get,
            url: url.to_string(),
            ..Request::default()
        }
    }

    #[test]
    fn disabled_headers_are_skipped() {
        let mut request = base_request("http://example.com");
        request.headers.push(KeyValue::new("X-Enabled", "yes"));
        let mut disabled = KeyValue::new("X-Disabled", "no");
        disabled.enabled = false;
        request.headers.push(disabled);

        let (resolved, warnings) = resolve(&request, &scope());
        assert!(warnings.is_empty());
        assert_eq!(
            resolved.headers,
            vec![ResolvedField {
                name: "X-Enabled".to_string(),
                value: "yes".to_string(),
            }]
        );
    }

    #[test]
    fn disabled_query_entries_are_skipped() {
        let mut request = base_request("http://example.com");
        request.query.push(KeyValue::new("page", "1"));
        let mut disabled = KeyValue::new("verbose", "true");
        disabled.enabled = false;
        request.query.push(disabled);

        let (resolved, _) = resolve(&request, &scope());
        assert_eq!(resolved.url, "http://example.com?page=1");
    }

    #[test]
    fn query_is_appended_to_a_bare_url() {
        let mut request = base_request("http://example.com/path");
        request.query.push(KeyValue::new("page", "1"));

        let (resolved, _) = resolve(&request, &scope());
        assert_eq!(resolved.url, "http://example.com/path?page=1");
    }

    #[test]
    fn query_is_appended_to_a_url_that_already_has_a_query_string() {
        let mut request = base_request("http://example.com/path?existing=1");
        request.query.push(KeyValue::new("page", "2"));

        let (resolved, _) = resolve(&request, &scope());
        assert_eq!(resolved.url, "http://example.com/path?existing=1&page=2");
    }

    #[test]
    fn query_keys_and_values_are_percent_encoded() {
        let mut request = base_request("http://example.com");
        request.query.push(KeyValue::new("a b", "c&d"));

        let (resolved, _) = resolve(&request, &scope());
        assert_eq!(resolved.url, "http://example.com?a%20b=c%26d");
    }

    #[test]
    fn json_body_gets_a_default_content_type_when_missing() {
        let mut request = base_request("http://example.com");
        request.body = Body::Json("{}".to_string());

        let (resolved, _) = resolve(&request, &scope());
        assert!(
            resolved
                .headers
                .iter()
                .any(|header| header.name == "Content-Type" && header.value == "application/json")
        );
    }

    #[test]
    fn json_body_keeps_a_user_set_content_type() {
        let mut request = base_request("http://example.com");
        request.headers.push(KeyValue::new(
            "content-type",
            "application/json; charset=utf-8",
        ));
        request.body = Body::Json("{}".to_string());

        let (resolved, _) = resolve(&request, &scope());
        let content_types: Vec<&ResolvedField> = resolved
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case("content-type"))
            .collect();
        assert_eq!(content_types.len(), 1);
        assert_eq!(content_types[0].value, "application/json; charset=utf-8");
    }

    #[test]
    fn xml_body_gets_a_default_content_type() {
        let mut request = base_request("http://example.com");
        request.body = Body::Xml("<a/>".to_string());

        let (resolved, _) = resolve(&request, &scope());
        assert!(
            resolved
                .headers
                .iter()
                .any(|header| header.name == "Content-Type" && header.value == "application/xml")
        );
    }

    #[test]
    fn text_body_gets_no_default_content_type() {
        let mut request = base_request("http://example.com");
        request.body = Body::Text("hello".to_string());

        let (resolved, _) = resolve(&request, &scope());
        assert!(
            !resolved
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("content-type"))
        );
    }

    #[test]
    fn no_body_gets_no_default_content_type() {
        let request = base_request("http://example.com");

        let (resolved, _) = resolve(&request, &scope());
        assert!(
            !resolved
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("content-type"))
        );
    }

    #[test]
    fn form_body_is_urlencoded_with_a_default_content_type() {
        let mut request = base_request("http://example.com");
        request.body = Body::Form(vec![
            KeyValue::new("name", "a b"),
            KeyValue::new("id", "1&2"),
        ]);

        let (resolved, _) = resolve(&request, &scope());
        let ResolvedBody::Bytes(bytes) = &resolved.body else {
            panic!("expected a body");
        };
        assert_eq!(String::from_utf8_lossy(bytes), "name=a+b&id=1%262");
        assert!(
            resolved
                .headers
                .iter()
                .any(|header| header.name == "Content-Type"
                    && header.value == "application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn disabled_form_fields_are_skipped() {
        let mut request = base_request("http://example.com");
        let mut disabled = KeyValue::new("secret", "shh");
        disabled.enabled = false;
        request.body = Body::Form(vec![KeyValue::new("a", "1"), disabled]);

        let (resolved, _) = resolve(&request, &scope());
        let ResolvedBody::Bytes(bytes) = &resolved.body else {
            panic!("expected a body");
        };
        assert_eq!(String::from_utf8_lossy(bytes), "a=1");
    }

    #[test]
    fn missing_variable_in_the_url_is_reported() {
        let request = base_request("http://example.com/{{missing}}");

        let (resolved, warnings) = resolve(&request, &scope());
        assert_eq!(resolved.url, "http://example.com/{{missing}}");
        assert_eq!(
            warnings,
            vec![TemplateWarning::UnknownVariable("missing".to_string())]
        );
    }
}
