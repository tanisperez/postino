//! The environment picker shown in the title bar: a dropdown listing the workspace's
//! `environments/*.env` files (`plans/mvp.md`, section 3.4), plus a "No environment" option.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::root::AppView;

impl AppView {
    /// Renders the environment picker button and its dropdown menu.
    pub(crate) fn render_env_picker(
        &mut self,
        weak: WeakEntity<Self>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return Button::new("env-picker")
                .ghost()
                .small()
                .label("No workspace")
                .disabled(true)
                .into_any_element();
        };
        let environments = workspace.list_environments().unwrap_or_default();
        let active = self.state.active_environment.clone();
        let label = active
            .clone()
            .unwrap_or_else(|| "No environment".to_string());

        Button::new("env-picker")
            .ghost()
            .small()
            .icon(Icon::new(IconName::Globe).small())
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                let none_weak = weak.clone();
                menu = menu.item(
                    PopupMenuItem::new("No environment")
                        .checked(active.is_none())
                        .on_click(move |_, _, cx| {
                            let _ = none_weak.update(cx, |view, cx| {
                                view.state.active_environment = None;
                                cx.notify();
                            });
                        }),
                );
                if !environments.is_empty() {
                    menu = menu.separator();
                }
                for name in &environments {
                    let select_weak = weak.clone();
                    let name = name.clone();
                    let checked = active.as_deref() == Some(name.as_str());
                    menu = menu.item(PopupMenuItem::new(name.clone()).checked(checked).on_click(
                        move |_, _, cx| {
                            let name = name.clone();
                            let _ = select_weak.update(cx, |view, cx| {
                                view.state.active_environment = Some(name);
                                cx.notify();
                            });
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }
}
