//! [`KeyValueTable`] and [`KeyValueRow`]: the Params/Headers/Form body editor table
//! (`plans/ui-redesign.md` phase 3, design reference `Components.dc.html` "Key-value table").
//!
//! Wraps gpui-kit's [`Checkbox`] for the enabled column; the grid itself (header row, column
//! widths, the disabled-row strikethrough) is a plain `div` layout colored from
//! [`crate::theme::Palette`], since there is no gpui-kit table widget shaped like this one (a
//! fixed four-column grid with a trailing "Add" row, not a scrollable data table).

use std::rc::Rc;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::theme::PaletteExt;
use crate::theme::metrics::{KEY_VALUE_SIDE_COL_WIDTH, RADIUS_MD};

/// One row of a [`KeyValueTable`].
pub struct KeyValueRow {
    key: SharedString,
    value: SharedString,
    enabled: bool,
    on_toggle: Option<ToggleHandler>,
    on_delete: Option<RowHandler>,
}

/// A checkbox toggle handler, factored out because clippy's `type_complexity` flags the inline
/// form.
type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;
/// A plain row click handler (delete, add), see [`ToggleHandler`].
type RowHandler = Rc<dyn Fn(&mut Window, &mut App)>;

impl KeyValueRow {
    /// A new, enabled row.
    pub fn new(key: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            enabled: true,
            on_toggle: None,
            on_delete: None,
        }
    }

    /// Sets whether the row is enabled: a disabled row shows `fg_subtle` with a struck-through
    /// key, and is skipped when resolving the request.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the checkbox's change handler.
    #[allow(dead_code)] // wired by phase 5, once KeyValueTable backs Params/Headers/Form
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }

    /// Sets the delete button's click handler.
    #[allow(dead_code)] // wired by phase 5, once KeyValueTable backs Params/Headers/Form
    pub fn on_delete(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_delete = Some(Rc::new(handler));
        self
    }
}

/// A key-value table: header row on `surface`, a checkbox column (30), key (1fr), value (1.4fr)
/// and a delete column (30), plus a trailing "Add" row.
#[derive(IntoElement)]
pub struct KeyValueTable {
    id: ElementId,
    rows: Vec<KeyValueRow>,
    add_label: SharedString,
    on_add: Option<RowHandler>,
}

impl KeyValueTable {
    /// An empty table; add rows with [`Self::row`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            rows: Vec::new(),
            add_label: "Add".into(),
            on_add: None,
        }
    }

    /// Appends one row.
    pub fn row(mut self, row: KeyValueRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Overrides the trailing row's label (defaults to `"Add"`).
    #[allow(dead_code)] // wired by phase 5, once KeyValueTable backs Params/Headers/Form
    pub fn add_label(mut self, label: impl Into<SharedString>) -> Self {
        self.add_label = label.into();
        self
    }

    /// Sets the trailing row's click handler.
    #[allow(dead_code)] // wired by phase 5, once KeyValueTable backs Params/Headers/Form
    pub fn on_add(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_add = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for KeyValueTable {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();
        let side_col = px(KEY_VALUE_SIDE_COL_WIDTH);

        let header = h_flex()
            .h(px(28.0))
            .items_center()
            .bg(palette.surface)
            .border_b_1()
            .border_color(palette.border)
            .text_size(px(11.5))
            .text_color(palette.fg_subtle)
            .child(div().flex_none().w(side_col))
            .child(div().flex_grow(1.0).child("Key"))
            .child(div().flex_grow(1.4).child("Value"))
            .child(div().flex_none().w(side_col));

        let rows = self.rows.into_iter().enumerate().map(|(index, row)| {
            let enabled = row.enabled;
            let toggle_handler = row.on_toggle;
            let delete_handler = row.on_delete;
            let text_color = if enabled {
                palette.fg
            } else {
                palette.fg_subtle
            };

            let checkbox_cell = div().flex_none().w(side_col).flex().justify_center().child(
                Checkbox::new(("kv-enabled", index))
                    .checked(enabled)
                    .when_some(toggle_handler, |checkbox, handler| {
                        checkbox.on_click(move |checked, window, cx| handler(*checked, window, cx))
                    }),
            );
            let key_cell = div()
                .flex_grow(1.0)
                .text_color(text_color)
                .when(!enabled, |cell| cell.line_through())
                .child(row.key);
            let value_cell = div().flex_grow(1.4).text_color(text_color).child(row.value);
            let delete_cell = div()
                .id(("kv-delete", index))
                .flex_none()
                .w(side_col)
                .flex()
                .justify_center()
                .cursor_pointer()
                .child(
                    Icon::new(IconName::Delete)
                        .small()
                        .text_color(palette.fg_subtle),
                )
                .when_some(delete_handler, |cell, handler| {
                    cell.on_click(move |_, window, cx| handler(window, cx))
                });

            h_flex()
                .h(px(30.0))
                .items_center()
                .border_b_1()
                .border_color(palette.border)
                .font_family(mono_font.clone())
                .text_size(px(12.5))
                .child(checkbox_cell)
                .child(key_cell)
                .child(value_cell)
                .child(delete_cell)
        });

        let add_row = {
            let mut row = h_flex()
                .id("kv-add-row")
                .h(px(30.0))
                .items_center()
                .px(side_col)
                .gap_2()
                .cursor_pointer()
                .text_color(palette.fg_subtle)
                .text_size(px(12.5))
                .child(Icon::new(IconName::Plus).small())
                .child(self.add_label);
            if let Some(handler) = self.on_add {
                row = row.on_click(move |_, window, cx| handler(window, cx));
            }
            row
        };

        v_flex()
            .id(self.id)
            .border_1()
            .border_color(palette.border)
            .rounded(px(RADIUS_MD))
            .overflow_hidden()
            .child(header)
            .children(rows)
            .child(add_row)
    }
}
