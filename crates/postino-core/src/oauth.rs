//! OAuth 2.0 profiles: the settings of one authorization server and client, written once in the
//! workspace (`[oauth.<name>]` in `.postino/project.toml`) and referenced by name from every
//! request that uses it (`::: auth oauth2`, [`crate::Auth::OAuth2`]).
//!
//! Every text field may hold `{{ }}` markers, resolved against the active environment when a
//! token is requested, so what changes per environment lives in variables and the secrets in
//! `.local.env` files.

use crate::KeyValue;

/// How a profile obtains its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OAuthGrant {
    /// Authorization code, the user signs in in the system browser. PKCE optional.
    #[default]
    AuthorizationCode,
    /// Client credentials, no user involved.
    ClientCredentials,
    /// Resource owner password credentials.
    Password,
}

impl OAuthGrant {
    /// Every grant, in the order the grant selector lists them.
    pub const ALL: [OAuthGrant; 3] = [
        OAuthGrant::AuthorizationCode,
        OAuthGrant::ClientCredentials,
        OAuthGrant::Password,
    ];

    /// The value written in the profile file, which is also the `grant_type` of the token
    /// request.
    pub fn as_str(self) -> &'static str {
        match self {
            OAuthGrant::AuthorizationCode => "authorization_code",
            OAuthGrant::ClientCredentials => "client_credentials",
            OAuthGrant::Password => "password",
        }
    }

    /// The grant written as `value` in the profile file.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|grant| grant.as_str() == value)
    }

    /// Whether the user has to sign in in the browser.
    pub fn is_interactive(self) -> bool {
        self == OAuthGrant::AuthorizationCode
    }
}

/// Where the authorization server redirects to after the user signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OAuthRedirect {
    /// `http://127.0.0.1:{port}/callback` on a free port chosen on each sign-in (RFC 8252).
    #[default]
    Loopback,
    /// A fixed local URL, port included, for providers that require the exact URL.
    Fixed,
    /// A web URL Postino cannot listen on: the user pastes the URL it ended at.
    Manual,
}

impl OAuthRedirect {
    /// Every option, in the order the selector lists them.
    pub const ALL: [OAuthRedirect; 3] = [
        OAuthRedirect::Loopback,
        OAuthRedirect::Fixed,
        OAuthRedirect::Manual,
    ];

    /// The value written in the profile file.
    pub fn as_str(self) -> &'static str {
        match self {
            OAuthRedirect::Loopback => "loopback",
            OAuthRedirect::Fixed => "fixed",
            OAuthRedirect::Manual => "manual",
        }
    }

    /// The option written as `value` in the profile file.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|option| option.as_str() == value)
    }
}

/// How the client authenticates to the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientAuth {
    /// `Authorization: Basic base64(client_id:client_secret)`.
    #[default]
    BasicHeader,
    /// `client_id` and `client_secret` in the form body.
    InBody,
}

impl ClientAuth {
    /// The value written in the profile file.
    pub fn as_str(self) -> &'static str {
        match self {
            ClientAuth::BasicHeader => "basic",
            ClientAuth::InBody => "body",
        }
    }

    /// The option written as `value` in the profile file.
    pub fn parse(value: &str) -> Option<Self> {
        [ClientAuth::BasicHeader, ClientAuth::InBody]
            .into_iter()
            .find(|option| option.as_str() == value)
    }
}

/// Where the access token goes in the requests that use the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenPlacement {
    /// `Authorization: <prefix> <token>`.
    #[default]
    Header,
    /// A query parameter named after [`OAuthProfile::token_param`].
    Query,
}

impl TokenPlacement {
    /// The value written in the profile file.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenPlacement::Header => "header",
            TokenPlacement::Query => "query",
        }
    }

    /// The option written as `value` in the profile file.
    pub fn parse(value: &str) -> Option<Self> {
        [TokenPlacement::Header, TokenPlacement::Query]
            .into_iter()
            .find(|option| option.as_str() == value)
    }
}

/// Which request of the flow an [`ExtraParam`] is added to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParamTarget {
    /// The form body of every token request.
    #[default]
    Token,
    /// The query of the authorization URL opened in the browser.
    Auth,
}

/// A parameter the provider needs beyond the standard ones, for example `audience`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtraParam {
    /// The parameter name.
    pub key: String,
    /// Its value, may hold `{{ }}` markers.
    pub value: String,
    /// Which request it goes in.
    pub target: ParamTarget,
}

/// The default prefix of the `Authorization` header.
pub const DEFAULT_TOKEN_PREFIX: &str = "Bearer";
/// The default name of the query parameter the token goes in.
pub const DEFAULT_TOKEN_PARAM: &str = "access_token";

/// One named OAuth 2.0 configuration of the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthProfile {
    /// The name requests reference, also the key of its table in the project file.
    pub name: String,
    /// How the token is obtained.
    pub grant: OAuthGrant,
    /// The authorization endpoint (authorization code only).
    pub auth_url: String,
    /// The token endpoint.
    pub token_url: String,
    /// The client id.
    pub client_id: String,
    /// The client secret, optional for public clients with PKCE.
    pub client_secret: String,
    /// The scopes, separated by spaces.
    pub scope: String,
    /// The user name (password grant only).
    pub username: String,
    /// The password (password grant only).
    pub password: String,
    /// Whether the authorization code grant uses PKCE with SHA-256.
    pub pkce: bool,
    /// Where the browser is redirected to (authorization code only).
    pub redirect: OAuthRedirect,
    /// The redirect URL of [`OAuthRedirect::Fixed`] and [`OAuthRedirect::Manual`].
    pub redirect_url: String,
    /// How the client authenticates to the token endpoint.
    pub client_auth: ClientAuth,
    /// Where the token goes in the requests.
    pub token_placement: TokenPlacement,
    /// The scheme before the token in the `Authorization` header.
    pub token_prefix: String,
    /// The query parameter name when the token goes in the query.
    pub token_param: String,
    /// Parameters added to the token or the authorization request.
    pub extra_params: Vec<ExtraParam>,
}

impl OAuthProfile {
    /// A new profile named `name` with `grant` and every other field at its default.
    pub fn new(name: impl Into<String>, grant: OAuthGrant) -> Self {
        OAuthProfile {
            name: name.into(),
            grant,
            auth_url: String::new(),
            token_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            scope: String::new(),
            username: String::new(),
            password: String::new(),
            pkce: true,
            redirect: OAuthRedirect::Loopback,
            redirect_url: String::new(),
            client_auth: ClientAuth::BasicHeader,
            token_placement: TokenPlacement::Header,
            token_prefix: DEFAULT_TOKEN_PREFIX.to_string(),
            token_param: DEFAULT_TOKEN_PARAM.to_string(),
            extra_params: Vec::new(),
        }
    }

    /// The extra parameters that go in `target`, as key-value pairs.
    pub fn params_for(&self, target: ParamTarget) -> Vec<KeyValue> {
        self.extra_params
            .iter()
            .filter(|param| param.target == target && !param.key.trim().is_empty())
            .map(|param| KeyValue::new(param.key.clone(), param.value.clone()))
            .collect()
    }
}

/// Whether `name` can name a profile: not empty, and only ASCII letters, digits, `-` and `_`, so
/// it is a bare TOML key and fits on the `profile = ` line of a request unquoted.
pub fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn grants_round_trip_their_file_value() {
        for grant in OAuthGrant::ALL {
            assert_eq!(OAuthGrant::parse(grant.as_str()), Some(grant));
        }
        assert_eq!(OAuthGrant::parse("implicit"), None);
        assert!(OAuthGrant::AuthorizationCode.is_interactive());
        assert!(!OAuthGrant::ClientCredentials.is_interactive());
    }

    #[test]
    fn options_round_trip_their_file_value() {
        for option in OAuthRedirect::ALL {
            assert_eq!(OAuthRedirect::parse(option.as_str()), Some(option));
        }
        assert_eq!(ClientAuth::parse("body"), Some(ClientAuth::InBody));
        assert_eq!(TokenPlacement::parse("query"), Some(TokenPlacement::Query));
        assert_eq!(TokenPlacement::parse("cookie"), None);
    }

    #[test]
    fn profile_names() {
        assert!(valid_profile_name("acme-sso"));
        assert!(valid_profile_name("partner_legacy2"));
        assert!(!valid_profile_name(""));
        assert!(!valid_profile_name("acme sso"));
        assert!(!valid_profile_name("acme.sso"));
    }

    #[test]
    fn params_for_splits_by_target_and_skips_blank_keys() {
        let mut profile = OAuthProfile::new("p", OAuthGrant::ClientCredentials);
        profile.extra_params = vec![
            ExtraParam {
                key: "audience".into(),
                value: "https://api".into(),
                target: ParamTarget::Token,
            },
            ExtraParam {
                key: "prompt".into(),
                value: "login".into(),
                target: ParamTarget::Auth,
            },
            ExtraParam {
                key: " ".into(),
                value: "x".into(),
                target: ParamTarget::Token,
            },
        ];
        assert_eq!(
            profile.params_for(ParamTarget::Token),
            vec![KeyValue::new("audience", "https://api")]
        );
        assert_eq!(
            profile.params_for(ParamTarget::Auth),
            vec![KeyValue::new("prompt", "login")]
        );
    }
}
