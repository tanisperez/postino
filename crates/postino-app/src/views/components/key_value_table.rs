//! [`KeyValueTable`] and [`KeyValueRow`]: the Params/Headers/Form body editor table.
//!
//! Wraps gpui-kit's [`Checkbox`] for the enabled column; the grid itself (header row, column
//! widths, the disabled-row strikethrough) is a plain `div` layout colored from
//! [`crate::theme::Palette`], since there is no gpui-kit table widget shaped like this one (a
//! fixed four-column grid with a trailing "Add" row, not a scrollable data table).
//!
//! A row's key/value cell is either plain text ([`KeyValueRow::new`], used by the components
//! gallery) or an arbitrary element ([`KeyValueRow::with_elements`]), which is how the request editor wires
//! this table to the request editor's live `Input` entities: the disabled-row strikethrough only
//! applies to the text variant, since a caller-supplied element (an `Input`) owns its own text
//! styling.

use std::rc::Rc;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::state::request_edit::{SortColumn, SortOrder};
use crate::theme::PaletteExt;
use crate::theme::metrics::{
    CHECKBOX_HALO_PADDING, KEY_VALUE_CELL_GAP, KEY_VALUE_ROW_HEIGHT, KEY_VALUE_SIDE_COL_WIDTH,
    RADIUS_MD, RADIUS_SM,
};

/// A [`KeyValueRow`]'s key or value cell: plain text (the gallery demo) or a caller-supplied
/// element (a live `Input`, wired in by the request editor).
enum Cell {
    Text(SharedString),
    Element(AnyElement),
}

/// One row of a [`KeyValueTable`].
pub struct KeyValueRow {
    key: Cell,
    value: Cell,
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
    /// A new, enabled row showing `key` and `value` as plain text.
    pub fn new(key: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            key: Cell::Text(key.into()),
            value: Cell::Text(value.into()),
            enabled: true,
            on_toggle: None,
            on_delete: None,
        }
    }

    /// A new, enabled row whose key and value cells render `key` and `value` as given, for
    /// example a live `Input` bound to the request editor's own entity, instead of static text.
    pub fn with_elements(key: impl IntoElement, value: impl IntoElement) -> Self {
        Self {
            key: Cell::Element(key.into_any_element()),
            value: Cell::Element(value.into_any_element()),
            enabled: true,
            on_toggle: None,
            on_delete: None,
        }
    }

    /// Sets whether the row is enabled: a disabled row shows `fg_subtle` with a struck-through
    /// key (text cells only; an element cell is dimmed instead, see this module's doc comment),
    /// and is skipped when resolving the request.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the checkbox's change handler.
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }

    /// Sets the delete button's click handler.
    pub fn on_delete(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_delete = Some(Rc::new(handler));
        self
    }
}

/// A header click handler, called with the column to sort by, see [`ToggleHandler`].
type SortHandler = Rc<dyn Fn(SortColumn, &mut Window, &mut App)>;

/// A key-value table: header row on `surface`, a checkbox column (30), key (1fr), value (1.4fr)
/// and a delete column (30), plus a trailing "Add" row. With [`Self::on_sort`], the Key and Value
/// headers sort the rows when clicked, and [`Self::sort`] shows an arrow next to the sorted one.
#[derive(IntoElement)]
pub struct KeyValueTable {
    id: ElementId,
    rows: Vec<KeyValueRow>,
    add_label: SharedString,
    on_add: Option<RowHandler>,
    sort: Option<(SortColumn, SortOrder)>,
    on_sort: Option<SortHandler>,
}

impl KeyValueTable {
    /// An empty table; add rows with [`Self::row`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            rows: Vec::new(),
            add_label: t!("request.kv.add").into_owned().into(),
            on_add: None,
            sort: None,
            on_sort: None,
        }
    }

    /// Appends one row.
    pub fn row(mut self, row: KeyValueRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Overrides the trailing row's label (defaults to the translated "Add").
    #[allow(dead_code)] // no caller needs a non-default label yet
    pub fn add_label(mut self, label: impl Into<SharedString>) -> Self {
        self.add_label = label.into();
        self
    }

    /// Sets the trailing row's click handler.
    pub fn on_add(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_add = Some(Rc::new(handler));
        self
    }

    /// Shows an arrow next to the header of the column the rows are sorted by.
    pub fn sort(mut self, sort: Option<(SortColumn, SortOrder)>) -> Self {
        self.sort = sort;
        self
    }

    /// Makes the Key and Value headers clickable: a click calls `handler` with that column.
    pub fn on_sort(
        mut self,
        handler: impl Fn(SortColumn, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_sort = Some(Rc::new(handler));
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
            .child(header_cell(
                SortColumn::Key,
                t!("request.kv.key").into(),
                1.0,
                self.sort,
                self.on_sort.clone(),
                palette.fg,
            ))
            .child(header_cell(
                SortColumn::Value,
                t!("common.value").into(),
                1.4,
                self.sort,
                self.on_sort.clone(),
                palette.fg,
            ))
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
            // One step above the usual `hover` and `pressed`, which barely show on an area this
            // small.
            let (hover, pressed) = (palette.pressed, palette.border_strong);

            let checkbox_cell = div().flex_none().w(side_col).flex().justify_center().child(
                // A halo around the box on hover and while pressed. gpui-component's checkbox
                // takes its box colors from the theme, so the box itself cannot change.
                Checkbox::new(("kv-enabled", index))
                    .p(px(CHECKBOX_HALO_PADDING))
                    .rounded(px(RADIUS_SM))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(pressed))
                    .checked(enabled)
                    .when_some(toggle_handler, |checkbox, handler| {
                        checkbox.on_click(move |checked, window, cx| handler(*checked, window, cx))
                    }),
            );
            // Half the gap on each side of the split, so the key and the value fields never touch.
            let half_gap = px(KEY_VALUE_CELL_GAP / 2.0);
            let key_cell = key_value_cell(row.key, 1.0, text_color, enabled)
                .pr(half_gap)
                .into_any_element();
            let value_cell = key_value_cell(row.value, 1.4, text_color, enabled)
                .pl(half_gap)
                .into_any_element();
            let delete_cell = div()
                .id(("kv-delete", index))
                .flex_none()
                .w(side_col)
                .flex()
                .justify_center()
                .cursor_pointer()
                .rounded(px(4.0))
                .hover(|style| style.bg(palette.hover))
                .active(|style| style.bg(palette.pressed))
                .child(
                    Icon::new(IconName::Delete)
                        .small()
                        .text_color(palette.fg_subtle),
                )
                .when_some(delete_handler, |cell, handler| {
                    cell.on_click(move |_, window, cx| handler(window, cx))
                });

            h_flex()
                .h(px(KEY_VALUE_ROW_HEIGHT))
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
                .hover(|style| style.bg(palette.hover))
                .active(|style| style.bg(palette.pressed))
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

/// Renders the Key or Value header, `flex_grow`-sized by `grow` with a zero basis like the cells
/// below it, so an arrow never shifts the split. With `on_sort` it is clickable and hovers in
/// `hover_color`; the arrow shows when `sort` names this column.
fn header_cell(
    column: SortColumn,
    label: SharedString,
    grow: f32,
    sort: Option<(SortColumn, SortOrder)>,
    on_sort: Option<SortHandler>,
    hover_color: Hsla,
) -> AnyElement {
    let arrow = sort
        .filter(|(sorted, _)| *sorted == column)
        .map(|(_, order)| match order {
            SortOrder::Ascending => gpui_kit::assets::IconName::ArrowUp,
            SortOrder::Descending => gpui_kit::assets::IconName::ArrowDown,
        });
    let id = match column {
        SortColumn::Key => "kv-sort-key",
        SortColumn::Value => "kv-sort-value",
    };
    h_flex()
        .id(id)
        .flex_grow(grow)
        .flex_basis(px(0.0))
        .min_w_0()
        .h_full()
        .items_center()
        .gap_1()
        .child(label)
        .children(arrow.map(|icon| Icon::new(icon).xsmall()))
        .when_some(on_sort, |cell, handler| {
            cell.cursor_pointer()
                .hover(|style| style.text_color(hover_color))
                .on_click(move |_, window, cx| handler(column, window, cx))
        })
        .into_any_element()
}

/// Renders one key or value cell, `flex_grow`-sized by `grow`. A text cell is colored and
/// struck through when disabled; an element cell (a live `Input`)
/// keeps its own styling and is only dimmed when disabled, since it cannot be recolored or
/// struck through generically.
fn key_value_cell(cell: Cell, grow: f32, text_color: Hsla, enabled: bool) -> Div {
    match cell {
        Cell::Text(text) => div()
            .flex_grow(grow)
            .text_color(text_color)
            .when(!enabled, |cell| cell.line_through())
            .child(text),
        // A zero basis keeps the 1 : 1.4 split whatever the element's own width, which a row of
        // variable chips (not shrinkable, unlike an `Input`) would otherwise skew.
        Cell::Element(element) => div()
            .flex_grow(grow)
            .flex_basis(px(0.0))
            .min_w_0()
            .when(!enabled, |cell| cell.opacity(0.5))
            .child(element),
    }
}
