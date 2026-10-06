//! The environment picker shown in the title bar: [`EnvPill`]/[`EnvMenuItem`] listing the
//! workspace's `environments/*.env` files plus "No environment", with `Ctrl 1..9`/`Ctrl 0`
//! shortcuts (`Cmd` on macOS) for the first nine environments.

use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::theme::metrics::ENV_PILL_HEIGHT;
use crate::views::components::{EnvMenuItem, EnvPill};

use super::root::AppView;

/// The modifier key label the shortcut hints show, matching `main.rs`'s key bindings: `Cmd` on
/// macOS, `Ctrl` elsewhere.
#[cfg(target_os = "macos")]
const MODIFIER_KEY: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
const MODIFIER_KEY: &str = "Ctrl";

/// How many environments get a `Ctrl 1..9` shortcut (the
/// first nine environments).
const SHORTCUT_COUNT: usize = 9;

impl AppView {
    /// Renders the title bar's environment picker.
    pub(crate) fn render_env_picker(
        &mut self,
        weak: WeakEntity<Self>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return Button::new("env-pill")
                .ghost()
                .h(px(ENV_PILL_HEIGHT))
                .label(t!("request.env.no_workspace"))
                .disabled(true)
                .into_any_element();
        };
        let environments = workspace.list_environments().unwrap_or_default();
        let active = self.state.active_environment.clone();

        let mut items = vec![
            EnvMenuItem::none()
                .active(active.is_none())
                .shortcut(format!("{MODIFIER_KEY}+0"))
                .on_click({
                    let weak = weak.clone();
                    move |_, cx| {
                        let _ = weak.update(cx, |view, cx| view.select_environment(None, cx));
                    }
                }),
        ];
        for (index, name) in environments.into_iter().enumerate() {
            let is_active = active.as_deref() == Some(name.as_str());
            let mut item = EnvMenuItem::named(name.clone()).active(is_active);
            if index < SHORTCUT_COUNT {
                item = item.shortcut(format!("{MODIFIER_KEY}+{}", index + 1));
            }
            let weak = weak.clone();
            item = item.on_click(move |_, cx| {
                let name = name.clone();
                let _ = weak.update(cx, |view, cx| view.select_environment(Some(name), cx));
            });
            items.push(item);
        }

        EnvPill::new("env-pill", items).into_any_element()
    }
}
