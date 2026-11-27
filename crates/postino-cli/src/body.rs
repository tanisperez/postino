//! Writing a response body to stdout (#296).
//!
//! Redirected to a file or a pipe, the body goes out byte for byte. On a terminal it goes out as
//! UTF-8 text: the Windows console refuses anything else ("Windows stdio in console mode does not
//! support writing non UTF-8 byte sequences"), and a UTF-8 terminal elsewhere would show a Latin-1
//! page as garbage. A body in UTF-8 is written unchanged; one whose `Content-Type` names a Latin-1
//! charset (`ISO-8859-1`, `windows-1252`, `us-ascii`) is decoded as Latin-1; anything else keeps
//! its valid UTF-8 and shows U+FFFD in place of the rest.

use std::borrow::Cow;
use std::io::{IsTerminal, Write};

use postino_core::Response;

/// Writes `response`'s body to `out`, as text when stdout is a terminal (see the module docs).
pub(crate) fn write_body(out: &mut dyn Write, response: &Response) -> std::io::Result<()> {
    write_body_to(out, response, std::io::stdout().is_terminal())
}

fn write_body_to(out: &mut dyn Write, response: &Response, terminal: bool) -> std::io::Result<()> {
    if terminal {
        out.write_all(terminal_text(&response.body, content_type(response)).as_bytes())
    } else {
        out.write_all(&response.body)
    }
}

fn content_type(response: &Response) -> Option<&str> {
    response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
}

/// The charsets that are Latin-1 for printing purposes. `windows-1252` differs only in
/// 0x80 to 0x9F, printable punctuation there and control codes in Latin-1, close enough.
const LATIN1: [&str; 6] = [
    "iso-8859-1",
    "iso8859-1",
    "latin1",
    "latin-1",
    "windows-1252",
    "us-ascii",
];

fn terminal_text<'a>(body: &'a [u8], content_type: Option<&str>) -> Cow<'a, str> {
    if let Ok(text) = std::str::from_utf8(body) {
        return Cow::Borrowed(text);
    }
    let latin1 = content_type
        .and_then(charset)
        .is_some_and(|charset| LATIN1.iter().any(|name| charset.eq_ignore_ascii_case(name)));
    if latin1 {
        Cow::Owned(body.iter().map(|&byte| char::from(byte)).collect())
    } else {
        String::from_utf8_lossy(body)
    }
}

/// The `charset` parameter of a `Content-Type` value, without quotes.
fn charset(content_type: &str) -> Option<&str> {
    content_type.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches('"'))
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_core::ResolvedField;

    fn response(content_type: &str, body: &[u8]) -> Response {
        Response {
            status: 200,
            headers: vec![ResolvedField {
                name: "Content-Type".to_string(),
                value: content_type.to_string(),
            }],
            body: body.to_vec(),
            time: std::time::Duration::ZERO,
            size: body.len(),
            tls_warning: None,
            legacy_tls: false,
        }
    }

    fn written(response: &Response, terminal: bool) -> Vec<u8> {
        let mut out = Vec::new();
        write_body_to(&mut out, response, terminal).expect("write");
        out
    }

    #[test]
    fn a_latin1_page_is_decoded_on_a_terminal() {
        let page = response("text/html; charset=ISO-8859-1", b"Espa\xf1a");
        assert_eq!(written(&page, true), "España".as_bytes());
    }

    #[test]
    fn a_redirected_body_keeps_its_bytes() {
        let page = response("text/html; charset=ISO-8859-1", b"Espa\xf1a");
        assert_eq!(written(&page, false), b"Espa\xf1a");
    }

    #[test]
    fn utf8_is_written_unchanged_and_unknown_bytes_are_replaced() {
        let utf8 = response("application/json", "{\"a\":\"ñ\"}".as_bytes());
        assert_eq!(written(&utf8, true), "{\"a\":\"ñ\"}".as_bytes());
        let binary = response("application/octet-stream", b"a\xffb");
        assert_eq!(written(&binary, true), "a\u{fffd}b".as_bytes());
    }

    #[test]
    fn the_charset_parameter_is_found_anywhere_and_unquoted() {
        assert_eq!(
            charset("text/plain; format=flowed; Charset=\"windows-1252\""),
            Some("windows-1252")
        );
        assert_eq!(charset("text/plain"), None);
    }
}
