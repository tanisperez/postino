//! The sidebar: a header with "new request" / "new folder" / "open folder" buttons, and the
//! workspace's collection tree below it, with a right-click context menu for new/rename/delete
//! (`plans/mvp.md`, phase 8).

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::tree::{TreeEntry, TreeItem, TreeState, tree as tree_view};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use postino_workspace::Node;

use super::root::AppView;

/// The extension every request file uses (`plans/mvp.md`, section 3.2), used here only to tell a
/// request row from a folder row when rendering.
const REQUEST_EXTENSION: &str = ".postino";

impl AppView {
    /// Renders the sidebar's header and collection tree.
    pub(crate) fn render_sidebar(
        &mut self,
        weak: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_workspace = self.state.workspace.is_some();
        let tree_state = self.tree_state.clone();

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .child(div().text_sm().child("Collections"))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("new-request-root")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::Plus).small())
                                    .tooltip("New request")
                                    .disabled(!has_workspace)
                                    .on_click({
                                        let weak = weak.clone();
                                        move |_, window, cx| {
                                            open_new_request_dialog(weak.clone(), None, window, cx);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("new-folder-root")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::Folder).small())
                                    .tooltip("New folder")
                                    .disabled(!has_workspace)
                                    .on_click({
                                        let weak = weak.clone();
                                        move |_, window, cx| {
                                            open_new_folder_dialog(weak.clone(), None, window, cx);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("open-folder")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::FolderOpen).small())
                                    .tooltip("Open workspace folder")
                                    .on_click({
                                        let weak = weak.clone();
                                        move |_, _, cx| {
                                            pick_workspace_folder(weak.clone(), cx);
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(has_workspace, |this| {
                        this.child(
                            tree_view(&tree_state, |_ix, entry, selected, _window, _cx| {
                                render_tree_row(entry, selected)
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
                                .text_color(cx.theme().muted_foreground)
                                .child("Open a folder to get started."),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// Turns the workspace's collection tree into the `TreeItem`s the sidebar's `Tree` renders.
/// Every folder starts expanded, since the sample workspaces used during development are small
/// enough that a fully collapsed tree would just hide everything behind an extra click.
pub(crate) fn build_tree_items(nodes: &[Node]) -> Vec<TreeItem> {
    nodes.iter().map(build_tree_item).collect()
}

fn build_tree_item(node: &Node) -> TreeItem {
    match node {
        Node::Folder(folder) => TreeItem::new(folder.id.clone(), folder.name.clone())
            .children(build_tree_items(&folder.children))
            .expanded(true),
        Node::Request(request) => {
            let label = match &request.broken {
                Some(_) => format!("{} (broken)", request.name),
                None => request.name.clone(),
            };
            TreeItem::new(request.id.clone(), label)
        }
    }
}

/// Renders one visible row of the sidebar tree: a folder or file icon, and the label built by
/// [`build_tree_item`] (already carrying the "(broken)" marker when it applies).
fn render_tree_row(entry: &TreeEntry, selected: bool) -> ListItem {
    let item = entry.item();
    let is_request = item.id.ends_with(REQUEST_EXTENSION);
    let icon = if is_request {
        IconName::File
    } else if entry.is_expanded() {
        IconName::FolderOpen
    } else {
        IconName::Folder
    };

    ListItem::new(item.id.clone()).selected(selected).child(
        h_flex()
            .items_center()
            .gap_1()
            .pl(px(entry.depth() as f32 * 12.))
            .child(Icon::new(icon).small())
            .child(div().text_sm().child(item.label.clone())),
    )
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
        menu = menu.item(
            PopupMenuItem::new("New Request").on_click(move |_, window, cx| {
                open_new_request_dialog(
                    new_request_weak.clone(),
                    Some(parent_for_request.clone()),
                    window,
                    cx,
                );
            }),
        );

        let new_folder_weak = weak.clone();
        let parent_for_folder = id.clone();
        menu = menu.item(
            PopupMenuItem::new("New Folder").on_click(move |_, window, cx| {
                open_new_folder_dialog(
                    new_folder_weak.clone(),
                    Some(parent_for_folder.clone()),
                    window,
                    cx,
                );
            }),
        );
        menu = menu.separator();
    }

    let rename_weak = weak.clone();
    let rename_id = id.clone();
    let rename_name = current_name.clone();
    menu = menu.item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| {
        open_rename_dialog(
            rename_weak.clone(),
            rename_id.clone(),
            rename_name.clone(),
            window,
            cx,
        );
    }));

    let delete_weak = weak.clone();
    let delete_id = id.clone();
    let delete_name = current_name;
    menu = menu.item(PopupMenuItem::new("Delete").on_click(move |_, window, cx| {
        open_delete_confirmation(
            delete_weak.clone(),
            delete_id.clone(),
            delete_name.clone(),
            window,
            cx,
        );
    }));

    menu
}

/// Opens a native folder picker and, when the user confirms a folder, opens it as the workspace.
fn pick_workspace_folder(view: WeakEntity<AppView>, cx: &mut App) {
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
        let _ = view.update(cx, |view, cx| view.open_workspace_at(&root, cx));
    })
    .detach();
}

/// Opens an in-app dialog (never a native blocking one) asking for a new request's name, inside
/// `parent` (the workspace root when `None`).
fn open_new_request_dialog(
    view: WeakEntity<AppView>,
    parent: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Request name"));
    window.open_dialog(cx, move |dialog, _, _| {
        let input_for_content = input.clone();
        let input_for_ok = input.clone();
        let view = view.clone();
        let parent = parent.clone();
        dialog
            .title("New request")
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

/// Opens an in-app dialog asking for a new folder's name, inside `parent`.
fn open_new_folder_dialog(
    view: WeakEntity<AppView>,
    parent: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
    window.open_dialog(cx, move |dialog, _, _| {
        let input_for_content = input.clone();
        let input_for_ok = input.clone();
        let view = view.clone();
        let parent = parent.clone();
        dialog
            .title("New folder")
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
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
    input.update(cx, |state, cx| {
        state.set_value(current_name.clone(), window, cx);
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let input_for_content = input.clone();
        let input_for_ok = input.clone();
        let view = view.clone();
        let id = id.clone();
        dialog
            .title("Rename")
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
            .title("Delete?")
            .description(format!(
                "\"{name}\" will be permanently deleted. This cannot be undone."
            ))
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Delete")
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Cancel")
                    .show_cancel(true),
            )
            .on_ok(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.delete(id.clone(), cx));
                true
            })
    });
}
