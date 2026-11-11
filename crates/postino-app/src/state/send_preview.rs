//! What the response pane shows before the first send of a request (the design's "Empty States"
//! page, 2a): the request resolved with the active environment, a summary of what goes with it
//! (environment, headers, authorization, body, scripts) and where each variable's value comes
//! from. Plain Rust, unit tested without a window. Built only when the request, the environment
//! or the session overrides change (`views/send_preview.rs` caches it), never on every frame:
//! resolving interpolates the whole body.
//!
//! Nothing runs: no script, no network. A variable the pre script sets with `vars.set(...)` is
//! only known by name, as in the editor's chips.

use std::collections::HashSet;

use postino_core::log_safe::is_sensitive_header;
use postino_core::{Auth, Body, Environment, Request, VariableKind};
use postino_runner::{SessionEnv, VariableLocation, preview, used_variables};

use super::define_variable::looks_sensitive;
use super::script_heuristics;
use super::variable_hint::{Origin, VariableContext, VariableState};

/// Longest value, in characters, the preview shows before cutting it with an ellipsis.
const MAX_VALUE_CHARS: usize = 120;

/// The authorization a request sends, as the preview names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthSummary {
    /// No authorization.
    None,
    /// Basic, with a user name and password.
    Basic,
    /// A bearer token.
    Bearer,
    /// An API key, in a header or the query string.
    ApiKey,
    /// An OAuth 2.0 profile of the workspace, by name.
    OAuth2(String),
}

/// The body a request sends, as the preview names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodySummary {
    /// No body.
    None,
    /// JSON text.
    Json,
    /// Plain text.
    Text,
    /// XML text.
    Xml,
    /// An urlencoded form.
    Form,
    /// A multipart form.
    Multipart,
}

/// What the preview shows as a variable's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewValue {
    /// The value, shortened when long.
    Value(String),
    /// A secret, shown as `variable_hint::MASK`.
    Masked,
    /// Set by the pre script when it runs: unknown until then.
    SetByScript,
    /// Nothing defines it.
    Undefined,
}

/// Where a variable's value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewSource {
    /// A file of the active environment, by file name (`local.env`, `local.local.env`).
    File(String),
    /// An override a script set during this session (`env.set`).
    Session,
    /// The request's own pre script (`vars.set`).
    PreScript,
    /// Not found in the active environment's files, by their names.
    NotInFiles { base: String, local: String },
    /// Not found, and no environment is active.
    NoEnvironment,
}

/// One variable the request uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewVariable {
    /// Its name.
    pub name: String,
    /// What is shown as its value.
    pub value: PreviewValue,
    /// Where the value comes from.
    pub source: PreviewSource,
}

/// Everything the "Will be sent" panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendPreview {
    /// The method, as sent.
    pub method: postino_core::Method,
    /// The final URL with its query string. An undefined variable stays as its `{{name}}`.
    pub url: String,
    /// Whether the URL or the query string uses an undefined variable.
    pub url_has_undefined: bool,
    /// The active environment, `None` for "No environment".
    pub environment: Option<String>,
    /// How many headers are sent, the ones Postino adds included.
    pub headers: usize,
    /// The authorization.
    pub auth: AuthSummary,
    /// The body.
    pub body: BodySummary,
    /// Whether the request has a pre script.
    pub pre_script: bool,
    /// Whether the request has a post script.
    pub post_script: bool,
    /// Every variable the request uses, each once, in order of appearance. Template functions
    /// are left out.
    pub variables: Vec<PreviewVariable>,
}

impl SendPreview {
    /// The variables nothing defines, in order.
    pub fn undefined(&self) -> Vec<String> {
        self.variables
            .iter()
            .filter(|variable| variable.value == PreviewValue::Undefined)
            .map(|variable| variable.name.clone())
            .collect()
    }
}

/// Builds the preview of `request` for the active environment `environment_name` (with its
/// loaded `environment` and the names its `.local.env` defines, `local_names`) and the session
/// overrides `session`.
pub fn build(
    request: &Request,
    environment_name: Option<&str>,
    environment: &Environment,
    local_names: &HashSet<String>,
    session: &SessionEnv,
) -> SendPreview {
    let empty = Environment::default();
    let scope_environment = if environment_name.is_some() {
        environment
    } else {
        &empty
    };
    let resolved = preview(request, scope_environment, session).resolved;
    let context = VariableContext::new(
        environment_name.map(str::to_string),
        &environment.variables,
        session.as_slice(),
        script_heuristics::vars_set_names(&request.pre_script),
    );

    let used = used_variables(request);
    let mut names: Vec<String> = Vec::new();
    let mut secret: HashSet<String> = HashSet::new();
    let mut in_url: HashSet<String> = HashSet::new();
    for variable in &used {
        if !names.contains(&variable.name) {
            names.push(variable.name.clone());
        }
        let sensitive_place = match &variable.location {
            VariableLocation::Auth => true,
            VariableLocation::Header(key) => is_sensitive_header(key),
            _ => false,
        };
        if sensitive_place {
            secret.insert(variable.name.clone());
        }
        if matches!(
            variable.location,
            VariableLocation::Url | VariableLocation::Query
        ) {
            in_url.insert(variable.name.clone());
        }
    }

    let variables: Vec<PreviewVariable> = names
        .into_iter()
        .map(|name| {
            let force_mask = secret.contains(&name);
            preview_variable(&context, environment_name, local_names, name, force_mask)
        })
        .collect();
    let url_has_undefined = variables.iter().any(|variable| {
        variable.value == PreviewValue::Undefined && in_url.contains(&variable.name)
    });

    SendPreview {
        method: resolved.method.clone(),
        url: resolved.url,
        url_has_undefined,
        environment: environment_name.map(str::to_string),
        headers: resolved.headers.len(),
        auth: match &request.auth {
            Auth::None => AuthSummary::None,
            Auth::Basic { .. } => AuthSummary::Basic,
            Auth::Bearer { .. } => AuthSummary::Bearer,
            Auth::ApiKey { .. } => AuthSummary::ApiKey,
            Auth::OAuth2 { profile, .. } => AuthSummary::OAuth2(profile.clone()),
        },
        body: match &request.body {
            Body::None => BodySummary::None,
            Body::Json(_) => BodySummary::Json,
            Body::Text(_) => BodySummary::Text,
            Body::Xml(_) => BodySummary::Xml,
            Body::Form(_) => BodySummary::Form,
            Body::Multipart(_) => BodySummary::Multipart,
        },
        pre_script: !request.pre_script.trim().is_empty(),
        post_script: !request.post_script.trim().is_empty(),
        variables,
    }
}

/// How the variable `name` shows. `force_mask` hides its value whatever the name is: it is used
/// in the authorization or a sensitive header.
fn preview_variable(
    context: &VariableContext,
    environment_name: Option<&str>,
    local_names: &HashSet<String>,
    name: String,
    force_mask: bool,
) -> PreviewVariable {
    let (value, source) = match context.state_of(&name, VariableKind::Variable) {
        // `used_variables` leaves template functions out, so only the pre script's names land here.
        VariableState::Function | VariableState::ScriptDefined => {
            (PreviewValue::SetByScript, PreviewSource::PreScript)
        }
        VariableState::Resolved { value, origin } => {
            let masked = !value.is_empty()
                && (force_mask || is_sensitive_header(&name) || looks_sensitive(&name));
            let shown = if masked {
                PreviewValue::Masked
            } else {
                PreviewValue::Value(shorten(&value))
            };
            let source = match origin {
                Origin::Session => PreviewSource::Session,
                Origin::Environment(environment) => {
                    let file = if local_names.contains(&name) {
                        format!("{environment}.local.env")
                    } else {
                        format!("{environment}.env")
                    };
                    PreviewSource::File(file)
                }
            };
            (shown, source)
        }
        VariableState::Unknown => (
            PreviewValue::Undefined,
            match environment_name {
                Some(environment) => PreviewSource::NotInFiles {
                    base: format!("{environment}.env"),
                    local: format!("{environment}.local.env"),
                },
                None => PreviewSource::NoEnvironment,
            },
        ),
    };
    PreviewVariable {
        name,
        value,
        source,
    }
}

/// The first few characters of `value`, with an ellipsis when it was cut.
fn shorten(value: &str) -> String {
    match value.char_indices().nth(MAX_VALUE_CHARS) {
        Some((end, _)) => format!("{}\u{2026}", &value[..end]),
        None => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use postino_core::{KeyValue, Method};
    use pretty_assertions::assert_eq;

    fn environment(entries: &[(&str, &str)]) -> Environment {
        Environment {
            name: "local".to_string(),
            variables: entries
                .iter()
                .map(|(key, value)| KeyValue::new(*key, *value))
                .collect(),
        }
    }

    fn request(url: &str) -> Request {
        Request {
            method: Method::Get,
            url: url.to_string(),
            ..Request::default()
        }
    }

    #[test]
    fn a_plain_request_resolves_with_its_environment() {
        let env = environment(&[("baseUrl", "http://localhost:8080")]);
        let preview = build(
            &request("{{baseUrl}}/"),
            Some("local"),
            &env,
            &HashSet::new(),
            &SessionEnv::new(),
        );
        assert_eq!(preview.url, "http://localhost:8080/");
        assert!(!preview.url_has_undefined);
        assert_eq!(preview.environment.as_deref(), Some("local"));
        assert_eq!(preview.auth, AuthSummary::None);
        assert_eq!(preview.body, BodySummary::None);
        assert!(!preview.pre_script && !preview.post_script);
        assert_eq!(
            preview.variables,
            [PreviewVariable {
                name: "baseUrl".to_string(),
                value: PreviewValue::Value("http://localhost:8080".to_string()),
                source: PreviewSource::File("local.env".to_string()),
            }]
        );
        assert!(preview.undefined().is_empty());
    }

    #[test]
    fn an_undefined_variable_stays_in_the_url_and_names_the_files() {
        let preview = build(
            &request("{{baseUrl}}/"),
            Some("local"),
            &environment(&[]),
            &HashSet::new(),
            &SessionEnv::new(),
        );
        // As sent: a URL without a scheme gets `http://`.
        assert_eq!(preview.url, "http://{{baseUrl}}/");
        assert!(preview.url_has_undefined);
        assert_eq!(preview.undefined(), ["baseUrl"]);
        assert_eq!(
            preview.variables[0].source,
            PreviewSource::NotInFiles {
                base: "local.env".to_string(),
                local: "local.local.env".to_string(),
            }
        );
    }

    #[test]
    fn without_an_environment_its_variables_are_undefined() {
        let env = environment(&[("baseUrl", "http://x")]);
        let preview = build(
            &request("{{baseUrl}}/"),
            None,
            &env,
            &HashSet::new(),
            &SessionEnv::new(),
        );
        assert_eq!(preview.url, "http://{{baseUrl}}/");
        assert_eq!(preview.variables[0].source, PreviewSource::NoEnvironment);
    }

    #[test]
    fn values_name_their_layer_and_secrets_are_masked() {
        let env = environment(&[
            ("host", "api.test"),
            ("apiToken", "s3cr3t"),
            ("user", "ana"),
        ]);
        let local = HashSet::from(["apiToken".to_string()]);
        let mut session = SessionEnv::new();
        session.set("user", "luis");
        let mut req = request("https://{{host}}/{{user}}");
        req.auth = Auth::Bearer {
            token: "{{apiToken}}".to_string(),
        };
        req.pre_script = "vars.set('nonce', 1)".to_string();
        req.headers = vec![KeyValue::new("X-Nonce", "{{nonce}}")];
        let preview = build(&req, Some("local"), &env, &local, &session);
        let shown: Vec<(&str, &PreviewValue, &PreviewSource)> = preview
            .variables
            .iter()
            .map(|v| (v.name.as_str(), &v.value, &v.source))
            .collect();
        assert_eq!(
            shown,
            [
                (
                    "host",
                    &PreviewValue::Value("api.test".to_string()),
                    &PreviewSource::File("local.env".to_string())
                ),
                (
                    "user",
                    &PreviewValue::Value("luis".to_string()),
                    &PreviewSource::Session
                ),
                (
                    "nonce",
                    &PreviewValue::SetByScript,
                    &PreviewSource::PreScript
                ),
                (
                    "apiToken",
                    &PreviewValue::Masked,
                    &PreviewSource::File("local.local.env".to_string())
                ),
            ]
        );
        assert_eq!(preview.auth, AuthSummary::Bearer);
        assert!(preview.pre_script);
        assert_eq!(preview.url, "https://api.test/luis");
    }

    #[test]
    fn a_long_value_is_cut() {
        let long = "x".repeat(300);
        let env = environment(&[("v", long.as_str())]);
        let preview = build(
            &request("http://h/{{v}}"),
            Some("local"),
            &env,
            &HashSet::new(),
            &SessionEnv::new(),
        );
        let PreviewValue::Value(shown) = &preview.variables[0].value else {
            panic!("expected a value");
        };
        assert_eq!(shown.chars().count(), MAX_VALUE_CHARS + 1);
        assert!(shown.ends_with('\u{2026}'));
    }
}
