//! [`OAuthError`], why a token could not be obtained.

use std::io;

use postino_http::HttpError;

use crate::store::StoreError;

/// A field of a profile, named in errors so the UI can point at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileField {
    /// The authorization URL.
    AuthUrl,
    /// The token URL.
    TokenUrl,
    /// The client id.
    ClientId,
    /// The client secret.
    ClientSecret,
    /// The scope.
    Scope,
    /// The user name.
    Username,
    /// The password.
    Password,
    /// The redirect URL.
    RedirectUrl,
    /// An extra parameter.
    ExtraParam,
}

impl ProfileField {
    /// The key of the field in the profile file.
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileField::AuthUrl => "auth_url",
            ProfileField::TokenUrl => "token_url",
            ProfileField::ClientId => "client_id",
            ProfileField::ClientSecret => "client_secret",
            ProfileField::Scope => "scope",
            ProfileField::Username => "username",
            ProfileField::Password => "password",
            ProfileField::RedirectUrl => "redirect_url",
            ProfileField::ExtraParam => "params",
        }
    }
}

/// Why a token could not be obtained. The messages are English, the UI shows them inside a
/// translated frame or translates the variants it knows.
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    /// The request names a profile the workspace does not have.
    #[error("there is no OAuth profile named \"{0}\"")]
    UnknownProfile(String),
    /// The profile is missing a field its grant needs.
    #[error("the profile has no {}", .0.as_str())]
    MissingField(ProfileField),
    /// A `{{ }}` marker of the profile did not resolve in the active environment.
    #[error("unknown variable {{{{{name}}}}} in {}", field.as_str())]
    UnresolvedVariable {
        /// The field that holds it.
        field: ProfileField,
        /// The variable name, or the whole marker for a function call.
        name: String,
    },
    /// The token endpoint answered with an OAuth error (RFC 6749, section 5.2).
    #[error("{error}{}", description.as_deref().map(|text| format!(": {text}")).unwrap_or_default())]
    Server {
        /// The HTTP status.
        status: u16,
        /// The `error` code, for example `invalid_client`.
        error: String,
        /// The `error_description`, when the server sent one.
        description: Option<String>,
    },
    /// The token endpoint answered with something that is not a token nor an OAuth error.
    #[error("the token endpoint answered {status} without an access token{}", detail_suffix(.detail))]
    InvalidResponse {
        /// The HTTP status.
        status: u16,
        /// What was wrong.
        detail: String,
    },
    /// The token request could not be sent.
    #[error(transparent)]
    Send(#[from] HttpError),
    /// The user has to sign in in the browser, and nothing can show it (the command line, a
    /// load test).
    #[error("the profile needs a browser sign-in, sign in from the Postino app first")]
    SignInRequired,
    /// The user cancelled the sign-in.
    #[error("sign-in cancelled")]
    Cancelled,
    /// The sign-in did not finish within its time limit.
    #[error("sign-in timed out after 5 minutes")]
    TimedOut,
    /// The authorization server redirected back with an error.
    #[error("{error}{}", description.as_deref().map(|text| format!(": {text}")).unwrap_or_default())]
    Denied {
        /// The `error` code, for example `access_denied` or `redirect_uri_mismatch`.
        error: String,
        /// The `error_description`, when there was one.
        description: Option<String>,
    },
    /// The redirect carried a `state` other than the one sent, a possible forgery.
    #[error("the redirect's state does not match the sign-in, ignored")]
    StateMismatch,
    /// A pasted redirect URL has no authorization code.
    #[error("the URL has no authorization code")]
    NoCode,
    /// Postino could not listen for the redirect.
    #[error("cannot listen on {address}: {source}")]
    Listen {
        /// The address.
        address: String,
        /// What failed.
        #[source]
        source: io::Error,
    },
    /// The fixed redirect URL is not a local `http://host:port/path` URL.
    #[error(
        "the redirect URL must be http://localhost:<port>/<path> or http://127.0.0.1:<port>/<path>"
    )]
    InvalidRedirectUrl,
    /// The token could not be read or saved.
    #[error(transparent)]
    Store(#[from] StoreError),
}

fn detail_suffix(detail: &str) -> String {
    if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    }
}

impl OAuthError {
    /// Whether this is the server rejecting a refresh token or code (`invalid_grant`), after
    /// which the grant is repeated from the start.
    pub fn is_invalid_grant(&self) -> bool {
        matches!(self, OAuthError::Server { error, .. } if error == "invalid_grant")
    }

    /// The OAuth error code, when the server or the redirect sent one.
    pub fn code(&self) -> Option<&str> {
        match self {
            OAuthError::Server { error, .. } | OAuthError::Denied { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn messages() {
        assert_eq!(
            OAuthError::Server {
                status: 401,
                error: "invalid_client".into(),
                description: None
            }
            .to_string(),
            "invalid_client"
        );
        assert_eq!(
            OAuthError::UnresolvedVariable {
                field: ProfileField::ClientSecret,
                name: "clientSecret".into()
            }
            .to_string(),
            "unknown variable {{clientSecret}} in client_secret"
        );
        assert_eq!(
            OAuthError::MissingField(ProfileField::TokenUrl).to_string(),
            "the profile has no token_url"
        );
        assert!(
            OAuthError::Server {
                status: 400,
                error: "invalid_grant".into(),
                description: Some("expired".into())
            }
            .is_invalid_grant()
        );
    }
}
