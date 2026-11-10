//! Requests to the token endpoint (RFC 6749, sections 4 and 6), and the masked copy of each one
//! the console shows.

use std::time::SystemTime;

use base64::Engine;
use postino_core::functions::url_encode;
use postino_core::{ClientAuth, Method, ResolvedBody, ResolvedField, ResolvedRequest, Response};
use postino_http::SendOptions;

use crate::error::OAuthError;
use crate::resolve::ResolvedProfile;
use crate::token::{TokenSet, unix_seconds};

/// What replaces a secret in the console.
pub const MASK: &str = "••••••••";

/// Form fields that hold secrets, masked in the console.
const SECRET_FIELDS: [&str; 6] = [
    "client_secret",
    "code",
    "code_verifier",
    "refresh_token",
    "password",
    "assertion",
];

/// Response fields that hold secrets, masked in the console.
const SECRET_RESPONSE_FIELDS: [&str; 3] = ["access_token", "refresh_token", "id_token"];

/// One request to the token endpoint, as the console shows it: secrets masked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenExchange {
    /// The `grant_type` sent.
    pub grant_type: String,
    /// The request, with the `Authorization` value and the secret form fields masked.
    pub request: ResolvedRequest,
    /// The response, with the tokens masked. `None` when sending failed.
    pub response: Option<Response>,
    /// When the request was sent.
    pub sent_at: SystemTime,
}

/// The form fields of a token request for `grant_type`, before the client authentication.
pub(crate) fn grant_form(grant_type: &str, fields: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut form = vec![("grant_type".to_string(), grant_type.to_string())];
    form.extend(
        fields
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(name, value)| (name.to_string(), value.to_string())),
    );
    form
}

/// Builds the token request: `form` plus the client authentication and the profile's extra
/// token parameters.
pub(crate) fn build_request(
    profile: &ResolvedProfile,
    mut form: Vec<(String, String)>,
) -> ResolvedRequest {
    let mut headers = vec![
        ResolvedField {
            name: "Content-Type".to_string(),
            value: "application/x-www-form-urlencoded".to_string(),
        },
        ResolvedField {
            name: "Accept".to_string(),
            value: "application/json".to_string(),
        },
    ];
    // A public client (no secret) always sends its id in the body: there is nothing to put in a
    // Basic header.
    if profile.client_auth == ClientAuth::BasicHeader && !profile.client_secret.is_empty() {
        // RFC 6749, section 2.3.1: both are form-urlencoded before the Basic encoding.
        let credentials = format!(
            "{}:{}",
            url_encode(&profile.client_id),
            url_encode(&profile.client_secret)
        );
        headers.push(ResolvedField {
            name: "Authorization".to_string(),
            value: format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(credentials)
            ),
        });
    } else {
        form.push(("client_id".to_string(), profile.client_id.clone()));
        if !profile.client_secret.is_empty() {
            form.push(("client_secret".to_string(), profile.client_secret.clone()));
        }
    }
    form.extend(profile.token_params.iter().cloned());
    ResolvedRequest {
        method: Method::Post,
        url: profile.token_url.clone(),
        headers,
        body: ResolvedBody::Bytes(encode_form(&form).into_bytes()),
    }
}

/// `application/x-www-form-urlencoded` text of `form`.
pub(crate) fn encode_form(form: &[(String, String)]) -> String {
    form.iter()
        .map(|(name, value)| format!("{}={}", url_encode(name), url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Sends a token request built by [`build_request`] and reads the token from the answer. Every
/// attempt, failed or not, is appended to `exchanges` with its secrets masked.
pub(crate) fn request_token(
    request: &ResolvedRequest,
    grant_type: &str,
    requested_scope: &str,
    options: &SendOptions,
    exchanges: &mut Vec<TokenExchange>,
) -> Result<TokenSet, OAuthError> {
    let sent_at = SystemTime::now();
    let sent = postino_http::send(request, options);
    let masked_request = mask_request(request);
    match sent {
        Err(error) => {
            exchanges.push(TokenExchange {
                grant_type: grant_type.to_string(),
                request: masked_request,
                response: None,
                sent_at,
            });
            Err(OAuthError::Send(error))
        }
        Ok(response) => {
            let parsed = parse_token_response(&response, requested_scope, SystemTime::now());
            exchanges.push(TokenExchange {
                grant_type: grant_type.to_string(),
                request: masked_request,
                response: Some(mask_response(&response)),
                sent_at,
            });
            parsed
        }
    }
}

/// Reads a token endpoint's answer: a token on a 2xx with an `access_token`, the OAuth error it
/// describes otherwise.
pub(crate) fn parse_token_response(
    response: &Response,
    requested_scope: &str,
    now: SystemTime,
) -> Result<TokenSet, OAuthError> {
    let json: Option<serde_json::Value> = serde_json::from_slice(&response.body).ok();
    let object = json.as_ref().and_then(serde_json::Value::as_object);
    let text = |name: &str| {
        object
            .and_then(|object| object.get(name))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    if (200..300).contains(&response.status)
        && let Some(access_token) = text("access_token").filter(|token| !token.is_empty())
    {
        let expires_in = object
            .and_then(|object| object.get("expires_in"))
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
            });
        let obtained_at = unix_seconds(now);
        return Ok(TokenSet {
            access_token,
            refresh_token: text("refresh_token").filter(|token| !token.is_empty()),
            id_token: text("id_token").filter(|token| !token.is_empty()),
            token_type: text("token_type").unwrap_or_else(|| "Bearer".to_string()),
            expires_at: expires_in.map(|seconds| obtained_at + seconds),
            obtained_at,
            scope: text("scope").unwrap_or_else(|| requested_scope.to_string()),
        });
    }
    if let Some(error) = text("error") {
        return Err(OAuthError::Server {
            status: response.status,
            error,
            description: text("error_description"),
        });
    }
    let detail = if object.is_none() {
        "not a JSON object".to_string()
    } else if (200..300).contains(&response.status) {
        String::new()
    } else {
        "no error code".to_string()
    };
    Err(OAuthError::InvalidResponse {
        status: response.status,
        detail,
    })
}

/// `request` with the `Authorization` value and the secret form fields masked.
pub(crate) fn mask_request(request: &ResolvedRequest) -> ResolvedRequest {
    let headers = request
        .headers
        .iter()
        .map(|header| {
            if header.name.eq_ignore_ascii_case("authorization") {
                let scheme = header.value.split(' ').next().unwrap_or_default();
                ResolvedField {
                    name: header.name.clone(),
                    value: format!("{scheme} {MASK}"),
                }
            } else {
                header.clone()
            }
        })
        .collect();
    let body = match &request.body {
        ResolvedBody::Bytes(bytes) => {
            let text = String::from_utf8_lossy(bytes);
            let masked = text
                .split('&')
                .map(|pair| match pair.split_once('=') {
                    Some((name, _)) if SECRET_FIELDS.contains(&name) => format!("{name}={MASK}"),
                    _ => pair.to_string(),
                })
                .collect::<Vec<_>>()
                .join("&");
            ResolvedBody::Bytes(masked.into_bytes())
        }
        ResolvedBody::None => ResolvedBody::None,
    };
    ResolvedRequest {
        method: request.method.clone(),
        url: request.url.clone(),
        headers,
        body,
    }
}

/// `response` with the tokens of a JSON body masked. A body that is not a JSON object is kept
/// as it is: it holds no token.
pub(crate) fn mask_response(response: &Response) -> Response {
    let mut masked = response.clone();
    if let Ok(serde_json::Value::Object(mut object)) =
        serde_json::from_slice::<serde_json::Value>(&response.body)
    {
        let mut changed = false;
        for field in SECRET_RESPONSE_FIELDS {
            if let Some(value) = object.get_mut(field) {
                *value = serde_json::Value::String(MASK.to_string());
                changed = true;
            }
        }
        if changed {
            masked.body = serde_json::to_vec(&object).unwrap_or_default();
            masked.size = masked.body.len();
        }
    }
    masked
}

#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) mod tests {
    use super::*;
    use crate::token::from_unix_seconds;
    use postino_core::{OAuthGrant, OAuthRedirect};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    pub(crate) fn profile(client_auth: ClientAuth, secret: &str) -> ResolvedProfile {
        ResolvedProfile {
            name: "acme".into(),
            grant: OAuthGrant::ClientCredentials,
            auth_url: String::new(),
            token_url: "https://auth/token".into(),
            client_id: "my id".into(),
            client_secret: secret.into(),
            scope: "read".into(),
            username: String::new(),
            password: String::new(),
            pkce: true,
            redirect: OAuthRedirect::Loopback,
            redirect_url: String::new(),
            client_auth,
            token_params: vec![("audience".into(), "https://api".into())],
            auth_params: Vec::new(),
        }
    }

    fn body(request: &ResolvedRequest) -> String {
        match &request.body {
            ResolvedBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            ResolvedBody::None => String::new(),
        }
    }

    fn response(status: u16, body: &str) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
            time: Duration::ZERO,
            size: body.len(),
            tls_warning: None,
        }
    }

    #[test]
    fn client_authentication_goes_in_a_basic_header_or_the_body() {
        let form = grant_form("client_credentials", &[("scope", "read")]);
        let request = build_request(&profile(ClientAuth::BasicHeader, "s&cret"), form.clone());
        let authorization = request
            .headers
            .iter()
            .find(|header| header.name == "Authorization")
            .expect("authorization");
        assert_eq!(
            authorization.value,
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("my%20id:s%26cret")
            )
        );
        assert_eq!(
            body(&request),
            "grant_type=client_credentials&scope=read&audience=https%3A%2F%2Fapi"
        );

        let request = build_request(&profile(ClientAuth::InBody, "s"), form.clone());
        assert!(
            request
                .headers
                .iter()
                .all(|header| header.name != "Authorization")
        );
        assert_eq!(
            body(&request),
            "grant_type=client_credentials&scope=read&client_id=my%20id&client_secret=s&audience=https%3A%2F%2Fapi"
        );

        // A public client sends its id in the body even with a Basic header chosen.
        let request = build_request(&profile(ClientAuth::BasicHeader, ""), form);
        assert!(body(&request).contains("client_id=my%20id"));
        assert!(!body(&request).contains("client_secret"));
    }

    #[test]
    fn a_token_response_is_read() {
        let now = from_unix_seconds(1_000);
        let token = parse_token_response(
            &response(
                200,
                r#"{"access_token":"a","token_type":"Bearer","expires_in":"3600","refresh_token":"r"}"#,
            ),
            "read",
            now,
        )
        .expect("token");
        assert_eq!(token.access_token, "a");
        assert_eq!(token.refresh_token.as_deref(), Some("r"));
        assert_eq!(token.expires_at, Some(4_600));
        assert_eq!(token.scope, "read");
    }

    #[test]
    fn error_responses_are_read() {
        let now = SystemTime::now();
        let error = parse_token_response(
            &response(
                400,
                r#"{"error":"invalid_grant","error_description":"expired"}"#,
            ),
            "",
            now,
        )
        .expect_err("error");
        assert!(error.is_invalid_grant());
        assert!(matches!(
            parse_token_response(&response(500, "oops"), "", now),
            Err(OAuthError::InvalidResponse { status: 500, .. })
        ));
        assert!(matches!(
            parse_token_response(&response(200, r#"{"token_type":"x"}"#), "", now),
            Err(OAuthError::InvalidResponse { status: 200, .. })
        ));
    }

    #[test]
    fn secrets_are_masked() {
        let request = build_request(
            &profile(ClientAuth::BasicHeader, "secret"),
            grant_form("refresh_token", &[("refresh_token", "8xLO")]),
        );
        let masked = mask_request(&request);
        assert!(
            masked
                .headers
                .iter()
                .any(|header| header.value == format!("Basic {MASK}"))
        );
        assert_eq!(
            body(&masked),
            format!("grant_type=refresh_token&refresh_token={MASK}&audience=https%3A%2F%2Fapi")
        );
        let masked = mask_response(&response(
            200,
            r#"{"access_token":"a","token_type":"Bearer","expires_in":3600}"#,
        ));
        let text = String::from_utf8(masked.body.clone()).expect("utf8");
        assert!(text.contains(MASK) && !text.contains("\"a\""), "{text}");
        assert_eq!(masked.size, masked.body.len());
    }
}
