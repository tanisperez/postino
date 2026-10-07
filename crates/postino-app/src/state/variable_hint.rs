//! What the request editor knows about each `{{ }}` marker: whether it resolves (and so is drawn
//! as a defined chip or in red), and what its tooltip says. Plain Rust over the same layers the
//! runner resolves with (`postino_core::VarScope`), unit tested without a window.
//!
//! A marker resolves when it is a template function, a variable the pre script sets with
//! `vars.set(...)` (the script never runs while editing, so only its name is known), or a
//! variable found in the session overrides or the active environment. Without an environment
//! selected the environment layer is empty, so its variables are not defined, which is the point:
//! the red marker tells the user to pick one.

use std::collections::HashSet;

use postino_core::log_safe::is_sensitive_header;
use postino_core::{KeyValue, VariableKind, VariableSpan, variable_spans};

/// What a masked value is shown as.
pub const MASK: &str = "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}";

/// Largest text, in bytes, scanned for markers to style in a body editor. A bigger body is left
/// unstyled so typing in it never costs time proportional to its size.
pub const MAX_MARKER_SCAN_BYTES: usize = 1_000_000;

/// Longest value, in characters, a tooltip shows before cutting it with an ellipsis.
const MAX_VALUE_CHARS: usize = 200;

/// The layer a resolved value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The active environment, by name.
    Environment(String),
    /// A runtime override set by a script (`env.set`), kept for the app session.
    Session,
}

/// How one marker resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableState {
    /// A template function call, `{{ uuid() }}`: always fine, nothing to show.
    Function,
    /// A variable set by the request's own pre script. Its value is only known once it runs.
    ScriptDefined,
    /// A variable with a value in a layer.
    Resolved { value: String, origin: Origin },
    /// Nothing defines it.
    Unknown,
}

impl VariableState {
    /// Whether the marker is drawn as defined (accent) rather than in red.
    pub fn is_defined(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

/// The layers a marker is resolved against, captured once per render. Cheap to clone and compare:
/// the editor compares two of them to know when the body decorations must be recomputed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VariableContext {
    environment: Option<String>,
    environment_vars: Vec<KeyValue>,
    session_vars: Vec<KeyValue>,
    script_names: HashSet<String>,
}

impl VariableContext {
    /// A context for the active `environment` (`None` when none is selected, in which case
    /// `environment_vars` is ignored), the session overrides and the names the pre script sets.
    pub fn new(
        environment: Option<String>,
        environment_vars: &[KeyValue],
        session_vars: &[KeyValue],
        script_names: HashSet<String>,
    ) -> Self {
        let environment_vars = if environment.is_some() {
            environment_vars.to_vec()
        } else {
            Vec::new()
        };
        Self {
            environment,
            environment_vars,
            session_vars: session_vars.to_vec(),
            script_names,
        }
    }

    /// How `span` resolves. Layers are tried in the runner's order: the pre script, then the
    /// session overrides, then the environment.
    pub fn state(&self, span: &VariableSpan) -> VariableState {
        self.state_of(&span.name, span.kind)
    }

    /// How the marker called `name`, of kind `kind`, resolves.
    pub fn state_of(&self, name: &str, kind: VariableKind) -> VariableState {
        if kind == VariableKind::FunctionCall {
            return VariableState::Function;
        }
        if self.script_names.contains(name) {
            return VariableState::ScriptDefined;
        }
        if let Some(value) = lookup(&self.session_vars, name) {
            return VariableState::Resolved {
                value: value.to_string(),
                origin: Origin::Session,
            };
        }
        match (&self.environment, lookup(&self.environment_vars, name)) {
            (Some(name), Some(value)) => VariableState::Resolved {
                value: value.to_string(),
                origin: Origin::Environment(name.clone()),
            },
            _ => VariableState::Unknown,
        }
    }

    /// Whether `span` is drawn as defined.
    pub fn is_defined(&self, span: &VariableSpan) -> bool {
        self.state(span).is_defined()
    }

    /// Whether the variable called `name` is undefined, for a click on its chip.
    pub fn is_unknown_variable(&self, name: &str) -> bool {
        !self.state_of(name, VariableKind::Variable).is_defined()
    }
}

/// Every marker of `text` with whether it is defined, for styling a body editor. Empty when the
/// text is larger than [`MAX_MARKER_SCAN_BYTES`].
pub fn markers(text: &str, context: &VariableContext) -> Vec<(VariableSpan, bool)> {
    if text.len() > MAX_MARKER_SCAN_BYTES || !text.contains("{{") {
        return Vec::new();
    }
    variable_spans(text)
        .into_iter()
        .map(|span| {
            let defined = context.is_defined(&span);
            (span, defined)
        })
        .collect()
}

/// What a tooltip shows for a defined variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// The variable name.
    pub name: String,
    /// The value to display, already masked or shortened. `None` when it is not known yet.
    pub value: Option<String>,
    /// Whether `value` is the mask standing in for a secret.
    pub masked: bool,
    /// Where the value comes from. `None` for a variable the pre script sets.
    pub origin: Option<Origin>,
}

/// The tooltip content for `span`, or `None` for a function call or an undefined variable.
/// `force_mask` hides the value whatever the name is, for a variable inside a sensitive header
/// such as `Authorization`.
pub fn hint(context: &VariableContext, span: &VariableSpan, force_mask: bool) -> Option<Hint> {
    match context.state(span) {
        VariableState::Function | VariableState::Unknown => None,
        VariableState::ScriptDefined => Some(Hint {
            name: span.name.clone(),
            value: None,
            masked: false,
            origin: None,
        }),
        VariableState::Resolved { value, origin } => {
            let masked = !value.is_empty() && (force_mask || is_sensitive_header(&span.name));
            let shown = if masked {
                MASK.to_string()
            } else {
                shorten(&value)
            };
            Some(Hint {
                name: span.name.clone(),
                value: Some(shown),
                masked,
                origin: Some(origin),
            })
        }
    }
}

/// The first few characters of `value`, with an ellipsis when it was cut.
fn shorten(value: &str) -> String {
    match value.char_indices().nth(MAX_VALUE_CHARS) {
        Some((end, _)) => format!("{}\u{2026}", &value[..end]),
        None => value.to_string(),
    }
}

fn lookup<'a>(layer: &'a [KeyValue], name: &str) -> Option<&'a str> {
    layer
        .iter()
        .find(|entry| entry.enabled && entry.key == name)
        .map(|entry| entry.value.as_str())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use postino_core::variable_spans;
    use pretty_assertions::assert_eq;

    fn vars(entries: &[(&str, &str)]) -> Vec<KeyValue> {
        entries
            .iter()
            .map(|(key, value)| KeyValue {
                key: key.to_string(),
                value: value.to_string(),
                enabled: true,
            })
            .collect()
    }

    fn span(text: &str) -> VariableSpan {
        variable_spans(text).remove(0)
    }

    fn context(environment: Option<&str>, env: &[(&str, &str)]) -> VariableContext {
        VariableContext::new(
            environment.map(str::to_string),
            &vars(env),
            &[],
            HashSet::new(),
        )
    }

    #[test]
    fn without_an_environment_its_variables_are_unknown() {
        let context = context(None, &[("baseUrl", "https://x")]);
        assert_eq!(context.state(&span("{{baseUrl}}")), VariableState::Unknown);
        assert!(!context.is_defined(&span("{{baseUrl}}")));
    }

    #[test]
    fn with_an_environment_a_defined_variable_resolves_with_its_origin() {
        let context = context(Some("dev"), &[("baseUrl", "https://x")]);
        assert_eq!(
            context.state(&span("{{ baseUrl }}")),
            VariableState::Resolved {
                value: "https://x".to_string(),
                origin: Origin::Environment("dev".to_string()),
            }
        );
        assert_eq!(context.state(&span("{{other}}")), VariableState::Unknown);
    }

    #[test]
    fn functions_are_defined_with_or_without_an_environment() {
        let context = context(None, &[]);
        assert_eq!(
            context.state(&span("{{ uuid() }}")),
            VariableState::Function
        );
        assert!(context.is_defined(&span("{{ uuid() }}")));
    }

    #[test]
    fn script_set_names_are_defined_without_a_value() {
        let context = VariableContext::new(None, &[], &[], HashSet::from(["token".to_string()]));
        assert_eq!(
            context.state(&span("{{token}}")),
            VariableState::ScriptDefined
        );
    }

    #[test]
    fn the_session_overrides_the_environment() {
        let context = VariableContext::new(
            Some("dev".to_string()),
            &vars(&[("host", "from-env")]),
            &vars(&[("host", "from-session")]),
            HashSet::new(),
        );
        assert_eq!(
            context.state(&span("{{host}}")),
            VariableState::Resolved {
                value: "from-session".to_string(),
                origin: Origin::Session,
            }
        );
    }

    #[test]
    fn a_disabled_entry_does_not_define_a_variable() {
        let mut env = vars(&[("host", "x")]);
        env[0].enabled = false;
        let context = VariableContext::new(Some("dev".to_string()), &env, &[], HashSet::new());
        assert_eq!(context.state(&span("{{host}}")), VariableState::Unknown);
    }

    #[test]
    fn hint_masks_a_sensitive_name_and_keeps_the_rest() {
        let context = context(
            Some("dev"),
            &[("apiToken", "s3cr3t"), ("baseUrl", "https://x")],
        );
        let secret = hint(&context, &span("{{apiToken}}"), false).unwrap();
        assert_eq!(secret.value.as_deref(), Some(MASK));
        assert!(secret.masked);
        let plain = hint(&context, &span("{{baseUrl}}"), false).unwrap();
        assert_eq!(plain.value.as_deref(), Some("https://x"));
        assert!(!plain.masked);
    }

    #[test]
    fn hint_masks_everything_when_forced() {
        let context = context(Some("dev"), &[("baseUrl", "https://x")]);
        let hint = hint(&context, &span("{{baseUrl}}"), true).unwrap();
        assert_eq!(hint.value.as_deref(), Some(MASK));
    }

    #[test]
    fn an_empty_value_is_never_masked() {
        let context = context(Some("dev"), &[("apiToken", "")]);
        let hint = hint(&context, &span("{{apiToken}}"), false).unwrap();
        assert_eq!(hint.value.as_deref(), Some(""));
        assert!(!hint.masked);
    }

    #[test]
    fn hint_is_none_for_functions_and_unknown_variables() {
        let context = context(Some("dev"), &[]);
        assert_eq!(hint(&context, &span("{{ uuid() }}"), false), None);
        assert_eq!(hint(&context, &span("{{missing}}"), false), None);
    }

    #[test]
    fn a_long_value_is_cut_with_an_ellipsis() {
        let long = "a".repeat(500);
        let context = context(Some("dev"), &[("blob", long.as_str())]);
        let shown = hint(&context, &span("{{blob}}"), false)
            .unwrap()
            .value
            .unwrap();
        assert_eq!(shown.chars().count(), MAX_VALUE_CHARS + 1);
        assert!(shown.ends_with('\u{2026}'));
    }

    #[test]
    fn markers_flags_each_marker_and_skips_huge_texts() {
        let context = context(Some("dev"), &[("a", "1")]);
        let found = markers(r#"{"x": "{{a}}", "y": "{{b}}"}"#, &context);
        assert_eq!(
            found
                .iter()
                .map(|(span, defined)| (span.name.as_str(), *defined))
                .collect::<Vec<_>>(),
            vec![("a", true), ("b", false)]
        );
        assert!(markers("no markers", &context).is_empty());
        let huge = format!("{{{{a}}}}{}", "x".repeat(MAX_MARKER_SCAN_BYTES));
        assert!(markers(&huge, &context).is_empty());
    }

    #[test]
    fn a_context_is_equal_only_when_its_layers_are() {
        let a = context(Some("dev"), &[("host", "x")]);
        assert_eq!(a, context(Some("dev"), &[("host", "x")]));
        assert_ne!(a, context(Some("dev"), &[("host", "y")]));
        assert_ne!(a, context(Some("prod"), &[("host", "x")]));
    }
}
