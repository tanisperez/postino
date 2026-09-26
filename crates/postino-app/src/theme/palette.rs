//! Postino's color palette: light and dark [`Palette`] built from the design tokens in
//! `plans/ui-redesign.md` section 2.1, and [`PaletteExt`], the extension trait views use to
//! reach them.
//!
//! Every hex and shadow literal below is copied verbatim from the `--bg:...` custom properties
//! in `postino_design_system/Postino Screens.dc.html` (the light and dark blocks), never
//! retyped from memory. [`LIGHT`] and [`DARK`] are the only place those literals are written:
//! both [`Palette`] (parsed [`Hsla`] fields, read directly by views) and the gpui-kit theme
//! family JSON built in `theme::mod` (via [`theme_colors_and_highlight`]) are derived from them,
//! so there is no second, hand-written copy of the color values.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, BoxShadow, Hsla, Rgba, px};

use postino_core::Method;

use crate::state::env_color::EnvColor;

/// The raw string values of one theme mode. Private: [`Palette::light`], [`Palette::dark`] and
/// [`theme_colors_and_highlight`] are the only consumers.
struct Tokens {
    bg: &'static str,
    surface: &'static str,
    raised: &'static str,
    overlay: &'static str,
    border: &'static str,
    border_strong: &'static str,
    fg: &'static str,
    fg_muted: &'static str,
    fg_subtle: &'static str,
    accent: &'static str,
    accent_fg: &'static str,
    accent_text: &'static str,
    accent_subtle: &'static str,
    hover: &'static str,
    success: &'static str,
    success_subtle: &'static str,
    warning: &'static str,
    warning_subtle: &'static str,
    danger: &'static str,
    danger_subtle: &'static str,
    info: &'static str,
    info_subtle: &'static str,
    m_get: &'static str,
    m_post: &'static str,
    m_put: &'static str,
    m_patch: &'static str,
    m_delete: &'static str,
    syn_key: &'static str,
    syn_str: &'static str,
    syn_num: &'static str,
    syn_bool: &'static str,
    /// The two `--shadow` layers, in CSS `box-shadow` order (offset y, blur, spread, color).
    /// Horizontal offset is always 0 in this design, so it is not stored.
    shadow: [ShadowLayer; 2],
}

/// One layer of a `--shadow` token.
struct ShadowLayer {
    offset_y: f32,
    blur: f32,
    spread: f32,
    color: (u8, u8, u8, f32),
}

const LIGHT: Tokens = Tokens {
    bg: "#fcfcfd",
    surface: "#f4f5f7",
    raised: "#ffffff",
    overlay: "#ffffff",
    border: "#e3e5ea",
    border_strong: "#cdd0d8",
    fg: "#1b1c22",
    fg_muted: "#5c606e",
    fg_subtle: "#8b8e9a",
    accent: "#4f57d8",
    accent_fg: "#ffffff",
    accent_text: "#3f47c4",
    accent_subtle: "#e8e9fd",
    hover: "#eceef2",
    success: "#1d8657",
    success_subtle: "#e2f4ea",
    warning: "#9a6500",
    warning_subtle: "#fbf0d9",
    danger: "#cf3535",
    danger_subtle: "#fde7e7",
    info: "#1c6fb0",
    info_subtle: "#e1eef9",
    m_get: "#1d8657",
    m_post: "#9a6500",
    m_put: "#1c6fb0",
    m_patch: "#7a4fd6",
    m_delete: "#cf3535",
    syn_key: "#3f47c4",
    syn_str: "#1d7a4f",
    syn_num: "#9a6500",
    syn_bool: "#b03a78",
    shadow: [
        ShadowLayer {
            offset_y: 12.0,
            blur: 32.0,
            spread: 0.0,
            color: (20, 22, 40, 0.14),
        },
        ShadowLayer {
            offset_y: 0.0,
            blur: 0.0,
            spread: 1.0,
            color: (20, 22, 40, 0.06),
        },
    ],
};

const DARK: Tokens = Tokens {
    bg: "#15161a",
    surface: "#1b1c21",
    raised: "#23252c",
    overlay: "#262830",
    border: "#2c2e36",
    border_strong: "#3b3e48",
    fg: "#e9eaee",
    fg_muted: "#a0a3ae",
    fg_subtle: "#6f7280",
    accent: "#5b63e6",
    accent_fg: "#ffffff",
    accent_text: "#a3a9ff",
    accent_subtle: "#272b4d",
    hover: "#25272e",
    success: "#4fc88f",
    success_subtle: "#15302a",
    warning: "#e8b75a",
    warning_subtle: "#352b15",
    danger: "#f27171",
    danger_subtle: "#3b1e21",
    info: "#62b3f0",
    info_subtle: "#162a3b",
    m_get: "#4fc88f",
    m_post: "#e8b75a",
    m_put: "#62b3f0",
    m_patch: "#b597f7",
    m_delete: "#f27171",
    syn_key: "#a3a9ff",
    syn_str: "#86d7a5",
    syn_num: "#e8b75a",
    syn_bool: "#f3a2c6",
    shadow: [
        ShadowLayer {
            offset_y: 12.0,
            blur: 32.0,
            spread: 0.0,
            color: (0, 0, 0, 0.45),
        },
        ShadowLayer {
            offset_y: 0.0,
            blur: 0.0,
            spread: 1.0,
            color: (255, 255, 255, 0.04),
        },
    ],
};

/// One [`Hsla`] field per design token (`plans/ui-redesign.md` section 2.1). Views read colors
/// only from here, through [`PaletteExt::palette`], never from `cx.theme()` directly and never
/// from a literal.
///
/// Most fields are read directly by views from phase 3 on (`bg`, `border`, `syn_key`, ...); a
/// handful already have a Rust-side helper in this file (`method_color`, `status_colors`,
/// `env_color`). Nothing calls either kind yet, so this struct allows dead code for itself.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Palette {
    /// Background of editor and content panels.
    pub bg: Hsla,
    /// Sidebar, title bar and tab bar background.
    pub surface: Hsla,
    /// Inputs, cards and the code editor background.
    pub raised: Hsla,
    /// Menus, popovers and modals background.
    pub overlay: Hsla,
    /// Default separator and border color.
    pub border: Hsla,
    /// Border color for controls (inputs, buttons).
    pub border_strong: Hsla,
    /// Primary text color.
    pub fg: Hsla,
    /// Secondary text, inactive tabs.
    pub fg_muted: Hsla,
    /// Placeholders, icons, line numbers.
    pub fg_subtle: Hsla,
    /// Primary button, focus ring, active tab.
    pub accent: Hsla,
    /// Text drawn on an `accent` background.
    pub accent_fg: Hsla,
    /// Text and links drawn in the accent color.
    pub accent_text: Hsla,
    /// Selection background, variable chips.
    pub accent_subtle: Hsla,
    /// Hover background for rows and ghost buttons.
    pub hover: Hsla,
    /// 2xx status, passing tests, GET.
    pub success: Hsla,
    /// Success status badge background.
    pub success_subtle: Hsla,
    /// 4xx status, warnings, POST.
    pub warning: Hsla,
    /// Warning status badge background.
    pub warning_subtle: Hsla,
    /// 5xx status, errors, DELETE.
    pub danger: Hsla,
    /// Danger status badge background.
    pub danger_subtle: Hsla,
    /// 3xx status, PUT.
    pub info: Hsla,
    /// Info status badge background.
    pub info_subtle: Hsla,
    /// GET method color.
    pub m_get: Hsla,
    /// POST method color.
    pub m_post: Hsla,
    /// PUT method color.
    pub m_put: Hsla,
    /// PATCH method color.
    pub m_patch: Hsla,
    /// DELETE method color.
    pub m_delete: Hsla,
    /// JSON object key syntax highlight color.
    pub syn_key: Hsla,
    /// String syntax highlight color.
    pub syn_str: Hsla,
    /// Number syntax highlight color.
    pub syn_num: Hsla,
    /// Boolean syntax highlight color.
    pub syn_bool: Hsla,
    /// The elevation shadow for `overlay`-level surfaces (menus, popovers, the Settings modal),
    /// the two layers of the design's `--shadow` token. There is no `ThemeConfigColors` field
    /// for it (`plans/ui-redesign-spikes.md` section 1.1), so views apply it directly with
    /// gpui's own `.shadow(...)`. Used from phase 3 on (`Card`, menus, the Settings dialog).
    pub shadow: Vec<BoxShadow>,
}

impl Palette {
    /// The Postino Light palette.
    pub fn light() -> Self {
        Self::from_tokens(&LIGHT)
    }

    /// The Postino Dark palette.
    pub fn dark() -> Self {
        Self::from_tokens(&DARK)
    }

    fn from_tokens(tokens: &Tokens) -> Self {
        Self {
            bg: hex(tokens.bg),
            surface: hex(tokens.surface),
            raised: hex(tokens.raised),
            overlay: hex(tokens.overlay),
            border: hex(tokens.border),
            border_strong: hex(tokens.border_strong),
            fg: hex(tokens.fg),
            fg_muted: hex(tokens.fg_muted),
            fg_subtle: hex(tokens.fg_subtle),
            accent: hex(tokens.accent),
            accent_fg: hex(tokens.accent_fg),
            accent_text: hex(tokens.accent_text),
            accent_subtle: hex(tokens.accent_subtle),
            hover: hex(tokens.hover),
            success: hex(tokens.success),
            success_subtle: hex(tokens.success_subtle),
            warning: hex(tokens.warning),
            warning_subtle: hex(tokens.warning_subtle),
            danger: hex(tokens.danger),
            danger_subtle: hex(tokens.danger_subtle),
            info: hex(tokens.info),
            info_subtle: hex(tokens.info_subtle),
            m_get: hex(tokens.m_get),
            m_post: hex(tokens.m_post),
            m_put: hex(tokens.m_put),
            m_patch: hex(tokens.m_patch),
            m_delete: hex(tokens.m_delete),
            syn_key: hex(tokens.syn_key),
            syn_str: hex(tokens.syn_str),
            syn_num: hex(tokens.syn_num),
            syn_bool: hex(tokens.syn_bool),
            shadow: tokens.shadow.iter().map(shadow_layer).collect(),
        }
    }

    /// The color a method is drawn in: the five standard methods get their own color, HEAD,
    /// OPTIONS and custom methods fall back to `fg_muted` (`plans/ui-redesign.md` section 2.1).
    /// Used from phase 3 on (`MethodBadge`).
    #[allow(dead_code)]
    pub fn method_color(&self, method: &Method) -> Hsla {
        match method {
            Method::Get => self.m_get,
            Method::Post => self.m_post,
            Method::Put => self.m_put,
            Method::Patch => self.m_patch,
            Method::Delete => self.m_delete,
            Method::Head | Method::Options | Method::Custom(_) => self.fg_muted,
        }
    }

    /// The method badge's background: the method color at 14% alpha, matching the design's
    /// `color-mix(... 14%, transparent)`. Used from phase 3 on (`MethodBadge`).
    #[allow(dead_code)]
    pub fn method_badge_bg(&self, method: &Method) -> Hsla {
        self.method_color(method).opacity(0.14)
    }

    /// The status badge's foreground and background for a response status code
    /// (`plans/ui-redesign.md` section 2.1): 2xx `success`, 3xx `info`, 4xx `warning`, 5xx
    /// `danger`. `None` ("Not sent" or a send failure) is `fg_muted` on `hover`. Used from
    /// phase 3 on (`StatusBadge`).
    #[allow(dead_code)]
    pub fn status_colors(&self, status: Option<u16>) -> (Hsla, Hsla) {
        match status {
            Some(code) if (200..300).contains(&code) => (self.success, self.success_subtle),
            Some(code) if (300..400).contains(&code) => (self.info, self.info_subtle),
            Some(code) if (400..500).contains(&code) => (self.warning, self.warning_subtle),
            Some(code) if code >= 500 => (self.danger, self.danger_subtle),
            _ => (self.fg_muted, self.hover),
        }
    }

    /// The dot color for an environment tier, from `state::env_color::EnvColor`. "No
    /// environment" has no [`EnvColor`] of its own (`state::env_color`'s doc comment): views draw
    /// its hollow ring straight from `fg_subtle` instead of calling this. Used from phase 3 on
    /// (`EnvPill`, `EnvMenu`).
    #[allow(dead_code)]
    pub fn env_color(&self, color: EnvColor) -> Hsla {
        match color {
            EnvColor::Danger => self.danger,
            EnvColor::Warning => self.warning,
            EnvColor::Success => self.success,
        }
    }
}

/// Gives any `App` (and anything that derefs to it, such as `Context<T>`, the same way
/// `cx.theme()` already works) one-call access to the palette matching the active theme mode.
/// Used from phase 3 on; nothing calls it yet.
#[allow(dead_code)]
pub trait PaletteExt {
    /// The light or dark [`Palette`], matching `cx.theme().is_dark()`.
    fn palette(&self) -> Palette;
}

impl PaletteExt for App {
    fn palette(&self) -> Palette {
        if self.theme().is_dark() {
            Palette::dark()
        } else {
            Palette::light()
        }
    }
}

/// The `"colors"` and `"highlight"` JSON fragments for one mode's `ThemeConfig`
/// (`plans/ui-redesign-spikes.md` section 1, table 1.1 and section 1.2), built straight from
/// this file's own [`LIGHT`]/[`DARK`] tokens, never a separate literal.
pub(super) fn theme_colors_and_highlight(dark: bool) -> (serde_json::Value, serde_json::Value) {
    let tokens = if dark { &DARK } else { &LIGHT };
    let colors = serde_json::json!({
        "background": tokens.bg,
        "sidebar.background": tokens.surface,
        "title_bar.background": tokens.surface,
        "tab_bar.background": tokens.surface,
        "secondary.background": tokens.raised,
        "popover.background": tokens.overlay,
        "border": tokens.border,
        "input.border": tokens.border_strong,
        "foreground": tokens.fg,
        "muted.foreground": tokens.fg_muted,
        "primary.background": tokens.accent,
        "primary.foreground": tokens.accent_fg,
        "link": tokens.accent_text,
        "accent.background": tokens.accent_subtle,
        "selection.background": tokens.accent_subtle,
        "list.active.background": tokens.accent_subtle,
        "list.hover.background": tokens.hover,
        "button.hover.background": tokens.hover,
        "table.hover.background": tokens.hover,
        "success.background": tokens.success,
        "warning.background": tokens.warning,
        "danger.background": tokens.danger,
        "info.background": tokens.info,
    });
    let highlight = serde_json::json!({
        "syntax": {
            "property": { "color": tokens.syn_key },
            "string": { "color": tokens.syn_str },
            "number": { "color": tokens.syn_num },
            "boolean": { "color": tokens.syn_bool },
        }
    });
    (colors, highlight)
}

/// Builds a [`BoxShadow`] from one `--shadow` layer.
fn shadow_layer(layer: &ShadowLayer) -> BoxShadow {
    let (r, g, b, a) = layer.color;
    BoxShadow::new(px(0.0), px(layer.offset_y), rgba(r, g, b, a))
        .blur_radius(px(layer.blur))
        .spread_radius(px(layer.spread))
}

/// Parses a `#rrggbb` literal into an opaque [`Hsla`]. Every call site passes one of the
/// constants above, copied verbatim from the design tokens, so malformed input cannot occur in
/// practice; a bad literal falls back to opaque black instead of panicking.
fn hex(value: &str) -> Hsla {
    let digits = value.trim_start_matches('#');
    let parsed = u32::from_str_radix(digits, 16).unwrap_or(0);
    rgba(
        ((parsed >> 16) & 0xff) as u8,
        ((parsed >> 8) & 0xff) as u8,
        (parsed & 0xff) as u8,
        1.0,
    )
}

/// Builds an [`Hsla`] from 8-bit RGB channels and a 0.0..1.0 alpha.
fn rgba(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
    .into()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
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
    fn light_palette_matches_design_tokens() {
        let palette = Palette::light();
        assert_eq!(to_hex(palette.bg), "#fcfcfd");
        assert_eq!(to_hex(palette.accent), "#4f57d8");
        assert_eq!(to_hex(palette.m_post), "#9a6500");
        assert_eq!(to_hex(palette.syn_key), "#3f47c4");
    }

    #[test]
    fn dark_palette_matches_design_tokens() {
        let palette = Palette::dark();
        assert_eq!(to_hex(palette.bg), "#15161a");
        assert_eq!(to_hex(palette.accent), "#5b63e6");
        assert_eq!(to_hex(palette.m_post), "#e8b75a");
        assert_eq!(to_hex(palette.syn_key), "#a3a9ff");
    }

    #[test]
    fn method_color_falls_back_to_fg_muted() {
        let palette = Palette::light();
        assert_eq!(palette.method_color(&Method::Get), palette.m_get);
        assert_eq!(palette.method_color(&Method::Head), palette.fg_muted);
        assert_eq!(
            palette.method_color(&Method::Custom("PURGE".to_string())),
            palette.fg_muted
        );
    }

    #[test]
    fn method_badge_bg_is_method_color_at_14_percent() {
        let palette = Palette::light();
        let badge = palette.method_badge_bg(&Method::Get);
        assert_eq!(badge.a, palette.m_get.a * 0.14);
    }

    #[test]
    fn status_colors_map_ranges_to_tokens() {
        let palette = Palette::light();
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
        let palette = Palette::light();
        assert_eq!(palette.env_color(EnvColor::Danger), palette.danger);
        assert_eq!(palette.env_color(EnvColor::Warning), palette.warning);
        assert_eq!(palette.env_color(EnvColor::Success), palette.success);
    }
}
