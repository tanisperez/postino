//! Postino's palette: telaio's [`Palette`] (its default tokens are Postino's own, listed in
//! `docs/design-system.md`) plus the domain colors no other app has, through [`PostinoColors`]:
//! one color per HTTP method, the status code mapping and the environment tiers.
//!
//! The method colors are written once, in [`LIGHT`] and [`DARK`] below (keep them in sync with
//! `docs/design-system.md`).

use gpui_kit::Hsla;

use postino_core::Method;
pub use telaio_ui::{Palette, PaletteExt, Tone};

use crate::state::env_color::EnvColor;

/// The method colors of one theme mode.
struct MethodTokens {
    get: &'static str,
    post: &'static str,
    put: &'static str,
    patch: &'static str,
    delete: &'static str,
}

const LIGHT: MethodTokens = MethodTokens {
    get: "#1d8657",
    post: "#9a6500",
    put: "#1c6fb0",
    patch: "#7a4fd6",
    delete: "#cf3535",
};

const DARK: MethodTokens = MethodTokens {
    get: "#4fc88f",
    post: "#e8b75a",
    put: "#62b3f0",
    patch: "#b597f7",
    delete: "#f27171",
};

/// The tone of a response status code: 2xx `Success`, 3xx `Info`, 4xx `Warning`, 5xx `Danger`.
/// `None` ("Not sent" or a send failure) and anything below 200 are `Neutral`.
pub fn status_tone(status: Option<u16>) -> Tone {
    match status {
        Some(code) if (200..300).contains(&code) => Tone::Success,
        Some(code) if (300..400).contains(&code) => Tone::Info,
        Some(code) if (400..500).contains(&code) => Tone::Warning,
        Some(code) if code >= 500 => Tone::Danger,
        _ => Tone::Neutral,
    }
}

/// Postino's domain colors, read from the active [`Palette`] (`cx.palette().method_color(..)`).
pub trait PostinoColors {
    /// The color a method is drawn in: the five standard methods get their own color, HEAD,
    /// OPTIONS and custom methods fall back to `fg_muted`. Used by the `MethodBadge` component.
    fn method_color(&self, method: &Method) -> Hsla;

    /// The method badge's background: the method color at 14% alpha, matching the design's
    /// `color-mix(... 14%, transparent)`. Used by the `MethodBadge` component's `pill` variant.
    fn method_badge_bg(&self, method: &Method) -> Hsla;

    /// The foreground and background for a response status code, see [`status_tone`].
    fn status_colors(&self, status: Option<u16>) -> (Hsla, Hsla);

    /// The dot color for an environment tier, from `state::env_color::EnvColor`. "No
    /// environment" has no [`EnvColor`] of its own (`state::env_color`'s doc comment): views draw
    /// its hollow ring straight from `fg_subtle` instead of calling this. Used by the `EnvPill`
    /// and `EnvMenu` components.
    fn env_color(&self, color: EnvColor) -> Hsla;
}

impl PostinoColors for Palette {
    fn method_color(&self, method: &Method) -> Hsla {
        let tokens = if self.dark { &DARK } else { &LIGHT };
        match method {
            Method::Get => telaio_ui::palette::hex(tokens.get),
            Method::Post => telaio_ui::palette::hex(tokens.post),
            Method::Put => telaio_ui::palette::hex(tokens.put),
            Method::Patch => telaio_ui::palette::hex(tokens.patch),
            Method::Delete => telaio_ui::palette::hex(tokens.delete),
            Method::Head | Method::Options | Method::Custom(_) => self.fg_muted,
        }
    }

    fn method_badge_bg(&self, method: &Method) -> Hsla {
        self.method_color(method).opacity(0.14)
    }

    fn status_colors(&self, status: Option<u16>) -> (Hsla, Hsla) {
        self.tone_colors(status_tone(status))
    }

    fn env_color(&self, color: EnvColor) -> Hsla {
        match color {
            EnvColor::Danger => self.danger,
            EnvColor::Warning => self.warning,
            EnvColor::Success => self.success,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Rgba;
    use pretty_assertions::assert_eq;

    /// Round-trips an [`Hsla`] back to a `#rrggbb` string for comparison against the design's
    /// own hex literals, sidestepping float precision noise from the HSL conversion.
    fn to_hex(color: Hsla) -> String {
        let rgba = Rgba::from(color);
        format!(
            "#{:02x}{:02x}{:02x}",
            (rgba.r * 255.0).round() as u8,
            (rgba.g * 255.0).round() as u8,
            (rgba.b * 255.0).round() as u8,
        )
    }

    #[test]
    fn palettes_match_design_tokens() {
        let light = Palette::default_light();
        assert_eq!(to_hex(light.bg), "#fcfcfd");
        assert_eq!(to_hex(light.accent), "#4f57d8");
        assert_eq!(to_hex(light.method_color(&Method::Post)), "#9a6500");
        let dark = Palette::default_dark();
        assert_eq!(to_hex(dark.bg), "#15161a");
        assert_eq!(to_hex(dark.accent), "#5b63e6");
        assert_eq!(to_hex(dark.method_color(&Method::Post)), "#e8b75a");
    }

    #[test]
    fn method_color_falls_back_to_fg_muted() {
        let palette = Palette::default_light();
        assert_eq!(to_hex(palette.method_color(&Method::Get)), "#1d8657");
        assert_eq!(palette.method_color(&Method::Head), palette.fg_muted);
        assert_eq!(
            palette.method_color(&Method::Custom("PURGE".to_string())),
            palette.fg_muted
        );
    }

    #[test]
    fn method_badge_bg_is_method_color_at_14_percent() {
        let palette = Palette::default_light();
        let badge = palette.method_badge_bg(&Method::Get);
        assert_eq!(badge.a, palette.method_color(&Method::Get).a * 0.14);
    }

    #[test]
    fn status_colors_map_ranges_to_tokens() {
        let palette = Palette::default_light();
        assert_eq!(
            palette.status_colors(Some(204)),
            (palette.success, palette.success_subtle)
        );
        assert_eq!(
            palette.status_colors(Some(301)),
            (palette.info, palette.info_subtle)
        );
        assert_eq!(
            palette.status_colors(Some(404)),
            (palette.warning, palette.warning_subtle)
        );
        assert_eq!(
            palette.status_colors(Some(503)),
            (palette.danger, palette.danger_subtle)
        );
        assert_eq!(
            palette.status_colors(None),
            (palette.fg_muted, palette.hover)
        );
    }

    #[test]
    fn env_color_maps_every_variant() {
        let palette = Palette::default_light();
        assert_eq!(palette.env_color(EnvColor::Danger), palette.danger);
        assert_eq!(palette.env_color(EnvColor::Warning), palette.warning);
        assert_eq!(palette.env_color(EnvColor::Success), palette.success);
    }
}
