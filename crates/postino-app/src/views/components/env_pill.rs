//! [`EnvPill`] and [`EnvMenuItem`]: the title bar's environment picker.
//!
//! Wraps gpui-kit's [`Button`] (the pill trigger) and [`PopupMenu`]/[`PopupMenuItem::element`]
//! (the dropdown): both take arbitrary child content, so the dot, check mark and shortcut hint
//! are laid out as plain `div`s colored from [`crate::theme::Palette`] inside them, rather than
//! building a whole custom popover from scratch.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::state::env_color::env_color;
use crate::theme::PaletteExt;
use crate::theme::metrics::{ENV_PILL_HEIGHT, MENU_DOT_COLUMN_WIDTH, MENU_ROW_HEIGHT, RADIUS_MD};

/// One row of an [`EnvPill`]'s dropdown: `None` is "No environment" (a hollow ring), `Some(name)`
/// is a named environment (a dot colored by [`env_color`]).
pub struct EnvMenuItem {
    name: Option<SharedString>,
    active: bool,
    shortcut: Option<SharedString>,
    on_click: Option<MenuItemHandler>,
}

/// A menu item click handler, factored out because clippy's `type_complexity` flags the inline
/// form.
type MenuItemHandler = Rc<dyn Fn(&mut Window, &mut App)>;

impl EnvMenuItem {
    /// The "No environment" row.
    pub fn none() -> Self {
        Self {
            name: None,
            active: false,
            shortcut: None,
            on_click: None,
        }
    }

    /// A row for the named environment `name`.
    pub fn named(name: impl Into<SharedString>) -> Self {
        Self {
            name: Some(name.into()),
            active: false,
            shortcut: None,
            on_click: None,
        }
    }

    /// Marks this row as the active environment (shows a check instead of its dot/ring).
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Sets the trailing shortcut hint, for example `"Ctrl 3"`.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Sets the click handler.
    #[allow(dead_code)] // The environment picker handles clicks through its own menu.
    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn label(&self) -> SharedString {
        self.name
            .clone()
            .unwrap_or_else(|| t!("common.no_environment").into_owned().into())
    }
}

/// The title bar's environment pill: a dot, the active environment's name, a chevron, on
/// `raised` with a border; opens an [`EnvMenuItem`] dropdown.
#[derive(IntoElement)]
pub struct EnvPill {
    id: ElementId,
    items: Vec<EnvMenuItem>,
}

impl EnvPill {
    /// A pill listing `items` in its dropdown. The active item's label (or "No environment")
    /// shows on the trigger itself.
    pub fn new(id: impl Into<ElementId>, items: Vec<EnvMenuItem>) -> Self {
        Self {
            id: id.into(),
            items,
        }
    }
}

impl RenderOnce for EnvPill {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        let active = self.items.iter().find(|item| item.active);
        let trigger_label = active
            .map(EnvMenuItem::label)
            .unwrap_or_else(|| t!("common.no_environment").into_owned().into());
        let trigger_dot = active
            .and_then(|item| item.name.as_deref())
            .map(|name| palette.env_color(env_color(name)));

        let mut trigger_dot_or_ring = div().flex_none().size(px(7.0)).rounded_full();
        trigger_dot_or_ring = match trigger_dot {
            Some(color) => trigger_dot_or_ring.bg(color),
            None => trigger_dot_or_ring
                .border_1()
                .border_color(palette.fg_subtle),
        };

        Button::new(self.id)
            .ghost()
            .h(px(ENV_PILL_HEIGHT))
            .px_2p5()
            .rounded(px(RADIUS_MD - 1.0))
            .border_1()
            .border_color(palette.border)
            .bg(palette.raised)
            .child(trigger_dot_or_ring)
            .child(trigger_label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for item in &self.items {
                    let color = item
                        .name
                        .as_deref()
                        .map(|name| palette.env_color(env_color(name)));
                    let label = item.label();
                    let active = item.active;
                    let shortcut = item.shortcut.clone();
                    let handler = item.on_click.clone();
                    let accent_text = palette.accent_text;
                    let fg_subtle = palette.fg_subtle;

                    let entry = PopupMenuItem::element(move |_, _| {
                        let mut leading = div()
                            .flex_none()
                            .w(px(MENU_DOT_COLUMN_WIDTH))
                            .flex()
                            .justify_center();
                        leading = if active {
                            leading
                                .child(Icon::new(IconName::Check).small().text_color(accent_text))
                        } else {
                            match color {
                                Some(color) => {
                                    leading.child(div().size(px(7.0)).rounded_full().bg(color))
                                }
                                None => leading.child(
                                    div()
                                        .size(px(7.0))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(fg_subtle),
                                ),
                            }
                        };

                        h_flex()
                            .h(px(MENU_ROW_HEIGHT))
                            .items_center()
                            .gap_2()
                            .px_2()
                            .w_full()
                            .child(leading)
                            .child(div().flex_1().child(label.clone()))
                            .when_some(shortcut.clone(), |row, shortcut| {
                                row.child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(fg_subtle)
                                        .child(shortcut),
                                )
                            })
                    })
                    .when_some(handler, |item, handler| {
                        item.on_click(move |_, window, cx| handler(window, cx))
                    });
                    menu = menu.item(entry);
                }
                menu
            })
    }
}
