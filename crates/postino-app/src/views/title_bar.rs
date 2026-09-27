//! The title bar (`plans/ui-redesign.md` section 2.3 point 1): the "P" logo, "Postino", the
//! workspace switcher, the search trigger, the environment pill, the Import menu, and the
//! settings gear. The window controls at the far right are drawn automatically by gpui-kit's
//! `TitleBar` (`plans/ui-redesign-spikes.md` section 9), so nothing here adds them. The search
//! trigger and the settings gear are inert until phases 7 and 6 build what they open.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::state;
use crate::theme::Palette;
use crate::theme::PaletteExt;
use crate::theme::metrics::{
    ENV_PILL_HEIGHT, MENU_DOT_COLUMN_WIDTH, MENU_ROW_HEIGHT, RADIUS_MD, SEARCH_TRIGGER_WIDTH,
};

use super::root::AppView;
use super::sidebar;

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

        TitleBar::new()
            .child(
                h_flex()
                    .size_full()
                    .items_center()
                    .gap_2()
                    .child(render_logo(&palette))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Postino"))
                    .child(div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).bg(palette.border))
                    .child(self.render_workspace_switcher(weak.clone(), &palette))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_center()
                            .child(render_search_trigger(&palette, mono_font)),
                    )
                    .child(self.render_env_picker(weak.clone(), cx))
                    .child(self.render_import_menu(cx))
                    .child(render_settings_gear(weak, &palette)),
            )
            .into_any_element()
    }

    /// Renders the workspace switcher: the folder icon, the open workspace's folder name (or a
    /// placeholder), a chevron, and its dropdown menu (recent workspaces, then "Open
    /// folder...").
    fn render_workspace_switcher(&self, weak: WeakEntity<Self>, palette: &Palette) -> AnyElement {
        let current_root = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().to_path_buf());
        let current_canonical = current_root
            .as_ref()
            .and_then(|root| root.canonicalize().ok());
        let label = current_root
            .as_ref()
            .and_then(|root| root.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Open a folder".to_string());
        let recent = state::config::recent_workspaces();
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
                    let shortened = state::format::shorten_path(path, home.as_deref());
                    let is_current = current_canonical.as_deref() == Some(path.as_path());
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
                            .px_2()
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
                let open_weak = weak.clone();
                menu = menu.item(PopupMenuItem::new("Open folder...").on_click(
                    move |_, window, cx| {
                        sidebar::pick_workspace_folder(open_weak.clone(), window, cx);
                    },
                ));
                menu
            })
            .into_any_element()
    }
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

/// The centered search trigger. Inert until phase 7 builds the command palette.
fn render_search_trigger(palette: &Palette, mono_font: SharedString) -> impl IntoElement {
    // TODO(phase 7): open the command palette.
    h_flex()
        .w(px(SEARCH_TRIGGER_WIDTH))
        .h(px(ENV_PILL_HEIGHT))
        .items_center()
        .gap_2()
        .px_2p5()
        .rounded(px(RADIUS_MD - 1.0))
        .bg(palette.raised)
        .border_1()
        .border_color(palette.border)
        .text_color(palette.fg_subtle)
        .child(Icon::new(IconName::Search).small())
        .child(div().flex_1().child("Search requests and actions"))
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
}

/// The settings gear: opens the Settings modal (`views/settings.rs`, `plans/ui-redesign.md`
/// phase 6).
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
        .child(Icon::new(IconName::Settings).small())
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.open_settings(window, cx));
        })
}

/// `"Ctrl K"` (`"Cmd K"` on macOS): the search trigger's key hint.
fn shortcut_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd K"
    } else {
        "Ctrl K"
    }
}
