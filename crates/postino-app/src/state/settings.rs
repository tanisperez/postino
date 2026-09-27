//! Application settings, persisted to `<config dir>/postino/settings.toml`
//! (`plans/ui-redesign.md`, section 1 "Settings file") and applied live: there is no explicit
//! "Save" action, every change made through the Settings view (wired in a later phase) writes
//! the file immediately.

use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The path, relative to the OS config directory, of the settings file.
const SETTINGS_FILE: &str = "postino/settings.toml";

/// Valid range of [`Settings::ui_font_size`], in points (`plans/ui-redesign.md`, section 1).
pub const UI_FONT_SIZE_RANGE: RangeInclusive<f32> = 11.0..=16.0;

/// Step of the UI font size stepper in the Settings view (`plans/ui-redesign.md` phase 6 item 4).
pub const UI_FONT_SIZE_STEP: f32 = 1.0;

/// Valid range of [`Settings::mono_font_size`], in points.
pub const MONO_FONT_SIZE_RANGE: RangeInclusive<f32> = 10.0..=18.0;

/// Step of the monospace font size stepper in the Settings view (`plans/ui-redesign.md` phase 6
/// item 5).
pub const MONO_FONT_SIZE_STEP: f32 = 0.5;

/// Which theme mode the app follows, chosen in Settings, "Appearance" (wired in phase 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// Follow the OS light/dark setting.
    #[default]
    System,
    /// Always use the light theme.
    Light,
    /// Always use the dark theme.
    Dark,
}

impl ThemeChoice {
    /// Parses a theme name as read from the settings file, case-insensitive. Returns `None` for
    /// anything else, so the caller can fall back to the default.
    fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "system" => Some(ThemeChoice::System),
            "light" => Some(ThemeChoice::Light),
            "dark" => Some(ThemeChoice::Dark),
            _ => None,
        }
    }
}

/// User-configurable appearance settings: the theme mode and the two fonts (UI and editor), with
/// their sizes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    /// The theme mode: system, light or dark.
    pub theme: ThemeChoice,
    /// The UI font family name.
    pub ui_font: String,
    /// The UI font size, in points, clamped to [`UI_FONT_SIZE_RANGE`].
    pub ui_font_size: f32,
    /// The monospace font family name used in code editors.
    pub mono_font: String,
    /// The monospace font size, in points, clamped to [`MONO_FONT_SIZE_RANGE`].
    pub mono_font_size: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemeChoice::default(),
            ui_font: "Geist".to_string(),
            ui_font_size: 13.0,
            mono_font: "Geist Mono".to_string(),
            mono_font_size: 12.5,
        }
    }
}

/// Loads the settings from `<config dir>/postino/settings.toml`, or [`Settings::default`] if the
/// OS config directory is unknown, the file is missing, or its content is not valid TOML.
pub fn load_settings() -> Settings {
    match dirs::config_dir() {
        Some(base) => read_settings(&base),
        None => Settings::default(),
    }
}

/// Saves `settings` to `<config dir>/postino/settings.toml`, creating the folder if needed.
/// Failing to persist this is not worth interrupting the user over, so any error (a missing
/// config directory, a read-only filesystem, ...) is silently ignored.
pub fn save_settings(settings: &Settings) {
    if let Some(base) = dirs::config_dir() {
        write_settings(&base, settings);
    }
}

/// The real path of the settings file, `None` when the OS config directory is unknown. Used by
/// the Settings view's "Saved to" line (`plans/ui-redesign.md` phase 6 item 2), shortened with
/// `~` by `state::format::shorten_path`.
pub fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join(SETTINGS_FILE))
}

/// Steps `current` by `step` (positive or negative), clamped to `range`. Used by the Settings
/// view's font size steppers (`plans/ui-redesign.md` phase 6 items 4 and 5): the `-`/`+` buttons
/// pass `-step`/`step` of [`UI_FONT_SIZE_STEP`] or [`MONO_FONT_SIZE_STEP`], with
/// [`UI_FONT_SIZE_RANGE`] or [`MONO_FONT_SIZE_RANGE`].
pub fn step_size(current: f32, step: f32, range: RangeInclusive<f32>) -> f32 {
    (current + step).clamp(*range.start(), *range.end())
}

/// Builds the options for a font picker: `bundled` first, always, then every other entry of
/// `installed` (as returned by `cx.text_system().all_font_names()`, already sorted), skipping the
/// bundled family's own name if it repeats there and de-duplicating (`plans/ui-redesign.md` phase
/// 6 items 4 and 5).
pub fn font_options(bundled: &str, installed: &[String]) -> Vec<String> {
    let mut options = vec![bundled.to_string()];
    for name in installed {
        if name != bundled && !options.contains(name) {
            options.push(name.clone());
        }
    }
    options
}

/// Reads and parses `<base>/postino/settings.toml`. Split out from [`load_settings`] so tests can
/// point `base` at a temporary directory instead of the real, per-user OS config directory.
///
/// Every field is read and validated independently: a missing file, a field of the wrong type,
/// or an out-of-range number falls back to its own default instead of discarding the whole file,
/// and a key this version does not know about is silently ignored.
fn read_settings(base: &Path) -> Settings {
    let Ok(content) = fs::read_to_string(base.join(SETTINGS_FILE)) else {
        return Settings::default();
    };
    parse_settings(&content)
}

/// Parses the content of a settings file into [`Settings`], falling back field by field to
/// [`Settings::default`] as documented on [`read_settings`].
fn parse_settings(content: &str) -> Settings {
    let defaults = Settings::default();
    let Ok(value) = content.parse::<toml::Value>() else {
        return defaults;
    };
    let table = value.as_table();

    let theme = table
        .and_then(|table| table.get("theme"))
        .and_then(toml::Value::as_str)
        .and_then(ThemeChoice::parse)
        .unwrap_or(defaults.theme);
    let ui_font = table
        .and_then(|table| table.get("ui_font"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .unwrap_or(defaults.ui_font);
    let ui_font_size = table
        .and_then(|table| table.get("ui_font_size"))
        .and_then(as_f32)
        .map(|size| size.clamp(*UI_FONT_SIZE_RANGE.start(), *UI_FONT_SIZE_RANGE.end()))
        .unwrap_or(defaults.ui_font_size);
    let mono_font = table
        .and_then(|table| table.get("mono_font"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .unwrap_or(defaults.mono_font);
    let mono_font_size = table
        .and_then(|table| table.get("mono_font_size"))
        .and_then(as_f32)
        .map(|size| size.clamp(*MONO_FONT_SIZE_RANGE.start(), *MONO_FONT_SIZE_RANGE.end()))
        .unwrap_or(defaults.mono_font_size);

    Settings {
        theme,
        ui_font,
        ui_font_size,
        mono_font,
        mono_font_size,
    }
}

/// Reads a TOML value as an `f32`, accepting both a float and an integer.
fn as_f32(value: &toml::Value) -> Option<f32> {
    value
        .as_float()
        .map(|value| value as f32)
        .or_else(|| value.as_integer().map(|value| value as f32))
}

/// Writes `settings` to `<base>/postino/settings.toml`, creating the folder if needed. Split out
/// from [`save_settings`] for the same testing reason as [`read_settings`].
fn write_settings(base: &Path, settings: &Settings) {
    let path = base.join(SETTINGS_FILE);
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(text) = toml::to_string_pretty(settings) else {
        return;
    };
    let _ = fs::write(path, text);
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn defaults_are_the_documented_values() {
        let settings = Settings::default();
        assert_eq!(settings.theme, ThemeChoice::System);
        assert_eq!(settings.ui_font, "Geist");
        assert_eq!(settings.ui_font_size, 13.0);
        assert_eq!(settings.mono_font, "Geist Mono");
        assert_eq!(settings.mono_font_size, 12.5);
    }

    #[test]
    fn missing_file_reads_as_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(read_settings(dir.path()), Settings::default());
    }

    #[test]
    fn round_trips_through_a_config_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let settings = Settings {
            theme: ThemeChoice::Dark,
            ui_font: "Inter".to_string(),
            ui_font_size: 14.0,
            mono_font: "Fira Code".to_string(),
            mono_font_size: 13.5,
        };

        write_settings(dir.path(), &settings);

        assert_eq!(read_settings(dir.path()), settings);
    }

    #[test]
    fn invalid_toml_reads_as_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("postino")).expect("create postino dir");
        fs::write(dir.path().join(SETTINGS_FILE), "not valid toml {{{").expect("write file");

        assert_eq!(read_settings(dir.path()), Settings::default());
    }

    #[test]
    fn an_invalid_field_falls_back_on_its_own_without_discarding_the_rest() {
        let settings = parse_settings("theme = \"dark\"\nui_font_size = \"not a number\"\n");
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.ui_font_size, Settings::default().ui_font_size);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let settings = parse_settings("theme = \"light\"\nsome_future_field = 42\n");
        assert_eq!(settings.theme, ThemeChoice::Light);
    }

    #[test]
    fn ui_font_size_is_clamped_to_its_range() {
        assert_eq!(parse_settings("ui_font_size = 3\n").ui_font_size, 11.0);
        assert_eq!(parse_settings("ui_font_size = 999\n").ui_font_size, 16.0);
    }

    #[test]
    fn mono_font_size_is_clamped_to_its_range() {
        assert_eq!(parse_settings("mono_font_size = 1\n").mono_font_size, 10.0);
        assert_eq!(
            parse_settings("mono_font_size = 999\n").mono_font_size,
            18.0
        );
    }

    #[test]
    fn theme_choice_parses_case_insensitively() {
        assert_eq!(ThemeChoice::parse("DARK"), Some(ThemeChoice::Dark));
        assert_eq!(ThemeChoice::parse("Light"), Some(ThemeChoice::Light));
        assert_eq!(ThemeChoice::parse("nonsense"), None);
    }

    #[test]
    fn step_size_steps_within_range() {
        assert_eq!(step_size(13.0, UI_FONT_SIZE_STEP, UI_FONT_SIZE_RANGE), 14.0);
        assert_eq!(
            step_size(13.0, -UI_FONT_SIZE_STEP, UI_FONT_SIZE_RANGE),
            12.0
        );
        assert_eq!(
            step_size(12.5, MONO_FONT_SIZE_STEP, MONO_FONT_SIZE_RANGE),
            13.0
        );
    }

    #[test]
    fn step_size_clamps_at_the_range_bounds() {
        assert_eq!(step_size(16.0, UI_FONT_SIZE_STEP, UI_FONT_SIZE_RANGE), 16.0);
        assert_eq!(
            step_size(11.0, -UI_FONT_SIZE_STEP, UI_FONT_SIZE_RANGE),
            11.0
        );
        assert_eq!(
            step_size(18.0, MONO_FONT_SIZE_STEP, MONO_FONT_SIZE_RANGE),
            18.0
        );
        assert_eq!(
            step_size(10.0, -MONO_FONT_SIZE_STEP, MONO_FONT_SIZE_RANGE),
            10.0
        );
    }

    #[test]
    fn font_options_puts_the_bundled_family_first() {
        let installed = vec![
            "Arial".to_string(),
            "Geist".to_string(),
            "Fira Code".to_string(),
        ];
        assert_eq!(
            font_options("Geist", &installed),
            vec![
                "Geist".to_string(),
                "Arial".to_string(),
                "Fira Code".to_string()
            ]
        );
    }

    #[test]
    fn font_options_deduplicates() {
        let installed = vec!["Arial".to_string(), "Arial".to_string()];
        assert_eq!(
            font_options("Geist", &installed),
            vec!["Geist".to_string(), "Arial".to_string()]
        );
    }
}
