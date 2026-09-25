//! Options controlling how [`crate::send`] performs a single request.

use std::time::Duration;

/// Options for [`crate::send`].
///
/// The defaults match what most HTTP clients do out of the box: a generous timeout, redirects
/// followed up to a sane limit, and TLS certificates verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOptions {
    /// The maximum time to wait for the whole request, from opening the connection to reading
    /// the last byte of the response body. This includes time spent following redirects.
    ///
    /// Defaults to 30 seconds.
    pub timeout: Duration,

    /// Whether to follow HTTP redirects (3xx responses with a `Location` header).
    ///
    /// When `false`, the redirect response itself is returned as a normal
    /// [`postino_core::Response`], instead of being followed.
    ///
    /// Defaults to `true`.
    pub follow_redirects: bool,

    /// The maximum number of redirects to follow before giving up with
    /// [`crate::HttpError::TooManyRedirects`]. Ignored when `follow_redirects` is `false`.
    ///
    /// Defaults to 10.
    pub max_redirects: u32,

    /// Whether to verify the server's TLS certificate for `https://` requests.
    ///
    /// Turning this off accepts any certificate, including self-signed or expired ones. It
    /// should only be used for testing against a known server.
    ///
    /// Defaults to `true`.
    pub verify_tls: bool,
}

impl Default for SendOptions {
    /// Thirty second timeout, redirects followed up to 10 times, TLS certificates verified.
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            follow_redirects: true,
            max_redirects: 10,
            verify_tls: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn defaults_match_the_documented_values() {
        let options = SendOptions::default();
        assert_eq!(options.timeout, Duration::from_secs(30));
        assert!(options.follow_redirects);
        assert_eq!(options.max_redirects, 10);
        assert!(options.verify_tls);
    }
}
