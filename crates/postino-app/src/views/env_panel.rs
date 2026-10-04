//! The Environments panel of the sidebar (GitHub #64): the workspace's environments with their
//! color dot, an "Active" tag and variable count, an inline "new environment" row, and a footer
//! that opens the `environments/` folder. The rows come from `state::env_panel`, loaded when the
//! workspace or an environment changes ([`AppView::refresh_env_rows`]), never in render.
//!
//! Clicking an environment row opens its editor tab (#65, `views/env_editor.rs`), and the row of
//! the environment shown by the active tab is tinted. The "Active" tag follows the active
//! environment, which the title bar's picker and the editor's "Use this environment" change.
//! The "No environment" row deactivates it.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::state::env_color::env_color;
use crate::state::env_panel::load_env_rows;
use crate::state::number::format_integer;
use crate::state::workspace_log::log_workspace_error;
use crate::theme::metrics::{ENV_ROW_HEIGHT, RADIUS_SM};
use crate::theme::{Palette, PaletteExt};
use crate::views::components::{IconButton, SectionLabel, edit_menu};

use super::root::AppView;

/// The folder the panel lists, relative to the workspace root.
const ENVIRONMENTS_FOLDER: &str = "environments";

impl AppView {
    /// Reloads [`AppView::env_rows`] from disk. Call when the workspace opens, an environment is
    /// created or one of its variables changes, and when the panel is opened.
    pub(crate) fn refresh_env_rows(&mut self) {
        self.env_rows = self
            .state
            .workspace
            .as_ref()
            .map(load_env_rows)
            .unwrap_or_default();
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
                self.refresh_env_rows();
                self.select_environment(Some(name), cx);
            }
            Err(error) => {
                log_workspace_error(&format!("create environment {name:?}"), &error);
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
                active.is_none() && selected.is_none(),
                active.is_none(),
                None,
                &palette,
                &mono_font,
            ));
            for (index, row) in self.env_rows.iter().enumerate() {
                rows = rows.child(env_row(
                    ("env-row", index),
                    weak.clone(),
                    Some(row.name.clone()),
                    selected.as_deref() == Some(row.name.as_str()),
                    active.as_deref() == Some(row.name.as_str()),
                    Some(row.var_count),
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

        v_flex()
            .size_full()
            .bg(palette.surface)
            .border_r_1()
            .border_color(palette.border)
            .child(header)
            .child(
                div()
                    .id("env-panel-rows")
                    .flex_1()
                    .min_h_0()
                    .px(px(6.0))
                    .overflow_y_scroll()
                    .child(rows),
            )
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
            .pl(px(8.0))
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
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(input)
                        .context_menu(edit_menu(input, cx))
                        .h(px(ENV_ROW_HEIGHT - 2.0))
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
/// environment.
#[allow(clippy::too_many_arguments)]
fn env_row(
    id: impl Into<ElementId>,
    weak: WeakEntity<AppView>,
    name: Option<String>,
    selected: bool,
    active: bool,
    var_count: Option<usize>,
    palette: &Palette,
    mono_font: &SharedString,
) -> AnyElement {
    let dot = div().flex_none().size(px(7.0)).rounded_full();
    let dot = match &name {
        Some(name) => dot.bg(palette.env_color(env_color(name))),
        None => dot.border_1().border_color(palette.fg_subtle),
    };
    let label = match &name {
        Some(name) => name.clone(),
        None => t!("common.no_environment").into_owned(),
    };

    h_flex()
        .id(id)
        .h(px(ENV_ROW_HEIGHT))
        .items_center()
        .gap_2()
        .px(px(8.0))
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
        .on_click(move |_, _, cx| {
            let name = name.clone();
            let _ = weak.update(cx, |view, cx| match name {
                Some(name) => view.open_environment_tab(name, cx),
                None => view.select_environment(None, cx),
            });
        })
        .into_any_element()
}

/// The muted "+ New environment" row at the end of the list.
fn new_env_row(weak: WeakEntity<AppView>, palette: &Palette) -> AnyElement {
    h_flex()
        .id("env-row-new")
        .h(px(ENV_ROW_HEIGHT))
        .items_center()
        .gap_2()
        .px(px(8.0))
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
