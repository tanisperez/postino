//! [`AppView`]: the single top-level `gpui` view for this phase. It owns the plain
//! [`AppState`](crate::state::AppState), the `gpui`-specific bits that have to live alongside it
//! (the sidebar's tree state), and lays out the title bar, sidebar and main area. Other `views/`
//! modules add methods to [`AppView`] (`impl AppView` blocks split across files) so each panel's
//! rendering code lives next to what it renders, while the overall layout stays here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use postino_runner::{RunResult, ScriptEngine, SendOptions};
use postino_script::QuickJsEngine;

use crate::actions::{SaveActiveTab, SendActiveTab};
use crate::state::ui_tabs::{RequestTab, ResponseTab};
use crate::state::{self, AppState};
use crate::views::request_editor::RequestEditorEntities;
use crate::views::send::SendingTask;

/// The main window view: title bar, resizable sidebar, and main area (open tabs plus the
/// request editor and response viewer).
pub struct AppView {
    /// Plain application state: workspace, open tabs, active environment, session environment.
    pub(crate) state: AppState,
    /// The sidebar collection tree's interaction state (selection, expansion, scrolling). This
    /// is `gpui` state, so it lives here rather than in [`AppState`].
    pub(crate) tree_state: Entity<TreeState>,
    /// The id of the tree entry that was open as a tab the last time we checked, so a new
    /// selection opens a tab exactly once instead of reloading the file from disk on every
    /// re-render (which would discard in-progress edits).
    pub(crate) last_selected_request: Option<String>,
    /// The message of the last operation that failed, shown as a banner until the next
    /// successful operation clears it. `gpui` has no blocking error dialogs in this app by
    /// design (`AGENTS.md`: no native blocking dialogs), so failures show inline instead.
    pub(crate) workspace_error: Option<String>,
    /// The `gpui` entities behind the request editor's editable fields (Phase 9). See
    /// `views/request_editor.rs`.
    pub(crate) request_editor: RequestEditorEntities,
    /// Which request editor tab (Params, Headers, ...) is active. Shared across every open tab
    /// for simplicity: switching tabs keeps the same editor tab selected, which matches how most
    /// tabbed editors behave.
    pub(crate) active_request_tab: RequestTab,
    /// Which response viewer tab (Body, Headers, ...) is active.
    pub(crate) active_response_tab: ResponseTab,
    /// Whether the response Body tab shows the raw body instead of the pretty-printed one.
    pub(crate) response_raw: bool,
    /// The request currently being sent, if any (Phase 9). See `views/send.rs`.
    pub(crate) sending: Option<SendingTask>,
    /// The last [`RunResult`] for each tab id that has been sent at least once.
    pub(crate) responses: HashMap<String, RunResult>,
    /// The script engine every send uses. `Arc` so it is cheap to clone into each background
    /// send task; `QuickJsEngine` needs no per-run state, a fresh QuickJS runtime is created for
    /// every script run (see `postino-script`).
    pub(crate) script_engine: Arc<dyn ScriptEngine>,
    /// The options every send uses (timeout, redirects, TLS verification). Defaults are fine for
    /// the MVP; there is no UI to change them yet.
    pub(crate) send_options: SendOptions,
}

impl AppView {
    /// Creates a fresh view, opening `initial_workspace` right away if one was given (the CLI
    /// argument or the remembered last workspace, see `main.rs`).
    pub fn new(initial_workspace: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let mut view = Self {
            state: AppState::new(),
            tree_state,
            last_selected_request: None,
            workspace_error: None,
            request_editor: RequestEditorEntities::default(),
            active_request_tab: RequestTab::default(),
            active_response_tab: ResponseTab::default(),
            response_raw: false,
            sending: None,
            responses: HashMap::new(),
            script_engine: Arc::new(QuickJsEngine),
            send_options: SendOptions::default(),
        };
        if let Some(root) = initial_workspace {
            view.open_workspace_at(&root, cx);
        }
        view.apply_debug_autosend(cx);
        view
    }

    /// Hidden debug hooks for end-to-end checks, since there is no tool to simulate clicks in
    /// this app (Phase 9's manual verification, `plans/mvp.md`). Both are no-ops when unset, so
    /// they never affect a normal launch; they exist for `make run`/manual testing, not as a
    /// supported feature, so neither is surfaced in any menu or documented outside this comment.
    ///
    /// - `POSTINO_ENV=<name>` selects an environment before sending, matching a name under
    ///   `environments/` in the opened workspace.
    /// - `POSTINO_AUTOSEND=<request id>` opens that request (a workspace-relative id, for
    ///   example `auth/login.postino`) and sends it immediately at startup.
    fn apply_debug_autosend(&mut self, cx: &mut Context<Self>) {
        if self.state.workspace.is_none() {
            return;
        }
        if let Ok(env_name) = std::env::var("POSTINO_ENV") {
            self.state.active_environment = Some(env_name);
        }
        if let Ok(request_id) = std::env::var("POSTINO_AUTOSEND") {
            self.open_request(request_id, cx);
            self.send_active_tab(cx);
        }
    }

    /// Opens `root` as the workspace, remembers it for next launch, and refreshes the sidebar
    /// tree. Used both at startup and by the "Open folder" picker.
    pub(crate) fn open_workspace_at(&mut self, root: &Path, cx: &mut Context<Self>) {
        match self.state.open_workspace(root) {
            Ok(()) => {
                self.workspace_error = None;
                // Remember an absolute path: a relative one (as given on the command line, for
                // example) would resolve against whatever directory the app happens to be
                // launched from next time, not necessarily the same folder.
                let absolute = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
                state::config::record_workspace(&absolute);
            }
            Err(error) => {
                self.workspace_error = Some(error.to_string());
            }
        }
        self.last_selected_request = None;
        self.refresh_tree(cx);
        cx.notify();
    }

    /// Rebuilds the sidebar tree from the workspace's current on-disk state. Call after any
    /// operation that changes the workspace (save, create, rename, delete).
    pub(crate) fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let items = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| crate::views::sidebar::build_tree_items(workspace.tree()))
            .unwrap_or_default();
        self.tree_state
            .update(cx, |state, cx| state.set_items(items, cx));
    }

    /// Loads `id` from the workspace and opens it as a tab (or activates it, if already open).
    pub(crate) fn open_request(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        match workspace.load_request(&id) {
            Ok(request) => {
                self.state.tabs.open(id, request);
                self.workspace_error = None;
            }
            Err(error) => {
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    /// Closes the tab at `index`.
    pub(crate) fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        self.state.tabs.close(index);
        cx.notify();
    }

    /// Saves the active tab's request to disk with `postino-format`'s canonical serialization,
    /// via [`postino_workspace::Workspace::save_request`]. Bound to `Ctrl+S` / `Cmd+S`.
    pub(crate) fn save_active_tab(&mut self, cx: &mut Context<Self>) {
        let Some(active_index) = self.state.tabs.active_index() else {
            return;
        };
        let Some(tab) = self.state.tabs.active() else {
            return;
        };
        let id = tab.id.clone();
        let request = tab.request.clone();

        let Some(workspace) = self.state.workspace.as_mut() else {
            return;
        };
        match workspace.save_request(&id, &request) {
            Ok(()) => {
                self.state.tabs.mark_saved(active_index);
                self.workspace_error = None;
                self.refresh_tree(cx);
            }
            Err(error) => {
                self.workspace_error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    /// Creates a new, blank request named `name` inside the folder `parent` (workspace root when
    /// `None`), then opens it as a tab.
    pub(crate) fn create_request(
        &mut self,
        parent: Option<String>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.state.workspace.as_mut() else {
            return;
        };
        match workspace.create_request(parent.as_deref(), &name) {
            Ok(id) => {
                self.workspace_error = None;
                self.refresh_tree(cx);
                self.open_request(id, cx);
                return;
            }
            Err(error) => self.workspace_error = Some(error.to_string()),
        }
        cx.notify();
    }

    /// Creates a new, empty folder named `name` inside the folder `parent` (workspace root when
    /// `None`).
    pub(crate) fn create_folder(
        &mut self,
        parent: Option<String>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.state.workspace.as_mut() else {
            return;
        };
        match workspace.create_folder(parent.as_deref(), &name) {
            Ok(_) => {
                self.workspace_error = None;
                self.refresh_tree(cx);
            }
            Err(error) => self.workspace_error = Some(error.to_string()),
        }
        cx.notify();
    }

    /// Renames the request or folder `id` to `new_name`, keeping any open tab under it pointing
    /// at the new path.
    pub(crate) fn rename(&mut self, id: String, new_name: String, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.workspace.as_mut() else {
            return;
        };
        match workspace.rename(&id, &new_name) {
            Ok(new_id) => {
                self.state.tabs.rename_prefix(&id, &new_id);
                self.workspace_error = None;
                self.refresh_tree(cx);
            }
            Err(error) => self.workspace_error = Some(error.to_string()),
        }
        cx.notify();
    }

    /// Deletes the request or folder `id`, closing any open tab under it.
    pub(crate) fn delete(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.workspace.as_mut() else {
            return;
        };
        match workspace.delete(&id) {
            Ok(()) => {
                self.state.tabs.close_prefix(&id);
                self.workspace_error = None;
                self.refresh_tree(cx);
            }
            Err(error) => self.workspace_error = Some(error.to_string()),
        }
        cx.notify();
    }

    /// Handles the `Ctrl+S` / `Cmd+S` key binding (see `main.rs`'s `bind_keys`).
    fn on_save_action(&mut self, _: &SaveActiveTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.save_active_tab(cx);
    }

    /// Handles the `Ctrl+Enter` / `Cmd+Enter` key binding (see `main.rs`'s `bind_keys`).
    fn on_send_action(&mut self, _: &SendActiveTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.send_active_tab(cx);
    }

    /// Renders the resizable sidebar and main area below the title bar.
    fn render_body(
        &mut self,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_resizable("postino-layout")
            .child(
                resizable_panel()
                    .size(px(280.))
                    .size_range(px(180.)..px(480.))
                    .flex_none()
                    .child(self.render_sidebar(weak.clone(), cx)),
            )
            .child(resizable_panel().child(self.render_main_area(weak, window, cx)))
            .into_any_element()
    }

    /// Renders the title bar's content: the app name, a spacer, the Import menu and the
    /// environment picker.
    fn render_title_bar(&mut self, weak: WeakEntity<Self>, cx: &mut Context<Self>) -> AnyElement {
        TitleBar::new()
            .child(div().text_sm().child("Postino"))
            .child(div().flex_1())
            .child(self.render_import_menu(cx))
            .child(self.render_env_picker(weak, cx))
            .into_any_element()
    }

    /// Renders the main area: the open-tabs bar on top, the request editor and response viewer
    /// split below it.
    fn render_main_area(
        &mut self,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tabs_bar = self.render_tabs_bar(weak, cx);

        v_flex()
            .size_full()
            .child(tabs_bar)
            .child(
                v_resizable("postino-main")
                    .child(resizable_panel().child(self.render_request_editor(window, cx)))
                    .child(resizable_panel().child(self.render_response_view(cx))),
            )
            .into_any_element()
    }

    /// Renders the bar of open tabs, with a dirty marker and a close button on each.
    fn render_tabs_bar(&mut self, weak: WeakEntity<Self>, _cx: &mut Context<Self>) -> AnyElement {
        if self.state.tabs.open_tabs().is_empty() {
            return div().into_any_element();
        }

        let active_index = self.state.tabs.active_index().unwrap_or(0);
        let mut bar = TabBar::new("open-tabs").selected_index(active_index);

        for (index, tab) in self.state.tabs.open_tabs().iter().enumerate() {
            let label = if tab.dirty {
                format!("* {}", tab.id)
            } else {
                tab.id.clone()
            };
            let select_weak = weak.clone();
            let close_weak = weak.clone();
            bar = bar.child(
                Tab::new()
                    .label(label)
                    .suffix(
                        Button::new(("close-tab", index))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Close).small())
                            .on_click(move |_, _, cx| {
                                let _ = close_weak.update(cx, |view, cx| view.close_tab(index, cx));
                            }),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = select_weak.update(cx, |view, cx| {
                            view.state.tabs.set_active(index);
                            cx.notify();
                        });
                    }),
            );
        }
        bar.into_any_element()
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Follow the sidebar's selection: opening a request tab happens here, once per actual
        // selection change, rather than from a click handler on every tree row. Guarding on
        // `last_selected_request` keeps a re-render from reloading the file (and discarding
        // in-progress edits) just because the same row is still selected.
        let selected_id = self
            .tree_state
            .read(cx)
            .selected_item()
            .map(|item| item.id.to_string());
        if let Some(id) = selected_id
            && id.ends_with(".postino")
            && self.last_selected_request.as_deref() != Some(id.as_str())
        {
            self.last_selected_request = Some(id.clone());
            self.open_request(id, cx);
        }

        let weak = cx.weak_entity();
        let theme = cx.theme();
        let background = theme.background;
        let danger_bg = theme.danger;
        let danger_fg = theme.danger_foreground;

        let title_bar = self.render_title_bar(weak.clone(), cx);
        let error_banner = self.workspace_error.clone().map(|message| {
            h_flex()
                .w_full()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .bg(danger_bg)
                .text_color(danger_fg)
                .child(Icon::new(IconName::TriangleAlert).small())
                .child(message)
        });
        let body = self.render_body(weak, window, cx);

        v_flex()
            .size_full()
            .bg(background)
            .on_action(cx.listener(Self::on_save_action))
            .on_action(cx.listener(Self::on_send_action))
            .child(title_bar)
            .children(error_banner)
            .child(body)
    }
}
