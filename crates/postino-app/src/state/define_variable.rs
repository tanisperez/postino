//! Plain logic behind the Define variable dialog (`plans/ui-redesign.md` phase 7 item 3): the
//! secret-name heuristic that defaults the "Store in .local.env" switch on, and which
//! environment the dialog preselects when it opens. `views/define_variable.rs` renders the
//! dialog and owns its `gpui` entities; this module holds only what can be unit tested without a
//! window.

/// Case-insensitive substrings that mark a variable name as likely sensitive. Defining a
/// variable whose name contains one of these defaults the "Store in .local.env (not versioned)"
/// switch on.
const SENSITIVE_NEEDLES: [&str; 4] = ["token", "secret", "password", "key"];

/// Whether `name` looks like it holds a secret, by a case-insensitive substring check against
/// [`SENSITIVE_NEEDLES`].
pub fn looks_sensitive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SENSITIVE_NEEDLES
        .iter()
        .any(|needle| lower.contains(needle))
}

/// The environment the Define dialog preselects when it opens: the active environment, if the
/// workspace still has one by that name, otherwise the workspace's first environment (already
/// sorted, matching the environment picker). `None` (the dialog then starts in "New
/// environment..." mode) only when the workspace has no environment at all.
pub fn initial_environment(active: Option<&str>, environments: &[String]) -> Option<String> {
    if let Some(active) = active
        && environments.iter().any(|name| name == active)
    {
        return Some(active.to_string());
    }
    environments.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn recognizes_every_sensitive_needle_case_insensitively() {
        assert!(looks_sensitive("authToken"));
        assert!(looks_sensitive("API_SECRET"));
        assert!(looks_sensitive("password"));
        assert!(looks_sensitive("apiKey"));
    }

    #[test]
    fn an_ordinary_name_is_not_sensitive() {
        assert!(!looks_sensitive("baseUrl"));
        assert!(!looks_sensitive("userId"));
    }

    #[test]
    fn initial_environment_prefers_the_active_one_when_it_still_exists() {
        let environments = vec!["local".to_string(), "prod".to_string()];
        assert_eq!(
            initial_environment(Some("prod"), &environments),
            Some("prod".to_string())
        );
    }

    #[test]
    fn initial_environment_falls_back_to_the_first_one_with_no_active_environment() {
        let environments = vec!["local".to_string(), "prod".to_string()];
        assert_eq!(
            initial_environment(None, &environments),
            Some("local".to_string())
        );
    }

    #[test]
    fn initial_environment_falls_back_to_the_first_one_when_the_active_one_is_gone() {
        let environments = vec!["local".to_string()];
        assert_eq!(
            initial_environment(Some("stale"), &environments),
            Some("local".to_string())
        );
    }

    #[test]
    fn initial_environment_is_none_when_the_workspace_has_no_environment() {
        assert_eq!(initial_environment(None, &[]), None);
    }
}
