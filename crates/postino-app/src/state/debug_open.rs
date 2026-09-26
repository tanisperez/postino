//! Parses the `POSTINO_OPEN` debug hook's value (`plans/ui-redesign-spikes.md` section 10), the
//! same style as `views/root.rs`'s existing `apply_debug_autosend`: a hidden hook for the
//! orchestrator to screenshot a specific UI state at startup, not a supported feature, not
//! surfaced in any menu.

/// A UI state `POSTINO_OPEN` can request at startup. Only `Components` exists so far (phase 3);
/// later phases add `Settings`, `Palette`, `LoadTest`, ... to this same enum, per
/// `plans/ui-redesign.md` phase 3's instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugOpenTarget {
    /// Renders the components gallery in the main area, mirroring
    /// `postino_design_system/Components.dc.html`.
    Components,
}

/// Parses `POSTINO_OPEN`'s value into a [`DebugOpenTarget`]. An unrecognized value is `None`
/// (silently ignored, same convention as the rest of `POSTINO_OPEN`'s callers).
pub fn parse(value: &str) -> Option<DebugOpenTarget> {
    match value {
        "components" => Some(DebugOpenTarget::Components),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_components() {
        assert_eq!(parse("components"), Some(DebugOpenTarget::Components));
    }

    #[test]
    fn unrecognized_value_is_none() {
        assert_eq!(parse("settings"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("Components"), None);
    }
}
