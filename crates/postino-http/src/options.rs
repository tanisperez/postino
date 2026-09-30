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

    /// What to do when an `https://` server presents an invalid TLS certificate (self-signed,
    /// expired, for another host, ...). See [`InvalidCertificates`].
    ///
    /// Defaults to [`InvalidCertificates::SendWithWarning`].
    pub invalid_certificates: InvalidCertificates,
}

/// What [`crate::send`] does when the server's TLS certificate is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InvalidCertificates {
    /// Verify the certificate. When it is rejected, send the request once more without
    /// verification and set [`postino_core::Response::tls_warning`] on the response.
    #[default]
    SendWithWarning,
    /// Verify the certificate. When it is rejected, fail with an error and do not retry.
    Reject,
    /// Never verify the certificate: any certificate is accepted, with no warning.
    Accept,
}

impl Default for SendOptions {
    /// Thirty second timeout, redirects followed up to 10 times, and requests to servers with an
    /// invalid TLS certificate sent anyway with a warning.
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            follow_redirects: true,
            max_redirects: 10,
            invalid_certificates: InvalidCertificates::SendWithWarning,
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
        assert_eq!(
            options.invalid_certificates,
            InvalidCertificates::SendWithWarning
        );
    }
}
