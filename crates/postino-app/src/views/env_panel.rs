//! The Environments panel of the sidebar (GitHub #64): the workspace's environments with their
//! color dot, an "Active" tag and variable count, an inline "new environment" row, and a footer
//! that opens the `environments/` folder. The rows come from `state::env_panel`, loaded when the
//! workspace or an environment changes ([`AppView::refresh_environments`]), never in render.
//!
//! Clicking an environment row opens its editor tab (#65, `views/env_editor.rs`), and the row of
//! the environment shown by the active tab is tinted. The "Active" tag follows the active
//! environment, which the title bar's picker and the editor's "Use this environment" change.
//! The "No environment" row deactivates it.
//!
//! Environments reorder by dragging their rows by the grip that shows on hover, like tabs: an
//! accent line shows where the dragged one lands. Dropping it on "No environment" makes it the
//! first, on the space below the list the last. "Move up" and "Move down" in the editor's menu
//! do the same from the keyboard. The order is saved to `.postino/project.toml` (#216).
//!
//! A right click on an environment opens its menu (#231, the design's "Context Menus" page):
//! open, use, color, move, rename, duplicate, reveal and delete, all acting on that row. The
//! empty part of the panel offers "New environment".

use std::path::PathBuf;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::actions::{MoveEnvironmentDown, MoveEnvironmentUp};
use crate::state::env_color::{EnvColor, auto_color};
use crate::state::env_panel::{load_env_rows, moved_by, moved_order};
use crate::state::number::format_integer;
use crate::state::workspace_log::log_workspace_error;
use crate::theme::metrics::{ENV_ROW_HEIGHT, ICON_BUTTON_HEADER, INPUT_PADDING_X, RADIUS_SM};
use crate::theme::{Palette, PaletteExt, PostinoColors};
use crate::views::components::{IconButton, SectionLabel, edit_menu};

use super::root::AppView;

/// The folder the panel lists, relative to the workspace root.
const ENVIRONMENTS_FOLDER: &str = "environments";

/// The width of the grip slot at the start of every row, so the dots line up.
const GRIP_WIDTH: f32 = 10.0;

/// The group name of an environment row, for the grip that shows on hover.
const ROW_GROUP: &str = "env-row";

/// The value carried while an environment row is dragged, also rendered as the preview under
/// the pointer.
#[derive(Clone)]
struct DraggedEnvironment {
    /// The row's index in [`AppView::env_rows`].
    index: usize,
    name: SharedString,
    color: Hsla,
}

impl Render for DraggedEnvironment {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        h_flex()
            .h(px(ENV_ROW_HEIGHT))
            .items_center()
            .gap_2()
            .px(px(8.0))
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(palette.border_strong)
            .bg(palette.overlay)
            .shadow(palette.shadow.clone())
            .text_color(palette.fg)
            .child(
                Icon::new(gpui_kit::assets::IconName::GripVertical)
                    .with_size(px(12.0))
                    .text_color(palette.fg_muted),
            )
            .child(
                div()
                    .flex_none()
                    .size(px(7.0))
                    .rounded_full()
                    .bg(self.color),
            )
            .child(self.name.clone())
    }
}

/// The 2 px accent line inside a row's top edge (`top`) or bottom edge, where a dragged
/// environment lands.
fn drop_line(accent: Hsla, top: bool) -> Vec<BoxShadow> {
    let offset = if top { 2.0 } else { -2.0 };
    vec![BoxShadow::new(px(0.0), px(offset), accent).inset()]
}

/// The empty grip slot of the rows that cannot be dragged.
fn grip_spacer() -> Div {
    div().flex_none().w(px(GRIP_WIDTH))
}

impl AppView {
    /// Reloads [`AppView::env_rows`] and the active environment's variables from disk. Call when
    /// the workspace opens, an environment is created or one of its variables changes, and when
    /// the panel is opened.
    pub(crate) fn refresh_environments(&mut self) {
        let (rows, colors) = self
            .state
            .workspace
            .as_ref()
            .map(load_env_rows)
            .unwrap_or_default();
        self.env_rows = rows;
        self.state.env_colors = colors;
        self.state.reload_environment();
    }

    /// Shows the inline "new environment" input and focuses it. A no-op without a workspace.
    pub(crate) fn begin_new_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.workspace.is_none() {
            return;
        }
        if let Some(input) = &self.new_env_input {
            input.update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("navigation.env.name_placeholder"))
        });
        cx.subscribe_in(
            &input,
            window,
            |view, entity, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let name = entity.read(cx).value().trim().to_string();
                    view.commit_new_environment(name, window, cx);
                }
                // Ignore the blur of an input that was already replaced or committed.
                InputEvent::Blur if view.new_env_input.as_ref() == Some(entity) => {
                    view.cancel_new_environment(window, cx);
                }
                _ => {}
            },
        )
        .detach();
        input.update(cx, |input, cx| input.focus(window, cx));
        self.new_env_input = Some(input);
        cx.notify();
    }

    /// Hides the inline input without creating anything and gives the focus back to the window.
    fn cancel_new_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_env_input = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Creates the environment `name` (`Workspace::create_environment`), selects it and hides
    /// the inline input. An empty name just cancels; a failure shows in the error banner.
    fn commit_new_environment(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_env_input = None;
        self.focus_handle.focus(window, cx);
        if name.is_empty() {
            cx.notify();
            return;
        }
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        match workspace.create_environment(&name) {
            Ok(()) => {
                log::debug!("created environment {name}");
                self.workspace_error = None;
                self.refresh_environments();
                self.select_environment(Some(name), cx);
            }
            Err(error) => {
                log_workspace_error(&format!("create environment {name:?}"), &error);
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    /// Moves the environment at `from` of [`AppView::env_rows`] to `to` (past the end for the
    /// last place) and saves the new order, which the picker and the `Ctrl 1..9` shortcuts follow.
    fn reorder_environment(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let names: Vec<String> = self.env_rows.iter().map(|row| row.name.clone()).collect();
        if let Some(order) = moved_order(&names, from, to) {
            self.save_environment_order(&order, cx);
        }
    }

    /// Moves the environment `name` one place up or down: "Move up" and "Move down" in the
    /// environment editor's menu and `Alt Up` / `Alt Down`.
    pub(crate) fn move_environment(&mut self, name: &str, up: bool, cx: &mut Context<Self>) {
        let names: Vec<String> = self.env_rows.iter().map(|row| row.name.clone()).collect();
        if let Some(order) = moved_by(&names, name, up) {
            self.save_environment_order(&order, cx);
        }
    }

    /// Copies the environment `name`, both files, as "<name> copy" right after it, with its
    /// color. The copy is what is on disk: unsaved edits of an open tab stay in that tab.
    pub(crate) fn duplicate_environment(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        match workspace.duplicate_environment(name, &t!("shell.sidebar.copy_suffix")) {
            Ok(copy) => {
                log::debug!("duplicated environment {name} as {copy}");
                self.workspace_error = None;
                self.refresh_environments();
            }
            Err(error) => {
                log_workspace_error(&format!("duplicate environment {name:?}"), &error);
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    /// Writes `order` to `.postino/project.toml` (created if needed) and reloads the rows.
    fn save_environment_order(&mut self, order: &[String], cx: &mut Context<Self>) {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        match workspace.set_environment_order(order) {
            Ok(()) => {
                log::debug!("reordered environments");
                self.workspace_error = None;
                self.refresh_environments();
            }
            Err(error) => {
                log_workspace_error("save environment order", &error);
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    /// Renders the Environments panel.
    pub(crate) fn render_env_panel(
        &mut self,
        weak: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let has_workspace = self.state.workspace.is_some();
        let active = self.state.active_environment.clone();
        let selected = self
            .state
            .tabs
            .active_environment_name()
            .map(str::to_string);
        let mono_font = cx.theme().mono_font_family.clone();

        let header = h_flex()
            .justify_between()
            .items_center()
            .pt(px(10.0))
            .pr(px(10.0))
            .pb(px(6.0))
            .pl(px(14.0))
            .child(SectionLabel::new(t!("navigation.env.title")))
            .child(
                IconButton::new("new-environment-header", IconName::Plus)
                    .box_size(ICON_BUTTON_HEADER)
                    .tooltip(t!("navigation.env.new"))
                    .disabled(!has_workspace)
                    .on_click({
                        let weak = weak.clone();
                        move |_, window, cx| {
                            let _ =
                                weak.update(cx, |view, cx| view.begin_new_environment(window, cx));
                        }
                    }),
            );

        let mut rows = v_flex().gap(px(1.0));
        if has_workspace {
            rows = rows.child(env_row(
                "env-row-none",
                weak.clone(),
                None,
                None,
                active.is_none() && selected.is_none(),
                active.is_none(),
                None,
                None,
                &palette,
                &mono_font,
            ));
            let last = self.env_rows.len().saturating_sub(1);
            for (index, row) in self.env_rows.iter().enumerate() {
                rows = rows.child(env_row(
                    ("env-row", index),
                    weak.clone(),
                    Some(index),
                    Some((row.name.clone(), row.color)),
                    selected.as_deref() == Some(row.name.as_str()),
                    active.as_deref() == Some(row.name.as_str()),
                    Some(row.var_count),
                    Some(EnvMenuTarget {
                        name: row.name.clone(),
                        is_active: active.as_deref() == Some(row.name.as_str()),
                        can_move_up: index > 0,
                        can_move_down: index < last,
                        chosen: self.state.env_colors.chosen(&row.name),
                        path: self.state.workspace.as_ref().map(|workspace| {
                            workspace.environment_path(&row.name, postino_workspace::EnvLayer::Base)
                        }),
                    }),
                    &palette,
                    &mono_font,
                ));
            }
            rows = rows.child(match &self.new_env_input {
                Some(input) => self.render_new_env_input(input, &palette, cx),
                None => new_env_row(weak.clone(), &palette),
            });
        } else {
            rows = rows.child(
                div()
                    .p_2()
                    .text_sm()
                    .text_color(palette.fg_subtle)
                    .child(t!("navigation.env.empty")),
            );
        }

        let list = div()
            .id("env-panel-rows")
            .flex_1()
            .min_h_0()
            .px(px(6.0))
            .overflow_y_scroll()
            // Below the rows (and on "New environment"): the last place.
            .on_drop({
                let weak = weak.clone();
                move |dragged: &DraggedEnvironment, _window, cx| {
                    let from = dragged.index;
                    let _ = weak.update(cx, |view, cx| {
                        view.reorder_environment(from, usize::MAX, cx);
                    });
                }
            })
            .child(rows);
        // The menu of the panel's empty part. A row's own menu wins over it.
        let list = if has_workspace {
            let weak = weak.clone();
            list.context_menu(move |menu, _, _| {
                let weak = weak.clone();
                menu.item(PopupMenuItem::new(t!("navigation.env.new")).on_click(
                    move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| view.begin_new_environment(window, cx));
                    },
                ))
            })
            .into_any_element()
        } else {
            list.into_any_element()
        };

        v_flex()
            .size_full()
            .bg(palette.surface)
            .border_r_1()
            .border_color(palette.border)
            .child(header)
            .child(list)
            .child(self.render_env_footer(&palette, &mono_font))
            .into_any_element()
    }

    /// The inline input row that replaces "+ New environment" while a name is being typed.
    fn render_new_env_input(
        &self,
        input: &Entity<InputState>,
        palette: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        h_flex()
            .h(px(ENV_ROW_HEIGHT))
            .items_center()
            .gap_2()
            .pl(px(4.0))
            .pr(px(8.0))
            .rounded(px(RADIUS_SM))
            .bg(palette.raised)
            .border_1()
            .border_color(palette.accent)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    view.cancel_new_environment(window, cx);
                }
            }))
            .child(grip_spacer())
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(input)
                        .context_menu(edit_menu(input, cx))
                        .px(px(INPUT_PADDING_X))
                        .h(px(ENV_ROW_HEIGHT - 2.0))
                        .py_0()
                        .bordered(false)
                        .bg(palette.bg.opacity(0.0)),
                ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(10.5))
                    .text_color(palette.fg_subtle)
                    .child(t!("navigation.env.create_hint")),
            )
            .into_any_element()
    }

    /// The footer: the `environments/` folder and a "Reveal" button opening it in the OS file
    /// manager (the workspace root when the folder does not exist yet).
    fn render_env_footer(&self, palette: &Palette, mono_font: &SharedString) -> AnyElement {
        let root = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().to_path_buf());
        let tooltip = SharedString::from(t!("navigation.env.reveal_tooltip").into_owned());
        h_flex()
            .w_full()
            .border_t_1()
            .border_color(palette.border)
            .px(px(14.0))
            .py(px(8.0))
            .items_center()
            .gap_2()
            .text_size(px(12.0))
            .text_color(palette.fg_subtle)
            .child(Icon::new(IconName::FolderOpen).small())
            .child(
                div()
                    .font_family(mono_font.clone())
                    .text_size(px(11.5))
                    .child(format!("{ENVIRONMENTS_FOLDER}/")),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("env-reveal")
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(RADIUS_SM))
                    .cursor_pointer()
                    .hover({
                        let hover = palette.hover;
                        let fg = palette.fg;
                        move |style| style.bg(hover).text_color(fg)
                    })
                    .active({
                        let pressed = palette.pressed;
                        move |style| style.bg(pressed)
                    })
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tooltip.clone())
                            .build(window, cx)
                    })
                    .child(t!("navigation.env.reveal"))
                    .on_click(move |_, _, cx| {
                        let Some(root) = &root else {
                            return;
                        };
                        let folder = root.join(ENVIRONMENTS_FOLDER);
                        cx.open_with_system(if folder.is_dir() { &folder } else { root });
                    }),
            )
            .into_any_element()
    }
}

/// One environment row. `name` is `None` for "No environment" (a hollow ring dot, no count).
/// The selected row is tinted, and the active one carries the "Active" tag when it is a named
/// environment. `index` is the row's place in [`AppView::env_rows`]: such a row can be dragged
/// by its grip and takes a dragged environment in its place. "No environment" (`None`) takes it
/// as the first.
#[allow(clippy::too_many_arguments)]
fn env_row(
    id: impl Into<ElementId>,
    weak: WeakEntity<AppView>,
    index: Option<usize>,
    name: Option<(String, EnvColor)>,
    selected: bool,
    active: bool,
    var_count: Option<usize>,
    menu: Option<EnvMenuTarget>,
    palette: &Palette,
    mono_font: &SharedString,
) -> AnyElement {
    let menu_weak = weak.clone();
    let dot = div().flex_none().size(px(7.0)).rounded_full();
    let dot = match &name {
        Some((_, color)) => dot.bg(palette.env_color(*color)),
        None => dot.border_1().border_color(palette.fg_subtle),
    };
    let label = match &name {
        Some((name, _)) => name.clone(),
        None => t!("common.no_environment").into_owned(),
    };
    let accent = palette.accent;
    let drop_weak = weak.clone();
    let dragged = name
        .as_ref()
        .zip(index)
        .map(|((name, color), index)| DraggedEnvironment {
            index,
            name: SharedString::from(name.clone()),
            color: palette.env_color(*color),
        });
    let grip = match &dragged {
        Some(_) => div()
            .flex_none()
            .w(px(GRIP_WIDTH))
            .flex()
            .justify_center()
            .opacity(0.0)
            .group_hover(ROW_GROUP, |style| style.opacity(1.0))
            .child(
                Icon::new(gpui_kit::assets::IconName::GripVertical)
                    .with_size(px(12.0))
                    .text_color(palette.fg_subtle),
            ),
        None => grip_spacer(),
    };

    let row = h_flex()
        .id(id)
        .group(ROW_GROUP)
        .h(px(ENV_ROW_HEIGHT))
        .items_center()
        .gap_2()
        .pl(px(4.0))
        .pr(px(8.0))
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .when(selected, |row| {
            row.bg(palette.accent_subtle)
                .text_color(palette.accent_text)
        })
        .when(!selected, |row| {
            row.text_color(palette.fg)
                .hover(|style| style.bg(palette.hover))
                .active(|style| style.bg(palette.pressed))
        })
        .child(grip)
        .child(dot)
        .child(div().flex_1().min_w_0().truncate().child(label))
        .when(active && name.is_some(), |row| {
            row.child(
                div()
                    .flex_none()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.success)
                    .child(t!("navigation.env.active")),
            )
        })
        .when_some(var_count, |row, count| {
            row.child(
                div()
                    .flex_none()
                    .font_family(mono_font.clone())
                    .text_size(px(11.0))
                    .text_color(palette.fg_subtle)
                    .child(format_integer(count as u64)),
            )
        })
        .when_some(dragged, |row, dragged| {
            row.on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
        })
        // The line marks where the dragged environment lands: above this row when it comes from
        // below, below it when it comes from above, below "No environment" unless it is already
        // the first.
        .drag_over::<DraggedEnvironment>(move |style, dragged, _, _| {
            let target = index.unwrap_or(0);
            if dragged.index > target {
                style.shadow(drop_line(accent, index.is_some()))
            } else if dragged.index < target {
                style.shadow(drop_line(accent, false))
            } else {
                style
            }
        })
        .on_drop(move |dragged: &DraggedEnvironment, _window, cx| {
            let from = dragged.index;
            let to = index.unwrap_or(0);
            let _ = drop_weak.update(cx, |view, cx| view.reorder_environment(from, to, cx));
        })
        .on_click(move |_, _, cx| {
            let name = name.as_ref().map(|(name, _)| name.clone());
            let _ = weak.update(cx, |view, cx| match name {
                Some(name) => view.open_environment_tab(name, cx),
                None => view.select_environment(None, cx),
            });
        });
    match menu {
        Some(target) => row
            .context_menu(move |menu, window, cx| {
                env_context_menu(menu_weak.clone(), &target, menu, window, cx)
            })
            .into_any_element(),
        None => row.into_any_element(),
    }
}

/// What the menu of one environment row needs, read when the panel is rendered.
#[derive(Debug, Clone)]
struct EnvMenuTarget {
    name: String,
    /// Whether it is the active environment.
    is_active: bool,
    can_move_up: bool,
    can_move_down: bool,
    /// The color the user chose, `None` when it is automatic.
    chosen: Option<EnvColor>,
    /// Its `.env` file.
    path: Option<PathBuf>,
}

/// Builds the context menu of the environment `target`: text only rows, with the key of the ones
/// that have one. Every item acts on that environment, whatever tab is active.
fn env_context_menu(
    weak: WeakEntity<AppView>,
    target: &EnvMenuTarget,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    // An item that runs `run` on the view with the environment's name.
    let item = |label: std::borrow::Cow<'static, str>,
                run: fn(&mut AppView, String, &mut Window, &mut Context<AppView>)| {
        let weak = weak.clone();
        let name = target.name.clone();
        PopupMenuItem::new(label.into_owned()).on_click(move |_, window, cx| {
            let name = name.clone();
            let _ = weak.update(cx, |view, cx| run(view, name, window, cx));
        })
    };
    let open = item(t!("common.open"), |view, name, _, cx| {
        view.open_environment_tab(name, cx);
    });
    let use_it = item(t!("environments.use"), |view, name, _, cx| {
        view.select_environment(Some(name), cx);
    })
    .disabled(target.is_active);
    // The actions only show their keys: the click handlers move the right-clicked environment,
    // which need not be the one of the active tab the keys act on.
    let move_up = item(t!("environments.menu.move_up"), |view, name, _, cx| {
        view.move_environment(&name, true, cx);
    })
    .action(Box::new(MoveEnvironmentUp))
    .disabled(!target.can_move_up);
    let move_down = item(t!("environments.menu.move_down"), |view, name, _, cx| {
        view.move_environment(&name, false, cx);
    })
    .action(Box::new(MoveEnvironmentDown))
    .disabled(!target.can_move_down);
    let rename = item(t!("common.rename"), |view, name, window, cx| {
        view.open_env_rename_dialog(name, window, cx);
    });
    let duplicate = item(t!("shell.sidebar.duplicate"), |view, name, _, cx| {
        view.duplicate_environment(&name, cx);
    });
    let reveal = {
        let path = target.path.clone();
        PopupMenuItem::new(t!("environments.menu.reveal")).on_click(move |_, _, cx| {
            if let Some(path) = &path {
                cx.reveal_path(path);
            }
        })
    };
    let delete = item(t!("common.delete"), |view, name, window, cx| {
        view.open_env_delete_dialog(name, window, cx);
    });

    let color_weak = weak.clone();
    let color_name = target.name.clone();
    let chosen = target.chosen;
    menu.item(open)
        .item(use_it)
        .separator()
        .submenu(
            t!("environments.menu.color_submenu"),
            window,
            cx,
            move |menu, _, _| color_menu(color_weak.clone(), &color_name, chosen, menu),
        )
        .item(move_up)
        .item(move_down)
        .separator()
        .item(rename)
        .item(duplicate)
        .item(reveal)
        .separator()
        .item(delete)
}

/// The Color submenu: "Automatic" and the nine named colors, the current one checked.
fn color_menu(
    weak: WeakEntity<AppView>,
    name: &str,
    chosen: Option<EnvColor>,
    menu: PopupMenu,
) -> PopupMenu {
    let item = |label: String, shown: EnvColor, color: Option<EnvColor>| {
        let weak = weak.clone();
        let name = name.to_string();
        PopupMenuItem::element(move |_, cx| {
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_none()
                        .size(px(8.0))
                        .rounded_full()
                        .bg(cx.palette().env_color(shown)),
                )
                .child(label.clone())
        })
        .checked(chosen == color)
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |view, cx| view.set_environment_color(&name, color, cx));
        })
    };
    let mut menu = menu
        .item(item(
            t!("environments.color.automatic").into_owned(),
            auto_color(name),
            None,
        ))
        .separator();
    for color in EnvColor::ALL {
        menu = menu.item(item(t!(color.label_key()).into_owned(), color, Some(color)));
    }
    menu
}

/// The muted "+ New environment" row at the end of the list.
fn new_env_row(weak: WeakEntity<AppView>, palette: &Palette) -> AnyElement {
    h_flex()
        .id("env-row-new")
        .h(px(ENV_ROW_HEIGHT))
        .items_center()
        .gap_2()
        .pl(px(4.0))
        .pr(px(8.0))
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .text_color(palette.fg_subtle)
        .hover({
            let hover = palette.hover;
            let fg = palette.fg_muted;
            move |style| style.bg(hover).text_color(fg)
        })
        .active({
            let pressed = palette.pressed;
            move |style| style.bg(pressed)
        })
        .child(grip_spacer())
        .child(
            div()
                .flex_none()
                .w(px(7.0))
                .flex()
                .justify_center()
                .child(Icon::new(IconName::Plus).with_size(px(12.0))),
        )
        .child(t!("navigation.env.new"))
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.begin_new_environment(window, cx));
        })
        .into_any_element()
}
