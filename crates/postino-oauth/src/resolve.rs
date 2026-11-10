//! [`ResolvedProfile`]: a profile with its `{{ }}` markers resolved in the active environment.

use postino_core::{
    ClientAuth, OAuthGrant, OAuthProfile, OAuthRedirect, ParamTarget, TemplateWarning, VarScope,
    interpolate,
};

use crate::error::{OAuthError, ProfileField};

/// A profile ready to talk to the authorization server: every field interpolated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfile {
    /// The profile name.
    pub name: String,
    /// How the token is obtained.
    pub grant: OAuthGrant,
    /// The authorization endpoint.
    pub auth_url: String,
    /// The token endpoint.
    pub token_url: String,
    /// The client id.
    pub client_id: String,
    /// The client secret, empty for a public client.
    pub client_secret: String,
    /// The scope asked for: the request's override, or the profile's.
    pub scope: String,
    /// The user name of the password grant.
    pub username: String,
    /// The password of the password grant.
    pub password: String,
    /// Whether the authorization code grant uses PKCE.
    pub pkce: bool,
    /// Where the browser is redirected to.
    pub redirect: OAuthRedirect,
    /// The fixed or manual redirect URL.
    pub redirect_url: String,
    /// How the client authenticates to the token endpoint.
    pub client_auth: ClientAuth,
    /// Extra parameters of the token requests.
    pub token_params: Vec<(String, String)>,
    /// Extra parameters of the authorization URL.
    pub auth_params: Vec<(String, String)>,
}

/// Resolves `profile` against `vars`, asking for `scope_override` instead of the profile's scope
/// when it is not empty. Fails on the first unresolved marker, naming the field and the variable,
/// and when a field the grant needs is empty.
pub fn resolve_profile(
    profile: &OAuthProfile,
    scope_override: &str,
    vars: &VarScope,
) -> Result<ResolvedProfile, OAuthError> {
    let resolve = |text: &str, field: ProfileField| -> Result<String, OAuthError> {
        let interpolated = interpolate(text, vars);
        if let Some(warning) = interpolated.warnings.first() {
            let name = match warning {
                TemplateWarning::UnknownVariable(name) => name.clone(),
                TemplateWarning::Function(error) => error.to_string(),
            };
            return Err(OAuthError::UnresolvedVariable { field, name });
        }
        Ok(interpolated.text.trim().to_string())
    };
    let required = |value: String, field: ProfileField| {
        if value.is_empty() {
            Err(OAuthError::MissingField(field))
        } else {
            Ok(value)
        }
    };
    let grant = profile.grant;
    let token_url = required(
        resolve(&profile.token_url, ProfileField::TokenUrl)?,
        ProfileField::TokenUrl,
    )?;
    let client_id = required(
        resolve(&profile.client_id, ProfileField::ClientId)?,
        ProfileField::ClientId,
    )?;
    let auth_url = if grant == OAuthGrant::AuthorizationCode {
        required(
            resolve(&profile.auth_url, ProfileField::AuthUrl)?,
            ProfileField::AuthUrl,
        )?
    } else {
        String::new()
    };
    let (username, password) = if grant == OAuthGrant::Password {
        (
            required(
                resolve(&profile.username, ProfileField::Username)?,
                ProfileField::Username,
            )?,
            resolve(&profile.password, ProfileField::Password)?,
        )
    } else {
        (String::new(), String::new())
    };
    let redirect_url =
        if grant == OAuthGrant::AuthorizationCode && profile.redirect != OAuthRedirect::Loopback {
            required(
                resolve(&profile.redirect_url, ProfileField::RedirectUrl)?,
                ProfileField::RedirectUrl,
            )?
        } else {
            String::new()
        };
    let scope_text = if scope_override.trim().is_empty() {
        &profile.scope
    } else {
        scope_override
    };
    let params = |target: ParamTarget| -> Result<Vec<(String, String)>, OAuthError> {
        profile
            .params_for(target)
            .into_iter()
            .map(|param| {
                Ok((
                    param.key.trim().to_string(),
                    resolve(&param.value, ProfileField::ExtraParam)?,
                ))
            })
            .collect()
    };
    Ok(ResolvedProfile {
        name: profile.name.clone(),
        grant,
        auth_url,
        token_url,
        client_id,
        client_secret: resolve(&profile.client_secret, ProfileField::ClientSecret)?,
        scope: resolve(scope_text, ProfileField::Scope)?,
        username,
        password,
        pkce: profile.pkce,
        redirect: profile.redirect,
        redirect_url,
        client_auth: profile.client_auth,
        token_params: params(ParamTarget::Token)?,
        auth_params: params(ParamTarget::Auth)?,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_core::KeyValue;
    use pretty_assertions::assert_eq;

    fn profile() -> OAuthProfile {
        let mut profile = OAuthProfile::new("acme-machine", OAuthGrant::ClientCredentials);
        profile.token_url = "{{authBaseUrl}}/oauth/token".into();
        profile.client_id = "{{clientId}}".into();
        profile.client_secret = "{{clientSecret}}".into();
        profile.scope = "payments:read".into();
        profile
    }

    #[test]
    fn resolves_every_field_against_the_environment() {
        let environment = vec![
            KeyValue::new("authBaseUrl", "https://auth.dev.acme.dev"),
            KeyValue::new("clientId", "postino-dev"),
            KeyValue::new("clientSecret", "s3cr3t-dev"),
        ];
        let vars = VarScope {
            request_vars: &[],
            session_env: &[],
            environment: &environment,
        };
        let resolved = resolve_profile(&profile(), "", &vars).expect("resolve");
        assert_eq!(resolved.token_url, "https://auth.dev.acme.dev/oauth/token");
        assert_eq!(resolved.client_id, "postino-dev");
        assert_eq!(resolved.client_secret, "s3cr3t-dev");
        assert_eq!(resolved.scope, "payments:read");
        let overridden = resolve_profile(&profile(), "payments:write", &vars).expect("resolve");
        assert_eq!(overridden.scope, "payments:write");
    }

    #[test]
    fn an_unknown_variable_names_its_field() {
        let environment = vec![
            KeyValue::new("authBaseUrl", "https://auth"),
            KeyValue::new("clientId", "id"),
        ];
        let vars = VarScope {
            request_vars: &[],
            session_env: &[],
            environment: &environment,
        };
        let error = resolve_profile(&profile(), "", &vars).expect_err("unresolved");
        assert!(matches!(
            error,
            OAuthError::UnresolvedVariable { field: ProfileField::ClientSecret, ref name } if name == "clientSecret"
        ));
    }

    #[test]
    fn a_missing_required_field_is_an_error() {
        let mut profile = profile();
        profile.token_url.clear();
        let vars = VarScope {
            request_vars: &[],
            session_env: &[],
            environment: &[],
        };
        assert!(matches!(
            resolve_profile(&profile, "", &vars),
            Err(OAuthError::MissingField(ProfileField::TokenUrl))
        ));
    }
}
