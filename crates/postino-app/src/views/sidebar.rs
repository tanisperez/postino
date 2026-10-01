//! The sidebar: a header with "new request" / "new folder" icon buttons, a filter input, the
//! workspace's collection tree, and a footer with the workspace path and git branch
//! (`plans/ui-redesign.md` section 2.3 point 2). Right-clicking a tree row still opens the
//! rename/delete/new request/new folder context menu (`plans/mvp.md`, phase 8).

use std::collections::{HashMap, HashSet};

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::tree::{TreeEntry, TreeItem, TreeState, tree as tree_view};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::Method;
use postino_workspace::{Node, RequestEntry, git_branch};

use crate::state;
use crate::theme::PaletteExt;
use crate::theme::metrics::{
    METHOD_LABEL_WIDTH, RADIUS_MD, SIDEBAR_FILTER_HEIGHT, TREE_ROW_HEIGHT,
};
use crate::views::components::{IconButton, MethodBadge, SectionLabel};

use super::root::AppView;

/// The extension every request file uses (`plans/mvp.md`, section 3.2), used here only to tell a
/// request row from a folder row when rendering.
const REQUEST_EXTENSION: &str = ".postino";

impl AppView {
    /// Renders the sidebar: header, filter input, collection tree, and footer.
    pub(crate) fn render_sidebar(
        &mut self,
        weak: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_workspace = self.state.workspace.is_some();
        let tree_state = self.tree_state.clone();
        let palette = cx.palette();
        let mut methods_by_id = HashMap::new();
        if let Some(workspace) = self.state.workspace.as_ref() {
            collect_methods(workspace.tree(), &mut methods_by_id);
        }

        v_flex()
            .size_full()
            .bg(palette.surface)
            .border_r_1()
            .border_color(palette.border)
            .child(render_header(weak.clone(), has_workspace))
            .child(self.render_filter_row(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(px(6.0))
                    .when(has_workspace, |this| {
                        let row_palette = palette.clone();
                        this.child(
                            tree_view(&tree_state, move |_ix, entry, selected, _window, _cx| {
                                render_tree_row(
                                    entry,
                                    selected,
                                    methods_by_id.get(entry.item().id.as_ref()),
                                    &row_palette,
                                )
                            })
                            .context_menu(move |_ix, entry, menu, window, cx| {
                                build_context_menu(weak.clone(), entry, menu, window, cx)
                            })
                            .size_full(),
                        )
                    })
                    .when(!has_workspace, |this| {
                        this.child(
                            div()
                                .p_4()
                                .text_sm()
                                .text_color(palette.fg_subtle)
                                .child(t!("shell.sidebar.empty")),
                        )
                    }),
            )
            .child(self.render_footer(&palette))
            .into_any_element()
    }

    /// Re-applies the filter input's translated placeholder after a language change
    /// (`AppView::relocalize`).
    pub(crate) fn relocalize_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_filter_input.update(cx, |state, cx| {
            state.set_placeholder(t!("shell.sidebar.filter"), window, cx);
        });
    }

    /// Renders the filter row: a `list-filter` icon and a borderless [`Input`] inside a bordered
    /// `raised` pill, backed by [`AppView::sidebar_filter_input`]. Narrowing the tree as the user
    /// types happens in [`AppView::on_sidebar_filter_changed`], subscribed once in
    /// [`AppView::new`].
    fn render_filter_row(&self, cx: &Context<Self>) -> AnyElement {
        let palette = cx.palette();

        h_flex()
            .px(px(10.0))
            .pb(px(8.0))
            .child(
                h_flex()
                    .flex_1()
                    .h(px(SIDEBAR_FILTER_HEIGHT))
                    .items_center()
                    .gap_2()
                    .px(px(9.0))
                    .rounded(px(RADIUS_MD - 1.0))
                    .bg(palette.raised)
                    .border_1()
                    .border_color(palette.border)
                    .text_color(palette.fg_subtle)
                    .child(Icon::new(gpui_kit::assets::IconName::ListFilter).small())
                    .child(
                        Input::new(&self.sidebar_filter_input)
                            .h(px(SIDEBAR_FILTER_HEIGHT))
                            .bordered(false)
                            // Transparent so only the outer pill shows: without this, the
                            // `Input`'s own background paints a second, visible box inside it
                            // (`plans/ui-redesign.md` phase 5, reviewer fix item 9).
                            .bg(palette.bg.opacity(0.0)),
                    ),
            )
            .into_any_element()
    }

    /// Renders the footer: the `hard-drive` icon, the workspace path shortened with `~`, and,
    /// when the workspace is inside a git repository, the `git-branch` icon and branch name.
    fn render_footer(&self, palette: &crate::theme::Palette) -> AnyElement {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return div().into_any_element();
        };
        let path_label = state::format::shorten_path(workspace.root(), dirs::home_dir().as_deref());
        let branch = git_branch(workspace.root());

        h_flex()
            .w_full()
            .overflow_hidden()
            .border_t_1()
            .border_color(palette.border)
            .px(px(14.0))
            .py(px(8.0))
            .items_center()
            .gap_2()
            .text_size(px(12.0))
            .text_color(palette.fg_subtle)
            .child(Icon::new(IconName::HardDrive).small())
            // `min_w_0` plus `truncate` lets a long path shrink with an ellipsis instead of
            // pushing the branch name out of the sidebar.
            .child(div().flex_1().min_w_0().truncate().child(path_label))
            .children(branch.map(|branch| {
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.0))
                    .child(Icon::new(gpui_kit::assets::IconName::GitBranch).small())
                    .child(branch)
            }))
            .into_any_element()
    }
}

/// Renders the sidebar's header row: the "Collections" section label, and the "new request" /
/// "new folder" icon buttons.
fn render_header(weak: WeakEntity<AppView>, has_workspace: bool) -> impl IntoElement {
    h_flex()
        .justify_between()
        .items_center()
        .pt(px(10.0))
        .pr(px(10.0))
        .pb(px(6.0))
        .pl(px(14.0))
        .child(SectionLabel::new(t!("shell.sidebar.collections")))
        .child(
            h_flex()
                .gap(px(2.0))
                .child(
                    IconButton::new("new-request-root", IconName::Plus)
                        .tooltip(t!("common.new_request"))
                        .disabled(!has_workspace)
                        .on_click({
                            let weak = weak.clone();
                            move |_, window, cx| {
                                open_new_request_dialog(weak.clone(), None, window, cx);
                            }
                        }),
                )
                .child(
                    IconButton::new("new-folder-root", gpui_kit::assets::IconName::FolderPlus)
                        .tooltip(t!("common.new_folder"))
                        .disabled(!has_workspace)
                        .on_click({
                            let weak = weak.clone();
                            move |_, window, cx| {
                                open_new_folder_dialog(weak.clone(), None, window, cx);
                            }
                        }),
                ),
        )
}

/// Turns the workspace's collection tree into the `TreeItem`s the sidebar's `Tree` renders,
/// unfiltered. `expansion`, when given, restores each folder's expand flag from a snapshot taken
/// before the sidebar filter went from empty to non-empty (`AppView::on_sidebar_filter_changed`);
/// `None` expands every folder, matching the workspace's normal, filter-free behavior.
pub(crate) fn build_tree_items(
    nodes: &[Node],
    expansion: Option<&HashMap<String, bool>>,
) -> Vec<TreeItem> {
    nodes
        .iter()
        .map(|node| build_tree_item(node, expansion))
        .collect()
}

fn build_tree_item(node: &Node, expansion: Option<&HashMap<String, bool>>) -> TreeItem {
    match node {
        Node::Folder(folder) => {
            let expanded = expansion
                .and_then(|map| map.get(&folder.id))
                .copied()
                .unwrap_or(true);
            TreeItem::new(folder.id.clone(), folder.name.clone())
                .children(build_tree_items(&folder.children, expansion))
                .expanded(expanded)
        }
        Node::Request(request) => TreeItem::new(request.id.clone(), request_label(request)),
    }
}

/// Turns the workspace's collection tree into the `TreeItem`s shown while the sidebar filter is
/// non-empty: only the ids `state::sidebar_filter::visible_ids` returns, every included folder
/// forced expanded so a match is never hidden (`plans/ui-redesign.md` section 2.3 point 2).
pub(crate) fn build_filtered_tree_items(nodes: &[Node], query: &str) -> Vec<TreeItem> {
    let visible = state::sidebar_filter::visible_ids(nodes, query);
    build_filtered_items(nodes, &visible)
}

fn build_filtered_items(nodes: &[Node], visible: &HashSet<String>) -> Vec<TreeItem> {
    nodes
        .iter()
        .filter(|node| visible.contains(node.id()))
        .map(|node| build_filtered_item(node, visible))
        .collect()
}

fn build_filtered_item(node: &Node, visible: &HashSet<String>) -> TreeItem {
    match node {
        Node::Folder(folder) => TreeItem::new(folder.id.clone(), folder.name.clone())
            .children(build_filtered_items(&folder.children, visible))
            .expanded(true),
        Node::Request(request) => TreeItem::new(request.id.clone(), request_label(request)),
    }
}

/// The label shown for a request row: its name, with a "(broken)" suffix when the file failed to
/// parse.
fn request_label(request: &RequestEntry) -> String {
    match &request.broken {
        Some(_) => t!("shell.sidebar.broken", name = request.name.as_str()).into_owned(),
        None => request.name.clone(),
    }
}

/// Collects every request's method by id, for [`render_tree_row`] to look up: `TreeItem` (built by
/// [`build_tree_item`]/[`build_filtered_item`]) has no field for it, so the tree row renderer
/// looks it up from this side map instead (`plans/ui-redesign.md` section 2.3 point 2: "method
/// label 34 wide for requests").
fn collect_methods(nodes: &[Node], into: &mut HashMap<String, Method>) {
    for node in nodes {
        match node {
            Node::Folder(folder) => collect_methods(&folder.children, into),
            Node::Request(request) => {
                if let Some(method) = &request.method {
                    into.insert(request.id.clone(), method.clone());
                }
            }
        }
    }
}

/// Width of a row's leading icon slot: a folder's chevron, or an empty spacer of the same width
/// for a request, so a root-level request's method label starts at the same x as a sibling
/// folder's chevron does (`Main A.dc.html`'s `r.icon` column is always this wide, even when
/// empty, `plans/ui-redesign.md` phase 5, reviewer fix item C).
const TREE_ROW_ICON_WIDTH: f32 = 13.0;

/// Renders one visible row of the sidebar tree, matching `Main A.dc.html`'s tree rows
/// (`plans/ui-redesign.md` phase 5, reviewer fix item 8): a folder shows a chevron (down when
/// expanded, right when collapsed) and no folder icon; a request shows no file icon, just the
/// method label (an empty [`METHOD_LABEL_WIDTH`]-wide spacer when `method` is `None`, a broken
/// request, so names stay aligned with their siblings) and the label built by
/// [`build_tree_item`]/[`build_filtered_item`] (already carrying the "(broken)" marker when it
/// applies), indented 16 px per depth level. The row itself is exactly [`TREE_ROW_HEIGHT`] tall:
/// `ListItem`'s own default padding is overridden below, since it would otherwise add to that
/// height. Selected rows get `accent_text` label color (the `accent_subtle` background and the
/// `hover` background come from `ListItem`'s own theme mapping, set up in phase 2).
fn render_tree_row(
    entry: &TreeEntry,
    selected: bool,
    method: Option<&Method>,
    palette: &crate::theme::Palette,
) -> ListItem {
    let item = entry.item();
    let is_request = item.id.ends_with(REQUEST_EXTENSION);

    let mut icon_slot = div()
        .flex_none()
        .w(px(TREE_ROW_ICON_WIDTH))
        .flex()
        .items_center()
        .justify_center();
    if !is_request {
        let chevron = if entry.is_expanded() {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        icon_slot = icon_slot.child(Icon::new(chevron).small().text_color(palette.fg_subtle));
    }

    let mut row = h_flex()
        .h(px(TREE_ROW_HEIGHT))
        .items_center()
        .gap_1()
        .pl(px(entry.depth() as f32 * 16.0))
        .child(icon_slot);
    if is_request {
        row = row.child(match method {
            Some(method) => MethodBadge::label(method.clone()).into_any_element(),
            None => div()
                .flex_none()
                .w(px(METHOD_LABEL_WIDTH))
                .into_any_element(),
        });
    }
    let label_color = if selected {
        palette.accent_text
    } else {
        palette.fg
    };
    row = row.child(
        div()
            .text_sm()
            .text_color(label_color)
            .child(item.label.clone()),
    );

    ListItem::new(item.id.clone())
        .selected(selected)
        .py(px(0.0))
        .px(px(8.0))
        .child(row)
}

/// Builds the right-click context menu for a tree entry: new request/folder (folders only),
/// rename and delete (both kinds).
fn build_context_menu(
    weak: WeakEntity<AppView>,
    entry: &TreeEntry,
    mut menu: PopupMenu,
    _window: &mut Window,
    _cx: &mut Context<TreeState>,
) -> PopupMenu {
    let item = entry.item();
    let id = item.id.to_string();
    let is_request = id.ends_with(REQUEST_EXTENSION);
    let current_name = item.label.to_string();

    if !is_request {
        let new_request_weak = weak.clone();
        let parent_for_request = id.clone();
        menu = menu.item(PopupMenuItem::new(t!("common.new_request")).on_click(
            move |_, window, cx| {
                open_new_request_dialog(
                    new_request_weak.clone(),
                    Some(parent_for_request.clone()),
                    window,
                    cx,
                );
            },
        ));

        let new_folder_weak = weak.clone();
        let parent_for_folder = id.clone();
        menu = menu.item(PopupMenuItem::new(t!("common.new_folder")).on_click(
            move |_, window, cx| {
                open_new_folder_dialog(
                    new_folder_weak.clone(),
                    Some(parent_for_folder.clone()),
                    window,
                    cx,
                );
            },
        ));
        menu = menu.separator();
    }

    let load_test_weak = weak.clone();
    let load_test_id = id.clone();
    menu = menu.item(PopupMenuItem::new(t!("shell.sidebar.load_test")).on_click(
        move |_, _window, cx| {
            let load_test_id = load_test_id.clone();
            let _ = load_test_weak.update(cx, |view, cx| {
                if is_request {
                    view.open_load_test_for_request(load_test_id, cx);
                } else {
                    view.open_load_test_for_collection(load_test_id, cx);
                }
            });
        },
    ));
    menu = menu.separator();

    let rename_weak = weak.clone();
    let rename_id = id.clone();
    let rename_name = current_name.clone();
    menu = menu.item(
        PopupMenuItem::new(t!("common.rename")).on_click(move |_, window, cx| {
            open_rename_dialog(
                rename_weak.clone(),
                rename_id.clone(),
                rename_name.clone(),
                window,
                cx,
            );
        }),
    );

    let delete_weak = weak.clone();
    let delete_id = id.clone();
    let delete_name = current_name;
    menu = menu.item(
        PopupMenuItem::new(t!("common.delete")).on_click(move |_, window, cx| {
            open_delete_confirmation(
                delete_weak.clone(),
                delete_id.clone(),
                delete_name.clone(),
                window,
                cx,
            );
        }),
    );

    menu
}

/// Opens a native folder picker and, when the user confirms a folder, opens it as the workspace.
/// Also called from the title bar's workspace switcher menu ("Open folder...").
pub(crate) fn pick_workspace_folder(view: WeakEntity<AppView>, window: &mut Window, cx: &mut App) {
    let window_handle = window.window_handle();
    let options = PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    };
    let receiver = cx.prompt_for_paths(options);
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(mut paths))) = receiver.await else {
            return;
        };
        let Some(root) = paths.pop() else {
            return;
        };
        let _ = cx.update_window(window_handle, |_, window, cx| {
            let _ = view.update(cx, |view, cx| view.open_workspace_at(&root, window, cx));
        });
    })
    .detach();
}

/// Opens an in-app dialog (never a native blocking one) asking for a new request's name, inside
/// `parent` (the workspace root when `None`). Also called from the open-tabs bar's trailing "+"
/// button (`plans/ui-redesign.md` section 2.3 point 3).
pub(crate) fn open_new_request_dialog(
    view: WeakEntity<AppView>,
    parent: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder(t!("shell.sidebar.request_name")));
    window.open_dialog(cx, move |dialog, _, _| {
        let input_for_content = input.clone();
        let input_for_ok = input.clone();
        let view = view.clone();
        let parent = parent.clone();
        dialog
            .title(t!("common.new_request"))
            .content(move |content, _, _| content.child(Input::new(&input_for_content)))
            .on_ok(move |_, _, cx| {
                let name = input_for_ok.read(cx).value().trim().to_string();
                if !name.is_empty() {
                    let _ =
                        view.update(cx, |view, cx| view.create_request(parent.clone(), name, cx));
                }
                true
            })
    });
}

/// Opens an in-app dialog asking for a new folder's name, inside `parent`. Also called from the
/// command palette's "New folder" action (`plans/ui-redesign.md` phase 7 item 1).
pub(crate) fn open_new_folder_dialog(
    view: WeakEntity<AppView>,
    parent: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder(t!("shell.sidebar.folder_name")));
    window.open_dialog(cx, move |dialog, _, _| {
        let input_for_content = input.clone();
        let input_for_ok = input.clone();
        let view = view.clone();
        let parent = parent.clone();
        dialog
            .title(t!("common.new_folder"))
            .content(move |content, _, _| content.child(Input::new(&input_for_content)))
            .on_ok(move |_, _, cx| {
                let name = input_for_ok.read(cx).value().trim().to_string();
                if !name.is_empty() {
                    let _ =
                        view.update(cx, |view, cx| view.create_folder(parent.clone(), name, cx));
                }
                true
            })
    });
}

/// Opens an in-app dialog, pre-filled with `current_name`, to rename `id`.
fn open_rename_dialog(
    view: WeakEntity<AppView>,
    id: String,
    current_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(t!("shell.sidebar.name")));
    input.update(cx, |state, cx| {
        state.set_value(current_name.clone(), window, cx);
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let input_for_content = input.clone();
        let input_for_ok = input.clone();
        let view = view.clone();
        let id = id.clone();
        dialog
            .title(t!("common.rename"))
            .content(move |content, _, _| content.child(Input::new(&input_for_content)))
            .on_ok(move |_, _, cx| {
                let name = input_for_ok.read(cx).value().trim().to_string();
                if !name.is_empty() {
                    let _ = view.update(cx, |view, cx| view.rename(id.clone(), name, cx));
                }
                true
            })
    });
}

/// Opens an in-app confirmation (never a native blocking dialog) before deleting `id`.
fn open_delete_confirmation(
    view: WeakEntity<AppView>,
    id: String,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_alert_dialog(cx, move |alert, _, _| {
        let view = view.clone();
        let id = id.clone();
        alert
            .title(t!("shell.sidebar.delete_title"))
            .description(t!("shell.sidebar.delete_text", name = name.as_str()))
            .button_props(
                DialogButtonProps::default()
                    .ok_text(t!("common.delete"))
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text(t!("common.cancel"))
                    .show_cancel(true),
            )
            .on_ok(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.delete(id.clone(), cx));
                true
            })
    });
}
