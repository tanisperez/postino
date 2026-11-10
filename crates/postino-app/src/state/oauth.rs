//! OAuth 2.0 in the app (#225, #226, design page "OAuth"): the workspace profiles, which requests
//! use each, the hints of the Profile settings panel, the token card's texts, the response
//! notices and the error cards. Kept free of `gpui` types so every rule is unit tested.

use std::time::{Duration, SystemTime};

use postino_core::{Auth, OAuthGrant, OAuthProfile, TokenPlacement, VariableKind, variable_spans};
use postino_oauth::{OAuthError, TokenEvent, TokenSet};
use postino_workspace::{EnvLayers, Node};
use rust_i18n::t;

/// `12 min`, `45 s`, `2 h`: a short duration in the console and the token card.
pub fn format_ago(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 2 * 3_600 {
        format!("{} h", seconds / 3_600)
    } else if seconds >= 60 {
        format!("{} min", seconds / 60)
    } else {
        format!("{seconds} s")
    }
}

/// `11:42`, the local time of `time`.
pub fn format_clock_short(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%H:%M")
        .to_string()
}

/// Whole minutes from now until `time`, rounded to the nearest, never negative.
pub fn minutes_until(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::now())
        .map(|duration| (duration.as_secs() + 30) / 60)
        .unwrap_or(0)
}

/// `Today 10:04`, or the date and time when it was not today.
pub fn format_obtained(time: SystemTime) -> String {
    let local = chrono::DateTime::<chrono::Local>::from(time);
    if local.date_naive() == chrono::Local::now().date_naive() {
        t!("oauth.today_at", time = local.format("%H:%M").to_string()).into_owned()
    } else {
        local.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// The grant as the profile picker shows it under a name.
pub fn grant_label(profile: &OAuthProfile) -> String {
    match profile.grant {
        OAuthGrant::AuthorizationCode if profile.pkce => t!("oauth.grant_code_pkce"),
        OAuthGrant::AuthorizationCode => t!("oauth.grant_code"),
        OAuthGrant::ClientCredentials => t!("oauth.grant_client_credentials"),
        OAuthGrant::Password => t!("oauth.grant_password"),
    }
    .into_owned()
}

/// The label of a grant in the grant type selector.
pub fn grant_option_label(grant: OAuthGrant) -> String {
    match grant {
        OAuthGrant::AuthorizationCode => t!("oauth.grant_code"),
        OAuthGrant::ClientCredentials => t!("oauth.grant_client_credentials"),
        OAuthGrant::Password => t!("oauth.grant_password"),
    }
    .into_owned()
}

/// A request that uses a profile: its id, name and method, for the "Used by" list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileUser {
    /// The request id.
    pub id: String,
    /// The request name.
    pub name: String,
    /// Its method, `None` when the file is broken.
    pub method: Option<postino_core::Method>,
}

/// The requests of `tree` that use the profile `name`, in tree order.
pub fn profile_users(tree: &[Node], name: &str) -> Vec<ProfileUser> {
    let mut users = Vec::new();
    collect_users(tree, &mut |entry| {
        if entry.oauth_profile.as_deref() == Some(name) {
            users.push(ProfileUser {
                id: entry.id.clone(),
                name: entry.name.clone(),
                method: entry.method.clone(),
            });
        }
    });
    users
}

/// How many requests of `tree` use each profile.
pub fn usage_count(tree: &[Node], name: &str) -> usize {
    let mut count = 0;
    collect_users(tree, &mut |entry| {
        if entry.oauth_profile.as_deref() == Some(name) {
            count += 1;
        }
    });
    count
}

fn collect_users(tree: &[Node], visit: &mut impl FnMut(&postino_workspace::RequestEntry)) {
    for node in tree {
        match node {
            Node::Folder(folder) => collect_users(&folder.children, visit),
            Node::Request(entry) => visit(entry),
        }
    }
}

/// A field of the Profile settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileFieldKind {
    /// The authorization URL.
    AuthUrl,
    /// The token URL.
    TokenUrl,
    /// The client id.
    ClientId,
    /// The client secret.
    ClientSecret,
    /// The user name.
    Username,
    /// The password.
    Password,
    /// The scope.
    Scope,
    /// The fixed or manual redirect URL.
    RedirectUrl,
    /// The token prefix or query parameter name ("Send token as").
    TokenName,
}

impl ProfileFieldKind {
    /// The fields of `grant`, in the order the panel shows them (design 3a·2).
    pub fn for_grant(grant: OAuthGrant) -> &'static [ProfileFieldKind] {
        match grant {
            OAuthGrant::AuthorizationCode => &[
                ProfileFieldKind::AuthUrl,
                ProfileFieldKind::TokenUrl,
                ProfileFieldKind::ClientId,
                ProfileFieldKind::ClientSecret,
                ProfileFieldKind::Scope,
            ],
            OAuthGrant::ClientCredentials => &[
                ProfileFieldKind::TokenUrl,
                ProfileFieldKind::ClientId,
                ProfileFieldKind::ClientSecret,
                ProfileFieldKind::Scope,
            ],
            OAuthGrant::Password => &[
                ProfileFieldKind::TokenUrl,
                ProfileFieldKind::ClientId,
                ProfileFieldKind::ClientSecret,
                ProfileFieldKind::Username,
                ProfileFieldKind::Password,
                ProfileFieldKind::Scope,
            ],
        }
    }

    /// The label on the left.
    pub fn label(self) -> String {
        match self {
            ProfileFieldKind::AuthUrl => t!("oauth.field_auth_url"),
            ProfileFieldKind::TokenUrl => t!("oauth.field_token_url"),
            ProfileFieldKind::ClientId => t!("oauth.field_client_id"),
            ProfileFieldKind::ClientSecret => t!("oauth.field_client_secret"),
            ProfileFieldKind::Username => t!("request.auth.username"),
            ProfileFieldKind::Password => t!("request.auth.password"),
            ProfileFieldKind::Scope => t!("oauth.scope"),
            ProfileFieldKind::RedirectUrl => t!("oauth.field_redirect_url"),
            ProfileFieldKind::TokenName => t!("oauth.field_token_name"),
        }
        .into_owned()
    }

    /// Whether the field holds a secret: masked, and warned about when written in clear.
    pub fn is_secret(self) -> bool {
        matches!(
            self,
            ProfileFieldKind::ClientSecret | ProfileFieldKind::Password
        )
    }

    /// The field's text in `profile`.
    pub fn value(self, profile: &OAuthProfile) -> &str {
        match self {
            ProfileFieldKind::AuthUrl => &profile.auth_url,
            ProfileFieldKind::TokenUrl => &profile.token_url,
            ProfileFieldKind::ClientId => &profile.client_id,
            ProfileFieldKind::ClientSecret => &profile.client_secret,
            ProfileFieldKind::Username => &profile.username,
            ProfileFieldKind::Password => &profile.password,
            ProfileFieldKind::Scope => &profile.scope,
            ProfileFieldKind::RedirectUrl => &profile.redirect_url,
            ProfileFieldKind::TokenName => match profile.token_placement {
                TokenPlacement::Header => &profile.token_prefix,
                TokenPlacement::Query => &profile.token_param,
            },
        }
    }

    /// Sets the field's text in `profile`.
    pub fn set(self, profile: &mut OAuthProfile, value: String) {
        match self {
            ProfileFieldKind::AuthUrl => profile.auth_url = value,
            ProfileFieldKind::TokenUrl => profile.token_url = value,
            ProfileFieldKind::ClientId => profile.client_id = value,
            ProfileFieldKind::ClientSecret => profile.client_secret = value,
            ProfileFieldKind::Username => profile.username = value,
            ProfileFieldKind::Password => profile.password = value,
            ProfileFieldKind::Scope => profile.scope = value,
            ProfileFieldKind::RedirectUrl => profile.redirect_url = value,
            ProfileFieldKind::TokenName => match profile.token_placement {
                TokenPlacement::Header => profile.token_prefix = value,
                TokenPlacement::Query => profile.token_param = value,
            },
        }
    }

    /// The placeholder of an empty field: examples that use variables, as the design asks for
    /// new profiles.
    pub fn placeholder(self, grant: OAuthGrant) -> String {
        match self {
            ProfileFieldKind::AuthUrl => "{{authBaseUrl}}/authorize".to_string(),
            ProfileFieldKind::TokenUrl => "{{authBaseUrl}}/oauth/token".to_string(),
            ProfileFieldKind::ClientId => "{{clientId}}".to_string(),
            ProfileFieldKind::ClientSecret if grant == OAuthGrant::AuthorizationCode => {
                t!("oauth.optional_with_pkce").into_owned()
            }
            ProfileFieldKind::ClientSecret => "{{clientSecret}}".to_string(),
            ProfileFieldKind::Username => "{{apiUser}}".to_string(),
            ProfileFieldKind::Password => "{{apiPassword}}".to_string(),
            ProfileFieldKind::Scope => "openid profile".to_string(),
            ProfileFieldKind::RedirectUrl => "http://localhost:8765/callback".to_string(),
            ProfileFieldKind::TokenName => String::new(),
        }
    }
}

/// What the right column of a Profile settings field says (design 3a·2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldHint {
    /// Nothing.
    None,
    /// The field is one variable, defined in this file of the active environment
    /// (`dev.env`, `dev.local.env`). `local` paints it green.
    File {
        /// The file name.
        file: String,
        /// Whether it is the gitignored `.local.env`.
        local: bool,
    },
    /// The client secret of an authorization code profile is empty: a public client.
    PublicClient,
    /// A secret written in clear in the shared profile: "Move to .local.env", orange.
    PlainSecret,
}

/// The variable `text` consists of, when it is exactly one `{{name}}` marker.
pub fn single_variable(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let spans = variable_spans(trimmed);
    match spans.as_slice() {
        [span]
            if span.kind == VariableKind::Variable
                && span.range.start == 0
                && span.range.end == trimmed.len() =>
        {
            Some(span.name.clone())
        }
        _ => None,
    }
}

/// The hint of `field` holding `text`, for a profile of `grant`, in the environment `environment`
/// whose files are `layers` (`None` for "No environment").
pub fn field_hint(
    field: ProfileFieldKind,
    text: &str,
    grant: OAuthGrant,
    environment: &str,
    layers: Option<&EnvLayers>,
) -> FieldHint {
    if text.trim().is_empty() {
        return if field == ProfileFieldKind::ClientSecret && grant == OAuthGrant::AuthorizationCode
        {
            FieldHint::PublicClient
        } else {
            FieldHint::None
        };
    }
    if let Some(name) = single_variable(text) {
        let Some(layers) = layers else {
            return FieldHint::None;
        };
        let has = |variables: &Option<Vec<postino_core::KeyValue>>| {
            variables
                .as_ref()
                .is_some_and(|variables| variables.iter().any(|variable| variable.key == name))
        };
        // The local file wins when both define it, as when sending.
        return if has(&layers.local) {
            FieldHint::File {
                file: format!("{environment}.local.env"),
                local: true,
            }
        } else if has(&layers.base) {
            FieldHint::File {
                file: format!("{environment}.env"),
                local: false,
            }
        } else {
            FieldHint::None
        };
    }
    if field.is_secret() && variable_spans(text).is_empty() {
        return FieldHint::PlainSecret;
    }
    FieldHint::None
}

/// The variable name offered by "Move to .local.env": the profile name in camel case and the
/// field, `acme-sso` and Password giving `acmeSsoPassword`.
pub fn suggested_variable(profile: &str, field: ProfileFieldKind) -> String {
    let mut name = String::new();
    let mut upper = false;
    for c in profile.chars() {
        if c == '-' || c == '_' {
            upper = true;
        } else if name.is_empty() {
            name.push(c.to_ascii_lowercase());
        } else if upper {
            name.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            name.push(c);
        }
    }
    let suffix = match field {
        ProfileFieldKind::Password => "Password",
        ProfileFieldKind::ClientSecret => "ClientSecret",
        _ => "Secret",
    };
    if name.is_empty() {
        let mut lower = suffix.to_string();
        lower[..1].make_ascii_lowercase();
        return lower;
    }
    format!("{name}{suffix}")
}

/// A name for a copy of `name` not in `taken`: `acme-sso-copy`, `acme-sso-copy-2`, ...
pub fn copy_name(name: &str, taken: &[OAuthProfile]) -> String {
    let free = |candidate: &str| taken.iter().all(|profile| profile.name != candidate);
    let base = format!("{name}-copy");
    if free(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| free(candidate))
        .unwrap_or(base)
}

/// Why a profile name cannot be used, `None` when it can.
pub fn name_problem(name: &str, taken: &[OAuthProfile], current: Option<&str>) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some(t!("oauth.name_empty").into_owned());
    }
    if !postino_core::valid_profile_name(name) {
        return Some(t!("oauth.name_invalid").into_owned());
    }
    if Some(name) != current && taken.iter().any(|profile| profile.name == name) {
        return Some(t!("oauth.name_taken", name = name).into_owned());
    }
    None
}

/// The state of the token card (design 3a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardState {
    /// A token that can be sent.
    Valid,
    /// An expired token (or one within the 30 s margin).
    Expired,
    /// No token for this environment.
    None,
}

/// What the token card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenCard {
    /// Which of the three states.
    pub state: CardState,
    /// "Token valid", "Token expired 12 min ago", "No token for dev yet".
    pub title: String,
    /// The line under the title.
    pub subtitle: String,
    /// The primary button: "Refresh now", "Sign in now", "Get token now".
    pub primary: String,
    /// The second button when there is a token: "Sign in again" or "Get new token".
    pub again: String,
    /// The line at the right of the buttons.
    pub foot: String,
    /// "Sends Authorization: Bearer eyJhbGciOi…".
    pub sends: String,
}

/// The first `count` characters of `text` and an ellipsis when it was longer.
pub fn abbreviate(text: &str, count: usize) -> String {
    let mut out: String = text.chars().take(count).collect();
    if text.chars().count() > count {
        out.push('…');
    }
    out
}

/// The texts of the token card of `profile` for `environment` with the stored `token`.
pub fn token_card(
    profile: &OAuthProfile,
    environment: &str,
    token: Option<&TokenSet>,
    now: SystemTime,
) -> TokenCard {
    let interactive = profile.grant.is_interactive();
    let state = match token {
        Some(token) if token.is_valid_at(now) => CardState::Valid,
        Some(_) => CardState::Expired,
        None => CardState::None,
    };
    let has_refresh = token.is_some_and(|token| token.refresh_token.is_some());
    let title = match (state, token) {
        (CardState::Valid, _) => t!("oauth.card_valid").into_owned(),
        (CardState::Expired, Some(token)) => t!(
            "oauth.card_expired",
            ago = format_ago(token.expired_for_at(now).unwrap_or_default())
        )
        .into_owned(),
        _ if environment.is_empty() => t!("oauth.card_none_no_env").into_owned(),
        _ => t!("oauth.card_none", env = environment).into_owned(),
    };
    let subtitle = match state {
        CardState::Valid => {
            let expiry = match token.and_then(|token| token.remaining_at(now)) {
                Some(remaining) => {
                    t!("oauth.expires_in", time = format_ago(remaining)).into_owned()
                }
                None => t!("oauth.never_expires").into_owned(),
            };
            let renewal = if has_refresh {
                t!("oauth.renewed_with_refresh")
            } else if interactive {
                t!("oauth.renewed_by_sign_in")
            } else {
                t!("oauth.renewed_by_grant")
            };
            format!("{expiry} · {renewal}")
        }
        CardState::Expired if has_refresh && interactive => {
            t!("oauth.expired_refresh_no_browser").into_owned()
        }
        CardState::Expired if has_refresh => t!("oauth.expired_refresh").into_owned(),
        CardState::Expired if interactive => t!("oauth.expired_sign_in").into_owned(),
        CardState::Expired => t!("oauth.expired_grant").into_owned(),
        CardState::None if interactive => t!("oauth.none_sign_in").into_owned(),
        CardState::None => t!("oauth.none_grant").into_owned(),
    };
    let primary = match state {
        CardState::Valid | CardState::Expired => t!("oauth.refresh_now"),
        CardState::None if interactive => t!("oauth.sign_in_now"),
        CardState::None => t!("oauth.get_token_now"),
    }
    .into_owned();
    let again = if interactive {
        t!("oauth.sign_in_again")
    } else {
        t!("oauth.get_new_token")
    }
    .into_owned();
    let foot = match state {
        CardState::None => t!("oauth.same_token", profile = profile.name).into_owned(),
        _ => t!("oauth.stored_memory").into_owned(),
    };
    let value = match (state, token) {
        (CardState::Valid, Some(token)) => abbreviate(&token.access_token, 10),
        (CardState::Expired, _) => t!("oauth.renewed_on_send").into_owned(),
        _ => t!("oauth.obtained_on_send").into_owned(),
    };
    let sends = match profile.token_placement {
        TokenPlacement::Header if profile.token_prefix.trim().is_empty() => {
            format!("Authorization: {value}")
        }
        TokenPlacement::Header => {
            format!("Authorization: {} {value}", profile.token_prefix.trim())
        }
        TokenPlacement::Query => format!("?{}={value}", profile.token_param),
    };
    TokenCard {
        state,
        title,
        subtitle,
        primary,
        again,
        foot,
        sends,
    }
}

/// The "Obtained" line of the card: `Today 10:04 · refresh token ✓ · id token ✓`.
pub fn obtained_line(token: &TokenSet) -> String {
    let mut parts = vec![format_obtained(token.obtained_at_time())];
    parts.push(if token.refresh_token.is_some() {
        t!("oauth.has_refresh").into_owned()
    } else {
        t!("oauth.no_refresh").into_owned()
    });
    if token.id_token.is_some() {
        parts.push(t!("oauth.has_id_token").into_owned());
    }
    parts.join(" · ")
}

/// A claim as the card's Claims row shows it: `sub "u_8121"`, `exp 10:46`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimPart {
    /// The claim name.
    pub name: String,
    /// Its value as shown.
    pub value: String,
    /// Whether the value is a time (`exp`), painted amber.
    pub time: bool,
}

/// The claims shown on the card: `sub`, `aud` and `exp`, when the token has them.
pub fn card_claims(token: &TokenSet, now: SystemTime) -> Vec<ClaimPart> {
    let claims = token.claims();
    let mut parts = Vec::new();
    for name in ["sub", "aud"] {
        if let Some((_, value)) = claims.iter().find(|(claim, _)| claim == name) {
            let shown = match value {
                serde_json::Value::Array(items) => items
                    .iter()
                    .map(|item| item.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            parts.push(ClaimPart {
                name: name.to_string(),
                value: shown,
                time: false,
            });
        }
    }
    if let Some(exp) = claims
        .iter()
        .find(|(claim, _)| claim == "exp")
        .and_then(|(_, value)| value.as_u64())
    {
        let at = postino_oauth::from_unix_seconds(exp);
        let mut value = format_clock_short(at);
        if postino_oauth::unix_seconds(now) >= exp {
            value.push(' ');
            value.push_str(&t!("oauth.expired_suffix"));
        }
        parts.push(ClaimPart {
            name: "exp".to_string(),
            value,
            time: true,
        });
    }
    parts
}

/// Every claim as pretty JSON for the "All claims" dialog, with the times (`exp`, `iat`, `nbf`,
/// `auth_time`) followed by their local time in a comment line.
pub fn all_claims_text(token: &TokenSet) -> String {
    let claims = token.claims();
    let object: serde_json::Map<String, serde_json::Value> = claims.iter().cloned().collect();
    let mut text =
        serde_json::to_string_pretty(&serde_json::Value::Object(object)).unwrap_or_default();
    let times: Vec<String> = claims
        .iter()
        .filter(|(name, _)| matches!(name.as_str(), "exp" | "iat" | "nbf" | "auth_time"))
        .filter_map(|(name, value)| {
            let seconds = value.as_u64()?;
            let local =
                chrono::DateTime::<chrono::Local>::from(postino_oauth::from_unix_seconds(seconds));
            Some(format!("{name}: {}", local.format("%Y-%m-%d %H:%M:%S")))
        })
        .collect();
    if !times.is_empty() {
        text.push_str("\n\n");
        text.push_str(&times.join("\n"));
    }
    text
}

/// What the response shows about the token, besides the console (designs 3b and 3c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenNotice {
    /// The chip next to the status: "Token renewed".
    Renewed {
        /// The tooltip: profile, environment and new expiry.
        tooltip: String,
    },
    /// The green line above the body: "New token for acme-sso · dev · expires in 60 min".
    New {
        /// The text.
        text: String,
    },
}

/// The notice of a run that got a token by `event`.
pub fn token_notice(
    profile: &str,
    environment: &str,
    event: TokenEvent,
    token: &TokenSet,
) -> Option<TokenNotice> {
    let who = if environment.is_empty() {
        profile.to_string()
    } else {
        format!("{profile} · {environment}")
    };
    let expiry = match token.expires_at_time() {
        Some(at) => t!("oauth.expires_in_minutes", minutes = minutes_until(at)).into_owned(),
        None => t!("oauth.never_expires").into_owned(),
    };
    match event {
        TokenEvent::Reused => None,
        TokenEvent::Renewed => {
            let expiry = match token.expires_at_time() {
                Some(at) => t!("oauth.expires_at", time = format_clock_short(at)).into_owned(),
                None => t!("oauth.never_expires").into_owned(),
            };
            Some(TokenNotice::Renewed {
                tooltip: format!("{who} · {expiry}"),
            })
        }
        TokenEvent::New => Some(TokenNotice::New {
            text: t!("oauth.new_token_notice", who = who, expiry = expiry).into_owned(),
        }),
    }
}

/// The color of an error card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorTone {
    /// Red: the token could not be obtained.
    Danger,
    /// Amber: the provider rejected the redirect URL.
    Warning,
    /// Blue: the API rejected a token.
    Info,
}

/// The buttons of an error card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorAction {
    /// Opens the Auth tab with Profile settings unfolded.
    EditProfile,
    /// Opens the Console at the token request.
    OpenInConsole,
    /// Copies the redirect URL to register.
    CopyRedirectUrl,
    /// Gets a new token and sends again.
    NewTokenAndResend,
    /// Hides the card.
    Dismiss,
    /// Sends again (after a cancel or a timeout).
    Retry,
}

impl ErrorAction {
    /// The button label.
    pub fn label(self) -> String {
        match self {
            ErrorAction::EditProfile => t!("oauth.edit_profile"),
            ErrorAction::OpenInConsole => t!("oauth.open_in_console"),
            ErrorAction::CopyRedirectUrl => t!("oauth.copy_redirect_url"),
            ErrorAction::NewTokenAndResend => t!("oauth.new_token_and_resend"),
            ErrorAction::Dismiss => t!("oauth.dismiss"),
            ErrorAction::Retry => t!("oauth.try_again"),
        }
        .into_owned()
    }
}

/// An error card of the response panel (design 3d).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCard {
    /// Its color.
    pub tone: ErrorTone,
    /// The bold line.
    pub title: String,
    /// The explanation.
    pub message: String,
    /// The monospace line under it.
    pub detail: String,
    /// The first (filled) and second buttons.
    pub actions: [ErrorAction; 2],
}

/// The explanation of a standard OAuth error code, `None` for any other.
fn explain_code(code: &str, environment: &str) -> Option<String> {
    let env = if environment.is_empty() {
        t!("oauth.no_environment").into_owned()
    } else {
        environment.to_string()
    };
    Some(
        match code {
            "invalid_client" => t!("oauth.code_invalid_client", env = env),
            "invalid_grant" => t!("oauth.code_invalid_grant"),
            "invalid_scope" => t!("oauth.code_invalid_scope"),
            "unauthorized_client" => t!("oauth.code_unauthorized_client"),
            "unsupported_grant_type" => t!("oauth.code_unsupported_grant_type"),
            "access_denied" => t!("oauth.code_access_denied"),
            "invalid_request" => t!("oauth.code_invalid_request"),
            _ => return None,
        }
        .into_owned(),
    )
}

/// The card of a run whose token could not be obtained. `exchange_line` is the last token
/// request as `POST auth.dev.acme.dev/oauth/token → 401`, when there was one.
pub fn error_card(
    profile: &str,
    environment: &str,
    error: &OAuthError,
    exchange_line: Option<String>,
) -> ErrorCard {
    let title = t!("oauth.couldnt_get_token", profile = profile).into_owned();
    let default_actions = [ErrorAction::EditProfile, ErrorAction::OpenInConsole];
    match error {
        OAuthError::Denied { error: code, .. } if code == "redirect_uri_mismatch" => ErrorCard {
            tone: ErrorTone::Warning,
            title: t!("oauth.redirect_rejected").into_owned(),
            message: t!("oauth.redirect_rejected_message").into_owned(),
            detail: format!("error={code}"),
            actions: [ErrorAction::CopyRedirectUrl, ErrorAction::EditProfile],
        },
        OAuthError::Server {
            error: code,
            description,
            ..
        }
        | OAuthError::Denied {
            error: code,
            description,
        } => {
            let explanation = explain_code(code, environment)
                .or_else(|| description.clone())
                .unwrap_or_default();
            let message = if explanation.is_empty() {
                code.clone()
            } else {
                format!("{code}: {explanation}")
            };
            ErrorCard {
                tone: ErrorTone::Danger,
                title,
                message,
                detail: exchange_line.unwrap_or_else(|| format!("error={code}")),
                actions: default_actions,
            }
        }
        OAuthError::Cancelled | OAuthError::TimedOut => ErrorCard {
            tone: ErrorTone::Danger,
            title,
            message: if matches!(error, OAuthError::Cancelled) {
                t!("oauth.cancelled").into_owned()
            } else {
                t!("oauth.timed_out").into_owned()
            },
            detail: String::new(),
            actions: [ErrorAction::Retry, ErrorAction::EditProfile],
        },
        OAuthError::UnknownProfile(name) => ErrorCard {
            tone: ErrorTone::Danger,
            title,
            message: t!("oauth.unknown_profile", profile = name).into_owned(),
            detail: String::new(),
            actions: [ErrorAction::EditProfile, ErrorAction::OpenInConsole],
        },
        other => ErrorCard {
            tone: ErrorTone::Danger,
            title,
            message: other.to_string(),
            detail: exchange_line.unwrap_or_default(),
            actions: default_actions,
        },
    }
}

/// The card of a 401 from the API to a request sent with a stored token (design 3d, third card).
/// `www_authenticate` is that response header, when it has one.
pub fn unauthorized_card(www_authenticate: Option<&str>) -> ErrorCard {
    ErrorCard {
        tone: ErrorTone::Info,
        title: t!("oauth.api_rejected").into_owned(),
        message: t!("oauth.api_rejected_message").into_owned(),
        detail: www_authenticate
            .map(|value| format!("WWW-Authenticate: {value}"))
            .unwrap_or_default(),
        actions: [ErrorAction::NewTokenAndResend, ErrorAction::Dismiss],
    }
}

/// `POST auth.dev.acme.dev/oauth/token → 401`: the line under an error card.
pub fn exchange_line(method: &str, url: &str, status: Option<u16>) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let without_query = without_scheme.split('?').next().unwrap_or_default();
    match status {
        Some(status) => format!("{method} {without_query} → {status}"),
        None => format!("{method} {without_query}"),
    }
}

/// The profile name of a request's auth, when it is OAuth 2.0.
pub fn profile_of(auth: &Auth) -> Option<&str> {
    match auth {
        Auth::OAuth2 { profile, .. } => Some(profile),
        _ => None,
    }
}

/// The redirect URL the Redirect row shows for `profile`.
pub fn redirect_shown(profile: &OAuthProfile) -> String {
    match profile.redirect {
        postino_core::OAuthRedirect::Loopback => "http://127.0.0.1:{port}/callback".to_string(),
        _ => profile.redirect_url.clone(),
    }
}

/// The note under the Redirect row.
pub fn redirect_note(redirect: postino_core::OAuthRedirect) -> String {
    match redirect {
        postino_core::OAuthRedirect::Loopback => t!("oauth.redirect_loopback_note"),
        postino_core::OAuthRedirect::Fixed => t!("oauth.redirect_fixed_note"),
        postino_core::OAuthRedirect::Manual => t!("oauth.redirect_manual_note"),
    }
    .into_owned()
}

/// The summary next to "Advanced": `Basic header · Bearer header · 1 extra param`.
pub fn advanced_summary(profile: &OAuthProfile) -> String {
    let client = match profile.client_auth {
        postino_core::ClientAuth::BasicHeader => t!("oauth.client_auth_basic"),
        postino_core::ClientAuth::InBody => t!("oauth.client_auth_body"),
    };
    let token = match profile.token_placement {
        TokenPlacement::Header => format!(
            "{} {}",
            profile.token_prefix.trim(),
            t!("oauth.header_suffix")
        )
        .trim()
        .to_string(),
        TokenPlacement::Query => format!("{} {}", profile.token_param, t!("oauth.query_suffix")),
    };
    let count = profile
        .extra_params
        .iter()
        .filter(|param| !param.key.trim().is_empty())
        .count();
    let params = crate::state::locale::plural("oauth.extra_params", count);
    format!("{client} · {token} · {params}")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_core::{KeyValue, OAuthRedirect};
    use postino_workspace::{Folder, RequestEntry};
    use pretty_assertions::assert_eq;

    fn layers() -> EnvLayers {
        EnvLayers {
            base: Some(vec![
                KeyValue::new("clientId", "postino-dev"),
                KeyValue::new("clientSecret", "overridden"),
            ]),
            local: Some(vec![KeyValue::new("clientSecret", "s3cr3t")]),
        }
    }

    #[test]
    fn hints_name_the_file_of_a_single_variable() {
        let grant = OAuthGrant::ClientCredentials;
        assert_eq!(
            field_hint(
                ProfileFieldKind::ClientId,
                "{{clientId}}",
                grant,
                "dev",
                Some(&layers())
            ),
            FieldHint::File {
                file: "dev.env".into(),
                local: false
            }
        );
        assert_eq!(
            field_hint(
                ProfileFieldKind::ClientSecret,
                " {{ clientSecret }} ",
                grant,
                "dev",
                Some(&layers())
            ),
            FieldHint::File {
                file: "dev.local.env".into(),
                local: true
            }
        );
        assert_eq!(
            field_hint(
                ProfileFieldKind::TokenUrl,
                "{{clientId}}/token",
                grant,
                "dev",
                Some(&layers())
            ),
            FieldHint::None
        );
        assert_eq!(
            field_hint(
                ProfileFieldKind::ClientId,
                "{{missing}}",
                grant,
                "dev",
                Some(&layers())
            ),
            FieldHint::None
        );
        assert_eq!(
            field_hint(ProfileFieldKind::ClientId, "{{clientId}}", grant, "", None),
            FieldHint::None
        );
    }

    #[test]
    fn secrets_in_clear_and_public_clients() {
        assert_eq!(
            field_hint(
                ProfileFieldKind::Password,
                "hunter2",
                OAuthGrant::Password,
                "dev",
                None
            ),
            FieldHint::PlainSecret
        );
        assert_eq!(
            field_hint(
                ProfileFieldKind::Username,
                "ana",
                OAuthGrant::Password,
                "dev",
                None
            ),
            FieldHint::None
        );
        assert_eq!(
            field_hint(
                ProfileFieldKind::ClientSecret,
                "",
                OAuthGrant::AuthorizationCode,
                "dev",
                None
            ),
            FieldHint::PublicClient
        );
        assert_eq!(
            field_hint(
                ProfileFieldKind::ClientSecret,
                "",
                OAuthGrant::ClientCredentials,
                "dev",
                None
            ),
            FieldHint::None
        );
    }

    #[test]
    fn suggested_names_and_copies() {
        assert_eq!(
            suggested_variable("acme-sso", ProfileFieldKind::Password),
            "acmeSsoPassword"
        );
        assert_eq!(
            suggested_variable("Partner_legacy", ProfileFieldKind::ClientSecret),
            "partnerLegacyClientSecret"
        );
        assert_eq!(
            suggested_variable("", ProfileFieldKind::Password),
            "password"
        );
        let taken = vec![
            OAuthProfile::new("a", OAuthGrant::Password),
            OAuthProfile::new("a-copy", OAuthGrant::Password),
        ];
        assert_eq!(copy_name("a", &taken), "a-copy-2");
        assert_eq!(copy_name("b", &taken), "b-copy");
        assert!(name_problem("a", &taken, None).is_some());
        assert!(name_problem("a", &taken, Some("a")).is_none());
        assert!(name_problem("bad name", &taken, None).is_some());
        assert!(name_problem(" ", &taken, None).is_some());
        assert!(name_problem("new", &taken, None).is_none());
    }

    #[test]
    fn usage_comes_from_the_tree() {
        let entry = |id: &str, profile: Option<&str>| {
            Node::Request(RequestEntry {
                id: id.into(),
                name: id.into(),
                broken: None,
                method: Some(postino_core::Method::Get),
                oauth_profile: profile.map(str::to_string),
            })
        };
        let tree = vec![
            Node::Folder(Folder {
                id: "charges".into(),
                name: "charges".into(),
                children: vec![entry("charges/a", Some("sso")), entry("charges/b", None)],
            }),
            entry("c", Some("sso")),
            entry("d", Some("machine")),
        ];
        assert_eq!(usage_count(&tree, "sso"), 2);
        let users = profile_users(&tree, "sso");
        assert_eq!(users[0].id, "charges/a");
        assert_eq!(users[0].method, Some(postino_core::Method::Get));
        assert_eq!(usage_count(&tree, "none"), 0);
    }

    fn token(expires_at: Option<u64>, refresh: bool) -> TokenSet {
        TokenSet {
            access_token: "eyJhbGciOiJSUzI1NiIsImtpZCI6Ijk4".into(),
            refresh_token: refresh.then(|| "r".to_string()),
            id_token: None,
            token_type: "Bearer".into(),
            expires_at,
            obtained_at: 1_000,
            scope: String::new(),
        }
    }

    #[test]
    fn the_card_follows_the_token() {
        let now = postino_oauth::from_unix_seconds(10_000);
        let profile = OAuthProfile::new("acme-sso", OAuthGrant::AuthorizationCode);
        let valid = token_card(&profile, "dev", Some(&token(Some(12_520), true)), now);
        assert_eq!(valid.state, CardState::Valid);
        assert_eq!(valid.title, "Token valid");
        assert_eq!(
            valid.subtitle,
            "Expires in 42 min · renewed automatically with the refresh token"
        );
        assert_eq!(valid.primary, "Refresh now");
        assert_eq!(valid.again, "Sign in again");
        assert_eq!(valid.foot, "Kept in memory until Postino closes");
        assert_eq!(valid.sends, "Authorization: Bearer eyJhbGciOi…");

        let expired = token_card(&profile, "dev", Some(&token(Some(9_280), true)), now);
        assert_eq!(expired.title, "Token expired 12 min ago");
        assert_eq!(
            expired.subtitle,
            "Send will renew it first with the refresh token, without opening the browser"
        );
        assert_eq!(expired.sends, "Authorization: Bearer (renewed on Send)");

        let none = token_card(&profile, "dev", None, now);
        assert_eq!(none.title, "No token for dev yet");
        assert_eq!(
            none.subtitle,
            "Send opens your browser to sign in, then sends the request"
        );
        assert_eq!(none.primary, "Sign in now");
        assert_eq!(none.foot, "Same token for every request using acme-sso");
        assert_eq!(none.sends, "Authorization: Bearer (obtained on Send)");

        let machine = OAuthProfile::new("machine", OAuthGrant::ClientCredentials);
        let none = token_card(&machine, "dev", None, now);
        assert_eq!(none.primary, "Get token now");
        assert_eq!(none.again, "Get new token");
        assert_eq!(none.subtitle, "Send gets one first, then sends the request");
    }

    #[test]
    fn notices_and_errors() {
        let mut set = token(
            Some(postino_oauth::unix_seconds(SystemTime::now()) + 3_600),
            true,
        );
        set.obtained_at = postino_oauth::unix_seconds(SystemTime::now());
        assert_eq!(
            token_notice("acme-sso", "dev", TokenEvent::New, &set),
            Some(TokenNotice::New {
                text: "New token for acme-sso · dev · expires in 60 min".into()
            })
        );
        assert!(matches!(
            token_notice("acme-sso", "dev", TokenEvent::Renewed, &set),
            Some(TokenNotice::Renewed { .. })
        ));
        assert_eq!(token_notice("a", "dev", TokenEvent::Reused, &set), None);

        let card = error_card(
            "acme-sso",
            "dev",
            &OAuthError::Server {
                status: 401,
                error: "invalid_client".into(),
                description: None,
            },
            Some(exchange_line(
                "POST",
                "https://auth.dev.acme.dev/oauth/token?x=1",
                Some(401),
            )),
        );
        assert_eq!(card.title, "Couldn't get a token for acme-sso");
        assert_eq!(
            card.message,
            "invalid_client: the client ID or secret is wrong for dev."
        );
        assert_eq!(card.detail, "POST auth.dev.acme.dev/oauth/token → 401");
        assert_eq!(
            card.actions,
            [ErrorAction::EditProfile, ErrorAction::OpenInConsole]
        );

        let redirect = error_card(
            "acme-sso",
            "dev",
            &OAuthError::Denied {
                error: "redirect_uri_mismatch".into(),
                description: None,
            },
            None,
        );
        assert_eq!(redirect.tone, ErrorTone::Warning);
        assert_eq!(redirect.detail, "error=redirect_uri_mismatch");

        let api = unauthorized_card(Some("Bearer error=\"invalid_token\""));
        assert_eq!(
            api.detail,
            "WWW-Authenticate: Bearer error=\"invalid_token\""
        );
        assert_eq!(api.actions[0], ErrorAction::NewTokenAndResend);
    }

    #[test]
    fn redirect_and_advanced_texts() {
        let mut profile = OAuthProfile::new("acme-sso", OAuthGrant::AuthorizationCode);
        profile.extra_params.push(postino_core::ExtraParam {
            key: "audience".into(),
            value: "https://api.acme.dev".into(),
            target: postino_core::ParamTarget::Token,
        });
        assert_eq!(redirect_shown(&profile), "http://127.0.0.1:{port}/callback");
        profile.redirect = OAuthRedirect::Fixed;
        profile.redirect_url = "http://localhost:8765/callback".into();
        assert_eq!(redirect_shown(&profile), "http://localhost:8765/callback");
        assert_eq!(
            advanced_summary(&profile),
            "Basic header · Bearer header · 1 extra param"
        );
        assert_eq!(abbreviate("abc", 5), "abc");
        assert_eq!(abbreviate("abcdef", 3), "abc…");
        assert_eq!(single_variable("{{a}}"), Some("a".into()));
        assert_eq!(single_variable("{{a}}{{b}}"), None);
    }
}
