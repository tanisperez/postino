//! Plain logic behind the Define variable dialog: the secret-name heuristic that defaults the
//! "Store in .local.env" switch on, which environment the dialog preselects when it opens, and
//! the hints under the name (the environments that already define it, a similar known name).
//! `views/define_variable.rs` renders the dialog and owns its `gpui` entities; this module holds
//! only what can be unit tested without a window.

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

/// Largest edit distance at which a known name counts as similar to the typed one.
const MAX_SIMILAR_DISTANCE: usize = 2;

/// The variable names each environment defines, read once when the dialog opens so typing a name
/// never touches the disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownNames {
    environments: Vec<(String, Vec<String>)>,
}

impl KnownNames {
    /// Builds the index from each environment's name and variable names, in display order.
    pub fn new(environments: Vec<(String, Vec<String>)>) -> Self {
        Self { environments }
    }

    /// The environments that already define `name`, in display order.
    pub fn defined_in(&self, name: &str) -> Vec<String> {
        self.environments
            .iter()
            .filter(|(_, names)| names.iter().any(|known| known == name))
            .map(|(environment, _)| environment.clone())
            .collect()
    }

    /// The known name closest to `name`, when one differs from it only by case or by at most
    /// [`MAX_SIMILAR_DISTANCE`] edits (and fewer than half its length, so a short name does not
    /// match everything). `None` when `name` itself is known, since there is nothing to fix.
    pub fn similar(&self, name: &str) -> Option<String> {
        let all = || self.environments.iter().flat_map(|(_, names)| names.iter());
        if name.is_empty() || all().any(|known| known == name) {
            return None;
        }
        let lower = name.to_lowercase();
        let max = MAX_SIMILAR_DISTANCE.min(name.chars().count().saturating_sub(1) / 2);
        all()
            .map(|known| (edit_distance(&lower, &known.to_lowercase()), known))
            .filter(|(distance, _)| *distance <= max)
            .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)))
            .map(|(_, known)| known.clone())
    }
}

/// Levenshtein distance between `a` and `b`, by characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != *cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    previous[b.len()]
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

    fn known() -> KnownNames {
        KnownNames::new(vec![
            (
                "local".to_string(),
                vec!["baseUrl".to_string(), "token".to_string()],
            ),
            ("prod".to_string(), vec!["baseUrl".to_string()]),
        ])
    }

    #[test]
    fn defined_in_lists_every_environment_with_the_name() {
        assert_eq!(known().defined_in("baseUrl"), vec!["local", "prod"]);
        assert_eq!(known().defined_in("token"), vec!["local"]);
        assert!(known().defined_in("other").is_empty());
    }

    #[test]
    fn similar_finds_a_case_or_typo_variant() {
        assert_eq!(known().similar("baseURL"), Some("baseUrl".to_string()));
        assert_eq!(known().similar("baseUr"), Some("baseUrl".to_string()));
        assert_eq!(known().similar("tokn"), Some("token".to_string()));
    }

    #[test]
    fn similar_is_none_for_a_known_name_or_a_distant_one() {
        assert_eq!(known().similar("baseUrl"), None);
        assert_eq!(known().similar("userId"), None);
        assert_eq!(known().similar(""), None);
        // A short name only matches by case, not by an edit.
        assert_eq!(known().similar("to"), None);
    }

    #[test]
    fn edit_distance_counts_insertions_deletions_and_substitutions() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", "abc"), 0);
    }
}
