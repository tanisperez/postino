//! Parses the `POSTINO_OPEN` debug hook's value (`plans/ui-redesign-spikes.md` section 10), the
//! same style as `views/root.rs`'s existing `apply_debug_autosend`: a hidden hook for the
//! orchestrator to screenshot a specific UI state at startup, not a supported feature, not
//! surfaced in any menu.

/// A UI state `POSTINO_OPEN` can request at startup. `Components` (phase 3), `Settings` (phase
/// 6), `Palette`/`Snippet`/`Define` (phase 7) and `LoadTest` (phase 8) exist so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugOpenTarget {
    /// Renders the components gallery in the main area, mirroring
    /// `postino_design_system/Components.dc.html`.
    Components,
    /// Opens the Settings modal, mirroring `postino_design_system/Settings.dc.html`
    /// (`plans/ui-redesign.md` phase 6).
    Settings,
    /// Opens the Settings modal on its "Requests" pane.
    SettingsRequests,
    /// Opens the command palette (`plans/ui-redesign.md` phase 7 item 1).
    Palette,
    /// Opens the Code snippet dialog for the active tab's request (`plans/ui-redesign.md` phase
    /// 7 item 2). A no-op (like every `POSTINO_OPEN` target with nothing to show) when no tab is
    /// open.
    Snippet,
    /// Opens the Define variable dialog, prefilled with the name `"exampleVar"`
    /// (`plans/ui-redesign.md` phase 7 item 3).
    Define,
    /// Opens a load test tab preselecting the first request found in the workspace tree, for
    /// screenshotting `postino_design_system/Performance.dc.html`'s config panel
    /// (`plans/ui-redesign.md` phase 8). A no-op when no workspace is open or it has no
    /// requests.
    LoadTest,
}

/// Parses `POSTINO_OPEN`'s value into a [`DebugOpenTarget`]. An unrecognized value is `None`
/// (silently ignored, same convention as the rest of `POSTINO_OPEN`'s callers).
pub fn parse(value: &str) -> Option<DebugOpenTarget> {
    match value {
        "components" => Some(DebugOpenTarget::Components),
        "settings" => Some(DebugOpenTarget::Settings),
        "settings-requests" => Some(DebugOpenTarget::SettingsRequests),
        "palette" => Some(DebugOpenTarget::Palette),
        "snippet" => Some(DebugOpenTarget::Snippet),
        "define" => Some(DebugOpenTarget::Define),
        "loadtest" => Some(DebugOpenTarget::LoadTest),
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
    fn parses_settings() {
        assert_eq!(parse("settings"), Some(DebugOpenTarget::Settings));
    }

    #[test]
    fn parses_settings_requests() {
        assert_eq!(
            parse("settings-requests"),
            Some(DebugOpenTarget::SettingsRequests)
        );
    }

    #[test]
    fn parses_palette() {
        assert_eq!(parse("palette"), Some(DebugOpenTarget::Palette));
    }

    #[test]
    fn parses_snippet() {
        assert_eq!(parse("snippet"), Some(DebugOpenTarget::Snippet));
    }

    #[test]
    fn parses_define() {
        assert_eq!(parse("define"), Some(DebugOpenTarget::Define));
    }

    #[test]
    fn parses_loadtest() {
        assert_eq!(parse("loadtest"), Some(DebugOpenTarget::LoadTest));
    }

    #[test]
    fn unrecognized_value_is_none() {
        assert_eq!(parse("load-test"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("Components"), None);
    }
}
