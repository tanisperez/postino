//! Turning a resolved request into copyable code, for the "Code" snippet generator
//! (`plans/ui-redesign.md`, phase 1b).
//!
//! The input is a [`ResolvedRequest`] (typically produced by `postino_runner::preview`), already
//! interpolated. A `{{ }}` marker left over from an undefined variable is not special-cased: it
//! is already part of the resolved text and is simply copied into the snippet as is.

use postino_core::{Method, ResolvedBody, ResolvedField, ResolvedRequest};

/// A target language for [`render_snippet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetLanguage {
    /// A `curl` command line.
    Curl,
    /// JavaScript using the `fetch` API.
    JsFetch,
    /// Python using the `requests` library.
    PythonRequests,
}

/// Renders `request` as a copyable code snippet in `language`.
pub fn render_snippet(request: &ResolvedRequest, language: SnippetLanguage) -> String {
    match language {
        SnippetLanguage::Curl => render_curl(request),
        SnippetLanguage::JsFetch => render_js_fetch(request),
        SnippetLanguage::PythonRequests => render_python_requests(request),
    }
}

/// The interpreted shape of a [`ResolvedBody`], used to decide how a snippet renders it.
///
/// [`ResolvedRequest`] only distinguishes "no body" from "body bytes": the field structure of a
/// form body is folded into a single urlencoded byte string by `postino-runner` (see
/// `crates/postino-runner/src/resolve.rs`). This type recovers that structure from the
/// `Content-Type` header, so a form body can still be rendered field by field; every other body
/// (json, text, xml) is kept as plain text.
enum BodyShape {
    /// No body.
    None,
    /// A `application/x-www-form-urlencoded` body, decoded back into its `(key, value)` fields,
    /// in order.
    Form(Vec<(String, String)>),
    /// Any other body, as text.
    Raw(String),
}

/// Classifies the body of `request`. See [`BodyShape`].
fn body_shape(request: &ResolvedRequest) -> BodyShape {
    match &request.body {
        ResolvedBody::None => BodyShape::None,
        ResolvedBody::Bytes(bytes) => {
            if is_form_urlencoded(&request.headers) {
                BodyShape::Form(decode_form_urlencoded(bytes))
            } else {
                BodyShape::Raw(String::from_utf8_lossy(bytes).into_owned())
            }
        }
    }
}

/// Whether `headers` declares a `Content-Type` of `application/x-www-form-urlencoded`.
fn is_form_urlencoded(headers: &[ResolvedField]) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("content-type")
            && header
                .value
                .to_ascii_lowercase()
                .contains("x-www-form-urlencoded")
    })
}

/// Decodes an `application/x-www-form-urlencoded` body back into its `(key, value)` fields, in
/// order: pairs split on `&`, each pair split on the first `=`, each part percent-decoded. A
/// malformed pair with no `=` is treated as a key with an empty value.
fn decode_form_urlencoded(bytes: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(bytes);
    if text.is_empty() {
        return Vec::new();
    }
    text.split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

/// Percent-decodes a single `application/x-www-form-urlencoded` component: `+` becomes a space,
/// `%XX` becomes the byte `XX`, everything else is copied verbatim. A malformed `%` sequence is
/// copied verbatim rather than rejected, since a snippet is best effort by nature.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Renders `request` as a `curl` command line, one flag per line with ` \` continuations.
///
/// `-X` is omitted for `GET`. A form body becomes one `--data-urlencode 'key=value'` flag per
/// field (curl re-encodes the value itself); any other body becomes a single `--data-raw` flag.
fn render_curl(request: &ResolvedRequest) -> String {
    let mut lines = Vec::new();
    if !matches!(request.method, Method::Get) {
        lines.push(format!("-X {}", request.method));
    }
    lines.push(shell_quote(&request.url));
    for header in &request.headers {
        lines.push(format!(
            "-H {}",
            shell_quote(&format!("{}: {}", header.name, header.value))
        ));
    }
    match body_shape(request) {
        BodyShape::None => {}
        BodyShape::Raw(text) => lines.push(format!("--data-raw {}", shell_quote(&text))),
        BodyShape::Form(fields) => {
            for (key, value) in fields {
                lines.push(format!(
                    "--data-urlencode {}",
                    shell_quote(&format!("{key}={value}"))
                ));
            }
        }
    }
    let mut out = String::from("curl \\\n");
    let last = lines.len().saturating_sub(1);
    for (index, line) in lines.into_iter().enumerate() {
        out.push_str("  ");
        out.push_str(&line);
        out.push_str(if index == last { "\n" } else { " \\\n" });
    }
    out.pop();
    out
}

/// Wraps `input` in single quotes for a POSIX shell, escaping an embedded single quote as
/// `'\''` (close the quote, an escaped literal quote, reopen the quote).
fn shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

/// Renders `request` as JavaScript using `fetch`.
///
/// `headers` is only emitted when non-empty; `body` is only emitted when the request has one. A
/// form body becomes `new URLSearchParams({...})`; any other body becomes a single string.
fn render_js_fetch(request: &ResolvedRequest) -> String {
    let mut out = String::new();
    out.push_str(&format!("await fetch({}, {{\n", js_string(&request.url)));
    out.push_str(&format!(
        "  method: {},\n",
        js_string(&request.method.to_string())
    ));
    if !request.headers.is_empty() {
        out.push_str("  headers: {\n");
        for header in &request.headers {
            out.push_str(&format!(
                "    {}: {},\n",
                js_string(&header.name),
                js_string(&header.value)
            ));
        }
        out.push_str("  },\n");
    }
    match body_shape(request) {
        BodyShape::None => {}
        BodyShape::Raw(text) => out.push_str(&format!("  body: {},\n", js_string(&text))),
        BodyShape::Form(fields) => {
            out.push_str("  body: new URLSearchParams({\n");
            for (key, value) in fields {
                out.push_str(&format!(
                    "    {}: {},\n",
                    js_string(&key),
                    js_string(&value)
                ));
            }
            out.push_str("  }),\n");
        }
    }
    out.push_str("});");
    out
}

/// Renders `input` as a double-quoted JavaScript string literal.
fn js_string(input: &str) -> String {
    format!("\"{}\"", escape_double_quoted(input))
}

/// Renders `request` as Python using the `requests` library.
///
/// `headers` is only emitted when non-empty; `data` is only emitted when the request has a body.
/// A form body becomes a `dict` of its fields; any other body becomes a single string.
fn render_python_requests(request: &ResolvedRequest) -> String {
    let mut out = String::from("import requests\n\n");
    out.push_str("response = requests.request(\n");
    out.push_str(&format!(
        "    {},\n",
        python_string(&request.method.to_string())
    ));
    out.push_str(&format!("    {},\n", python_string(&request.url)));
    if !request.headers.is_empty() {
        out.push_str("    headers={\n");
        for header in &request.headers {
            out.push_str(&format!(
                "        {}: {},\n",
                python_string(&header.name),
                python_string(&header.value)
            ));
        }
        out.push_str("    },\n");
    }
    match body_shape(request) {
        BodyShape::None => {}
        BodyShape::Raw(text) => out.push_str(&format!("    data={},\n", python_string(&text))),
        BodyShape::Form(fields) => {
            out.push_str("    data={\n");
            for (key, value) in fields {
                out.push_str(&format!(
                    "        {}: {},\n",
                    python_string(&key),
                    python_string(&value)
                ));
            }
            out.push_str("    },\n");
        }
    }
    out.push_str(")\n");
    out
}

/// Renders `input` as a double-quoted Python string literal.
fn python_string(input: &str) -> String {
    format!("\"{}\"", escape_double_quoted(input))
}

/// Escapes `input` for use inside a double-quoted string literal, JavaScript or Python: a
/// backslash, a double quote, and the common control characters (newline, carriage return, tab).
fn escape_double_quoted(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn get_request(url: &str) -> ResolvedRequest {
        ResolvedRequest {
            method: Method::Get,
            url: url.to_string(),
            headers: Vec::new(),
            body: ResolvedBody::None,
        }
    }

    fn json_post_request() -> ResolvedRequest {
        ResolvedRequest {
            method: Method::Post,
            url: "https://example.com/users".to_string(),
            headers: vec![ResolvedField {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            }],
            body: ResolvedBody::Bytes(br#"{"id": 1}"#.to_vec()),
        }
    }

    fn form_post_request() -> ResolvedRequest {
        ResolvedRequest {
            method: Method::Post,
            url: "https://example.com/users".to_string(),
            headers: vec![ResolvedField {
                name: "Content-Type".to_string(),
                value: "application/x-www-form-urlencoded".to_string(),
            }],
            body: ResolvedBody::Bytes(b"a=1&b=2".to_vec()),
        }
    }

    #[test]
    fn curl_renders_get_request_without_dash_x_or_body() {
        let snippet = render_snippet(
            &get_request("https://example.com/users"),
            SnippetLanguage::Curl,
        );
        assert_eq!(snippet, "curl \\\n  'https://example.com/users'");
    }

    #[test]
    fn curl_renders_post_json_body() {
        let snippet = render_snippet(&json_post_request(), SnippetLanguage::Curl);
        assert_eq!(
            snippet,
            "curl \\\n  -X POST \\\n  'https://example.com/users' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"id\": 1}'"
        );
    }

    #[test]
    fn curl_renders_form_body_as_data_urlencode_per_field() {
        let snippet = render_snippet(&form_post_request(), SnippetLanguage::Curl);
        assert_eq!(
            snippet,
            "curl \\\n  -X POST \\\n  'https://example.com/users' \\\n  -H 'Content-Type: application/x-www-form-urlencoded' \\\n  --data-urlencode 'a=1' \\\n  --data-urlencode 'b=2'"
        );
    }

    #[test]
    fn curl_escapes_a_single_quote_in_the_body() {
        let request = ResolvedRequest {
            method: Method::Post,
            url: "https://example.com/echo".to_string(),
            headers: Vec::new(),
            body: ResolvedBody::Bytes(b"It's a test".to_vec()),
        };
        let snippet = render_snippet(&request, SnippetLanguage::Curl);
        assert_eq!(
            snippet,
            "curl \\\n  -X POST \\\n  'https://example.com/echo' \\\n  --data-raw 'It'\\''s a test'"
        );
    }

    #[test]
    fn curl_escapes_a_single_quote_in_a_header_value() {
        let request = ResolvedRequest {
            method: Method::Get,
            url: "https://example.com/users".to_string(),
            headers: vec![ResolvedField {
                name: "X-Author".to_string(),
                value: "O'Reilly".to_string(),
            }],
            body: ResolvedBody::None,
        };
        let snippet = render_snippet(&request, SnippetLanguage::Curl);
        assert_eq!(
            snippet,
            "curl \\\n  'https://example.com/users' \\\n  -H 'X-Author: O'\\''Reilly'"
        );
    }

    #[test]
    fn curl_includes_dash_x_for_a_custom_method() {
        let request = ResolvedRequest {
            method: Method::Custom("PURGE".to_string()),
            url: "https://example.com/cache".to_string(),
            headers: Vec::new(),
            body: ResolvedBody::None,
        };
        let snippet = render_snippet(&request, SnippetLanguage::Curl);
        assert_eq!(
            snippet,
            "curl \\\n  -X PURGE \\\n  'https://example.com/cache'"
        );
    }

    #[test]
    fn fetch_renders_get_request_without_headers_or_body() {
        let snippet = render_snippet(
            &get_request("https://example.com/users"),
            SnippetLanguage::JsFetch,
        );
        assert_eq!(
            snippet,
            "await fetch(\"https://example.com/users\", {\n  method: \"GET\",\n});"
        );
    }

    #[test]
    fn fetch_renders_post_json_body_with_header() {
        let snippet = render_snippet(&json_post_request(), SnippetLanguage::JsFetch);
        assert_eq!(
            snippet,
            "await fetch(\"https://example.com/users\", {\n  method: \"POST\",\n  headers: {\n    \"Content-Type\": \"application/json\",\n  },\n  body: \"{\\\"id\\\": 1}\",\n});"
        );
    }

    #[test]
    fn fetch_renders_form_body_as_url_search_params() {
        let snippet = render_snippet(&form_post_request(), SnippetLanguage::JsFetch);
        assert_eq!(
            snippet,
            "await fetch(\"https://example.com/users\", {\n  method: \"POST\",\n  headers: {\n    \"Content-Type\": \"application/x-www-form-urlencoded\",\n  },\n  body: new URLSearchParams({\n    \"a\": \"1\",\n    \"b\": \"2\",\n  }),\n});"
        );
    }

    #[test]
    fn fetch_escapes_a_double_quote_in_a_header_value() {
        let request = ResolvedRequest {
            method: Method::Get,
            url: "https://example.com/users".to_string(),
            headers: vec![ResolvedField {
                name: "X-Note".to_string(),
                value: "He said \"hi\"".to_string(),
            }],
            body: ResolvedBody::None,
        };
        let snippet = render_snippet(&request, SnippetLanguage::JsFetch);
        assert_eq!(
            snippet,
            "await fetch(\"https://example.com/users\", {\n  method: \"GET\",\n  headers: {\n    \"X-Note\": \"He said \\\"hi\\\"\",\n  },\n});"
        );
    }

    #[test]
    fn fetch_uses_the_custom_method() {
        let request = ResolvedRequest {
            method: Method::Custom("PURGE".to_string()),
            url: "https://example.com/cache".to_string(),
            headers: Vec::new(),
            body: ResolvedBody::None,
        };
        let snippet = render_snippet(&request, SnippetLanguage::JsFetch);
        assert_eq!(
            snippet,
            "await fetch(\"https://example.com/cache\", {\n  method: \"PURGE\",\n});"
        );
    }

    #[test]
    fn python_renders_get_request_without_headers_or_body() {
        let snippet = render_snippet(
            &get_request("https://example.com/users"),
            SnippetLanguage::PythonRequests,
        );
        assert_eq!(
            snippet,
            "import requests\n\nresponse = requests.request(\n    \"GET\",\n    \"https://example.com/users\",\n)\n"
        );
    }

    #[test]
    fn python_renders_post_json_body_with_header() {
        let snippet = render_snippet(&json_post_request(), SnippetLanguage::PythonRequests);
        assert_eq!(
            snippet,
            "import requests\n\nresponse = requests.request(\n    \"POST\",\n    \"https://example.com/users\",\n    headers={\n        \"Content-Type\": \"application/json\",\n    },\n    data=\"{\\\"id\\\": 1}\",\n)\n"
        );
    }

    #[test]
    fn python_renders_form_body_as_a_dict() {
        let snippet = render_snippet(&form_post_request(), SnippetLanguage::PythonRequests);
        assert_eq!(
            snippet,
            "import requests\n\nresponse = requests.request(\n    \"POST\",\n    \"https://example.com/users\",\n    headers={\n        \"Content-Type\": \"application/x-www-form-urlencoded\",\n    },\n    data={\n        \"a\": \"1\",\n        \"b\": \"2\",\n    },\n)\n"
        );
    }

    #[test]
    fn decode_form_urlencoded_reverses_the_form_encoding_used_when_resolving() {
        assert_eq!(
            decode_form_urlencoded(b"first+name=Jane+Doe&note=100%25"),
            vec![
                ("first name".to_string(), "Jane Doe".to_string()),
                ("note".to_string(), "100%".to_string()),
            ]
        );
    }
}
