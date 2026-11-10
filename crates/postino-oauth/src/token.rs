//! [`TokenSet`], what a token endpoint returned, as stored between sends.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};

/// A token whose expiry is closer than this counts as expired, so a request never leaves with a
/// token that dies in flight.
pub const EXPIRY_MARGIN: Duration = Duration::from_secs(30);

/// The tokens of one profile and environment, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    /// The access token sent with the requests.
    pub access_token: String,
    /// The refresh token, when the server returned one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// The OpenID Connect id token, when the server returned one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    /// The `token_type` the server returned, usually `Bearer`.
    #[serde(default)]
    pub token_type: String,
    /// When the access token expires, in seconds since the Unix epoch. `None` when the server
    /// did not say, which counts as never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// When the token was obtained, in seconds since the Unix epoch.
    pub obtained_at: u64,
    /// The scope granted, or the one asked for when the server did not say.
    #[serde(default)]
    pub scope: String,
}

/// Seconds since the Unix epoch of `time`, 0 before it.
pub fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// The [`SystemTime`] of `seconds` since the Unix epoch.
pub fn from_unix_seconds(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

impl TokenSet {
    /// When the access token expires, `None` when it never does.
    pub fn expires_at_time(&self) -> Option<SystemTime> {
        self.expires_at.map(from_unix_seconds)
    }

    /// When the token was obtained.
    pub fn obtained_at_time(&self) -> SystemTime {
        from_unix_seconds(self.obtained_at)
    }

    /// Whether the access token can still be sent at `now`: it does not expire in the next
    /// [`EXPIRY_MARGIN`].
    pub fn is_valid_at(&self, now: SystemTime) -> bool {
        match self.expires_at {
            None => true,
            Some(expires_at) => unix_seconds(now) + EXPIRY_MARGIN.as_secs() < expires_at,
        }
    }

    /// How long the access token stays valid after `now`, zero when it has expired, `None` when
    /// it never expires.
    pub fn remaining_at(&self, now: SystemTime) -> Option<Duration> {
        let expires_at = self.expires_at?;
        Some(Duration::from_secs(
            expires_at.saturating_sub(unix_seconds(now)),
        ))
    }

    /// How long ago the access token expired, zero when it has not yet. `None` when it never
    /// expires.
    pub fn expired_for_at(&self, now: SystemTime) -> Option<Duration> {
        let expires_at = self.expires_at?;
        Some(Duration::from_secs(
            unix_seconds(now).saturating_sub(expires_at),
        ))
    }

    /// The claims of the id token, or of the access token when it is a JWT and there is no id
    /// token, as `(name, JSON value)` pairs in the token's order. Empty when neither is a JWT.
    pub fn claims(&self) -> Vec<(String, serde_json::Value)> {
        self.id_token
            .as_deref()
            .and_then(jwt_claims)
            .or_else(|| jwt_claims(&self.access_token))
            .unwrap_or_default()
    }

    /// The claims of the access token alone, when it is a JWT.
    pub fn access_token_claims(&self) -> Vec<(String, serde_json::Value)> {
        jwt_claims(&self.access_token).unwrap_or_default()
    }
}

/// The payload claims of a JWT (`header.payload.signature`), not verified: Postino only shows
/// them. `None` when `token` is not a JWT with a JSON object payload.
pub fn jwt_claims(token: &str) -> Option<Vec<(String, serde_json::Value)>> {
    let mut parts = token.split('.');
    let (_header, payload, _signature) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let object = value.as_object()?;
    Some(
        object
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn token(expires_at: Option<u64>) -> TokenSet {
        TokenSet {
            access_token: "a".into(),
            refresh_token: None,
            id_token: None,
            token_type: "Bearer".into(),
            expires_at,
            obtained_at: 1_000,
            scope: String::new(),
        }
    }

    #[test]
    fn a_token_expiring_within_the_margin_is_not_valid() {
        let now = from_unix_seconds(10_000);
        assert!(token(None).is_valid_at(now));
        assert!(token(Some(10_031)).is_valid_at(now));
        assert!(!token(Some(10_030)).is_valid_at(now));
        assert!(!token(Some(9_000)).is_valid_at(now));
        assert_eq!(
            token(Some(9_280)).expired_for_at(now),
            Some(Duration::from_secs(720))
        );
        assert_eq!(
            token(Some(12_520)).remaining_at(now),
            Some(Duration::from_secs(2_520))
        );
        assert_eq!(token(None).remaining_at(now), None);
    }

    #[test]
    fn claims_come_from_the_id_token_first() {
        let encode = |json: &str| {
            format!(
                "eyJhbGciOiJub25lIn0.{}.sig",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
            )
        };
        let mut set = token(None);
        assert!(set.claims().is_empty());
        set.access_token = encode(r#"{"sub":"access"}"#);
        assert_eq!(set.claims()[0].1, serde_json::json!("access"));
        set.id_token = Some(encode(r#"{"sub":"u_8121","aud":"https://api.acme.dev"}"#));
        let claims = set.claims();
        assert_eq!(claims.len(), 2);
        assert!(
            claims
                .iter()
                .any(|(name, value)| name == "sub" && value == "u_8121")
        );
        assert_eq!(jwt_claims("not.a"), None);
        assert_eq!(jwt_claims("a.b.c.d"), None);
    }

    #[test]
    fn stored_json_round_trips() {
        let mut set = token(Some(5));
        set.refresh_token = Some("r".into());
        let json = serde_json::to_string(&set).expect("json");
        assert_eq!(serde_json::from_str::<TokenSet>(&json).expect("parse"), set);
    }
}
