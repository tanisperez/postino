//! OAuth 2.0 profiles from imported collections (#227): Postman, Bruno and OpenAPI describe an
//! OAuth configuration per request or per scheme; Postino keeps it once, as a profile of the
//! workspace (`[oauth.<name>]` in `.postino/project.toml`), and the requests reference it.
//!
//! [`ProfileBuilder`] gathers the profiles of one import: identical configurations share one
//! profile, names never repeat, and a secret written in clear (a client secret, a password)
//! becomes a variable of `.local.env` so the shared profile never holds it. Stored tokens are
//! never imported.

use postino_core::{
    ClientAuth, KeyValue, OAuthGrant, OAuthProfile, OAuthRedirect, TokenPlacement,
    valid_profile_name, variable_spans,
};

/// The OAuth fields of an imported auth, by name. Names are compared ignoring case, `_` and `-`,
/// so Postman's `accessTokenUrl`, Bruno's `access_token_url` and OpenCollection's
/// `accessTokenUrl` read the same.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OAuthFields(pub Vec<(String, String)>);

impl OAuthFields {
    fn normalized(name: &str) -> String {
        name.chars()
            .filter(|c| *c != '_' && *c != '-')
            .flat_map(char::to_lowercase)
            .collect()
    }

    /// The first non-empty value among `names`.
    fn get(&self, names: &[&str]) -> String {
        names
            .iter()
            .find_map(|name| {
                let wanted = Self::normalized(name);
                self.0
                    .iter()
                    .find(|(key, value)| {
                        Self::normalized(key) == wanted && !value.trim().is_empty()
                    })
                    .map(|(_, value)| value.trim().to_string())
            })
            .unwrap_or_default()
    }
}

/// Builds a profile (without its final name) from the fields of a Postman, Bruno or
/// OpenCollection OAuth 2.0 auth. `notes` gets what did not map exactly.
pub fn profile_from_fields(fields: &OAuthFields, notes: &mut Vec<String>) -> OAuthProfile {
    let grant_text = fields.get(&["grant_type", "grantType"]);
    let grant = match grant_text.as_str() {
        "client_credentials" => OAuthGrant::ClientCredentials,
        "password" | "password_credentials" => OAuthGrant::Password,
        "implicit" => {
            notes.push(
                "OAuth 2.0 implicit grant imported as authorization code with PKCE: OAuth 2.1 \
                 removes implicit"
                    .to_string(),
            );
            OAuthGrant::AuthorizationCode
        }
        _ => OAuthGrant::AuthorizationCode,
    };
    let mut profile = OAuthProfile::new(String::new(), grant);
    profile.auth_url = fields.get(&["authUrl", "authorization_url", "authorizationUrl"]);
    profile.token_url = fields.get(&["accessTokenUrl", "access_token_url", "tokenUrl"]);
    profile.client_id = fields.get(&["clientId", "client_id"]);
    profile.client_secret = fields.get(&["clientSecret", "client_secret"]);
    profile.scope = fields.get(&["scope"]);
    if grant == OAuthGrant::Password {
        profile.username = fields.get(&["username"]);
        profile.password = fields.get(&["password"]);
    }
    if grant == OAuthGrant::AuthorizationCode {
        let pkce = fields.get(&["pkce", "usePkce"]);
        profile.pkce = grant_text == "authorization_code_with_pkce"
            || grant_text == "implicit"
            || pkce == "true"
            || !fields.get(&["challengeAlgorithm"]).is_empty();
        let redirect = fields.get(&["redirect_uri", "redirectUri", "callback_url", "callbackUrl"]);
        let (kind, url) = redirect_of(&redirect, notes);
        profile.redirect = kind;
        profile.redirect_url = url;
    }
    let client_auth = fields.get(&[
        "client_authentication",
        "credentials_placement",
        "credentialsPlacement",
    ]);
    if client_auth == "body" {
        profile.client_auth = ClientAuth::InBody;
    }
    let token_in = fields.get(&["addTokenTo", "token_placement", "tokenPlacement"]);
    if matches!(token_in.as_str(), "queryParams" | "url" | "query") {
        profile.token_placement = TokenPlacement::Query;
        let name = fields.get(&["token_query_key_name", "tokenQueryKeyName", "tokenQueryKey"]);
        if !name.is_empty() {
            profile.token_param = name;
        }
    }
    let prefix = fields.get(&["headerPrefix", "token_header_prefix", "tokenHeaderPrefix"]);
    if !prefix.is_empty() {
        profile.token_prefix = prefix;
    }
    profile
}

/// Where the browser is redirected to, from an imported callback URL: a local URL with a port is
/// a fixed redirect, another web URL is pasted by hand, and none is the loopback redirect.
/// Postman's own callback only works with Postman, so it becomes the loopback.
fn redirect_of(url: &str, notes: &mut Vec<String>) -> (OAuthRedirect, String) {
    let url = url.trim();
    if url.is_empty() {
        return (OAuthRedirect::Loopback, String::new());
    }
    if url.contains("oauth.pstmn.io") {
        notes.push(format!(
            "OAuth 2.0 redirect {url} only works with Postman: the profile uses http://127.0.0.1 \
             instead, register it in the provider"
        ));
        return (OAuthRedirect::Loopback, String::new());
    }
    let local = ["http://localhost:", "http://127.0.0.1:"]
        .iter()
        .any(|prefix| url.starts_with(prefix));
    if local {
        (OAuthRedirect::Fixed, url.to_string())
    } else {
        (OAuthRedirect::Manual, url.to_string())
    }
}

/// A profile name from `text`: lowercase letters, digits and `-`.
pub fn profile_slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() || !valid_profile_name(&slug) {
        "oauth".to_string()
    } else {
        slug
    }
}

/// The variable name of a secret of `profile`: `acme-sso` and `ClientSecret` give
/// `acmeSsoClientSecret`.
fn secret_variable(profile: &str, suffix: &str) -> String {
    let mut name = String::new();
    let mut upper = false;
    for c in profile.chars() {
        if c == '-' || c == '_' {
            upper = true;
        } else if upper && !name.is_empty() {
            name.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            name.push(c);
            upper = false;
        }
    }
    format!("{name}{suffix}")
}

/// The OAuth profiles of one import.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileBuilder {
    /// The profiles, in the order they were first seen.
    pub profiles: Vec<OAuthProfile>,
    /// The secrets taken out of them, for `.local.env`.
    pub secrets: Vec<KeyValue>,
}

impl ProfileBuilder {
    /// Adds `profile` under a name made from `hint`, unless an identical one is already there,
    /// and returns the name the requests should use. A client secret or password written in
    /// clear is replaced by a variable, whose value goes to [`Self::secrets`].
    pub fn add(&mut self, hint: &str, mut profile: OAuthProfile) -> String {
        let base = profile_slug(hint);
        // Same settings under any name: share it.
        if let Some(existing) = self.profiles.iter().find(|existing| {
            let mut candidate = profile.clone();
            candidate.name = existing.name.clone();
            same_settings(existing, &candidate, &self.secrets)
        }) {
            return existing.name.clone();
        }
        let mut name = base.clone();
        let mut n = 2;
        while self.profiles.iter().any(|existing| existing.name == name) {
            name = format!("{base}-{n}");
            n += 1;
        }
        profile.name = name.clone();
        for (field, suffix) in [("client_secret", "ClientSecret"), ("password", "Password")] {
            let value = match field {
                "client_secret" => &mut profile.client_secret,
                _ => &mut profile.password,
            };
            if !value.is_empty() && variable_spans(value).is_empty() {
                let variable = secret_variable(&name, suffix);
                self.secrets.push(KeyValue::new(&variable, value.clone()));
                *value = format!("{{{{{variable}}}}}");
            }
        }
        self.profiles.push(profile);
        name
    }

    /// Adds every profile and secret of `other`. A profile with the same settings as one already
    /// here is shared, and one whose name is taken by different settings is renamed. Returns how
    /// the names of `other` map to the names here, `(old, new)`, for the ones that changed: look
    /// a name up once, never chain the pairs.
    pub fn merge(&mut self, other: &ProfileBuilder) -> Vec<(String, String)> {
        for secret in &other.secrets {
            if !self.secrets.iter().any(|entry| entry.key == secret.key) {
                self.secrets.push(secret.clone());
            }
        }
        let mut renames = Vec::new();
        for profile in &other.profiles {
            if let Some(same) = self.profiles.iter().find(|existing| {
                let mut candidate = profile.clone();
                candidate.name = existing.name.clone();
                **existing == candidate
            }) {
                if same.name != profile.name {
                    renames.push((profile.name.clone(), same.name.clone()));
                }
                continue;
            }
            let mut renamed = profile.clone();
            let mut n = 2;
            while self
                .profiles
                .iter()
                .any(|existing| existing.name == renamed.name)
            {
                renamed.name = format!("{}-{n}", profile.name);
                n += 1;
            }
            if renamed.name != profile.name {
                renames.push((profile.name.clone(), renamed.name.clone()));
            }
            self.profiles.push(renamed);
        }
        renames
    }

    /// The name `name` of a merged builder has here, after [`Self::merge`] returned `renames`.
    pub fn renamed<'a>(renames: &'a [(String, String)], name: &'a str) -> &'a str {
        renames
            .iter()
            .find(|(old, _)| old == name)
            .map_or(name, |(_, new)| new.as_str())
    }
}

/// Whether `existing` and `candidate` are the same configuration, a secret of `candidate`
/// written in clear matching the value of the variable `existing` holds instead.
fn same_settings(existing: &OAuthProfile, candidate: &OAuthProfile, secrets: &[KeyValue]) -> bool {
    let resolved = |text: &str| -> String {
        let spans = variable_spans(text);
        match spans.as_slice() {
            [span] if span.range.start == 0 && span.range.end == text.len() => secrets
                .iter()
                .find(|entry| entry.key == span.name)
                .map(|entry| entry.value.clone())
                .unwrap_or_else(|| text.to_string()),
            _ => text.to_string(),
        }
    };
    let mut left = existing.clone();
    left.client_secret = resolved(&left.client_secret);
    left.password = resolved(&left.password);
    left == *candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn fields(pairs: &[(&str, &str)]) -> OAuthFields {
        OAuthFields(
            pairs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        )
    }

    #[test]
    fn postman_and_bruno_fields_read_the_same() {
        let mut notes = Vec::new();
        let postman = profile_from_fields(
            &fields(&[
                ("grant_type", "client_credentials"),
                ("accessTokenUrl", "https://auth/token"),
                ("clientId", "id"),
                ("clientSecret", "s"),
                ("client_authentication", "body"),
                ("addTokenTo", "queryParams"),
            ]),
            &mut notes,
        );
        let bruno = profile_from_fields(
            &fields(&[
                ("grant_type", "client_credentials"),
                ("access_token_url", "https://auth/token"),
                ("client_id", "id"),
                ("client_secret", "s"),
                ("credentials_placement", "body"),
                ("token_placement", "url"),
            ]),
            &mut notes,
        );
        assert_eq!(postman, bruno);
        assert_eq!(postman.client_auth, ClientAuth::InBody);
        assert_eq!(postman.token_placement, TokenPlacement::Query);
        assert!(notes.is_empty());
    }

    #[test]
    fn grants_and_redirects() {
        let mut notes = Vec::new();
        let code = profile_from_fields(
            &fields(&[
                ("grant_type", "authorization_code_with_pkce"),
                ("redirect_uri", "https://oauth.pstmn.io/v1/callback"),
            ]),
            &mut notes,
        );
        assert!(code.pkce);
        assert_eq!(code.redirect, OAuthRedirect::Loopback);
        assert_eq!(notes.len(), 1);
        let implicit = profile_from_fields(&fields(&[("grant_type", "implicit")]), &mut notes);
        assert_eq!(implicit.grant, OAuthGrant::AuthorizationCode);
        assert_eq!(notes.len(), 2);
        let fixed = profile_from_fields(
            &fields(&[
                ("grant_type", "authorization_code"),
                ("callback_url", "http://localhost:8765/cb"),
            ]),
            &mut notes,
        );
        assert!(!fixed.pkce);
        assert_eq!(fixed.redirect, OAuthRedirect::Fixed);
        let password = profile_from_fields(
            &fields(&[("grant_type", "password_credentials"), ("username", "ana")]),
            &mut notes,
        );
        assert_eq!(password.grant, OAuthGrant::Password);
        assert_eq!(password.username, "ana");
    }

    #[test]
    fn the_builder_shares_profiles_and_moves_secrets() {
        let mut builder = ProfileBuilder::default();
        let mut profile = OAuthProfile::new("", OAuthGrant::ClientCredentials);
        profile.token_url = "https://auth/token".into();
        profile.client_secret = "s3cr3t".into();
        assert_eq!(builder.add("Acme API", profile.clone()), "acme-api");
        // The same configuration again, from another request: shared.
        assert_eq!(builder.add("Other", profile.clone()), "acme-api");
        let mut other = profile.clone();
        other.scope = "write".into();
        assert_eq!(builder.add("Acme API", other), "acme-api-2");
        assert_eq!(builder.profiles[0].client_secret, "{{acmeApiClientSecret}}");
        assert_eq!(
            builder.secrets[0],
            KeyValue::new("acmeApiClientSecret", "s3cr3t")
        );
        let mut variable = profile;
        variable.client_secret = "{{secret}}".into();
        variable.scope = "x".into();
        builder.add("v", variable);
        assert_eq!(builder.secrets.len(), 2);
        assert_eq!(profile_slug("  Ünïcode ++ "), "n-code");
        assert_eq!(profile_slug("!!!"), "oauth");
    }

    #[test]
    fn merging_renames_clashing_profiles() {
        let mut first = ProfileBuilder::default();
        first.add("acme", OAuthProfile::new("", OAuthGrant::ClientCredentials));
        let mut second = ProfileBuilder::default();
        second.add("acme", OAuthProfile::new("", OAuthGrant::Password));
        second.add("acme", OAuthProfile::new("", OAuthGrant::ClientCredentials));
        let renames = first.merge(&second);
        // Different settings under a taken name: renamed. The same settings as one already
        // there: shared, under the name it has here.
        assert_eq!(
            renames,
            vec![
                ("acme".to_string(), "acme-2".to_string()),
                ("acme-2".to_string(), "acme".to_string())
            ]
        );
        assert_eq!(first.profiles.len(), 2);
        assert_eq!(ProfileBuilder::renamed(&renames, "acme-2"), "acme");
        assert_eq!(ProfileBuilder::renamed(&renames, "other"), "other");
    }
}
