//! The body of a request.

use crate::key_value::KeyValue;

/// The body of a [`crate::Request`], as edited before interpolation.
///
/// Mirrors the `body <type>` section of the `.postino` format: `json`, `text` and `xml` are raw
/// text, `form` is a list of urlencoded fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Body {
    /// No body.
    #[default]
    None,
    /// A JSON body, kept as raw text so it can hold `{{ }}` placeholders that are not valid
    /// JSON on their own.
    Json(String),
    /// A plain text body.
    Text(String),
    /// An XML body.
    Xml(String),
    /// A `application/x-www-form-urlencoded` body.
    Form(Vec<KeyValue>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn default_is_none() {
        assert_eq!(Body::default(), Body::None);
    }
}
