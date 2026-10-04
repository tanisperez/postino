//! The Settings modal (`plans/ui-redesign.md` phase 6): opened from the title bar gear
//! (`views/title_bar.rs`), the command palette (phase 7 only needs to expose the action) and
//! `Ctrl ,` / `Cmd ,` (`main.rs`'s `bind_keys`, `actions::OpenSettings`). Closed with the `x`,
//! Escape, or a click on the dimmed backdrop, all handled by gpui-component's own `Dialog`
//! (`plans/ui-redesign-spikes.md` section 6).
//!
//! Built as a `Dialog` with fully custom content: a left nav ("Settings" title, the single
//! "Appearance" item) and a right column (header, scrollable body, footer), matching
//! `postino_design_system/Settings.dc.html`. Every control writes straight to
//! `AppState::settings` and calls [`AppView::apply_settings_live`], so it updates the global
//! `Theme` and persists to `settings.toml` immediately: there is no "Save" button.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::logging;
use crate::state;
use crate::state::locale::{self, LanguageChoice};
use crate::state::number::format_decimal;
use crate::state::settings::{
    InvalidTlsCertificates, LogLevel, Settings, SettingsCategory, ThemeChoice,
};
use crate::theme::metrics::{CONTROL_HEIGHT, RADIUS_LG, RADIUS_MD};
use crate::theme::{self, Palette, PaletteExt};

use super::components::{GhostButton, IconButton};
use super::root::AppView;

mod about;

/// Width of the modal itself.
const MODAL_WIDTH: f32 = 800.0;
/// Height of the modal itself.
const MODAL_HEIGHT: f32 = 720.0;
/// Least vertical margin kept between the modal and the window edges.
const MODAL_MARGIN_MIN: f32 = 16.0;
/// Width of the modal's left nav column.
const NAV_WIDTH: f32 = 188.0;
/// Height of the right column's header and footer strips.
const HEADER_FOOTER_HEIGHT: f32 = 52.0;
/// Width of a font picker's select box.
const SELECT_WIDTH: f32 = 220.0;
/// Width of a font size stepper's numeric readout.
const STEPPER_VALUE_WIDTH: f32 = 64.0;

impl AppView {
    /// Opens the Settings modal (`plans/ui-redesign.md` phase 6 item 1). A no-op when one is
    /// already open, so the gear and `Ctrl ,` / `Cmd ,` never stack a second modal on top
    /// (GitHub #18).
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let palette = cx.palette();
            let weak = weak.clone();
            // `Dialog` otherwise defaults `margin_top` to a tenth of the viewport and lets
            // `max_h` (viewport minus that margin) win over our own `h(720px)` on a short window
            // (this environment's own 1280x760 fallback window included), clipping the footer.
            // Centering the modal vertically ourselves, with a 16px floor so it never touches the
            // window edge, keeps the requested height on any window tall enough for it and only
            // shrinks (via the scrollable body, not this) on one that is not.
            let viewport_height: f32 = window.viewport_size().height.into();
            let margin_top = ((viewport_height - MODAL_HEIGHT) / 2.0).max(MODAL_MARGIN_MIN);
            dialog
                .w(px(MODAL_WIDTH))
                .h(px(MODAL_HEIGHT))
                .margin_top(px(margin_top))
                .p_0()
                .border_0()
                .bg(palette.overlay)
                .close_button(false)
                .content(move |content, window, cx| {
                    // `DialogContent` (`gpui-component-0.6.6/src/dialog/content.rs`) is
                    // `v_flex().flex_1()` inside the dialog's own `overflow_hidden()` wrapper, but
                    // a flex item's automatic minimum size defaults to its content's natural
                    // size, not 0: without overriding that here, `DialogContent` (and everything
                    // under it, including the scrollable body `render_right_column` builds)
                    // simply grows to fit tall content instead of being clamped to the 720px
                    // `h()`/`max_h` this dialog already has, which is what let the header/footer
                    // through at the default font sizes but not at larger ones, where the grown
                    // content pushed the footer past the ancestor's `overflow_hidden()` with no
                    // way to scroll to it. `min_h_0()` here is what makes the body's own
                    // `overflow_y_scroll()` (`render_right_column`) actually take effect instead.
                    let scroll_handle = window
                        .use_keyed_state("settings-body-scroll", cx, |_, _| ScrollHandle::default())
                        .read(cx)
                        .clone();
                    content
                        .min_h_0()
                        .child(render_settings_body(weak.clone(), scroll_handle, cx))
                })
        });
    }

    /// Applies `self.state.settings` to the live `Theme` (colors, fonts and sizes) and to the
    /// theme mode, refreshes `window`, and persists the file (`plans/ui-redesign.md` phase 6 item
    /// 6). Called after every control in the Settings view changes a value; there is no separate
    /// "Save" action.
    fn apply_settings_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        theme::apply_settings(cx, &self.state.settings);
        match self.state.settings.theme {
            ThemeChoice::System => Theme::sync_system_appearance(Some(window), cx),
            ThemeChoice::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemeChoice::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        }
        // The next request, normal or load test, reads these options.
        self.send_options.invalid_certificates =
            self.state.settings.invalid_tls_certificates.to_http();
        log::info!("settings changed: {:?}", self.state.settings);
        state::settings::save_settings(&self.state.settings);
        cx.notify();
    }

    /// Picks the Settings category shown in the right column.
    fn set_settings_category(&mut self, category: SettingsCategory, cx: &mut Context<Self>) {
        self.settings_category = category;
        cx.notify();
    }

    /// Picks the UI language: applies it process wide, lets every view re-apply the strings it
    /// cached outside render ([`Self::relocalize`]), and repaints every window.
    fn set_language(
        &mut self,
        choice: LanguageChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.settings.language = choice;
        self.apply_language(window, cx);
        self.apply_settings_live(window, cx);
    }

    /// Applies `self.state.settings.language` live, without persisting it.
    fn apply_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = locale::resolve(self.state.settings.language);
        locale::apply(language);
        log::info!("language changed to {}", language.code());
        self.relocalize(window, cx);
        cx.refresh_windows();
    }

    /// Picks what to do with an invalid TLS certificate.
    fn set_invalid_tls_certificates(
        &mut self,
        choice: InvalidTlsCertificates,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.settings.invalid_tls_certificates = choice;
        self.apply_settings_live(window, cx);
    }

    /// Picks the log level, applied to the logger at once.
    fn set_log_level(&mut self, level: LogLevel, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings.log_level = level;
        logging::set_level(level.to_filter());
        self.apply_settings_live(window, cx);
    }

    /// Turns the automatic update check at startup on or off.
    fn set_check_updates(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings.check_updates = enabled;
        self.apply_settings_live(window, cx);
    }

    /// Toggles between the light and dark theme (the command palette's "Toggle theme" action,
    /// `plans/ui-redesign.md` phase 7 item 1): picks the opposite of whichever mode is currently
    /// showing, so it also does the sensible thing from "System" (moving to whichever of Light or
    /// Dark the OS is not currently showing). Always lands on an explicit choice, like clicking a
    /// theme card, so "System" stops following the OS once toggled.
    pub(crate) fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = if cx.theme().is_dark() {
            ThemeChoice::Light
        } else {
            ThemeChoice::Dark
        };
        self.set_theme_choice(next, window, cx);
    }

    /// Picks a theme card (`plans/ui-redesign.md` phase 6 item 3).
    fn set_theme_choice(
        &mut self,
        choice: ThemeChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.settings.theme = choice;
        self.apply_settings_live(window, cx);
    }

    /// Picks the UI font family (`plans/ui-redesign.md` phase 6 item 4).
    fn set_ui_font(&mut self, font: String, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings.ui_font = font;
        self.apply_settings_live(window, cx);
    }

    /// Steps the UI font size by `delta` (`plans/ui-redesign.md` phase 6 item 4).
    fn step_ui_font_size(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings.ui_font_size = state::settings::step_size(
            self.state.settings.ui_font_size,
            delta,
            state::settings::UI_FONT_SIZE_RANGE,
        );
        self.apply_settings_live(window, cx);
    }

    /// Picks the monospace font family (`plans/ui-redesign.md` phase 6 item 5).
    fn set_mono_font(&mut self, font: String, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings.mono_font = font;
        self.apply_settings_live(window, cx);
    }

    /// Steps the monospace font size by `delta` (`plans/ui-redesign.md` phase 6 item 5).
    fn step_mono_font_size(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings.mono_font_size = state::settings::step_size(
            self.state.settings.mono_font_size,
            delta,
            state::settings::MONO_FONT_SIZE_RANGE,
        );
        self.apply_settings_live(window, cx);
    }

    /// Restores `Settings::default()` (`plans/ui-redesign.md` phase 6 item 6, "Reset to
    /// defaults").
    fn reset_settings_to_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings = Settings::default();
        logging::set_level(self.state.settings.log_level.to_filter());
        self.apply_language(window, cx);
        self.apply_settings_live(window, cx);
    }
}

/// Renders the dialog's whole content: the left nav plus the right column. `weak` lets every
/// control reach back into [`AppView`] to change a setting; `cx` is re-read fresh every time the
/// window repaints the dialog (the content builder gpui-component calls is a plain `Fn`, not a
/// `FnMut`, so nothing here is memoized across frames), which is what makes every change appear
/// immediately.
fn render_settings_body(
    weak: WeakEntity<AppView>,
    scroll_handle: ScrollHandle,
    cx: &mut App,
) -> AnyElement {
    let Some(view) = weak.upgrade() else {
        return div().into_any_element();
    };
    let settings = view.read(cx).state.settings.clone();
    let category = view.read(cx).settings_category;
    let about =
        (category == SettingsCategory::About).then(|| about::AboutPane::capture(view.read(cx)));
    let palette = cx.palette();
    let mono_font_family = cx.theme().mono_font_family.clone();
    let installed = cx.text_system().all_font_names();
    let modal_radius = cx.theme().radius_lg;

    h_flex()
        .size_full()
        .child(render_nav(
            weak.clone(),
            &palette,
            &mono_font_family,
            category,
            modal_radius,
        ))
        .child(render_right_column(
            weak,
            scroll_handle,
            category,
            &palette,
            &settings,
            &installed,
            &mono_font_family,
            about.as_ref(),
        ))
        .into_any_element()
}

/// The left nav: "Settings" title, one item per category, and the "Saved to" footer. Its left
/// corners repeat the modal's `radius`: the dialog's `overflow_hidden` clips to its bounds but
/// not to its rounded corners, so the nav's background would otherwise paint square ones.
fn render_nav(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    mono_font_family: &SharedString,
    current: SettingsCategory,
    radius: Pixels,
) -> AnyElement {
    let path_label = state::settings::settings_path()
        .map(|path| state::format::shorten_path(&path, dirs::home_dir().as_deref()))
        .unwrap_or_else(|| t!("common.unknown").into_owned());

    v_flex()
        .flex_none()
        .w(px(NAV_WIDTH))
        .h_full()
        .rounded_l(radius)
        .bg(palette.surface)
        .border_r_1()
        .border_color(palette.border)
        .px(px(10.0))
        .py(px(14.0))
        .gap(px(2.0))
        .child(
            div()
                .px(px(8.0))
                .pt(px(2.0))
                .pb(px(10.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(14.0))
                .child(t!("settings.title")),
        )
        .children(
            SettingsCategory::ALL.into_iter().map(|category| {
                render_nav_item(weak.clone(), palette, category, category == current)
            }),
        )
        .child(div().flex_1())
        .child(
            v_flex()
                .p(px(8.0))
                .text_color(palette.fg_subtle)
                .text_size(px(11.5))
                .line_height(relative(1.5))
                .child(t!("settings.saved_to"))
                .child(
                    div()
                        .font_family(mono_font_family.clone())
                        .child(path_label),
                ),
        )
        .into_any_element()
}

/// One nav item: highlighted when `selected`, hover and pressed backgrounds otherwise.
fn render_nav_item(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    category: SettingsCategory,
    selected: bool,
) -> AnyElement {
    let (id, icon) = match category {
        SettingsCategory::Appearance => ("settings-nav-appearance", Icon::new(IconName::Palette)),
        SettingsCategory::Requests => ("settings-nav-requests", Icon::new(IconName::Globe)),
        SettingsCategory::Advanced => (
            "settings-nav-advanced",
            Icon::new(gpui_kit::assets::IconName::Wrench),
        ),
        SettingsCategory::About => (
            "settings-nav-about",
            Icon::new(gpui_kit::assets::IconName::Info),
        ),
    };
    let item = h_flex()
        .id(id)
        .h(px(28.0))
        .items_center()
        .gap_2()
        .px(px(8.0))
        .rounded(px(RADIUS_MD - 1.0))
        .cursor_pointer();
    let item = if selected {
        item.bg(palette.accent_subtle)
            .text_color(palette.accent_text)
            .font_weight(FontWeight::MEDIUM)
    } else {
        let (hover, pressed) = (palette.hover, palette.pressed);
        item.hover(move |style| style.bg(hover))
            .active(move |style| style.bg(pressed))
    };
    item.child(icon.small())
        .child(category.label())
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |view, cx| view.set_settings_category(category, cx));
        })
        .into_any_element()
}

/// The right column: header, scrollable body (the selected category's sections) and footer.
/// The body's scrollbar is always shown (while the content overflows), unlike the theme's
/// default that only shows it while scrolling: on a short window nothing else tells the user
/// that more settings lie below.
#[allow(clippy::too_many_arguments)]
fn render_right_column(
    weak: WeakEntity<AppView>,
    scroll_handle: ScrollHandle,
    category: SettingsCategory,
    palette: &Palette,
    settings: &Settings,
    installed: &[String],
    mono_font_family: &SharedString,
    about: Option<&about::AboutPane>,
) -> AnyElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(render_header(palette, category))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(render_body(
                    weak.clone(),
                    &scroll_handle,
                    category,
                    palette,
                    settings,
                    installed,
                    mono_font_family,
                    about,
                ))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(settings_scrollbar(&scroll_handle, palette)),
                ),
        )
        .child(if about.is_some() {
            about::render_about_footer(palette)
        } else {
            render_footer(weak, palette)
        })
        .into_any_element()
}

/// The body's always-shown scrollbar. Its track takes the modal's own background (the theme's
/// track color is the window's `bg`, which shows as a darker strip on the modal) and no border,
/// so only the thumb stands out.
fn settings_scrollbar(scroll_handle: &ScrollHandle, palette: &Palette) -> Scrollbar {
    let track = palette.overlay;
    let border = palette.overlay.opacity(0.0);
    Scrollbar::vertical(scroll_handle)
        .mode(ScrollbarMode::Always)
        .styles(|styles| {
            styles
                .track(|style| style.bg(track).border_color(border))
                .track_hover(|style| style.bg(track).border_color(border))
                .track_active(|style| style.bg(track).border_color(border))
        })
}

/// The scrolled part of the right column: the selected category's sections.
#[allow(clippy::too_many_arguments)]
fn render_body(
    weak: WeakEntity<AppView>,
    scroll_handle: &ScrollHandle,
    category: SettingsCategory,
    palette: &Palette,
    settings: &Settings,
    installed: &[String],
    mono_font_family: &SharedString,
    about: Option<&about::AboutPane>,
) -> AnyElement {
    v_flex()
        .id("settings-body")
        .size_full()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .p(px(20.0))
        .gap(px(22.0))
        .children(match category {
            SettingsCategory::Appearance => vec![
                render_language_section(weak.clone(), palette, settings.language),
                divider(palette),
                render_theme_section(weak.clone(), palette, settings.theme),
                divider(palette),
                render_interface_section(
                    weak.clone(),
                    palette,
                    settings,
                    installed,
                    mono_font_family,
                ),
                divider(palette),
                render_editor_section(weak.clone(), palette, settings, installed, mono_font_family),
            ],
            SettingsCategory::Requests => vec![render_tls_section(
                weak.clone(),
                palette,
                settings.invalid_tls_certificates,
            )],
            SettingsCategory::Advanced => vec![render_logging_section(
                weak.clone(),
                palette,
                settings.log_level,
                mono_font_family,
            )],
            SettingsCategory::About => about
                .map(|pane| about::render_about(weak.clone(), palette, mono_font_family, pane))
                .unwrap_or_default(),
        })
        .into_any_element()
}

/// A full-width 1px divider between sections. `flex_none`, or the scrolled body, whose content
/// overflows it, shrinks this contentless line to nothing.
fn divider(palette: &Palette) -> AnyElement {
    div()
        .flex_none()
        .h(px(1.0))
        .bg(palette.border)
        .into_any_element()
}

/// The right column's header: the category title and the close button.
fn render_header(palette: &Palette, category: SettingsCategory) -> AnyElement {
    h_flex()
        .h(px(HEADER_FOOTER_HEIGHT))
        .flex_none()
        .items_center()
        .justify_between()
        .px(px(20.0))
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(category.label()),
        )
        .child(
            IconButton::new("settings-close", IconName::Close)
                .large()
                .tooltip(t!("common.close"))
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                }),
        )
        .into_any_element()
}

/// The right column's footer: "Changes apply immediately." and "Reset to defaults".
fn render_footer(weak: WeakEntity<AppView>, palette: &Palette) -> AnyElement {
    h_flex()
        .h(px(HEADER_FOOTER_HEIGHT))
        .flex_none()
        .items_center()
        .justify_between()
        .px(px(20.0))
        .border_t_1()
        .border_color(palette.border)
        .child(
            div()
                .text_color(palette.fg_subtle)
                .text_size(px(12.0))
                .child(t!("settings.footer.applied")),
        )
        .child(
            GhostButton::new("settings-reset", t!("settings.footer.reset")).on_click(
                move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.reset_settings_to_defaults(window, cx));
                },
            ),
        )
        .into_any_element()
}

/// The dropdown label of `choice`: a language's name in itself, or "Automatic" with the language
/// the system resolves to.
fn language_choice_label(choice: LanguageChoice) -> String {
    match choice.language() {
        Some(language) => language.native_name().to_string(),
        None => t!(
            "settings.language.auto",
            language = locale::resolve(LanguageChoice::Auto).native_name()
        )
        .into_owned(),
    }
}

/// The "Language" section: a dropdown with Automatic and the four languages.
fn render_language_section(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    current: LanguageChoice,
) -> AnyElement {
    let trigger = Button::new("settings-language")
        .ghost()
        .w(px(SELECT_WIDTH))
        .h(px(CONTROL_HEIGHT))
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .text_color(palette.fg)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(div().child(language_choice_label(current)))
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .small()
                        .text_color(palette.fg_subtle),
                ),
        );
    let control = trigger
        .dropdown_menu(move |mut menu, _, _| {
            for choice in LanguageChoice::ALL {
                let target = weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(language_choice_label(choice))
                        .checked(choice == current)
                        .on_click(move |_, window, cx| {
                            let _ =
                                target.update(cx, |view, cx| view.set_language(choice, window, cx));
                        }),
                );
            }
            menu
        })
        .into_any_element();

    v_flex()
        .gap(px(14.0))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("settings.language.title")),
        )
        .child(labeled_row(
            palette,
            t!("settings.language.label"),
            Some(t!("settings.language.description")),
            control,
        ))
        .into_any_element()
}

/// The "Theme" section: the description and the three theme cards.
fn render_theme_section(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    current: ThemeChoice,
) -> AnyElement {
    v_flex()
        .gap(px(10.0))
        .child(
            v_flex()
                .gap(px(2.0))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(t!("settings.theme.title")),
                )
                .child(
                    div()
                        .text_color(palette.fg_muted)
                        .text_size(px(12.0))
                        .child(t!("settings.theme.description")),
                ),
        )
        .child(
            h_flex().gap(px(12.0)).children(
                [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark]
                    .into_iter()
                    .map(|choice| {
                        render_theme_card(weak.clone(), palette, choice, choice == current)
                    }),
            ),
        )
        .into_any_element()
}

/// One theme card (System, Light or Dark): the mini light/dark preview, the radio and the label.
fn render_theme_card(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    choice: ThemeChoice,
    selected: bool,
) -> AnyElement {
    let (id, label): (&'static str, String) = match choice {
        ThemeChoice::System => (
            "settings-theme-system",
            t!("settings.theme.system").into_owned(),
        ),
        ThemeChoice::Light => (
            "settings-theme-light",
            t!("settings.theme.light").into_owned(),
        ),
        ThemeChoice::Dark => (
            "settings-theme-dark",
            t!("settings.theme.dark").into_owned(),
        ),
    };
    // System's preview is half light, half dark; Light and Dark show their own single mode on
    // both sides (`Settings.dc.html`'s own `renderVals()`: `[["System",L,D],["Light",L,L],
    // ["Dark",D,D]]`, the two palettes passed to each card's left and right half).
    let (left, right) = match choice {
        ThemeChoice::System => (Palette::light(), Palette::dark()),
        ThemeChoice::Light => (Palette::light(), Palette::light()),
        ThemeChoice::Dark => (Palette::dark(), Palette::dark()),
    };
    // Every card's left half draws its accent line in the dark palette's accent color, regardless
    // of which theme the card represents: a fixed swatch color `Settings.dc.html` uses for visual
    // consistency across the three previews, not the current theme mode's own accent (its own
    // markup hardcodes the same hex in all three `renderVals()` entries).
    let accent_line = Palette::dark().accent;

    let ring_color = if selected {
        palette.accent
    } else {
        palette.border_strong
    };
    let radio_color = if selected {
        palette.accent
    } else {
        palette.border_strong
    };

    let mut radio = div()
        .size(px(14.0))
        .rounded_full()
        .border_color(radio_color);
    radio = if selected {
        radio.border_4()
    } else {
        radio.border_1()
    };

    // A real border, not a `box-shadow` ring (`Settings.dc.html`'s own `0 0 0 Npx` trick): a
    // spread, zero-blur, zero-offset shadow on a `rounded()` + `overflow_hidden()` box rendered
    // as a filled halo covering the box's own content here, not a thin outline as CSS would, so
    // the border achieves the same "ring" look without that.
    let mut preview_box = h_flex()
        .h(px(92.0))
        .rounded(px(RADIUS_LG))
        .overflow_hidden()
        .border_color(ring_color);
    preview_box = if selected {
        preview_box.border_2()
    } else {
        preview_box.border_1()
    };

    v_flex()
        .id(id)
        .flex_1()
        .gap(px(8.0))
        .cursor_pointer()
        .hover(|style| style.opacity(0.85))
        .active(|style| style.opacity(0.7))
        .child(
            preview_box
                .child(
                    v_flex()
                        .flex_1()
                        .bg(left.surface)
                        .p(px(10.0))
                        .gap(px(5.0))
                        .child(preview_line(0.7, left.border_strong))
                        .child(preview_line(0.5, accent_line))
                        .child(preview_line(0.6, left.border_strong)),
                )
                .child(
                    v_flex()
                        .flex_grow(2.0)
                        .flex_shrink(1.0)
                        .flex_basis(relative(0.0))
                        .bg(right.bg)
                        .p(px(10.0))
                        .gap(px(6.0))
                        .child(
                            div()
                                .h(px(12.0))
                                .rounded(px(4.0))
                                .border_1()
                                .border_color(right.border_strong),
                        )
                        .child(preview_line(0.8, right.border_strong))
                        .child(preview_line(0.55, right.border_strong)),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(8.0))
                .child(radio)
                .child(label),
        )
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.set_theme_choice(choice, window, cx));
        })
        .into_any_element()
}

/// A mini preview "line": a thin, colored, rounded bar `width_fraction` of its parent's width.
fn preview_line(width_fraction: f32, color: Hsla) -> impl IntoElement {
    div()
        .h(px(5.0))
        .w(relative(width_fraction))
        .rounded(px(3.0))
        .bg(color)
}

/// The "Interface" section: the UI font picker and its size stepper.
fn render_interface_section(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    settings: &Settings,
    installed: &[String],
    mono_font_family: &SharedString,
) -> AnyElement {
    let options = state::settings::font_options("Geist", installed);
    v_flex()
        .gap(px(14.0))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("settings.interface.title")),
        )
        .child(labeled_row(
            palette,
            t!("settings.font"),
            Some(t!("settings.interface.font_description")),
            render_font_picker(
                FontPickerSpec {
                    id: "settings-ui-font",
                    palette,
                    mono_style: None,
                    bundled: "Geist",
                    current: &settings.ui_font,
                    options: &options,
                },
                weak.clone(),
                |view, font, window, cx| view.set_ui_font(font, window, cx),
            ),
        ))
        .child(labeled_row(
            palette,
            t!("settings.font_size"),
            None::<SharedString>,
            render_size_stepper(
                "settings-ui-size",
                weak,
                palette,
                settings.ui_font_size,
                mono_font_family.clone(),
                state::settings::UI_FONT_SIZE_STEP,
                |view, delta, window, cx| view.step_ui_font_size(delta, window, cx),
            ),
        ))
        .into_any_element()
}

/// The "Editor" section: the monospace font picker, its size stepper, and the live JSON preview.
fn render_editor_section(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    settings: &Settings,
    installed: &[String],
    mono_font_family: &SharedString,
) -> AnyElement {
    let options = state::settings::font_options("Geist Mono", installed);
    v_flex()
        .gap(px(14.0))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("settings.editor.title")),
        )
        .child(labeled_row(
            palette,
            t!("settings.editor.mono_font"),
            None::<SharedString>,
            render_font_picker(
                FontPickerSpec {
                    id: "settings-mono-font",
                    palette,
                    mono_style: Some(mono_font_family.clone()),
                    bundled: "Geist Mono",
                    current: &settings.mono_font,
                    options: &options,
                },
                weak.clone(),
                |view, font, window, cx| view.set_mono_font(font, window, cx),
            ),
        ))
        .child(labeled_row(
            palette,
            t!("settings.font_size"),
            None::<SharedString>,
            render_size_stepper(
                "settings-mono-size",
                weak.clone(),
                palette,
                settings.mono_font_size,
                mono_font_family.clone(),
                state::settings::MONO_FONT_SIZE_STEP,
                |view, delta, window, cx| view.step_mono_font_size(delta, window, cx),
            ),
        ))
        .child(render_json_preview(
            palette,
            mono_font_family.clone(),
            settings.mono_font_size,
        ))
        .into_any_element()
}

/// The "TLS" section of the Requests pane: what to do with an invalid certificate.
fn render_tls_section(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    current: InvalidTlsCertificates,
) -> AnyElement {
    let trigger = Button::new("settings-invalid-certificates")
        .ghost()
        .w(px(SELECT_WIDTH))
        .h(px(CONTROL_HEIGHT))
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .text_color(palette.fg)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(div().child(current.label()))
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .small()
                        .text_color(palette.fg_subtle),
                ),
        );
    let control = trigger
        .dropdown_menu(move |mut menu, _, _| {
            for choice in InvalidTlsCertificates::ALL {
                let target = weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(choice.label())
                        .checked(choice == current)
                        .on_click(move |_, window, cx| {
                            let _ = target.update(cx, |view, cx| {
                                view.set_invalid_tls_certificates(choice, window, cx)
                            });
                        }),
                );
            }
            menu
        })
        .into_any_element();

    v_flex()
        .gap(px(14.0))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("settings.tls.title")),
        )
        .child(labeled_row(
            palette,
            t!("settings.tls.invalid_certificates"),
            Some(t!("settings.tls.invalid_certificates_description")),
            control,
        ))
        .into_any_element()
}

/// The "Logging" section of the Advanced pane: the log level and where the log file is.
fn render_logging_section(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    current: LogLevel,
    mono_font_family: &SharedString,
) -> AnyElement {
    let trigger = Button::new("settings-log-level")
        .ghost()
        .w(px(SELECT_WIDTH))
        .h(px(CONTROL_HEIGHT))
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .text_color(palette.fg)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(div().child(current.label()))
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .small()
                        .text_color(palette.fg_subtle),
                ),
        );
    let control = trigger
        .dropdown_menu(move |mut menu, _, _| {
            for level in LogLevel::ALL {
                let target = weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(level.label())
                        .checked(level == current)
                        .on_click(move |_, window, cx| {
                            let _ =
                                target.update(cx, |view, cx| view.set_log_level(level, window, cx));
                        }),
                );
            }
            menu
        })
        .into_any_element();

    let log_dir = logging::log_dir();
    let path_label = logging::log_path()
        .map(|path| state::format::shorten_path(&path, dirs::home_dir().as_deref()))
        .unwrap_or_else(|| t!("common.unknown").into_owned());
    let open_folder = GhostButton::new(
        "settings-open-log-folder",
        t!("settings.logging.open_folder"),
    )
    .icon(Icon::new(gpui_kit::assets::IconName::FolderOpen))
    .disabled(log_dir.is_none())
    .on_click(move |_, _, cx| {
        if let Some(dir) = &log_dir {
            cx.open_with_system(dir);
        }
    });

    v_flex()
        .gap(px(14.0))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("settings.logging.title")),
        )
        .child(labeled_row(
            palette,
            t!("settings.logging.level"),
            Some(t!("settings.logging.level_description")),
            control,
        ))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap(px(16.0))
                .child(
                    v_flex()
                        .min_w_0()
                        .gap(px(2.0))
                        .child(div().child(t!("settings.logging.file")))
                        .child(
                            div()
                                .text_color(palette.fg_muted)
                                .text_size(px(12.0))
                                .font_family(mono_font_family.clone())
                                .child(path_label),
                        ),
                )
                .child(open_folder),
        )
        .child(
            div()
                .text_color(palette.fg_subtle)
                .text_size(px(12.0))
                .child(t!(
                    "settings.logging.rotation",
                    size = logging::MAX_FILE_SIZE / (1024 * 1024),
                    files = logging::MAX_FILES,
                    variable = logging::ENV_VAR,
                )),
        )
        .into_any_element()
}

/// A label (with an optional muted description) on the left, an arbitrary control on the right.
fn labeled_row(
    palette: &Palette,
    label: impl Into<SharedString>,
    description: Option<impl Into<SharedString>>,
    control: AnyElement,
) -> AnyElement {
    let label: SharedString = label.into();
    let leading = match description {
        Some(text) => v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.0))
            .child(div().child(label))
            .child(
                div()
                    .text_color(palette.fg_muted)
                    .text_size(px(12.0))
                    .child(text.into()),
            )
            .into_any_element(),
        None => div().flex_1().min_w_0().child(label).into_any_element(),
    };

    // The label column shrinks and wraps a long description instead of pushing the control
    // past the right edge, which kept its natural width and overflowed the modal.
    h_flex()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .child(leading)
        .child(div().flex_none().child(control))
        .into_any_element()
}

/// The display inputs of a [`render_font_picker`], grouped into one struct so the function itself
/// keeps a reasonable argument count.
struct FontPickerSpec<'a> {
    id: &'static str,
    palette: &'a Palette,
    /// The family the trigger renders its own label in, when it differs from the UI font (the
    /// Editor section's monospace picker, per `Settings.dc.html`), at 12.5px.
    mono_style: Option<SharedString>,
    bundled: &'a str,
    current: &'a str,
    options: &'a [String],
}

/// A font picker: a select-styled trigger showing the current family (`"<bundled> (bundled)"` for
/// the bundled one), opening a dropdown menu of `spec.options` (as built by
/// `state::settings::font_options`).
fn render_font_picker(
    spec: FontPickerSpec,
    weak: WeakEntity<AppView>,
    on_pick: impl Fn(&mut AppView, String, &mut Window, &mut Context<AppView>) + Clone + 'static,
) -> AnyElement {
    let FontPickerSpec {
        id,
        palette,
        mono_style,
        bundled,
        current,
        options,
    } = spec;
    let label = if current == bundled {
        t!("settings.font_bundled", name = current).into_owned()
    } else {
        current.to_string()
    };

    let trigger = Button::new(id)
        .ghost()
        .w(px(SELECT_WIDTH))
        .h(px(CONTROL_HEIGHT))
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .text_color(palette.fg);
    // The mono family and size go on the label itself: set on the `Button`, its own text size
    // wins and the label grows with the editor font size, pushing the chevron out of the box.
    let mut label_el = div().child(label);
    if let Some(family) = mono_style {
        label_el = label_el.font_family(family).text_size(px(12.5));
    }
    // Not `.label(...)`/`.icon(...)`: `Button` lays those out in its own inner content row,
    // which hardcodes `justify_center()` on a style field this crate has no builder to reach
    // (`content_style`, `gpui-component-0.6.6/src/button/button.rs:207,289,712`, `pub(crate)`).
    // A single, full-width child of our own, given to `Button` as an ordinary child instead,
    // sidesteps that: `Button`'s row centers *it* (a no-op once it already fills the width), and
    // this row's own `justify_between()` places the label left and the chevron right, matching
    // `Settings.dc.html`'s `justify-content:space-between` selects.
    let trigger = trigger.child(
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(label_el)
            .child(
                Icon::new(IconName::ChevronsUpDown)
                    .small()
                    .text_color(palette.fg_subtle),
            ),
    );

    let options = options.to_vec();
    let bundled = bundled.to_string();
    let current = current.to_string();
    trigger
        .dropdown_menu(move |mut menu, _, _| {
            for name in &options {
                let item_label = if *name == bundled {
                    t!("settings.font_bundled", name = name).into_owned()
                } else {
                    name.clone()
                };
                let selected = *name == current;
                let value = name.clone();
                let target = weak.clone();
                let on_pick = on_pick.clone();
                menu = menu.item(PopupMenuItem::new(item_label).checked(selected).on_click(
                    move |_, window, cx| {
                        let value = value.clone();
                        let on_pick = on_pick.clone();
                        let _ = target.update(cx, |view, cx| on_pick(view, value, window, cx));
                    },
                ));
            }
            menu
        })
        .into_any_element()
}

/// A `-`/`N px`/`+` stepper (`plans/ui-redesign.md` phase 6 items 4 and 5).
fn render_size_stepper(
    id_prefix: &'static str,
    weak: WeakEntity<AppView>,
    palette: &Palette,
    value: f32,
    mono_font_family: SharedString,
    step: f32,
    on_step: impl Fn(&mut AppView, f32, &mut Window, &mut Context<AppView>) + Clone + 'static,
) -> AnyElement {
    let minus_weak = weak.clone();
    let minus_on_step = on_step.clone();
    let plus_on_step = on_step;

    h_flex()
        .items_center()
        .gap(px(8.0))
        .child(stepper_button(
            format!("{id_prefix}-minus"),
            palette,
            IconName::Minus,
            move |window, cx| {
                let _ = minus_weak.update(cx, |view, cx| minus_on_step(view, -step, window, cx));
            },
        ))
        .child(
            div()
                .w(px(STEPPER_VALUE_WIDTH))
                .text_center()
                .font_family(mono_font_family)
                .child(format!(
                    "{} px",
                    format_decimal(f64::from(value), usize::from(value.fract() != 0.0))
                )),
        )
        .child(stepper_button(
            format!("{id_prefix}-plus"),
            palette,
            IconName::Plus,
            move |window, cx| {
                let _ = weak.update(cx, |view, cx| plus_on_step(view, step, window, cx));
            },
        ))
        .into_any_element()
}

/// One `-`/`+` square button of a [`render_size_stepper`].
fn stepper_button(
    id: impl Into<ElementId>,
    palette: &Palette,
    icon: IconName,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size(px(CONTROL_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .hover(|style| style.bg(palette.hover))
        .active(|style| style.bg(palette.pressed))
        .child(Icon::new(icon).small())
        .on_click(move |_, window, cx| on_click(window, cx))
}

/// The Editor section's live preview line: a syntax-highlighted JSON snippet in the chosen
/// monospace font and size (`plans/ui-redesign.md` phase 6 item 5).
fn render_json_preview(palette: &Palette, mono_font_family: SharedString, size: f32) -> AnyElement {
    div()
        .border_1()
        .border_color(palette.border)
        .rounded(px(RADIUS_MD))
        .bg(palette.bg)
        .px(px(14.0))
        .py(px(10.0))
        .child(
            h_flex()
                .items_baseline()
                .font_family(mono_font_family)
                .text_size(px(size))
                .line_height(relative(1.6))
                .child(json_span("{ ", palette.fg_muted))
                .child(json_span("\"token\"", palette.syn_key))
                .child(json_span(": ", palette.fg_muted))
                .child(json_span("\"eyJhbGciOi\u{2026}\"", palette.syn_str))
                .child(json_span(", ", palette.fg_muted))
                .child(json_span("\"expiresIn\"", palette.syn_key))
                .child(json_span(": ", palette.fg_muted))
                .child(json_span("3600", palette.syn_num))
                .child(json_span(" }", palette.fg_muted)),
        )
        .into_any_element()
}

/// One colored fragment of [`render_json_preview`]'s single line.
fn json_span(text: &'static str, color: Hsla) -> impl IntoElement {
    div().text_color(color).child(text)
}
