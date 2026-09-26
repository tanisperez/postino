//! [`UnderlineTabs`] and [`UnderlineTabItem`]: the request editor's Params/Headers/Body/... row
//! and the response viewer's Body/Headers/Tests/Console row (`plans/ui-redesign.md` phase 3,
//! design reference `Components.dc.html` "Tabs" and `Main A.dc.html`'s two inner tab rows).
//!
//! Custom element: the active tab's 2 px accent underline is drawn with an inset `BoxShadow`
//! (the same CSS `inset 0 -2px 0 accent` trick the design itself uses), which gpui-kit's
//! `Tab`/`TabBar` `.underline()` variant does not expose as an overridable color, so this is a
//! plain `div` row colored from [`crate::theme::Palette`].

use std::rc::Rc;

use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::theme::PaletteExt;
use crate::theme::metrics::REQUEST_INNER_TABS_HEIGHT;

/// One item of an [`UnderlineTabs`] row.
pub struct UnderlineTabItem {
    label: SharedString,
    count: Option<SharedString>,
    count_color: Option<Hsla>,
    selected: bool,
    on_click: Option<ItemHandler>,
}

/// A tab click handler, factored out because clippy's `type_complexity` flags the inline form.
type ItemHandler = Rc<dyn Fn(&mut Window, &mut App)>;

impl UnderlineTabItem {
    /// A new item labeled `label`.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            count: None,
            count_color: None,
            selected: false,
            on_click: None,
        }
    }

    /// Shows `count` after the label, in `fg_subtle` unless [`Self::count_color`] overrides it
    /// (for example the Tests tab's pass/fail count).
    pub fn count(mut self, count: impl Into<SharedString>) -> Self {
        self.count = Some(count.into());
        self
    }

    /// Overrides the count's color (for example `success` when every test passed).
    #[allow(dead_code)] // wired by phase 5, once UnderlineTabs backs the request/response tab rows
    pub fn count_color(mut self, color: Hsla) -> Self {
        self.count_color = Some(color);
        self
    }

    /// Marks this item as the active tab.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the click handler.
    #[allow(dead_code)] // wired by phase 5, once UnderlineTabs backs the request/response tab rows
    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

/// A row of underline tabs: 18 px gaps, a 1 px bottom border, a 2 px accent underline on the
/// active item (`Components.dc.html` "Tabs").
#[derive(IntoElement)]
pub struct UnderlineTabs {
    id: ElementId,
    items: Vec<UnderlineTabItem>,
    height: f32,
}

impl UnderlineTabs {
    /// An empty tab row; add items with [`Self::item`]. Defaults to
    /// [`REQUEST_INNER_TABS_HEIGHT`]; override with [`Self::height`] for the response viewer's
    /// 36 px row.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            height: REQUEST_INNER_TABS_HEIGHT,
        }
    }

    /// Appends one item.
    pub fn item(mut self, item: UnderlineTabItem) -> Self {
        self.items.push(item);
        self
    }

    /// Overrides the row's height.
    #[allow(dead_code)] // wired by phase 5, once UnderlineTabs backs the request/response tab rows
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

impl RenderOnce for UnderlineTabs {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        let height = self.height;

        h_flex()
            .id(self.id)
            .gap(px(18.0))
            .border_b_1()
            .border_color(palette.border)
            .children(self.items.into_iter().enumerate().map(|(index, item)| {
                let selected = item.selected;
                let handler = item.on_click;
                let count_color = item.count_color.unwrap_or(palette.fg_subtle);
                let mut tab = h_flex()
                    .id(("underline-tab", index))
                    .h(px(height))
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .text_size(px(13.0))
                    .text_color(if selected {
                        palette.fg
                    } else {
                        palette.fg_muted
                    })
                    .font_weight(if selected {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .when(selected, |tab| {
                        tab.shadow(vec![
                            BoxShadow::new(px(0.0), px(-2.0), palette.accent).inset(),
                        ])
                    })
                    .child(item.label);
                if let Some(count) = item.count {
                    tab = tab.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(count_color)
                            .child(count),
                    );
                }
                if let Some(handler) = handler {
                    tab = tab.on_click(move |_, window, cx| handler(window, cx));
                }
                tab
            }))
    }
}
