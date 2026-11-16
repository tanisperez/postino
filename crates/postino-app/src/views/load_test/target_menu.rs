//! The load test target picker: telaio's [`SearchableMenu`] under the "Choose a request" box of
//! the config panel, the design's "Pulido 1.0" page. One menu lists the requests grouped by
//! folder, each with its method in its color; the other lists the folders with their number of
//! requests. Both have a search box, a height cap with a scrollbar and the keyboard hints.
//!
//! The menus' rows are built when the workspace tree changes or the theme switches between light
//! and dark (the method colors), checked with a cheap key on render, never rebuilt on a plain
//! render.

use std::collections::HashMap;

use gpui_kit::Anchor;
use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::Method;
use telaio_ui::components::{MenuGroup, MenuRow, SearchableMenuEvent, SearchableMenuState};

use crate::state::load_test::{LoadTestTarget, TargetKind, folders_flat, request_groups};
use crate::state::number::format_integer;
use crate::theme::{PaletteExt, PostinoColors};
use crate::views::root::AppView;

/// Where the menus open: under the box, aligned to its left edge.
pub(crate) const TARGET_MENU_ANCHOR: Anchor = Anchor::TopLeft;

/// The two menus of the target picker and what their rows were built from.
pub(crate) struct LoadTargetMenus {
    /// The "Request" menu.
    pub(crate) requests: Entity<SearchableMenuState>,
    /// The "Collection" menu.
    pub(crate) folders: Entity<SearchableMenuState>,
    /// Bumped when the workspace tree changes ([`Self::invalidate`]).
    generation: u64,
    /// The generation and the dark flag the rows were built for.
    built: Option<(u64, bool)>,
    /// Each request's method, for the box of the picked request.
    methods: HashMap<String, Method>,
    /// Each folder's name and number of requests, for the pick.
    folder_info: HashMap<String, (String, usize)>,
}

impl LoadTargetMenus {
    /// Both menus, empty until the first [`AppView::sync_load_target_menus`].
    pub(crate) fn new(window: &mut Window, cx: &mut Context<AppView>) -> Self {
        let requests = new_menu(
            TargetKind::Request,
            t!("load_test.config.search_requests"),
            window,
            cx,
        );
        let folders = new_menu(
            TargetKind::Collection,
            t!("load_test.config.search_folders"),
            window,
            cx,
        );
        Self {
            requests,
            folders,
            generation: 0,
            built: None,
            methods: HashMap::new(),
            folder_info: HashMap::new(),
        }
    }

    /// Marks the rows as stale: the workspace tree changed.
    pub(crate) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// The method of the request `id`, if it is in the tree.
    pub(crate) fn method(&self, id: &str) -> Option<&Method> {
        self.methods.get(id)
    }

    /// The menu of `kind`.
    pub(crate) fn menu(&self, kind: TargetKind) -> &Entity<SearchableMenuState> {
        match kind {
            TargetKind::Request => &self.requests,
            TargetKind::Collection => &self.folders,
        }
    }
}

/// A menu with its placeholder, hints and empty message, reporting to the view.
fn new_menu(
    kind: TargetKind,
    placeholder: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> Entity<SearchableMenuState> {
    let placeholder = placeholder.into();
    let menu = cx.new(|cx| {
        SearchableMenuState::new(window, cx)
            .placeholder(placeholder)
            .empty_message(t!("load_test.config.no_match"))
            .hints([
                (
                    "↑↓".to_string(),
                    t!("shell.palette.hint_navigate").into_owned(),
                ),
                (
                    "↵".to_string(),
                    t!("load_test.config.hint_pick").into_owned(),
                ),
                (
                    "Esc".to_string(),
                    t!("shell.palette.hint_close").into_owned(),
                ),
            ])
    });
    cx.subscribe(&menu, move |view, _, event: &SearchableMenuEvent, cx| {
        if let SearchableMenuEvent::Activate(id) = event {
            view.pick_from_target_menu(kind, id, cx);
        }
    })
    .detach();
    menu
}

impl AppView {
    /// Rebuilds the menus' rows when the tree or the light/dark mode changed since they were
    /// built. Cheap otherwise: one comparison.
    pub(crate) fn sync_load_target_menus(&mut self, cx: &mut Context<Self>) {
        let dark = cx.theme().is_dark();
        let key = (self.load_target_menus.generation, dark);
        if self.load_target_menus.built == Some(key) {
            return;
        }
        self.load_target_menus.built = Some(key);
        let nodes = self
            .state
            .workspace
            .as_ref()
            .map_or(&[][..], |workspace| workspace.tree());
        let palette = cx.palette();

        let mut methods = HashMap::new();
        let request_groups: Vec<MenuGroup> = request_groups(nodes)
            .into_iter()
            .map(|group| {
                let rows: Vec<MenuRow> = group
                    .requests
                    .into_iter()
                    .map(|request| {
                        let row = MenuRow::new(request.id.clone(), request.name, IconName::File)
                            .lead(
                                request.method.to_string(),
                                palette.method_color(&request.method),
                            )
                            .search_text(request.id.clone());
                        methods.insert(request.id, request.method);
                        row
                    })
                    .collect();
                let label = if group.folder.is_empty() {
                    "/".to_string()
                } else {
                    group.folder
                };
                MenuGroup::new(label, rows)
            })
            .collect();

        let mut folder_info = HashMap::new();
        let folder_rows: Vec<MenuRow> = folders_flat(nodes)
            .into_iter()
            .map(|(id, name, count)| {
                let row = MenuRow::new(id.clone(), id.clone(), IconName::Folder)
                    .badge(format_integer(count as u64));
                folder_info.insert(id, (name, count));
                row
            })
            .collect();
        let folder_groups = vec![MenuGroup::new(
            t!("load_test.config.folders").into_owned(),
            folder_rows,
        )];

        self.load_target_menus.methods = methods;
        self.load_target_menus.folder_info = folder_info;
        self.load_target_menus
            .requests
            .update(cx, |menu, cx| menu.set_groups(request_groups, cx));
        self.load_target_menus
            .folders
            .update(cx, |menu, cx| menu.set_groups(folder_groups, cx));
    }

    /// A row of a target menu was chosen: it becomes the active load test tab's target.
    fn pick_from_target_menu(&mut self, kind: TargetKind, id: &str, cx: &mut Context<Self>) {
        let Some(tab_id) = self
            .state
            .tabs
            .active()
            .filter(|tab| tab.load_test().is_some())
            .map(|tab| tab.id.clone())
        else {
            return;
        };
        match kind {
            TargetKind::Request => {
                let label = id.strip_suffix(".postino").unwrap_or(id).to_string();
                self.pick_load_test_target(
                    &tab_id,
                    LoadTestTarget::Request(id.to_string()),
                    label,
                    1,
                    cx,
                );
            }
            TargetKind::Collection => {
                let Some((name, count)) = self.load_target_menus.folder_info.get(id).cloned()
                else {
                    return;
                };
                self.pick_load_test_target(
                    &tab_id,
                    LoadTestTarget::Collection(id.to_string()),
                    name,
                    count,
                    cx,
                );
            }
        }
    }
}
