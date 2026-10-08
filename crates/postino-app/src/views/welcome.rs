//! The welcome tab (GitHub #90): what Postino is, how it stores requests, how to start (New
//! workspace, Open folder, Import from Postman), the recent workspaces, links to the quick
//! start, the shortcuts and the command palette, and the "show at startup" checkbox.
//!
//! The recent workspaces come from the tab's own [`WelcomeTab`], read when it opens, so render
//! touches no file. The layout follows the "Welcome tab" section of `docs/design-system.md`.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::state;
use crate::state::shortcuts::MODIFIER_KEY;
use crate::state::welcome::{QUICK_START_URL, RecentWorkspace, WelcomeTab};
use crate::theme::metrics::{
    RADIUS_LG, RADIUS_MD, RADIUS_XS, WELCOME_CHECKBOX_SIZE, WELCOME_COLUMN_WIDTH,
    WELCOME_COLUMN_WIDTH_NARROW, WELCOME_LOGO_RADIUS, WELCOME_LOGO_SIZE, WELCOME_RECENT_NAME_WIDTH,
    WELCOME_ROW_HEIGHT, WELCOME_TOP_PADDING, WELCOME_TOP_PADDING_NARROW,
};
use crate::theme::{Palette, PaletteExt};

use super::components::{PrimaryButton, SecondaryButton, SectionLabel};
use super::import_menu::{self, ImportKind};
use super::new_workspace::{self, NewWorkspacePurpose};
use super::root::AppView;
use super::sidebar;

/// A click handler of a Start row, a link or the checkbox.
type ClickHandler = Box<dyn Fn(&mut Window, &mut App)>;

impl AppView {
    /// Opens the welcome tab, or brings it to the front with its recent workspaces read again.
    /// Also the command palette's "Show welcome" action.
    pub(crate) fn open_welcome_tab(&mut self, cx: &mut Context<Self>) {
        let welcome = WelcomeTab::load(&self.recent_workspaces, dirs::home_dir().as_deref());
        self.state.tabs.open_welcome(welcome);
        log::debug!("opened the welcome tab");
        cx.notify();
    }

    /// Renders the active welcome tab.
    pub(crate) fn render_welcome(&self, welcome: &WelcomeTab, cx: &Context<Self>) -> AnyElement {
        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();
        let weak = cx.weak_entity();
        // Next to the sidebar (shown while a workspace is open or being opened, see
        // `render_body`) the column is a little narrower and starts a little higher.
        let (column_width, top_padding) =
            if self.state.workspace.is_some() || self.opening.is_some() {
                (WELCOME_COLUMN_WIDTH_NARROW, WELCOME_TOP_PADDING_NARROW)
            } else {
                (WELCOME_COLUMN_WIDTH, WELCOME_TOP_PADDING)
            };

        let column = v_flex()
            .w_full()
            .max_w(px(column_width))
            // The design's line height; gpui's default is taller and spreads the blocks apart.
            .line_height(relative(1.25))
            .pt(px(top_padding))
            .px(px(40.0))
            .pb(px(32.0))
            .child(render_header(&palette))
            .child(render_files_card(&palette, &mono_font))
            .child(
                h_flex()
                    .mt(px(36.0))
                    .items_start()
                    .gap(px(40.0))
                    .child(div().flex_1().min_w_0().child(render_start(
                        weak.clone(),
                        &palette,
                        &mono_font,
                    )))
                    .child(
                        // 1.3 times the Start column, as in the design.
                        div()
                            .flex_1()
                            .min_w_0()
                            .map(|mut this| {
                                this.style().flex_grow = Some(1.3);
                                this
                            })
                            .children((!welcome.recents.is_empty()).then(|| {
                                render_recents(weak.clone(), &welcome.recents, &palette, &mono_font)
                            })),
                    ),
            )
            .child(div().flex_1().min_h(px(24.0)))
            .child(render_footer(
                weak,
                self.state.settings.show_welcome,
                &palette,
                &mono_font,
            ));

        div()
            .id("welcome-tab")
            .size_full()
            .bg(palette.bg)
            .overflow_y_scroll()
            // Stretched, not centered: the column starts at its top padding and its footer
            // sits at the bottom of the tab.
            .child(
                h_flex()
                    .w_full()
                    .min_h_full()
                    .items_stretch()
                    .justify_center()
                    .child(column),
            )
            .into_any_element()
    }

    /// The main area with no workspace and no tab open, once the welcome tab is closed: a line
    /// and the two ways to get a workspace.
    pub(crate) fn render_no_workspace(
        &self,
        weak: WeakEntity<Self>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let new_weak = weak.clone();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(16.0))
            .bg(palette.bg)
            .child(
                div()
                    .text_color(palette.fg_muted)
                    .child(t!("welcome.no_workspace")),
            )
            .child(
                h_flex()
                    .gap(px(8.0))
                    .child(
                        PrimaryButton::new("empty-new-workspace", t!("common.new_workspace"))
                            .icon(Icon::new(Lucide::Plus))
                            .on_click(move |_, window, cx| {
                                new_workspace::open_new_workspace_dialog(
                                    new_weak.clone(),
                                    NewWorkspacePurpose::Example,
                                    None,
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .child(
                        SecondaryButton::new("empty-open-folder", t!("common.open_folder"))
                            .icon(Icon::new(Lucide::FolderOpen))
                            .on_click(move |_, window, cx| {
                                sidebar::pick_workspace_folder(weak.clone(), window, cx);
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// The logo, "Postino" and the one line on what it is.
fn render_header(palette: &Palette) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap(px(16.0))
        .child(
            div()
                .flex_none()
                .size(px(WELCOME_LOGO_SIZE))
                .rounded(px(WELCOME_LOGO_RADIUS))
                .bg(palette.accent)
                .text_color(palette.accent_fg)
                .text_size(px(24.0))
                .font_weight(FontWeight::BOLD)
                .flex()
                .items_center()
                .justify_center()
                .child("P"),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(26.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.fg)
                        .child("Postino"),
                )
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(palette.fg_muted)
                        .child(t!("welcome.tagline")),
                ),
        )
}

/// The card on how Postino stores things: folder = workspace, subfolder = collection,
/// `.postino` file = request.
fn render_files_card(palette: &Palette, mono_font: &SharedString) -> impl IntoElement {
    let item = |icon: Lucide, word: SharedString, word_color: Hsla, meaning: SharedString| {
        h_flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .child(
                Icon::new(icon)
                    .with_size(px(13.0))
                    .text_color(palette.fg_subtle),
            )
            .child(
                div()
                    .font_family(mono_font.clone())
                    .text_color(word_color)
                    .child(word),
            )
            .child(format!("= {meaning}"))
    };
    let dot = || separator_dot(palette);

    v_flex()
        .mt(px(28.0))
        .gap(px(12.0))
        .py(px(14.0))
        .px(px(16.0))
        .rounded(px(RADIUS_LG))
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .child(
            div()
                .text_color(palette.fg)
                .child(t!("welcome.files_intro")),
        )
        .child(
            h_flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .text_size(px(12.5))
                .text_color(palette.fg_muted)
                .child(item(
                    Lucide::Folder,
                    t!("welcome.folder").into(),
                    palette.fg,
                    t!("welcome.workspace").into(),
                ))
                .child(dot())
                .child(item(
                    Lucide::FolderTree,
                    t!("welcome.subfolder").into(),
                    palette.fg,
                    t!("welcome.collection").into(),
                ))
                .child(dot())
                .child(item(
                    Lucide::FileText,
                    ".postino".into(),
                    palette.accent_text,
                    t!("welcome.request").into(),
                )),
        )
}

/// The faint `·` between the items of the files card and between the footer links.
fn separator_dot(palette: &Palette) -> impl IntoElement {
    div()
        .text_color(palette.fg_subtle.opacity(0.6))
        .child("\u{b7}")
}

/// The "Start" column: New workspace, Open folder, Import from Postman.
fn render_start(
    weak: WeakEntity<AppView>,
    palette: &Palette,
    mono_font: &SharedString,
) -> impl IntoElement {
    let new_weak = weak.clone();
    let open_weak = weak.clone();
    let rows: [(&str, Lucide, SharedString, Option<String>, ClickHandler); 3] = [
        (
            "welcome-new-workspace",
            Lucide::Plus,
            t!("common.new_workspace").into(),
            None,
            Box::new(move |window, cx| {
                new_workspace::open_new_workspace_dialog(
                    new_weak.clone(),
                    NewWorkspacePurpose::Example,
                    None,
                    window,
                    cx,
                );
            }),
        ),
        (
            "welcome-open-folder",
            Lucide::FolderOpen,
            t!("common.open_folder").into(),
            Some(format!("{MODIFIER_KEY}+O")),
            Box::new(move |window, cx| {
                sidebar::pick_workspace_folder(open_weak.clone(), window, cx);
            }),
        ),
        (
            "welcome-import",
            Lucide::Download,
            t!("welcome.import_postman").into(),
            None,
            Box::new(move |window, cx| {
                import_menu::pick_and_import(weak.clone(), ImportKind::Collection, window, cx);
            }),
        ),
    ];

    v_flex()
        .gap(px(6.0))
        .child(
            div()
                .px(px(10.0))
                .pb(px(6.0))
                .child(SectionLabel::new(t!("welcome.start"))),
        )
        .children(rows.into_iter().map(|(id, icon, label, keys, on_click)| {
            list_row(id, palette)
                .gap(px(10.0))
                .text_color(palette.fg)
                .child(
                    Icon::new(icon)
                        .with_size(px(15.0))
                        .text_color(palette.accent_text),
                )
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(keys.map(|keys| {
                    div()
                        .flex_none()
                        .font_family(mono_font.clone())
                        .text_size(px(11.0))
                        .text_color(palette.fg_subtle)
                        .child(keys)
                }))
                .on_click(move |_, window, cx| on_click(window, cx))
        }))
}

/// The "Recent" column: one row per recent workspace, name and shortened path. A click opens it.
fn render_recents(
    weak: WeakEntity<AppView>,
    recents: &[RecentWorkspace],
    palette: &Palette,
    mono_font: &SharedString,
) -> impl IntoElement {
    v_flex()
        .gap(px(6.0))
        .child(
            div()
                .px(px(10.0))
                .pb(px(6.0))
                .child(SectionLabel::new(t!("welcome.recent"))),
        )
        .children(recents.iter().enumerate().map(|(index, recent)| {
            let weak = weak.clone();
            let path = recent.path.clone();
            list_row(("welcome-recent", index), palette)
                .gap(px(12.0))
                .child(
                    div()
                        .flex_none()
                        .w(px(WELCOME_RECENT_NAME_WIDTH))
                        .truncate()
                        .text_color(palette.fg)
                        .child(recent.name.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(mono_font.clone())
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(recent.path_label.clone()),
                )
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.open_workspace_at(&path, window, cx));
                })
        }))
}

/// A clickable row of the Start and Recent lists, with the hover and pressed backgrounds.
fn list_row(id: impl Into<ElementId>, palette: &Palette) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(WELCOME_ROW_HEIGHT))
        .items_center()
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover))
        .active(|style| style.bg(palette.pressed))
}

/// The links (quick start, shortcuts, command palette) and the "show at startup" checkbox.
fn render_footer(
    weak: WeakEntity<AppView>,
    show_welcome: bool,
    palette: &Palette,
    mono_font: &SharedString,
) -> impl IntoElement {
    let shortcuts_weak = weak.clone();
    let search_weak = weak.clone();
    let link =
        |id: &'static str, label: SharedString, keys: Option<String>, on_click: ClickHandler| {
            let hover_color = palette.fg;
            h_flex()
                .id(id)
                .gap(px(4.0))
                .cursor_pointer()
                .text_color(palette.accent_text)
                .hover(move |style| style.text_color(hover_color))
                .child(label)
                .children(keys.map(|keys| {
                    div()
                        .font_family(mono_font.clone())
                        .text_size(px(11.0))
                        .child(format!("({keys})"))
                }))
                .on_click(move |_, window, cx| on_click(window, cx))
        };
    let dot = || separator_dot(palette);

    let checkbox_weak = weak;
    let (box_bg, box_border) = if show_welcome {
        (palette.accent, palette.accent)
    } else {
        (palette.accent.opacity(0.0), palette.border_strong)
    };

    h_flex()
        .flex_wrap()
        .items_center()
        .gap(px(16.0))
        .pt(px(16.0))
        .border_t_1()
        .border_color(palette.border)
        .text_size(px(12.5))
        .child(
            h_flex()
                .items_center()
                .gap(px(10.0))
                .child(link(
                    "welcome-quick-start",
                    t!("welcome.quick_start").into(),
                    None,
                    Box::new(|_, cx| cx.open_url(QUICK_START_URL)),
                ))
                .child(dot())
                .child(link(
                    "welcome-shortcuts",
                    t!("welcome.shortcuts").into(),
                    Some(state::shortcuts::open_hint()),
                    Box::new(move |window, cx| {
                        let _ =
                            shortcuts_weak.update(cx, |view, cx| view.open_shortcuts(window, cx));
                    }),
                ))
                .child(dot())
                .child(link(
                    "welcome-search",
                    t!("welcome.search").into(),
                    Some(format!("{MODIFIER_KEY}+K")),
                    Box::new(move |window, cx| {
                        let _ = search_weak
                            .update(cx, |view, cx| view.open_command_palette(window, cx));
                    }),
                )),
        )
        .child(div().flex_1())
        .child(
            h_flex()
                .id("welcome-show-on-start")
                .items_center()
                .gap(px(8.0))
                .cursor_pointer()
                .text_color(palette.fg_muted)
                .child(
                    div()
                        .flex_none()
                        .size(px(WELCOME_CHECKBOX_SIZE))
                        .rounded(px(RADIUS_XS))
                        .border_1()
                        .border_color(box_border)
                        .bg(box_bg)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(palette.accent_fg)
                        .children(
                            show_welcome.then(|| Icon::new(Lucide::Check).with_size(px(10.0))),
                        ),
                )
                .child(t!("welcome.show_on_start"))
                .on_click(move |_, window, cx| {
                    let _ = checkbox_weak.update(cx, |view, cx| {
                        view.set_show_welcome(!show_welcome, window, cx)
                    });
                }),
        )
}
