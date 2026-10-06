//! [`DocumentTab`] and [`DocumentTabs`]: the open-tab strip above the request editor
//! (`plans/ui-redesign.md` phase 3).
//!
//! Custom element, for the same reason as [`super::underline_tabs::UnderlineTabs`]: the active
//! tab's inset accent line needs an explicit `BoxShadow`, and the design's colors (transparent
//! or `bg` background, `fg`/`fg_muted` text) are simplest to pin directly from the palette on a
//! plain `div` row. The dirty dot / hover-reveals-close-`x` behavior uses gpui's `group_hover`,
//! toggling opacity between the two rather than swapping elements.
//!
//! Tabs reorder by dragging, like any editor: dropping a tab on another puts it in that tab's
//! place (an accent line on the side it will land), dropping it on the bar's empty space moves it
//! to the end. The bar only reports the move ([`DocumentTabs::on_reorder`]).

use std::rc::Rc;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use postino_core::Method;

use super::method_badge::MethodBadge;
use crate::theme::PaletteExt;
use crate::theme::metrics::{OPEN_TABS_BAR_HEIGHT, RADIUS_SM};

/// One open tab.
pub struct DocumentTab {
    icon: Option<Icon>,
    method: Option<Method>,
    label: SharedString,
    dirty: bool,
    selected: bool,
    tooltip: Option<SharedString>,
    on_click: Option<TabHandler>,
    on_close: Option<TabHandler>,
}

/// A tab click handler, factored out because clippy's `type_complexity` flags the inline form.
type TabHandler = Rc<dyn Fn(&mut Window, &mut App)>;
/// A reorder handler, called with the dragged tab's index and the index it moves to.
type ReorderHandler = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;

/// The value carried while a tab is dragged, also rendered as the preview under the pointer.
#[derive(Clone)]
struct DraggedTab {
    index: usize,
    method: Option<Method>,
    label: SharedString,
}

impl Render for DraggedTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        h_flex()
            .h(px(OPEN_TABS_BAR_HEIGHT))
            .items_center()
            .gap(px(6.0))
            .px_3()
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(palette.border_strong)
            .bg(palette.bg)
            .shadow(palette.shadow.clone())
            .opacity(0.9)
            .text_size(px(13.0))
            .text_color(palette.fg)
            .children(self.method.clone().map(MethodBadge::inline))
            .child(self.label.clone())
    }
}

impl DocumentTab {
    /// A new tab labeled `label` (the request's file stem, or `"Load test \u{b7} <name>"`).
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            method: None,
            label: label.into(),
            dirty: false,
            selected: false,
            tooltip: None,
            on_click: None,
            on_close: None,
        }
    }

    /// Sets a leading icon (for example `gauge` for a load test tab).
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Shows `method`'s [`MethodBadge`] `inline` variant before the tab's name (a request tab;
    /// omitted for a load test tab).
    pub fn method(mut self, method: Method) -> Self {
        self.method = Some(method);
        self
    }

    /// Marks the tab as having unsaved changes: a filled dot instead of a close `x`, until
    /// hovered.
    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    /// Marks this tab as the active one.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Sets a hover tooltip, for example the request's full workspace-relative id (the tab's own
    /// label is only its file stem, `plans/ui-redesign.md` section 2.3 point 3).
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Sets the handler for clicking the tab itself (activates it).
    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Sets the handler for the close button (or the dirty dot, once hovered).
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

/// The open-tabs bar: one [`DocumentTab`] per open request or load test, plus the caller's own
/// trailing "+" button (added with [`Self::suffix`]).
#[derive(IntoElement)]
pub struct DocumentTabs {
    id: ElementId,
    items: Vec<DocumentTab>,
    suffix: Option<AnyElement>,
    on_reorder: Option<ReorderHandler>,
}

impl DocumentTabs {
    /// An empty tab bar; add tabs with [`Self::item`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            suffix: None,
            on_reorder: None,
        }
    }

    /// Appends one tab.
    pub fn item(mut self, item: DocumentTab) -> Self {
        self.items.push(item);
        self
    }

    /// Makes the tabs draggable: `handler` gets the dragged tab's index and the index it moves to
    /// (`usize::MAX` for "the end", when dropped on the bar's empty space).
    pub fn on_reorder(
        mut self,
        handler: impl Fn(usize, usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_reorder = Some(Rc::new(handler));
        self
    }

    /// Sets the trailing element after the last tab (the "+" new-request button).
    pub fn suffix(mut self, suffix: impl IntoElement) -> Self {
        self.suffix = Some(suffix.into_any_element());
        self
    }
}

impl RenderOnce for DocumentTabs {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        let on_reorder = self.on_reorder;
        let bar_reorder = on_reorder.clone();

        h_flex()
            .id(self.id)
            .h(px(OPEN_TABS_BAR_HEIGHT))
            .bg(palette.surface)
            .border_b_1()
            .border_color(palette.border)
            .when_some(bar_reorder, |bar, handler| {
                bar.on_drop(move |dragged: &DraggedTab, window, cx| {
                    handler(dragged.index, usize::MAX, window, cx)
                })
            })
            .children(self.items.into_iter().enumerate().map(|(index, item)| {
                let group = SharedString::from(format!("doc-tab-{index}"));
                let selected = item.selected;
                let dirty = item.dirty;
                let click_handler = item.on_click;
                let close_handler = item.on_close;
                let middle_click_close = close_handler.clone();

                let mut tab = h_flex()
                    .id(("doc-tab", index))
                    .group(group.clone())
                    .h_full()
                    .items_center()
                    .gap(px(6.0))
                    .px_3()
                    .border_r_1()
                    .border_color(palette.border)
                    .cursor_pointer()
                    .hover(|style| style.bg(palette.hover))
                    .active(|style| style.bg(palette.pressed))
                    .text_size(px(13.0))
                    .text_color(if selected {
                        palette.fg
                    } else {
                        palette.fg_muted
                    })
                    .when(selected, |tab| {
                        tab.bg(palette.bg).shadow(vec![
                            BoxShadow::new(px(0.0), px(2.0), palette.accent).inset(),
                        ])
                    });
                if let Some(icon) = item.icon {
                    tab = tab.child(Icon::new(icon).small().text_color(palette.fg_subtle));
                }
                if let Some(handler) = on_reorder.clone() {
                    let dragged = DraggedTab {
                        index,
                        method: item.method.clone(),
                        label: item.label.clone(),
                    };
                    let accent = palette.accent;
                    tab = tab
                        .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
                        // The line marks where the dragged tab lands: before this tab when it
                        // comes from the right, after it when it comes from the left.
                        .drag_over::<DraggedTab>(move |style, dragged, _, _| {
                            if dragged.index > index {
                                style.border_l_2().border_color(accent)
                            } else if dragged.index < index {
                                style.border_r_2().border_color(accent)
                            } else {
                                style
                            }
                        })
                        .on_drop(move |dragged: &DraggedTab, window, cx| {
                            handler(dragged.index, index, window, cx)
                        });
                }
                if let Some(method) = item.method {
                    tab = tab.child(MethodBadge::inline(method));
                }
                tab = tab.child(item.label);
                if let Some(tooltip) = item.tooltip {
                    tab = tab
                        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
                }
                if let Some(handler) = click_handler {
                    tab = tab.on_click(move |_, window, cx| handler(window, cx));
                }
                // A middle click closes the tab, the same as its close button.
                if let Some(handler) = middle_click_close {
                    tab = tab.on_mouse_up(MouseButton::Middle, move |_, window, cx| {
                        handler(window, cx)
                    });
                }

                let mut end = div()
                    .id(("doc-tab-end", index))
                    .relative()
                    .size(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .rounded(px(4.0))
                    .hover(|style| style.bg(palette.hover))
                    .active(|style| style.bg(palette.pressed));
                if dirty {
                    end = end
                        .child(
                            div()
                                .size(px(8.0))
                                .rounded_full()
                                .bg(palette.fg_subtle)
                                .group_hover(group.clone(), |style| style.opacity(0.0)),
                        )
                        .child(
                            div()
                                .absolute()
                                .opacity(0.0)
                                .group_hover(group.clone(), |style| style.opacity(1.0))
                                .child(
                                    Icon::new(IconName::Close)
                                        .small()
                                        .text_color(palette.fg_subtle),
                                ),
                        );
                } else {
                    end = end.child(
                        Icon::new(IconName::Close)
                            .small()
                            .text_color(palette.fg_subtle),
                    );
                }
                if let Some(handler) = close_handler {
                    end = end.on_click(move |_, window, cx| handler(window, cx));
                }
                tab.child(end)
            }))
            .children(self.suffix)
    }
}
