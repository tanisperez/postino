//! Context menus opened from the keyboard (#232): `Shift+F10` and the menu key open the menu a
//! right click would, on the environment selected in the Environments panel while that panel is
//! shown, else on the active tab.
//!
//! gpui-component's `context_menu` only opens on a right click (or a long press), at the
//! pointer. This builds the same menu with the same builder (`tab_context_menu`,
//! `env_context_menu`) as a `PopupMenu` of its own, anchored under the tab or the row whose
//! bounds the last frame recorded ([`AppView::tab_bounds`], [`AppView::env_row_bounds`]). The menu
//! takes the focus, so the arrow keys, Enter and Escape work in it, and gives it back to what
//! had it when it closes.

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::env_panel::env_context_menu;
use super::root::AppView;
use super::tab_menu::tab_context_menu;
use crate::actions::OpenContextMenu;

/// Drawn over everything else in the window, like gpui-component's own popups.
const MENU_PRIORITY: usize = 1;

/// A context menu opened from the keyboard, while it is open.
pub(crate) struct KeyboardMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _dismiss: Subscription,
}

impl AppView {
    /// Handles `Shift+F10` and the menu key (see `main.rs`'s `bind_keys`).
    pub(crate) fn on_open_context_menu(
        &mut self,
        _: &OpenContextMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let weak = cx.weak_entity();
        // The Environments panel's selected row records its bounds only while the panel shows.
        if let Some(bounds) = self.env_row_bounds.get()
            && let Some(name) = self.state.tabs.active_environment_name()
            && let Some(index) = self.env_rows.iter().position(|row| row.name == name)
            && let Some(target) = self.env_menu_target(index)
        {
            self.open_keyboard_menu(
                bounds.bottom_left(),
                move |menu, window, cx| env_context_menu(weak.clone(), &target, menu, window, cx),
                window,
                cx,
            );
            return;
        }
        let Some(index) = self.state.tabs.active_index() else {
            return;
        };
        // The welcome tab has no menu, as with a right click.
        let welcome = self
            .state
            .tabs
            .open_tabs()
            .get(index)
            .is_some_and(|tab| tab.welcome().is_some());
        if welcome {
            return;
        }
        if let (Some(bounds), Some(target)) = (self.tab_bounds.get(), self.tab_menu_target(index)) {
            self.open_keyboard_menu(
                bounds.bottom_left(),
                move |menu, _, _| tab_context_menu(weak.clone(), &target, menu),
                window,
                cx,
            );
        }
    }

    /// Opens the menu `build` makes at `position`, with the focus in it.
    fn open_keyboard_menu(
        &mut self,
        position: Point<Pixels>,
        build: impl FnOnce(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Where the focus goes back when the menu closes. Without it nothing would be focused,
        // and every window shortcut would stop working until the next click.
        let previous = window
            .focused(cx)
            .unwrap_or_else(|| self.focus_handle.clone());
        let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
            build(menu, window, cx).action_context(previous)
        });
        menu.focus_handle(cx).focus(window, cx);
        let dismiss = cx.subscribe_in(&menu, window, |view, _, _: &DismissEvent, _, cx| {
            view.keyboard_menu = None;
            cx.notify();
        });
        self.keyboard_menu = Some(KeyboardMenu {
            menu,
            position,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    /// The open keyboard menu, over the window.
    pub(crate) fn render_keyboard_menu(&self) -> Option<AnyElement> {
        let open = self.keyboard_menu.as_ref()?;
        Some(
            deferred(
                anchored()
                    .position(open.position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(open.menu.clone()),
            )
            .with_priority(MENU_PRIORITY)
            .into_any_element(),
        )
    }
}
