//! The sidebar: a header with "new request" / "new folder" / "import" icon buttons, a filter input
//! and the workspace's collection tree. Right-clicking a tree row still opens the rename/delete/new
//! request/new folder context menu.
//!
//! A request or a folder moves to another folder by dragging it: the folder under the pointer (the
//! row's own folder for a request row, the root below the last row) is highlighted with everything
//! inside it, and dropping moves the entry there (`state::tree_drop`). A folder is never a target
//! for itself nor for its own subfolders.
//!
//! Ctrl+click (Cmd+click on macOS) picks several rows (`state::tree_selection`); dragging one of
//! them moves them all, and a plain click goes back to a single selection.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::tree::{TreeEntry, TreeItem, TreeState, tree as tree_view};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::Method;
use postino_workspace::{Node, RequestEntry};

use crate::actions::{DeleteSelectedEntry, RenameSelectedEntry};
use crate::state;
use crate::theme::PaletteExt;
use crate::theme::metrics::{
    ICON_BUTTON_HEADER, INPUT_PADDING_X, MODAL_WIDTH_DEFAULT, RADIUS_MD, RADIUS_SM,
    SIDEBAR_FILTER_HEIGHT, TREE_ROW_HEIGHT,
};
use crate::views::components::{
    ConfirmDialog, IconButton, MethodBadge, ModalPlacement, PrimaryButton, SecondaryButton,
    SectionLabel, edit_menu, framed_dialog, modal_body, modal_footer, modal_header,
    open_confirm_dialog, text_field,
};

use super::root::AppView;

/// The extension every request file uses, used here only to tell a request row from a folder row
/// when rendering.
const REQUEST_EXTENSION: &str = ".postino";

/// The value carried while a request or a folder is dragged in the tree, also rendered as the
/// preview under the pointer. `id` is the row grabbed; what moves is
/// `TreeSelection::dragged_ids` of it, all the picked rows when it is one of them.
#[derive(Clone)]
struct DraggedEntry {
    id: String,
    /// The request's method, `None` for a folder or a broken request.
    method: Option<Method>,
    is_folder: bool,
    label: SharedString,
    /// How many entries move, set when the drag starts. The preview shows the count above one.
    count: usize,
}

impl Render for DraggedEntry {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        h_flex()
            .h(px(TREE_ROW_HEIGHT))
            .items_center()
            .gap(px(6.0))
            .px_3()
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(palette.border_strong)
            .bg(palette.bg)
            .shadow(palette.shadow.clone())
            .opacity(0.9)
            .text_sm()
            .text_color(palette.fg)
            .when(self.count > 1, |this| {
                this.child(state::locale::plural(
                    "shell.sidebar.dragged_items",
                    self.count,
                ))
            })
            .when(self.count <= 1 && self.is_folder, |this| {
                this.child(
                    Icon::new(gpui_kit::assets::IconName::Folder)
                        .small()
                        .text_color(palette.fg_subtle),
                )
            })
            .when(self.count <= 1, |this| {
                this.children(self.method.clone().map(MethodBadge::inline))
                    .child(self.label.clone())
            })
    }
}

/// What a tree row needs to take part in moving an entry by drag and drop.
#[derive(Clone)]
struct RowDrop {
    view: WeakEntity<AppView>,
    /// The folder a dragged entry would go to (`""` for the root), `None` when there is none.
    target: Option<String>,
    /// The rows picked with Ctrl+click, highlighted instead of the tree's own selection.
    picked: state::tree_selection::TreeSelection,
}

/// The 1 px accent ring drawn inside the drop target: the target folder's row, or the whole tree
/// for the root.
fn drop_ring(palette: &crate::theme::Palette) -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.0), px(0.0), palette.accent)
            .spread_radius(px(1.0))
            .inset(),
    ]
}

impl AppView {
    /// Renders the sidebar: header, filter input, collection tree, and footer.
    pub(crate) fn render_sidebar(
        &mut self,
        weak: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_workspace = self.state.workspace.is_some();
        // The folder being opened replaces whatever the sidebar showed until its scan is done.
        let opening = self.opening.as_ref().map(|opening| {
            opening.root.file_name().map_or_else(
                || opening.root.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        });
        let tree_state = self.tree_state.clone();
        let palette = cx.palette();
        // A drag that ended outside the tree never dropped here: forget its target.
        let drop_target = self
            .sidebar_drop_target
            .clone()
            .filter(|_| cx.has_active_drag());
        let root_is_target = drop_target.as_deref() == Some("");
        let row_drop = RowDrop {
            view: weak.clone(),
            target: drop_target,
            picked: self.sidebar_selection.clone(),
        };
        let move_weak = weak.clone();
        let drop_weak = weak.clone();
        let mut methods_by_id = HashMap::new();
        if let Some(workspace) = self.state.workspace.as_ref() {
            collect_methods(workspace.tree(), &mut methods_by_id);
        }

        v_flex()
            .size_full()
            .bg(palette.surface)
            .border_r_1()
            .border_color(palette.border)
            .child(render_header(
                weak.clone(),
                has_workspace,
                self.render_import_menu(cx),
            ))
            .child(self.render_filter_row(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(px(6.0))
                    .when(root_is_target, |this| this.shadow(drop_ring(&palette)))
                    // Runs before the rows' own handlers (capture phase, back to front): the
                    // root while inside the tree, until the row under the pointer says otherwise.
                    .on_drag_move::<DraggedEntry>(move |event, _window, cx| {
                        let inside = event.bounds.contains(&event.event.position);
                        let entry_id = event.drag(cx).id.clone();
                        let view = move_weak.clone();
                        let _ = view.update(cx, |view, _| {
                            view.sidebar_drop_hover = inside.then(String::new);
                        });
                        cx.defer(move |cx| {
                            let _ = view.update(cx, |view, cx| {
                                view.commit_sidebar_drop(&entry_id, cx);
                            });
                        });
                    })
                    .on_drop(move |dragged: &DraggedEntry, _window, cx| {
                        let id = dragged.id.clone();
                        let _ = drop_weak.update(cx, |view, cx| view.drop_on_sidebar(id, cx));
                    })
                    .when_some(opening.clone(), |this, name| {
                        this.child(
                            div()
                                .p_4()
                                .text_sm()
                                .text_color(palette.fg_subtle)
                                .child(t!("shell.sidebar.opening", name = name)),
                        )
                    })
                    // Without a workspace, nor one being opened, `render_body` hides the sidebar.
                    .when(opening.is_none() && has_workspace, |this| {
                        let row_palette = palette.clone();
                        this.child(
                            tree_view(&tree_state, move |_ix, entry, selected, _window, _cx| {
                                render_tree_row(
                                    entry,
                                    selected,
                                    methods_by_id.get(entry.item().id.as_ref()),
                                    &row_palette,
                                    &row_drop,
                                )
                            })
                            .context_menu(move |_ix, entry, menu, window, cx| {
                                build_context_menu(weak.clone(), entry, menu, window, cx)
                            })
                            .size_full(),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The folder under the pointer, collected by the tree's drag move handlers, becomes the drop
    /// target of the entries dragged by `entry_id` once all of them ran, repainting only when it
    /// changed.
    fn commit_sidebar_drop(&mut self, entry_id: &str, cx: &mut Context<Self>) {
        let ids = self.sidebar_selection.dragged_ids(entry_id);
        let target = state::tree_selection::drop_target_for(&ids, self.sidebar_drop_hover.take());
        if target != self.sidebar_drop_target {
            self.sidebar_drop_target = target;
            cx.notify();
        }
    }

    /// A request or folder dropped on the tree: moves it, with the other picked rows when it is
    /// one of them, to the highlighted folder, if any.
    fn drop_on_sidebar(&mut self, id: String, cx: &mut Context<Self>) {
        self.sidebar_drop_hover = None;
        match self.sidebar_drop_target.take() {
            Some(folder) => {
                let ids = self.sidebar_selection.dragged_ids(&id);
                self.move_entries(ids, folder, cx);
            }
            None => cx.notify(),
        }
    }

    /// Ctrl+click on the row `id`: picks it, or unpicks it, next to the row the tree had selected.
    /// That row loses the tree's selection once it is unpicked, so only picked rows look selected.
    fn toggle_sidebar_pick(&mut self, id: &str, cx: &mut Context<Self>) {
        let anchor = self
            .tree_state
            .read(cx)
            .selected_item()
            .map(|item| item.id.to_string());
        self.sidebar_selection.toggle(id, anchor.as_deref());
        if anchor.is_some_and(|anchor| !self.sidebar_selection.contains(&anchor)) {
            self.tree_state
                .update(cx, |tree, cx| tree.set_selected_index(None, cx));
        }
        cx.notify();
    }

    /// A folder grabbed to drag it: the tree expanded or collapsed it on the press, so this puts it
    /// back as it was, keeping the rows under the pointer where they were, and selects it again.
    fn undo_folder_press_toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some((pressed, was_expanded)) = self.sidebar_folder_press.take() else {
            return;
        };
        if pressed != id {
            return;
        }
        self.refresh_tree_with(cx, |expansion| {
            expansion.insert(pressed, was_expanded);
        });
        let id = SharedString::from(id.to_string());
        self.tree_state.update(cx, |tree, cx| {
            let index = tree.index_of(&id);
            tree.set_selected_index(index, cx);
        });
    }

    /// A plain click on a row: back to the tree's single selection.
    fn clear_sidebar_picks(&mut self, cx: &mut Context<Self>) {
        if !self.sidebar_selection.is_empty() {
            self.sidebar_selection.clear();
            cx.notify();
        }
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
                    // An id keeps the hover state, so entering and leaving renders again.
                    .id("sidebar-filter")
                    .flex_1()
                    .h(px(SIDEBAR_FILTER_HEIGHT))
                    .items_center()
                    .gap_2()
                    .px(px(9.0))
                    .rounded(px(RADIUS_MD - 1.0))
                    .bg(palette.raised)
                    .border_1()
                    .border_color(palette.border)
                    // The hover of every text input (`components/hover_input.rs`).
                    .hover({
                        let border_strong = palette.border_strong;
                        move |style| style.border_color(border_strong)
                    })
                    .text_color(palette.fg_subtle)
                    .child(Icon::new(gpui_kit::assets::IconName::ListFilter).small())
                    .child(
                        Input::new(&self.sidebar_filter_input)
                            .context_menu(edit_menu(&self.sidebar_filter_input, cx))
                            // No left padding: the row's 8 px gap already separates the text
                            // from the icon, as in the title bar's search trigger.
                            .pl_0()
                            .pr(px(INPUT_PADDING_X))
                            .h(px(SIDEBAR_FILTER_HEIGHT))
                            .py_0()
                            .bordered(false)
                            // The row's `fg_subtle` is for the icon; typed text is plain `fg`.
                            .text_color(palette.fg)
                            // Transparent so only the outer pill shows: without this, the `Input`'s
                            // own background paints a second, visible box inside it.
                            .bg(palette.bg.opacity(0.0)),
                    ),
            )
            .into_any_element()
    }
}

/// Renders the sidebar's header row: the "Collections" section label, the "new request" / "new
/// folder" icon buttons, and `import_menu` (`AppView::render_import_menu`).
fn render_header(
    weak: WeakEntity<AppView>,
    has_workspace: bool,
    import_menu: AnyElement,
) -> impl IntoElement {
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
                        .box_size(ICON_BUTTON_HEADER)
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
                        .box_size(ICON_BUTTON_HEADER)
                        .tooltip(t!("common.new_folder"))
                        .disabled(!has_workspace)
                        .on_click({
                            let weak = weak.clone();
                            move |_, window, cx| {
                                open_new_folder_dialog(weak.clone(), None, window, cx);
                            }
                        }),
                )
                .child(import_menu),
        )
}

/// Turns the workspace's collection tree into the `TreeItem`s the sidebar's `Tree` renders,
/// unfiltered. `expansion` holds each folder's expand flag (see `state::tree_expansion`); a folder
/// missing from it is collapsed, so a freshly opened workspace shows only its top level.
pub(crate) fn build_tree_items(nodes: &[Node], expansion: &HashMap<String, bool>) -> Vec<TreeItem> {
    nodes
        .iter()
        .map(|node| build_tree_item(node, expansion))
        .collect()
}

fn build_tree_item(node: &Node, expansion: &HashMap<String, bool>) -> TreeItem {
    match node {
        Node::Folder(folder) => {
            let expanded = expansion.get(&folder.id).copied().unwrap_or(false);
            TreeItem::new(folder.id.clone(), folder.name.clone())
                .children(build_tree_items(&folder.children, expansion))
                .expanded(expanded)
        }
        Node::Request(request) => TreeItem::new(request.id.clone(), request_label(request)),
    }
}

/// Turns the workspace's collection tree into the `TreeItem`s shown while the sidebar filter is
/// non-empty: only the ids `state::sidebar_filter::visible_ids` returns, every included folder
/// forced expanded so a match is never hidden.
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
/// looks it up from this side map instead (the method
/// label is 34 px wide for requests).
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
/// folder's chevron does (the slot is always this wide, even when
/// empty).
const TREE_ROW_ICON_WIDTH: f32 = 13.0;

/// Extra space between a request row's method and its name, on top of the row's 4px gap.
const TREE_ROW_METHOD_GAP: f32 = 2.0;

/// Renders one visible row of the sidebar tree: a folder shows a chevron (down when expanded, right
/// when collapsed) and no folder icon; a request shows no file icon, just the method at its natural
/// width, a fixed gap from its name (nothing when `method` is `None`, a broken request), and the
/// label built by [`build_tree_item`]/[`build_filtered_item`] (already carrying the "(broken)"
/// marker when it applies), indented 16 px per depth level. The row itself is exactly
/// [`TREE_ROW_HEIGHT`] tall: `ListItem`'s own default padding is overridden below, since it would
/// otherwise add to that height. Selected rows get `accent_text` label color (the `accent_subtle`
/// background and the `hover` background come from `ListItem`'s own theme mapping, set up in
/// `theme`). While some rows are picked with Ctrl+click, they are the selected ones instead. Any
/// row can be dragged; while one is, the rows of the target folder get the `hover` background and
/// the folder's own row the accent ring.
fn render_tree_row(
    entry: &TreeEntry,
    selected: bool,
    method: Option<&Method>,
    palette: &crate::theme::Palette,
    drop: &RowDrop,
) -> ListItem {
    let item = entry.item();
    let is_request = item.id.ends_with(REQUEST_EXTENSION);
    let picked = drop.picked.contains(&item.id);
    let selected = if drop.picked.is_empty() {
        selected
    } else {
        picked
    };

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
    // The method at its natural width, a fixed gap before the name (the row's own 4px plus
    // this margin), however wide the method is. A broken request has no method to show.
    if let Some(method) = method.filter(|_| is_request) {
        row = row.child(
            div()
                .flex_none()
                .mr(px(TREE_ROW_METHOD_GAP))
                .child(MethodBadge::inline(method.clone())),
        );
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

    let id = item.id.to_string();
    let in_target = drop
        .target
        .as_deref()
        .is_some_and(|target| state::tree_drop::in_drop_target(&id, target));
    let is_target_folder = drop.target.as_deref() == Some(id.as_str());
    let drop_folder = state::tree_drop::row_drop_folder(&id, is_request).to_string();
    let view = drop.view.clone();
    let pick_view = drop.view.clone();
    let pick_id = id.clone();
    let was_expanded = entry.is_expanded();
    let click_view = drop.view.clone();
    let preview_view = drop.view.clone();

    ListItem::new(item.id.clone())
        .selected(selected)
        .py(px(0.0))
        .px(px(8.0))
        .when(in_target, |this| this.bg(palette.hover))
        // `Tree` sets `selected` from its own single selection, so picked rows paint the same
        // background themselves.
        .when(picked, |this| this.bg(palette.list_active))
        .when(is_target_folder, |this| this.shadow(drop_ring(palette)))
        // Before the tree's own handler, which selects the row (and opens a request): Ctrl+click
        // only picks it.
        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
            if event.modifiers.secondary() {
                cx.stop_propagation();
                let _ = pick_view.update(cx, |view, cx| view.toggle_sidebar_pick(&pick_id, cx));
            } else if !is_request {
                let _ = pick_view.update(cx, |view, _| {
                    view.sidebar_folder_press = Some((pick_id.clone(), was_expanded));
                });
            }
        })
        // A click, never the end of a drag: a picked row can be grabbed to move them all.
        .on_click(move |event, _window, cx| {
            if !event.modifiers().secondary() {
                let _ = click_view.update(cx, |view, cx| view.clear_sidebar_picks(cx));
            }
        })
        .on_drag(
            DraggedEntry {
                id: id.clone(),
                method: method.filter(|_| is_request).cloned(),
                is_folder: !is_request,
                label: item.label.clone(),
                count: 1,
            },
            move |dragged, _, _, cx| {
                if dragged.is_folder {
                    let _ = preview_view.update(cx, |view, cx| {
                        view.undo_folder_press_toggle(&dragged.id, cx);
                    });
                }
                let count = preview_view.upgrade().map_or(1, |view| {
                    view.read(cx)
                        .sidebar_selection
                        .dragged_ids(&dragged.id)
                        .len()
                });
                cx.new(|_| DraggedEntry {
                    count,
                    ..dragged.clone()
                })
            },
        )
        // After the tree container's handler: the row under the pointer names the folder.
        .on_drag_move::<DraggedEntry>(move |event, _window, cx| {
            if event.bounds.contains(&event.event.position) {
                let _ = view.update(cx, |view, _| {
                    if view.sidebar_drop_hover.is_some() {
                        view.sidebar_drop_hover = Some(drop_folder.clone());
                    }
                });
            }
        })
        .child(row)
}

/// Builds the right-click context menu for a tree entry: new request/folder (folders only), load
/// test, duplicate (requests only), rename and delete.
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

    if is_request {
        let duplicate_weak = weak.clone();
        let duplicate_id = id.clone();
        menu = menu.item(PopupMenuItem::new(t!("shell.sidebar.duplicate")).on_click(
            move |_, _window, cx| {
                let _ =
                    duplicate_weak.update(cx, |view, cx| view.duplicate(duplicate_id.clone(), cx));
            },
        ));
    }

    let rename_weak = weak.clone();
    let rename_id = id.clone();
    let rename_name = current_name.clone();
    // The actions only show their key next to the label: the click handler acts on the
    // right-clicked entry, which need not be the selected one the keys act on.
    menu = menu.item(
        PopupMenuItem::new(t!("common.rename"))
            .action(Box::new(RenameSelectedEntry))
            .on_click(move |_, window, cx| {
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
        PopupMenuItem::new(t!("common.delete"))
            .action(Box::new(DeleteSelectedEntry))
            .on_click(move |_, window, cx| {
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

/// What a name dialog does with the trimmed, non-empty name once it is confirmed.
type NameSubmit = Rc<dyn Fn(String, &mut App)>;

/// Confirms a name dialog: reads the field, closes the dialog and runs the [`NameSubmit`].
type NameConfirm = Rc<dyn Fn(&mut Window, &mut App)>;

/// Opens an in-app dialog (never a native blocking one) with a name field, a Cancel button and
/// `ok_label`. Enter in the field and the button confirm it; an empty name keeps the dialog open.
fn open_name_dialog(
    title: SharedString,
    ok_label: SharedString,
    input: Entity<InputState>,
    submit: NameSubmit,
    window: &mut Window,
    cx: &mut App,
) {
    // Confirms and closes the dialog; shared by Enter in the input and the button.
    let confirm: NameConfirm = {
        let input = input.clone();
        Rc::new(move |window, cx| {
            let name = input.read(cx).value().trim().to_string();
            if name.is_empty() {
                return;
            }
            window.close_dialog(cx);
            submit(name, cx);
        })
    };
    {
        let confirm = confirm.clone();
        window
            .subscribe(&input, cx, move |_, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    confirm(window, cx);
                }
            })
            .detach();
    }
    let dialog_input = input.clone();
    window.open_dialog(cx, move |dialog, window, _| {
        let input = dialog_input.clone();
        let confirm = confirm.clone();
        let title = title.clone();
        let ok_label = ok_label.clone();
        framed_dialog(
            dialog,
            MODAL_WIDTH_DEFAULT,
            None,
            ModalPlacement::DEFAULT,
            window,
            move |_, cx| {
                let confirm = confirm.clone();
                v_flex()
                    .child(modal_header(title.clone(), cx))
                    .child(modal_body().child(text_field(&input, cx)))
                    .child(
                        modal_footer(cx)
                            .child(
                                SecondaryButton::new("name-dialog-cancel", t!("common.cancel"))
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                PrimaryButton::new("name-dialog-ok", ok_label.clone())
                                    .on_click(move |_, window, cx| confirm(window, cx)),
                            ),
                    )
                    .into_any_element()
            },
        )
    });
    // Focus only after `open_dialog`, which captures the focused handle to restore on close
    // (see `views/command_palette.rs`). Selecting the whole text makes typing replace a
    // pre-filled name.
    input.update(cx, |state, cx| {
        state.focus(window, cx);
        state.select_all(window, cx);
    });
}

/// Opens the dialog asking for a new request's name, inside `parent` (the workspace root when
/// `None`). Also called from the open-tabs bar's trailing "+" button.
pub(crate) fn open_new_request_dialog(
    view: WeakEntity<AppView>,
    parent: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder(t!("shell.sidebar.request_name")));
    let submit: NameSubmit = Rc::new(move |name, cx| {
        let _ = view.update(cx, |view, cx| view.create_request(parent.clone(), name, cx));
    });
    open_name_dialog(
        t!("common.new_request").into(),
        t!("common.create").into(),
        input,
        submit,
        window,
        cx,
    );
}

/// Opens the dialog asking for a new folder's name, inside `parent`. Also called from the command
/// palette's "New folder" action.
pub(crate) fn open_new_folder_dialog(
    view: WeakEntity<AppView>,
    parent: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder(t!("shell.sidebar.folder_name")));
    let submit: NameSubmit = Rc::new(move |name, cx| {
        let _ = view.update(cx, |view, cx| view.create_folder(parent.clone(), name, cx));
    });
    open_name_dialog(
        t!("common.new_folder").into(),
        t!("common.create").into(),
        input,
        submit,
        window,
        cx,
    );
}

/// Opens the dialog, pre-filled with `current_name`, to rename `id`. Also called by `F2` in the
/// sidebar tree (`AppView::on_rename_selected_entry`).
pub(crate) fn open_rename_dialog(
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
    let submit: NameSubmit = Rc::new(move |name, cx| {
        let _ = view.update(cx, |view, cx| view.rename(id.clone(), name, cx));
    });
    open_name_dialog(
        t!("common.rename").into(),
        t!("common.rename").into(),
        input,
        submit,
        window,
        cx,
    );
}

/// Opens an in-app confirmation (never a native blocking dialog) before deleting `id`. Also
/// called by `Delete` in the sidebar tree (`AppView::on_delete_selected_entry`).
pub(crate) fn open_delete_confirmation(
    view: WeakEntity<AppView>,
    id: String,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    open_confirm_dialog(
        window,
        cx,
        ConfirmDialog {
            title: t!("shell.sidebar.delete_title").into(),
            text: t!("shell.sidebar.delete_text", name = name.as_str()).into(),
            confirm_label: t!("common.delete").into(),
            destructive: true,
            show_cancel: true,
            on_confirm: Rc::new(move |_, cx| {
                let _ = view.update(cx, |view, cx| view.delete(id.clone(), cx));
            }),
        },
    );
}
