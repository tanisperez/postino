//! The editable request model, as stored in a `.postino` file.

use crate::body::Body;
use crate::key_value::KeyValue;
use crate::method::Method;

/// A request as edited by the user and stored in a `.postino` file.
///
/// This is the "source" form, before variable interpolation. See [`crate::ResolvedRequest`] for
/// the form actually sent over the wire.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Request {
    /// The HTTP method.
    pub method: Method,
    /// The request URL, may contain `{{ }}` placeholders and a query string.
    pub url: String,
    /// The header list, in file order. Disabled headers are kept but not sent.
    pub headers: Vec<KeyValue>,
    /// The `::: query` section, in file order. Disabled entries are kept but not sent.
    pub query: Vec<KeyValue>,
    /// The request body.
    pub body: Body,
    /// The `::: pre` section, raw JavaScript. Empty when absent.
    pub pre_script: String,
    /// The `::: post` section, raw JavaScript. Empty when absent.
    pub post_script: String,
    /// The `::: docs` section, free Markdown notes. Empty when absent.
    pub docs: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn default_is_a_bare_get_request() {
        let request = Request::default();
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.url, "");
        assert!(request.headers.is_empty());
        assert!(request.query.is_empty());
        assert_eq!(request.body, Body::None);
        assert_eq!(request.pre_script, "");
        assert_eq!(request.post_script, "");
        assert_eq!(request.docs, "");
    }
}
