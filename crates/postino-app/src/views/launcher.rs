//! The launcher (the design's "Empty States" page, 1a): with a workspace open and no tab, the
//! main area is one panel with the workspace's summary, what to do next (each row with its key)
//! and the requests last opened in this workspace. It shows when Postino starts with the last
//! workspace and every time the last tab closes. The welcome tab is its counterpart without a
//! workspace: the two never show together.
//!
//! The content is `state::launcher`, rebuilt only when the tree or the recent list changes
//! ([`LauncherCache`]), never on every frame.

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{MODIFIER_KEY, ScrollbarOverlay};

use crate::state;
use crate::state::launcher::{LauncherModel, RecentRow};
use crate::state::locale::plural;
use crate::state::navigation::NavSection;
use crate::theme::metrics::{RADIUS_MD, WELCOME_ROW_HEIGHT};
use crate::theme::palette::PostinoColors;
use crate::theme::{Palette, PaletteExt};

use super::components::SectionLabel;
use super::root::AppView;
use super::sidebar;
use super::welcome::help_links;

/// Width of the launcher's column, as in the design.
const COLUMN_WIDTH: f32 = 580.0;
/// Space above the column.
const TOP_PADDING: f32 = 72.0;
/// Height of a Recent row.
const RECENT_ROW_HEIGHT: f32 = 32.0;
/// Width of the method column of a Recent row.
const RECENT_METHOD_WIDTH: f32 = 46.0;

/// The key that shows or hides the Environments panel: `Alt+3`, `Ctrl+3` on macOS, where
/// `Option` with a digit types characters.
#[cfg(target_os = "macos")]
const ENVIRONMENTS_KEY: &str = "Ctrl+3";
#[cfg(not(target_os = "macos"))]
const ENVIRONMENTS_KEY: &str = "Alt+3";

/// The launcher's model, rebuilt when [`Self::invalidate`] was called since, and its scroll.
#[derive(Default)]
pub(crate) struct LauncherCache {
    model: Option<Rc<LauncherModel>>,
    scroll: ScrollHandle,
}

impl LauncherCache {
    /// Forgets the model: the tree or the recent requests changed.
    pub(crate) fn invalidate(&mut self) {
        self.model = None;
    }
}

/// What a click on a Start row does.
type RowHandler = Rc<dyn Fn(&mut AppView, &mut Window, &mut Context<AppView>)>;

impl AppView {
    /// The launcher's model, built from the tree and the recent requests when it was invalidated.
    fn launcher_model(&mut self) -> Rc<LauncherModel> {
        if let Some(model) = &self.launcher.model {
            return model.clone();
        }
        let tree = self
            .state
            .workspace
            .as_ref()
            .map_or(&[][..], |workspace| workspace.tree());
        let model = Rc::new(state::launcher::build(tree, &self.recent_requests));
        log::debug!(
            "launcher rebuilt: {} requests, {} recent",
            model.requests,
            model.recents.len()
        );
        self.launcher.model = Some(model.clone());
        model
    }

    /// Remembers that the request `id` was opened, for the launcher's Recent list.
    pub(crate) fn record_recent_request(&mut self, id: &str) {
        let Some(root) = self.workspace_path.clone() else {
            return;
        };
        let now = chrono::Utc::now().timestamp();
        self.recent_requests = state::config::record_request(&root, id, now);
        self.launcher.invalidate();
    }

    /// Renders the launcher, the main area of a workspace with no tab open.
    pub(crate) fn render_launcher(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.launcher_model();
        let palette = cx.palette();
        let mono = cx.theme().mono_font_family.clone();
        let weak = cx.weak_entity();
        let scroll = self.launcher.scroll.clone();

        let mut column = v_flex()
            .w_full()
            .max_w(px(COLUMN_WIDTH))
            .pt(px(TOP_PADDING))
            .pb(px(32.0))
            .px(px(16.0))
            .gap(px(26.0))
            .child(self.render_launcher_header(&model, &palette))
            .child(render_start(weak.clone(), &palette, &mono));
        if !model.recents.is_empty() {
            column = column.child(render_recents(
                weak.clone(),
                &model.recents,
                &palette,
                &mono,
            ));
        }
        column = column.child(
            h_flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .pt(px(14.0))
                .border_t_1()
                .border_color(palette.raised)
                .text_size(px(12.0))
                .text_color(palette.fg_subtle)
                .child(Icon::new(Lucide::Upload).size(px(13.0)))
                .child(div().flex_1().min_w(px(200.0)).child(t!("launcher.drop")))
                .child(help_links("launcher", weak, &palette, &mono)),
        );

        div()
            .relative()
            .size_full()
            .bg(palette.bg)
            .child(
                div()
                    .id("launcher")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(h_flex().w_full().justify_center().child(column)),
            )
            .child(ScrollbarOverlay::new(&scroll))
            .into_any_element()
    }

    /// The workspace's name and its summary: requests, folders, environment and branch.
    fn render_launcher_header(&self, model: &LauncherModel, palette: &Palette) -> AnyElement {
        let name = self
            .workspace_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let counts = format!(
            "{} {}",
            plural("launcher.requests", model.requests),
            plural("launcher.folders", model.folders)
        );
        let dot = || div().child("\u{b7}");
        let environment: AnyElement = match self.state.active_environment.as_deref() {
            Some(environment) => h_flex()
                .gap(px(4.0))
                .child(t!("launcher.environment"))
                .child(
                    div()
                        .text_color(palette.env_color(self.state.env_colors.color(environment)))
                        .child(environment.to_string()),
                )
                .into_any_element(),
            None => div().child(t!("common.no_environment")).into_any_element(),
        };
        let branch = self.git_branch.clone().map(|branch| {
            h_flex()
                .gap(px(6.0))
                .child(dot())
                .child(t!("launcher.branch", branch = branch))
        });
        h_flex()
            .items_center()
            .gap(px(14.0))
            .child(
                div()
                    .flex_none()
                    .size(px(44.0))
                    .rounded(px(11.0))
                    .bg(palette.accent_subtle)
                    .text_color(palette.accent_text)
                    .text_size(px(22.0))
                    .font_weight(FontWeight::BOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("P"),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(3.0))
                    .child(
                        div()
                            .text_size(px(20.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(name),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap(px(6.0))
                            .text_color(palette.fg_subtle)
                            .child(counts)
                            .child(dot())
                            .child(environment)
                            .children(branch),
                    ),
            )
            .into_any_element()
    }
}

/// A clickable row with the hover and pressed backgrounds.
fn row(id: impl Into<ElementId>, height: f32, palette: &Palette) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(height))
        .items_center()
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover))
        .active(|style| style.bg(palette.pressed))
}

/// The Start list: what to do next, each with its key.
fn render_start(weak: WeakEntity<AppView>, palette: &Palette, mono: &SharedString) -> AnyElement {
    let entries: [(&str, Lucide, String, String, RowHandler); 5] = [
        (
            "launcher-new-request",
            Lucide::FilePlus,
            t!("common.new_request").into_owned(),
            format!("{MODIFIER_KEY}+N"),
            Rc::new(|view, window, cx| view.new_request(window, cx)),
        ),
        (
            "launcher-search",
            Lucide::Search,
            t!("launcher.search").into_owned(),
            format!("{MODIFIER_KEY}+K"),
            Rc::new(|view, window, cx| view.open_command_palette(window, cx)),
        ),
        (
            "launcher-import",
            Lucide::Download,
            t!("launcher.import").into_owned(),
            format!("{MODIFIER_KEY}+Shift+O"),
            Rc::new(|_, window, cx| {
                super::openapi_import::open_openapi_wizard(
                    cx.weak_entity(),
                    state::openapi_import::SourceKind::File,
                    window,
                    cx,
                );
            }),
        ),
        (
            "launcher-environments",
            Lucide::Variable,
            t!("navigation.env.title").into_owned(),
            ENVIRONMENTS_KEY.to_string(),
            Rc::new(|view, _, cx| {
                if !view.nav.is_active(NavSection::Environments) {
                    view.click_rail(NavSection::Environments, cx);
                }
            }),
        ),
        (
            "launcher-open-workspace",
            Lucide::FolderOpen,
            t!("launcher.open_workspace").into_owned(),
            format!("{MODIFIER_KEY}+O"),
            Rc::new(|_, window, cx| sidebar::pick_workspace_folder(cx.weak_entity(), window, cx)),
        ),
    ];
    v_flex()
        .gap(px(2.0))
        .child(
            div()
                .px(px(10.0))
                .pb(px(6.0))
                .child(SectionLabel::new(t!("welcome.start"))),
        )
        .children(entries.into_iter().map(|(id, icon, label, key, on_click)| {
            let weak = weak.clone();
            row(id, WELCOME_ROW_HEIGHT, palette)
                .gap(px(12.0))
                .child(
                    Icon::new(icon)
                        .size(px(15.0))
                        .text_color(palette.accent_text),
                )
                .child(div().flex_1().min_w_0().truncate().child(label))
                .child(
                    div()
                        .flex_none()
                        .font_family(mono.clone())
                        .text_size(px(11.0))
                        .text_color(palette.fg_muted)
                        .px(px(6.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(palette.border_strong)
                        .child(key),
                )
                .on_click(move |_, window, cx| {
                    let on_click = on_click.clone();
                    let _ = weak.update(cx, |view, cx| on_click(view, window, cx));
                })
        }))
        .into_any_element()
}

/// The Recent list: the requests last opened in this workspace, with when.
fn render_recents(
    weak: WeakEntity<AppView>,
    recents: &[RecentRow],
    palette: &Palette,
    mono: &SharedString,
) -> AnyElement {
    let now = chrono::Utc::now().timestamp();
    v_flex()
        .gap(px(2.0))
        .child(
            div()
                .px(px(10.0))
                .pb(px(6.0))
                .child(SectionLabel::new(t!("welcome.recent"))),
        )
        .children(recents.iter().enumerate().map(|(index, recent)| {
            let weak = weak.clone();
            let id = recent.id.clone();
            let (method, method_color) = match &recent.method {
                Some(method) => (method.to_string(), palette.method_color(method)),
                None => (String::new(), palette.fg_subtle),
            };
            row(("launcher-recent", index), RECENT_ROW_HEIGHT, palette)
                .gap(px(10.0))
                .child(
                    div()
                        .flex_none()
                        .w(px(RECENT_METHOD_WIDTH))
                        .font_family(mono.clone())
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(method_color)
                        .child(method),
                )
                .child(div().flex_none().child(recent.name.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(mono.clone())
                        .text_size(px(11.5))
                        .text_color(palette.fg_subtle)
                        .child(recent.folder.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(state::format::relative_time(recent.opened, now)),
                )
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    let _ = weak.update(cx, |view, cx| view.open_request(id, cx));
                })
        }))
        .into_any_element()
}
