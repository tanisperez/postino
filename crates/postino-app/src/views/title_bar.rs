//! The title bar: the "P" logo, "Postino", the workspace switcher, the search trigger, the
//! environment pill and the settings gear. The Import menu lives in the sidebar's header, next to
//! "new folder". The window controls at the far right are drawn automatically by gpui-kit's
//! `TitleBar`, so nothing here adds them. The search trigger opens the command palette
//! (`views/command_palette.rs`); the settings gear opens Settings (`views/settings.rs`).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::actions::OpenWorkspace;
use crate::state;
use crate::theme::Palette;
use crate::theme::PaletteExt;
use crate::theme::metrics::{
    ENV_PILL_HEIGHT, MENU_DOT_COLUMN_WIDTH, MENU_ROW_HEIGHT, RADIUS_MD, SEARCH_TRIGGER_WIDTH,
    TITLE_BAR_HEIGHT,
};

use super::root::AppView;
use super::{new_workspace, sidebar};

/// Longest path, in characters, shown next to a recent workspace in the switcher menu. Longer
/// ones get their middle elided so the menu stays as narrow as a normal one.
const MENU_PATH_MAX_CHARS: usize = 36;

impl AppView {
    /// Renders the whole title bar's content.
    pub(crate) fn render_title_bar(
        &mut self,
        weak: WeakEntity<Self>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();

        // gpui-kit's own default (34px) felt cramped; the window controls stretch with it.
        let bar = TitleBar::new().h(px(TITLE_BAR_HEIGHT)).child(
            h_flex()
                .size_full()
                .items_center()
                .gap_2()
                .child(render_logo(&palette))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Postino"))
                .child(div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).bg(palette.border))
                .child(no_drag(
                    self.render_workspace_switcher(weak.clone(), &palette),
                ))
                .child(div().flex_1())
                .child(no_drag(self.render_env_picker(weak.clone(), cx)))
                .child(no_drag(render_settings_gear(weak.clone(), &palette))),
        );

        // The search field is centered on the whole window, not in the gap between the side
        // groups: the environment pill changes width with the active environment's name, which
        // would otherwise move the field sideways. The overlay lives outside `TitleBar` because
        // the bar lays the window controls out beside its content, so centering inside the
        // content would be off by half of their width. The overlay has no handlers, so it does
        // not take mouse input away from the bar.
        div()
            .relative()
            .w_full()
            .flex_none()
            .child(bar)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(no_drag(render_search_trigger(weak, &palette, mono_font))),
            )
            .into_any_element()
    }

    /// Renders the workspace switcher: the folder icon, the open workspace's folder name (or a
    /// placeholder), a chevron, and its dropdown menu (recent workspaces, then "New workspace..."
    /// and "Open folder...").
    fn render_workspace_switcher(&self, weak: WeakEntity<Self>, palette: &Palette) -> AnyElement {
        let label = self
            .state
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.root().file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| t!("shell.title_bar.no_workspace").into_owned());
        // Cached by `open_workspace_at`: this runs on every render, so no file is read here.
        let current_path = self.workspace_path.clone();
        let recent = self.recent_workspaces.clone();
        let home = dirs::home_dir();
        let fg_subtle = palette.fg_subtle;
        let accent_text = palette.accent_text;
        let recent_is_empty = recent.is_empty();

        Button::new("workspace-switcher")
            .ghost()
            .h(px(ENV_PILL_HEIGHT))
            .px_2()
            .rounded(px(RADIUS_MD - 2.0))
            .text_color(palette.fg_muted)
            .icon(Icon::new(IconName::Folder).small())
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for path in &recent {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let shortened = state::format::elide_path(
                        &state::format::shorten_path(path, home.as_deref()),
                        MENU_PATH_MAX_CHARS,
                    );
                    let is_current = current_path.as_deref() == Some(path.as_path());
                    let select_weak = weak.clone();
                    let target = path.clone();

                    let entry = PopupMenuItem::element(move |_, _| {
                        let mut leading = div()
                            .flex_none()
                            .w(px(MENU_DOT_COLUMN_WIDTH))
                            .flex()
                            .justify_center();
                        if is_current {
                            leading = leading
                                .child(Icon::new(IconName::Check).small().text_color(accent_text));
                        }
                        h_flex()
                            .h(px(MENU_ROW_HEIGHT))
                            .items_center()
                            .gap_2()
                            .px_1()
                            .w_full()
                            .child(leading)
                            .child(div().flex_1().child(name.clone()))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(fg_subtle)
                                    .child(shortened.clone()),
                            )
                    })
                    .on_click(move |_, window, cx| {
                        let target = target.clone();
                        let _ = select_weak.update(cx, |view, cx| {
                            view.open_workspace_at(&target, window, cx);
                        });
                    });
                    menu = menu.item(entry);
                }
                if !recent_is_empty {
                    menu = menu.separator();
                }
                let new_weak = weak.clone();
                menu = menu.item(PopupMenuItem::new(t!("common.new_workspace")).on_click(
                    move |_, window, cx| {
                        new_workspace::open_new_workspace_dialog(
                            new_weak.clone(),
                            new_workspace::NewWorkspacePurpose::Example,
                            None,
                            window,
                            cx,
                        );
                    },
                ));
                let open_weak = weak.clone();
                // The action only shows its `Ctrl O` hint: the click handler runs instead of it.
                menu = menu.item(
                    PopupMenuItem::new(t!("common.open_folder"))
                        .action(Box::new(OpenWorkspace))
                        .on_click(move |_, window, cx| {
                            sidebar::pick_workspace_folder(open_weak.clone(), window, cx);
                        }),
                );
                menu
            })
            .into_any_element()
    }
}

/// Wraps an interactive title bar control so the mouse reaches it. `TitleBar` marks the whole bar
/// as a window drag area, and on Windows gpui answers `WM_NCHITTEST` with `HTCAPTION` for any
/// point whose hit test includes that area, so the system eats the clicks and hovers of every
/// control inside it. Occluding the control keeps the bar's area out of the hit test there.
fn no_drag(control: impl IntoElement) -> impl IntoElement {
    div().occlude().child(control)
}

/// The "P" logo square: 18px, radius 5, `accent` background, `accent_fg` text.
fn render_logo(palette: &Palette) -> impl IntoElement {
    div()
        .size(px(18.0))
        .rounded(px(5.0))
        .bg(palette.accent)
        .text_color(palette.accent_fg)
        .text_size(px(11.0))
        .font_weight(FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child("P")
}

/// The centered search trigger: opens the command palette (`views/command_palette.rs`).
fn render_search_trigger(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    mono_font: SharedString,
) -> impl IntoElement {
    h_flex()
        .id("title-bar-search-trigger")
        .w(px(SEARCH_TRIGGER_WIDTH))
        .h(px(ENV_PILL_HEIGHT))
        .items_center()
        .gap_2()
        .px_2p5()
        .cursor_pointer()
        .rounded(px(RADIUS_MD - 1.0))
        .bg(palette.raised)
        .border_1()
        .border_color(palette.border)
        .text_color(palette.fg_subtle)
        .child(Icon::new(IconName::Search).small())
        .child(div().flex_1().child(t!("shell.title_bar.search_hint")))
        .child(
            div()
                .font_family(mono_font)
                .text_size(px(11.0))
                .border_1()
                .border_color(palette.border)
                .rounded(px(4.0))
                .px(px(4.0))
                .child(shortcut_hint()),
        )
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.open_command_palette(window, cx));
        })
}

/// The settings gear: opens the Settings modal (`views/settings.rs`).
fn render_settings_gear(weak: WeakEntity<AppView>, palette: &Palette) -> impl IntoElement {
    div()
        .id("title-bar-settings-gear")
        .size(px(ENV_PILL_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .rounded(px(RADIUS_MD - 2.0))
        .text_color(palette.fg_muted)
        .hover(|style| style.bg(palette.hover))
        .active(|style| style.bg(palette.pressed))
        .child(Icon::new(IconName::Settings).with_size(px(16.0)))
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.open_settings(window, cx));
        })
}

/// `"Ctrl+K"` (`"Cmd+K"` on macOS): the search trigger's key hint.
fn shortcut_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+K"
    } else {
        "Ctrl+K"
    }
}
