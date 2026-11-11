//! The context menu of the open tabs (#230, the design's "Context Menus" page), and closing
//! several tabs at once: the menu's Close items, the close button of a tab and `Ctrl+W` all go
//! through [`AppView::request_close_tabs`], which asks once when some of the tabs have unsaved
//! changes.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_workspace::EnvLayer;
use rust_i18n::t;

use crate::actions::CloseActiveTab;
use crate::state;
use crate::state::CloseScope;
use crate::state::locale::plural;
use crate::state::navigation::NavSection;
use crate::theme::PaletteExt;
use crate::theme::metrics::{MODAL_WIDTH_DEFAULT, RADIUS_MD};
use crate::views::components::{
    ModalPlacement, PrimaryButton, SecondaryButton, framed_dialog, modal_body, modal_footer,
    modal_header,
};
use crate::views::sidebar::{open_new_request_dialog, open_rename_dialog};

use super::root::AppView;

/// What the unsaved tabs dialog runs: `true` saves before closing, `false` discards.
type CloseChoice = dyn Fn(bool, &mut Window, &mut App);

/// What a tab's file is, which decides the items of its menu beyond the Close ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TabFile {
    /// A request: the file items, Rename and Duplicate.
    Request,
    /// An environment, by name: the file items.
    Environment(String),
    /// A tab with no file of its own (a load test, a diff, the log, Welcome, What's new).
    None,
}

/// What the menu of one tab needs, read when the tab bar is rendered.
#[derive(Debug, Clone)]
pub(crate) struct TabMenuTarget {
    /// The tab's id.
    pub(crate) id: String,
    /// Its place in the bar.
    pub(crate) index: usize,
    /// How many tabs are open.
    pub(crate) count: usize,
    /// Whether some open tab has no unsaved changes.
    pub(crate) any_saved: bool,
    /// The tab's file.
    pub(crate) file: TabFile,
}

impl AppView {
    /// Closes the tabs with `ids`. When the active tab is among them, `keep` becomes the active
    /// one if it is still open (the tab the menu was opened on).
    fn close_tabs(&mut self, ids: &[String], keep: Option<&str>, cx: &mut Context<Self>) {
        let active = self.state.tabs.active().map(|tab| tab.id.clone());
        for id in ids {
            if let Some(index) = self.state.tabs.index_of(id) {
                self.close_tab(index, cx);
            }
        }
        let active_closed = active.is_some_and(|id| self.state.tabs.index_of(&id).is_none());
        if active_closed && let Some(index) = keep.and_then(|id| self.state.tabs.index_of(id)) {
            self.state.tabs.set_active(index);
        }
        cx.notify();
    }

    /// Closes the tabs with `ids`, asking first, once, when some have unsaved changes: save them
    /// all, close without saving, or cancel and close none.
    pub(crate) fn request_close_tabs(
        &mut self,
        ids: Vec<String>,
        keep: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let unsaved: Vec<SharedString> = self
            .state
            .tabs
            .unsaved_among(&ids)
            .into_iter()
            .map(|tab| match tab.environment() {
                Some(edit) => state::env_edit::status_path(&edit.name).into(),
                None => tab.id.clone().into(),
            })
            .collect();
        if unsaved.is_empty() {
            self.close_tabs(&ids, keep.as_deref(), cx);
            return;
        }

        let weak = cx.weak_entity();
        let close: Rc<CloseChoice> = Rc::new(move |save, window, cx| {
            window.close_dialog(cx);
            let _ = weak.update(cx, |view, cx| {
                if save {
                    view.save_and_close_tabs(&ids, keep.as_deref(), cx);
                } else {
                    view.close_tabs(&ids, keep.as_deref(), cx);
                }
            });
        });
        let title = plural("shell.tabs.unsaved.title", unsaved.len());
        window.open_dialog(cx, move |dialog, window, _| {
            let save = close.clone();
            // Enter saves, like the primary button.
            let dialog = dialog.overlay_closable(false).on_ok(move |_, window, cx| {
                save(true, window, cx);
                false
            });
            let close = close.clone();
            let unsaved = unsaved.clone();
            let title = title.clone();
            framed_dialog(
                dialog,
                MODAL_WIDTH_DEFAULT,
                None,
                ModalPlacement::DEFAULT,
                window,
                move |_, cx| {
                    let palette = cx.palette();
                    let mono_font = cx.theme().mono_font_family.clone();
                    let discard = close.clone();
                    let save = close.clone();
                    v_flex()
                        .child(modal_header(title.clone(), cx))
                        .child(
                            modal_body()
                                .child(
                                    div()
                                        .text_color(palette.fg_muted)
                                        .child(t!("shell.tabs.unsaved.text")),
                                )
                                .child(
                                    v_flex()
                                        .id("unsaved-tabs-list")
                                        .max_h(px(160.0))
                                        .overflow_y_scroll()
                                        .gap(px(4.0))
                                        .px(px(10.0))
                                        .py(px(8.0))
                                        .rounded(px(RADIUS_MD))
                                        .border_1()
                                        .border_color(palette.border)
                                        .bg(palette.bg)
                                        .font_family(mono_font)
                                        .text_size(px(12.0))
                                        .children(
                                            unsaved
                                                .iter()
                                                .map(|path| div().truncate().child(path.clone())),
                                        ),
                                ),
                        )
                        .child(
                            modal_footer(cx)
                                .child(
                                    SecondaryButton::new(
                                        "unsaved-tabs-cancel",
                                        t!("common.cancel"),
                                    )
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    SecondaryButton::new(
                                        "unsaved-tabs-discard",
                                        t!("shell.tabs.unsaved.discard"),
                                    )
                                    .on_click(move |_, window, cx| discard(false, window, cx)),
                                )
                                .child(
                                    PrimaryButton::new(
                                        "unsaved-tabs-save",
                                        t!("shell.tabs.unsaved.save_all"),
                                    )
                                    .on_click(move |_, window, cx| save(true, window, cx)),
                                ),
                        )
                        .into_any_element()
                },
            )
        });
    }

    /// "Save all": saves the unsaved tabs among `ids`, then closes every tab of `ids` that has
    /// nothing unsaved left. A tab that could not be saved stays open with its error.
    fn save_and_close_tabs(&mut self, ids: &[String], keep: Option<&str>, cx: &mut Context<Self>) {
        let unsaved: Vec<String> = self
            .state
            .tabs
            .unsaved_among(ids)
            .into_iter()
            .map(|tab| tab.id.clone())
            .collect();
        for id in &unsaved {
            if let Some(index) = self.state.tabs.index_of(id) {
                self.save_tab(index, cx);
            }
        }
        let failed: Vec<String> = self
            .state
            .tabs
            .unsaved_among(ids)
            .into_iter()
            .map(|tab| tab.id.clone())
            .collect();
        let closable: Vec<String> = ids
            .iter()
            .filter(|id| !failed.contains(id))
            .cloned()
            .collect();
        self.close_tabs(&closable, keep, cx);
        // The first tab that could not be saved is shown, so its error is in view.
        if let Some(index) = failed.first().and_then(|id| self.state.tabs.index_of(id)) {
            self.state.tabs.set_active(index);
        }
    }

    /// A menu item's "Close ...": the tabs `scope` names for the tab `id`.
    fn close_from_menu(
        &mut self,
        id: &str,
        scope: CloseScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.state.tabs.index_of(id) else {
            return;
        };
        let ids = self.state.tabs.close_targets(index, scope);
        self.request_close_tabs(ids, Some(id.to_string()), window, cx);
    }

    /// "Close all" of the tab bar's own menu.
    fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self
            .state
            .tabs
            .open_tabs()
            .iter()
            .map(|tab| tab.id.clone())
            .collect();
        self.request_close_tabs(ids, None, window, cx);
    }

    /// "Reveal in sidebar": shows the panel that lists the tab's file and selects it there.
    fn reveal_tab_in_sidebar(&mut self, id: &str, file: &TabFile, cx: &mut Context<Self>) {
        let section = match file {
            TabFile::Request => NavSection::Collections,
            TabFile::Environment(_) => NavSection::Environments,
            TabFile::None => return,
        };
        if !self.nav.is_active(section) {
            self.click_rail(section, cx);
        }
        match file {
            TabFile::Request => {
                self.refresh_tree_with(cx, |expansion| {
                    state::tree_expansion::reveal(expansion, id);
                });
                // Already "followed", so the render that follows the tree's selection does not
                // activate the tab.
                self.last_selected_request = Some(id.to_string());
                let id = SharedString::from(id.to_string());
                self.tree_state.update(cx, |tree, cx| {
                    let index = tree.index_of(&id);
                    tree.set_selected_index(index, cx);
                });
            }
            // The panel highlights the environment of the active tab.
            TabFile::Environment(_) => {
                if let Some(index) = self.state.tabs.index_of(id) {
                    self.state.tabs.set_active(index);
                }
            }
            TabFile::None => {}
        }
        cx.notify();
    }

    /// The file of the tab `id` on disk.
    fn tab_file_path(&self, id: &str, file: &TabFile) -> Option<PathBuf> {
        let workspace = self.state.workspace.as_ref()?;
        match file {
            TabFile::Request => Some(workspace.root().join(id)),
            TabFile::Environment(name) => Some(workspace.environment_path(name, EnvLayer::Base)),
            TabFile::None => None,
        }
    }
}

/// Builds the context menu of the tab `target`: text only rows, with the key of the ones that
/// have one. Every item acts on that tab, which need not be the active one.
pub(crate) fn tab_context_menu(
    weak: WeakEntity<AppView>,
    target: &TabMenuTarget,
    mut menu: PopupMenu,
) -> PopupMenu {
    let close_item = |label: std::borrow::Cow<'static, str>, scope: CloseScope, enabled: bool| {
        let weak = weak.clone();
        let id = target.id.clone();
        PopupMenuItem::new(label.into_owned())
            .disabled(!enabled)
            .on_click(move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| view.close_from_menu(&id, scope, window, cx));
            })
    };
    let last = target.index + 1 == target.count;
    menu = menu
        // The action only shows its key: the click handler closes the right-clicked tab, which
        // need not be the active one the key acts on.
        .item(
            close_item(t!("common.close"), CloseScope::This, true).action(Box::new(CloseActiveTab)),
        )
        .item(close_item(
            t!("shell.tabs.close_others"),
            CloseScope::Others,
            target.count > 1,
        ))
        .item(close_item(
            t!("shell.tabs.close_right"),
            CloseScope::Right,
            !last,
        ))
        .item(close_item(
            t!("shell.tabs.close_left"),
            CloseScope::Left,
            target.index > 0,
        ))
        .item(close_item(
            t!("shell.tabs.close_saved"),
            CloseScope::Saved,
            target.any_saved,
        ))
        .item(close_item(
            t!("shell.tabs.close_all"),
            CloseScope::All,
            true,
        ));

    if target.file == TabFile::None {
        return menu;
    }
    let copy_path = {
        let path = match &target.file {
            TabFile::Environment(name) => state::env_edit::status_path(name),
            _ => target.id.clone(),
        };
        PopupMenuItem::new(t!("shell.tabs.copy_path")).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
        })
    };
    let reveal_sidebar = {
        let weak = weak.clone();
        let id = target.id.clone();
        let file = target.file.clone();
        PopupMenuItem::new(t!("shell.tabs.reveal_sidebar")).on_click(move |_, _, cx| {
            let _ = weak.update(cx, |view, cx| view.reveal_tab_in_sidebar(&id, &file, cx));
        })
    };
    let reveal_file = {
        let weak = weak.clone();
        let id = target.id.clone();
        let file = target.file.clone();
        PopupMenuItem::new(t!("environments.menu.reveal")).on_click(move |_, _, cx| {
            let path = weak
                .read_with(cx, |view, _| view.tab_file_path(&id, &file))
                .ok()
                .flatten();
            if let Some(path) = path {
                cx.reveal_path(&path);
            }
        })
    };
    menu = menu
        .separator()
        .item(copy_path)
        .item(reveal_sidebar)
        .item(reveal_file);

    if target.file != TabFile::Request {
        return menu;
    }
    let rename = {
        let weak = weak.clone();
        let id = target.id.clone();
        PopupMenuItem::new(t!("common.rename")).on_click(move |_, window, cx| {
            let name = state::format::tab_label(&id).to_string();
            open_rename_dialog(weak.clone(), id.clone(), name, window, cx);
        })
    };
    let duplicate = {
        let weak = weak.clone();
        let id = target.id.clone();
        PopupMenuItem::new(t!("shell.sidebar.duplicate")).on_click(move |_, _, cx| {
            let _ = weak.update(cx, |view, cx| view.duplicate(id.clone(), cx));
        })
    };
    menu.separator().item(rename).item(duplicate)
}

/// Builds the menu of the empty part of the tab bar: New request and Close all.
pub(crate) fn tab_bar_context_menu(weak: WeakEntity<AppView>, menu: PopupMenu) -> PopupMenu {
    let new_request = {
        let weak = weak.clone();
        PopupMenuItem::new(t!("common.new_request")).on_click(move |_, window, cx| {
            open_new_request_dialog(weak.clone(), None, window, cx);
        })
    };
    let close_all =
        PopupMenuItem::new(t!("shell.tabs.close_all")).on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.close_all_tabs(window, cx));
        });
    menu.item(new_request).item(close_all)
}
