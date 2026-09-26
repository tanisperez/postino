//! Postino's visual foundations: the color palette, layout metrics, the bundled fonts, and the
//! gpui-kit theme family built from the palette (`plans/ui-redesign.md` phase 2).
//!
//! [`install`] does the startup work: register the fonts, build the "Postino Light"/"Postino
//! Dark" theme family JSON from [`palette::Palette`]'s own tokens, and load it into gpui-kit's
//! `ThemeRegistry` so `Theme::change`/`Theme::sync_system_appearance` resolve to it instead of
//! gpui-component's built-in default (`plans/ui-redesign-spikes.md` section 1).

pub mod metrics;
pub mod palette;

// Re-exported for views (`cx.palette()`); every component in `views/components/` uses it.
pub use palette::{Palette, PaletteExt};

use std::borrow::Cow;

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeRegistry};

use crate::state::settings::Settings;

/// Name of the light theme registered with gpui-kit's `ThemeRegistry`.
const LIGHT_THEME_NAME: &str = "Postino Light";
/// Name of the dark theme registered with gpui-kit's `ThemeRegistry`.
const DARK_THEME_NAME: &str = "Postino Dark";

/// General radius for gpui-kit's own stock widgets (inputs, buttons), matching the design's
/// radius scale (`plans/ui-redesign.md` section 2.2).
const GENERAL_RADIUS: u64 = 8;
/// Large-element radius (dialogs, notifications), matching the design's modal radius.
const LARGE_RADIUS: u64 = 14;

/// Registers the bundled fonts and loads the Postino Light/Dark theme family built from
/// `settings`'s font choices, making them the registry's default light and dark themes. Call
/// once, after `gpui_kit::init`, before opening any window: `ThemeRegistry` is set up as a global
/// inside `init`, and the window's own `Theme::change`/`Theme::sync_system_appearance` call is
/// what makes the loaded family actually visible.
pub fn install(cx: &mut App, settings: &Settings) {
    register_fonts(cx);

    let json = theme_family_json(settings).to_string();
    // Built by this module from a fixed, well-formed shape (`tests::family_json_parses_as_a_
    // theme_set` below covers it), so a parse failure here would be a bug in this function, not
    // a runtime condition: one of AGENTS.md's "truly impossible state" exceptions.
    #[allow(clippy::expect_used)]
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(&json)
        .expect("theme_family_json always builds a valid ThemeSet, see the tests in this module");

    let registry = ThemeRegistry::global(cx);
    let light = registry.themes().get(LIGHT_THEME_NAME).cloned();
    let dark = registry.themes().get(DARK_THEME_NAME).cloned();
    let theme = Theme::global_mut(cx);
    if let Some(light) = light {
        theme.light_theme = light;
    }
    if let Some(dark) = dark {
        theme.dark_theme = dark;
    }
}

/// Registers the bundled Geist and Geist Mono TTFs with `cx`'s text system
/// (`plans/ui-redesign-spikes.md` section 2). A failure here (the platform text system rejecting
/// well-formed, bundled font bytes) is not worth aborting startup over: the app still runs, just
/// falling back to gpui's own font resolution for the UI and mono families.
fn register_fonts(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-Medium.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-SemiBold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-Bold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-Medium.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-SemiBold.ttf").as_slice()),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

/// Builds the theme family JSON gpui-kit loads at startup, with both "Postino Light" and
/// "Postino Dark" `ThemeConfig` entries. Colors and syntax highlight come from
/// [`palette::theme_colors_and_highlight`], the palette's own tokens; fonts come from `settings`.
/// This is the single place the family is assembled: there is no hand-written theme JSON file.
fn theme_family_json(settings: &Settings) -> serde_json::Value {
    serde_json::json!({
        "name": "Postino",
        "themes": [
            theme_config_json(LIGHT_THEME_NAME, "light", false, settings),
            theme_config_json(DARK_THEME_NAME, "dark", true, settings),
        ],
    })
}

/// Builds one mode's `ThemeConfig` entry.
fn theme_config_json(name: &str, mode: &str, dark: bool, settings: &Settings) -> serde_json::Value {
    let (colors, highlight) = palette::theme_colors_and_highlight(dark);
    serde_json::json!({
        "name": name,
        "mode": mode,
        "font.family": settings.ui_font,
        "font.size": settings.ui_font_size,
        "mono_font.family": settings.mono_font,
        "mono_font.size": settings.mono_font_size,
        "radius": GENERAL_RADIUS,
        "radius.lg": LARGE_RADIUS,
        "colors": colors,
        "highlight": highlight,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use gpui_kit::component::ThemeSet;
    use pretty_assertions::assert_eq;

    #[test]
    fn family_json_parses_as_a_theme_set_with_both_modes() {
        let settings = Settings::default();
        let json = theme_family_json(&settings).to_string();

        let theme_set: ThemeSet =
            serde_json::from_str(&json).expect("theme_family_json must parse as a ThemeSet");

        assert_eq!(theme_set.themes.len(), 2);
        assert_eq!(theme_set.themes[0].name.as_ref(), LIGHT_THEME_NAME);
        assert!(!theme_set.themes[0].mode.is_dark());
        assert_eq!(theme_set.themes[1].name.as_ref(), DARK_THEME_NAME);
        assert!(theme_set.themes[1].mode.is_dark());
    }

    #[test]
    fn family_json_carries_the_settings_fonts() {
        let settings = Settings::default();
        let json = theme_family_json(&settings);

        assert_eq!(json["themes"][0]["font.family"], "Geist");
        assert_eq!(json["themes"][0]["mono_font.family"], "Geist Mono");
        assert_eq!(json["themes"][0]["font.size"], 13.0);
        assert_eq!(json["themes"][0]["mono_font.size"], 12.5);
    }

    #[test]
    fn family_json_colors_match_the_design_tokens() {
        let settings = Settings::default();
        let json = theme_family_json(&settings);

        assert_eq!(json["themes"][0]["colors"]["background"], "#fcfcfd");
        assert_eq!(json["themes"][0]["colors"]["primary.background"], "#4f57d8");
        assert_eq!(json["themes"][1]["colors"]["background"], "#15161a");
        assert_eq!(json["themes"][1]["colors"]["primary.background"], "#5b63e6");
    }
}
