//! Application settings, persisted to `<config dir>/postino/settings.toml`
//! (`plans/ui-redesign.md`, section 1 "Settings file") and applied live: there is no explicit
//! "Save" action, every change made through the Settings view (wired in a later phase) writes
//! the file immediately.

use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use postino_runner::InvalidCertificates;
use rust_i18n::t;
use serde::Serialize;

use super::locale::LanguageChoice;

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

/// A category of the Settings view's left nav.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsCategory {
    /// Theme, fonts and sizes.
    #[default]
    Appearance,
    /// How requests are sent (TLS certificates).
    Requests,
    /// Diagnostics: the log level and the log file.
    Advanced,
}

impl SettingsCategory {
    /// Every category, in the order the nav lists them.
    pub const ALL: [SettingsCategory; 3] = [
        SettingsCategory::Appearance,
        SettingsCategory::Requests,
        SettingsCategory::Advanced,
    ];

    /// The nav item label, also the pane's title.
    pub fn label(self) -> String {
        match self {
            SettingsCategory::Appearance => t!("settings.category.appearance"),
            SettingsCategory::Requests => t!("common.requests"),
            SettingsCategory::Advanced => t!("settings.category.advanced"),
        }
        .into_owned()
    }
}

/// What to do when a server's TLS certificate is invalid, chosen in Settings, "Requests". Stored
/// in `settings.toml` as `invalid_tls_certificates = "warn" | "reject" | "accept"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InvalidTlsCertificates {
    /// Send the request anyway and show a warning in the response.
    #[default]
    Warn,
    /// Fail the request with an error.
    Reject,
    /// Accept any certificate without a warning.
    Accept,
}

impl InvalidTlsCertificates {
    /// Every choice, in the order the Settings view lists them.
    pub const ALL: [InvalidTlsCertificates; 3] = [
        InvalidTlsCertificates::Warn,
        InvalidTlsCertificates::Reject,
        InvalidTlsCertificates::Accept,
    ];

    /// Parses a value as read from the settings file, case-insensitive. Returns `None` for
    /// anything else, so the caller can fall back to the default.
    fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "warn" => Some(InvalidTlsCertificates::Warn),
            "reject" => Some(InvalidTlsCertificates::Reject),
            "accept" => Some(InvalidTlsCertificates::Accept),
            _ => None,
        }
    }

    /// The label shown in the Settings view.
    pub fn label(self) -> String {
        match self {
            InvalidTlsCertificates::Warn => t!("settings.tls.warn"),
            InvalidTlsCertificates::Reject => t!("settings.tls.reject"),
            InvalidTlsCertificates::Accept => t!("settings.tls.accept"),
        }
        .into_owned()
    }

    /// The matching `postino-http` mode.
    pub fn to_http(self) -> InvalidCertificates {
        match self {
            InvalidTlsCertificates::Warn => InvalidCertificates::SendWithWarning,
            InvalidTlsCertificates::Reject => InvalidCertificates::Reject,
            InvalidTlsCertificates::Accept => InvalidCertificates::Accept,
        }
    }
}

/// How much Postino writes to its log file (`crate::logging`), chosen in Settings, "Advanced".
/// Stored in `settings.toml` as `log_level = "off" | "error" | "warn" | "info" | "debug" |
/// "trace"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Nothing is logged.
    Off,
    /// Only errors.
    Error,
    /// Errors and warnings.
    Warn,
    /// Startup, workspace and request summaries, plus warnings and errors.
    #[default]
    Info,
    /// Also timings and state changes useful to diagnose a problem.
    Debug,
    /// Everything Postino and gpui log, including every render, plus third party crates' Info
    /// messages. Verbose.
    Trace,
}

impl LogLevel {
    /// Every level, in the order the Settings view lists them.
    pub const ALL: [LogLevel; 6] = [
        LogLevel::Off,
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ];

    /// Parses a level as read from the settings file or the `POSTINO_LOG` environment variable,
    /// case-insensitive. Returns `None` for anything else, so the caller can fall back.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" => Some(LogLevel::Off),
            "error" => Some(LogLevel::Error),
            "warn" | "warning" => Some(LogLevel::Warn),
            "info" => Some(LogLevel::Info),
            "debug" => Some(LogLevel::Debug),
            "trace" => Some(LogLevel::Trace),
            _ => None,
        }
    }

    /// The label shown in the Settings view.
    pub fn label(self) -> String {
        match self {
            LogLevel::Off => t!("settings.log_level.off"),
            LogLevel::Error => t!("settings.log_level.error"),
            LogLevel::Warn => t!("settings.log_level.warn"),
            LogLevel::Info => t!("settings.log_level.info"),
            LogLevel::Debug => t!("settings.log_level.debug"),
            LogLevel::Trace => t!("settings.log_level.trace"),
        }
        .into_owned()
    }

    /// The matching `log` crate filter.
    pub fn to_filter(self) -> log::LevelFilter {
        match self {
            LogLevel::Off => log::LevelFilter::Off,
            LogLevel::Error => log::LevelFilter::Error,
            LogLevel::Warn => log::LevelFilter::Warn,
            LogLevel::Info => log::LevelFilter::Info,
            LogLevel::Debug => log::LevelFilter::Debug,
            LogLevel::Trace => log::LevelFilter::Trace,
        }
    }
}

/// User-configurable settings: the UI language, the theme mode, the two fonts (UI and editor) with their sizes,
/// how requests treat invalid TLS certificates, and the log level.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    /// The UI language: automatic (the system's) or a fixed one.
    pub language: LanguageChoice,
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
    /// What to do with an invalid TLS certificate.
    pub invalid_tls_certificates: InvalidTlsCertificates,
    /// How much is written to the log file.
    pub log_level: LogLevel,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: LanguageChoice::default(),
            theme: ThemeChoice::default(),
            ui_font: "Geist".to_string(),
            ui_font_size: 13.0,
            mono_font: "Geist Mono".to_string(),
            mono_font_size: 12.5,
            invalid_tls_certificates: InvalidTlsCertificates::default(),
            log_level: LogLevel::default(),
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
/// config directory, a read-only filesystem, ...) is only logged.
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

    let language = table
        .and_then(|table| table.get("language"))
        .and_then(toml::Value::as_str)
        .and_then(LanguageChoice::parse)
        .unwrap_or(defaults.language);
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
    let invalid_tls_certificates = table
        .and_then(|table| table.get("invalid_tls_certificates"))
        .and_then(toml::Value::as_str)
        .and_then(InvalidTlsCertificates::parse)
        .unwrap_or(defaults.invalid_tls_certificates);
    let log_level = table
        .and_then(|table| table.get("log_level"))
        .and_then(toml::Value::as_str)
        .and_then(LogLevel::parse)
        .unwrap_or(defaults.log_level);

    Settings {
        language,
        theme,
        ui_font,
        ui_font_size,
        mono_font,
        mono_font_size,
        invalid_tls_certificates,
        log_level,
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
    if let Err(error) = fs::create_dir_all(parent) {
        log::warn!("could not create {}: {error}", parent.display());
        return;
    }
    let text = match toml::to_string_pretty(settings) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("could not serialize the settings: {error}");
            return;
        }
    };
    if let Err(error) = fs::write(&path, text) {
        log::warn!("could not save {}: {error}", path.display());
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn defaults_are_the_documented_values() {
        let settings = Settings::default();
        assert_eq!(settings.language, LanguageChoice::Auto);
        assert_eq!(settings.theme, ThemeChoice::System);
        assert_eq!(settings.ui_font, "Geist");
        assert_eq!(settings.ui_font_size, 13.0);
        assert_eq!(settings.mono_font, "Geist Mono");
        assert_eq!(settings.mono_font_size, 12.5);
        assert_eq!(
            settings.invalid_tls_certificates,
            InvalidTlsCertificates::Warn
        );
        assert_eq!(settings.log_level, LogLevel::Info);
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
            language: LanguageChoice::Gl,
            theme: ThemeChoice::Dark,
            ui_font: "Inter".to_string(),
            ui_font_size: 14.0,
            mono_font: "Fira Code".to_string(),
            mono_font_size: 13.5,
            invalid_tls_certificates: InvalidTlsCertificates::Reject,
            log_level: LogLevel::Trace,
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
    fn invalid_tls_certificates_parses_every_value() {
        let parse = |text: &str| parse_settings(text).invalid_tls_certificates;
        assert_eq!(
            parse("invalid_tls_certificates = \"warn\"\n"),
            InvalidTlsCertificates::Warn
        );
        assert_eq!(
            parse("invalid_tls_certificates = \"reject\"\n"),
            InvalidTlsCertificates::Reject
        );
        assert_eq!(
            parse("invalid_tls_certificates = \"ACCEPT\"\n"),
            InvalidTlsCertificates::Accept
        );
    }

    #[test]
    fn invalid_tls_certificates_falls_back_to_warn() {
        let parse = |text: &str| parse_settings(text).invalid_tls_certificates;
        assert_eq!(parse(""), InvalidTlsCertificates::Warn);
        assert_eq!(
            parse("invalid_tls_certificates = \"nonsense\"\n"),
            InvalidTlsCertificates::Warn
        );
        assert_eq!(
            parse("invalid_tls_certificates = 3\n"),
            InvalidTlsCertificates::Warn
        );
    }

    #[test]
    fn invalid_tls_certificates_serializes_as_its_toml_value() {
        let settings = Settings {
            invalid_tls_certificates: InvalidTlsCertificates::Accept,
            ..Settings::default()
        };
        let text = toml::to_string_pretty(&settings).expect("serialize");
        assert!(
            text.contains("invalid_tls_certificates = \"accept\""),
            "{text}"
        );
    }

    #[test]
    fn invalid_tls_certificates_maps_to_the_http_modes() {
        assert_eq!(
            InvalidTlsCertificates::Warn.to_http(),
            InvalidCertificates::SendWithWarning
        );
        assert_eq!(
            InvalidTlsCertificates::Reject.to_http(),
            InvalidCertificates::Reject
        );
        assert_eq!(
            InvalidTlsCertificates::Accept.to_http(),
            InvalidCertificates::Accept
        );
    }

    #[test]
    fn log_level_parses_every_value_and_falls_back_to_info() {
        let parse = |text: &str| parse_settings(text).log_level;
        for level in LogLevel::ALL {
            let text = format!("log_level = \"{level:?}\"\n");
            assert_eq!(parse(&text), level);
        }
        assert_eq!(parse(""), LogLevel::Info);
        assert_eq!(parse("log_level = \"loud\"\n"), LogLevel::Info);
        assert_eq!(LogLevel::parse(" warning "), Some(LogLevel::Warn));
    }

    #[test]
    fn log_level_serializes_as_its_toml_value() {
        let settings = Settings {
            log_level: LogLevel::Debug,
            ..Settings::default()
        };
        let text = toml::to_string_pretty(&settings).expect("serialize");
        assert!(text.contains("log_level = \"debug\""), "{text}");
    }

    #[test]
    fn log_level_maps_to_the_log_filters() {
        assert_eq!(LogLevel::Off.to_filter(), log::LevelFilter::Off);
        assert_eq!(LogLevel::Info.to_filter(), log::LevelFilter::Info);
        assert_eq!(LogLevel::Trace.to_filter(), log::LevelFilter::Trace);
    }

    #[test]
    fn categories_start_on_appearance() {
        assert_eq!(SettingsCategory::default(), SettingsCategory::Appearance);
        assert_eq!(SettingsCategory::Requests.label(), "Requests");
    }

    #[test]
    fn language_parses_every_value_and_falls_back_to_auto() {
        let parse = |text: &str| parse_settings(text).language;
        assert_eq!(parse("language = \"es\"\n"), LanguageChoice::Es);
        assert_eq!(parse("language = \"IT\"\n"), LanguageChoice::It);
        assert_eq!(parse("language = \"auto\"\n"), LanguageChoice::Auto);
        assert_eq!(parse(""), LanguageChoice::Auto);
        assert_eq!(parse("language = \"klingon\"\n"), LanguageChoice::Auto);
        assert_eq!(parse("language = 3\n"), LanguageChoice::Auto);
    }

    #[test]
    fn language_serializes_as_its_toml_value() {
        let settings = Settings {
            language: LanguageChoice::Gl,
            ..Settings::default()
        };
        let text = toml::to_string_pretty(&settings).expect("serialize");
        assert!(text.contains("language = \"gl\""), "{text}");
    }

    #[test]
    fn labels_are_translated_with_the_requested_locale() {
        assert_eq!(t!("common.requests", locale = "es"), "Peticiones");
        assert_eq!(t!("settings.tls.reject", locale = "gl"), "Rexeitar");
        assert_eq!(t!("settings.log_level.off", locale = "it"), "Disattivato");
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
