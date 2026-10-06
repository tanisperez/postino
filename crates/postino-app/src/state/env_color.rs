//! Maps an environment name to the color category shown in the UI: the dot in the title bar's
//! environment pill and menu (see `docs/design-system.md`).

/// Which color category an environment's dot should use, derived from its name. "No
/// environment" is not covered here: the view draws a hollow ring for it instead of a color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvColor {
    /// The name contains "prod".
    Danger,
    /// The name contains "stag", "test", "qa" or "uat".
    Warning,
    /// Anything else.
    Success,
}

/// The name fragments, checked case-insensitively, that make an environment's dot [`EnvColor::Warning`].
const WARNING_NEEDLES: [&str; 4] = ["stag", "test", "qa", "uat"];

/// Derives the [`EnvColor`] of an environment from its name, case-insensitive.
pub fn env_color(name: &str) -> EnvColor {
    let lower = name.to_ascii_lowercase();
    if lower.contains("prod") {
        EnvColor::Danger
    } else if WARNING_NEEDLES.iter().any(|needle| lower.contains(needle)) {
        EnvColor::Warning
    } else {
        EnvColor::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn production_is_danger() {
        assert_eq!(env_color("production"), EnvColor::Danger);
        assert_eq!(env_color("PROD"), EnvColor::Danger);
    }

    #[test]
    fn staging_test_qa_and_uat_are_warning() {
        assert_eq!(env_color("staging"), EnvColor::Warning);
        assert_eq!(env_color("Test"), EnvColor::Warning);
        assert_eq!(env_color("qa-env"), EnvColor::Warning);
        assert_eq!(env_color("UAT"), EnvColor::Warning);
    }

    #[test]
    fn anything_else_is_success() {
        assert_eq!(env_color("local"), EnvColor::Success);
        assert_eq!(env_color("dev"), EnvColor::Success);
    }

    #[test]
    fn prod_wins_over_warning_needles_when_both_are_present() {
        assert_eq!(env_color("staging-prod-mirror"), EnvColor::Danger);
    }
}
