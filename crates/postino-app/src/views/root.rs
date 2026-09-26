//! [`AppView`]: the single top-level `gpui` view for this phase. It owns the plain
//! [`AppState`](crate::state::AppState), the `gpui`-specific bits that have to live alongside it
//! (the sidebar's tree state), and lays out the title bar, sidebar and main area. Other `views/`
//! modules add methods to [`AppView`] (`impl AppView` blocks split across files) so each panel's
//! rendering code lives next to what it renders, while the overall layout stays here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use postino_runner::{RunResult, ScriptEngine, SendOptions};
use postino_script::QuickJsEngine;

use crate::actions::{
    SaveActiveTab, SelectEnvironment1, SelectEnvironment2, SelectEnvironment3, SelectEnvironment4,
    SelectEnvironment5, SelectEnvironment6, SelectEnvironment7, SelectEnvironment8,
    SelectEnvironment9, SelectNoEnvironment, SendActiveTab,
};
use crate::state::debug_open::{self, DebugOpenTarget};
use crate::state::ui_tabs::{RequestTab, ResponseTab};
use crate::state::{self, AppState};
use crate::views::components::{
    DocumentTab, DocumentTabs, IconButton, InlineMessage, InlineMessageKind,
};
use crate::views::request_editor::RequestEditorEntities;
use crate::views::send::SendingTask;

use super::sidebar;

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
    /// The UI state `POSTINO_OPEN` requested at startup, if any (`state::debug_open`). `None` on
    /// a normal launch.
    pub(crate) debug_open: Option<DebugOpenTarget>,
    /// The `InputState` backing the components gallery's `UrlBar` demo
    /// (`views/components/gallery.rs`). Created lazily by [`Self::apply_debug_open`], never on a
    /// normal launch.
    pub(crate) gallery_url_input: Option<Entity<InputState>>,
    /// The sidebar filter's live text input (`plans/ui-redesign.md` section 2.3 point 2).
    /// Subscribed once in [`Self::new`]; edits call [`Self::on_sidebar_filter_changed`].
    pub(crate) sidebar_filter_input: Entity<InputState>,
    /// The sidebar tree's per-folder expand state, captured right before the filter went from
    /// empty to non-empty, so clearing the filter can restore it instead of resetting every
    /// folder to expanded. `None` while the filter is empty.
    pub(crate) sidebar_filter_pre_expansion: Option<HashMap<String, bool>>,
}

impl AppView {
    /// Creates a fresh view, opening `initial_workspace` right away if one was given (the CLI
    /// argument or the remembered last workspace, see `main.rs`). `window` is only needed for
    /// [`Self::apply_debug_open`] (opening a dialog, or here creating the gallery's `InputState`,
    /// needs it); every other debug hook ignores it.
    pub fn new(
        initial_workspace: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let sidebar_filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        cx.subscribe(
            &sidebar_filter_input,
            |view, _entity, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    view.on_sidebar_filter_changed(cx);
                }
            },
        )
        .detach();

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
            debug_open: None,
            gallery_url_input: None,
            sidebar_filter_input,
            sidebar_filter_pre_expansion: None,
        };
        if let Some(root) = initial_workspace {
            view.open_workspace_at(&root, window, cx);
        }
        view.apply_debug_autosend(cx);
        view.apply_debug_open(window, cx);
        view
    }

    /// `POSTINO_OPEN=<target>` opens a specific UI state at startup, for the orchestrator to
    /// screenshot after a UI phase (`plans/ui-redesign-spikes.md` section 10). A no-op when
    /// unset or unrecognized, same convention as [`Self::apply_debug_autosend`]: not a supported
    /// feature, not surfaced in any menu. Recognized targets are listed on
    /// [`crate::state::debug_open::DebugOpenTarget`].
    fn apply_debug_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(value) = std::env::var("POSTINO_OPEN") else {
            return;
        };
        let Some(target) = debug_open::parse(&value) else {
            return;
        };
        self.debug_open = Some(target);
        match target {
            DebugOpenTarget::Components => {
                self.gallery_url_input = Some(cx.new(|cx| {
                    InputState::new(window, cx).default_value("{{baseUrl}}/users/{{missing}}")
                }));
            }
        }
        cx.notify();
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

    /// Opens `root` as the workspace, remembers it for next launch, clears the sidebar filter
    /// (it belonged to the previous workspace), and refreshes the sidebar tree. Used at startup
    /// and by the title bar's workspace switcher ("Open folder..." and picking a recent
    /// workspace).
    pub(crate) fn open_workspace_at(
        &mut self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.sidebar_filter_pre_expansion = None;
        self.sidebar_filter_input.update(cx, |state, cx| {
            state.set_value(String::new(), window, cx);
        });
        self.refresh_tree(cx);
        cx.notify();
    }

    /// Rebuilds the sidebar tree from the workspace's current on-disk state, respecting the
    /// sidebar filter's current text. Call after any operation that changes the workspace (save,
    /// create, rename, delete).
    pub(crate) fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let query = self.sidebar_filter_input.read(cx).value().to_string();
        let items = self.tree_items_for_query(&query, None);
        self.tree_state
            .update(cx, |state, cx| state.set_items(items, cx));
    }

    /// Called on every edit to [`Self::sidebar_filter_input`]. Narrows the tree while the filter
    /// is non-empty (capturing the current expand state once, on the empty-to-non-empty
    /// transition); restores that captured expand state when the filter goes back to empty
    /// (`plans/ui-redesign.md` section 2.3 point 2).
    pub(crate) fn on_sidebar_filter_changed(&mut self, cx: &mut Context<Self>) {
        let query = self.sidebar_filter_input.read(cx).value().to_string();
        let items = if query.trim().is_empty() {
            let expansion = self.sidebar_filter_pre_expansion.take();
            self.tree_items_for_query(&query, expansion.as_ref())
        } else {
            if self.sidebar_filter_pre_expansion.is_none() {
                self.sidebar_filter_pre_expansion = Some(self.snapshot_tree_expansion(cx));
            }
            self.tree_items_for_query(&query, None)
        };
        self.tree_state
            .update(cx, |state, cx| state.set_items(items, cx));
    }

    /// Builds the sidebar's `TreeItem`s for the open workspace: the ordinary (optionally
    /// `expansion`-restored) tree when `query` is empty, the filtered tree otherwise. Empty (no
    /// workspace open) when there is nothing to show.
    fn tree_items_for_query(
        &self,
        query: &str,
        expansion: Option<&HashMap<String, bool>>,
    ) -> Vec<gpui_kit::component::tree::TreeItem> {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return Vec::new();
        };
        if query.trim().is_empty() {
            sidebar::build_tree_items(workspace.tree(), expansion)
        } else {
            sidebar::build_filtered_tree_items(workspace.tree(), query)
        }
    }

    /// Snapshots every folder's current expand flag from the live sidebar tree, so
    /// [`Self::on_sidebar_filter_changed`] can restore it once the filter is cleared.
    fn snapshot_tree_expansion(&self, cx: &Context<Self>) -> HashMap<String, bool> {
        let tree_state = self.tree_state.read(cx);
        let mut map = HashMap::new();
        let mut index = 0;
        while let Some(entry) = tree_state.entry(index) {
            if entry.is_folder() {
                map.insert(entry.item().id.to_string(), entry.is_expanded());
            }
            index += 1;
        }
        map
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

    /// Sets the active environment (`None` for "No environment"). Used by the environment
    /// picker's clicks and by [`Self::select_environment_by_shortcut`].
    pub(crate) fn select_environment(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.state.active_environment = name;
        cx.notify();
    }

    /// Selects the `one_based_index`-th environment (1..=9) of the open workspace, for the
    /// `Ctrl 1..9` / `Cmd 1..9` shortcuts (`plans/ui-redesign.md` phase 4 item 1). A no-op when
    /// there is no workspace open or fewer than `one_based_index` environments.
    pub(crate) fn select_environment_by_shortcut(
        &mut self,
        one_based_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.state.workspace.as_ref() else {
            return;
        };
        let environments = workspace.list_environments().unwrap_or_default();
        if let Some(name) = environments.into_iter().nth(one_based_index - 1) {
            self.select_environment(Some(name), cx);
        }
    }

    /// Handles the `Ctrl 1` / `Cmd 1` key binding.
    fn on_select_environment_1(
        &mut self,
        _: &SelectEnvironment1,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(1, cx);
    }

    /// Handles the `Ctrl 2` / `Cmd 2` key binding.
    fn on_select_environment_2(
        &mut self,
        _: &SelectEnvironment2,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(2, cx);
    }

    /// Handles the `Ctrl 3` / `Cmd 3` key binding.
    fn on_select_environment_3(
        &mut self,
        _: &SelectEnvironment3,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(3, cx);
    }

    /// Handles the `Ctrl 4` / `Cmd 4` key binding.
    fn on_select_environment_4(
        &mut self,
        _: &SelectEnvironment4,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(4, cx);
    }

    /// Handles the `Ctrl 5` / `Cmd 5` key binding.
    fn on_select_environment_5(
        &mut self,
        _: &SelectEnvironment5,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(5, cx);
    }

    /// Handles the `Ctrl 6` / `Cmd 6` key binding.
    fn on_select_environment_6(
        &mut self,
        _: &SelectEnvironment6,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(6, cx);
    }

    /// Handles the `Ctrl 7` / `Cmd 7` key binding.
    fn on_select_environment_7(
        &mut self,
        _: &SelectEnvironment7,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(7, cx);
    }

    /// Handles the `Ctrl 8` / `Cmd 8` key binding.
    fn on_select_environment_8(
        &mut self,
        _: &SelectEnvironment8,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(8, cx);
    }

    /// Handles the `Ctrl 9` / `Cmd 9` key binding.
    fn on_select_environment_9(
        &mut self,
        _: &SelectEnvironment9,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment_by_shortcut(9, cx);
    }

    /// Handles the `Ctrl 0` / `Cmd 0` key binding ("No environment").
    fn on_select_no_environment(
        &mut self,
        _: &SelectNoEnvironment,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_environment(None, cx);
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

    /// Renders the main area: the open-tabs bar on top, the request editor and response viewer
    /// split below it.
    fn render_main_area(
        &mut self,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.debug_open == Some(DebugOpenTarget::Components) {
            return self.render_components_gallery(window, cx);
        }

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

    /// Renders the open-tabs bar (`plans/ui-redesign.md` section 2.3 point 3): a [`DocumentTab`]
    /// per open request, with a dirty marker and a close button, and a trailing "+" that opens
    /// the same new-request dialog as the sidebar header.
    fn render_tabs_bar(&mut self, weak: WeakEntity<Self>, _cx: &mut Context<Self>) -> AnyElement {
        if self.state.tabs.open_tabs().is_empty() {
            return div().into_any_element();
        }

        let active_index = self.state.tabs.active_index();
        let mut tabs = DocumentTabs::new("open-tabs");
        for (index, tab) in self.state.tabs.open_tabs().iter().enumerate() {
            let select_weak = weak.clone();
            let close_weak = weak.clone();
            let full_id = tab.id.clone();
            let doc_tab = DocumentTab::new(state::format::tab_label(&tab.id).to_string())
                .method(tab.request.method.clone())
                .dirty(tab.dirty)
                .selected(active_index == Some(index))
                .tooltip(full_id)
                .on_click(move |_, cx| {
                    let _ = select_weak.update(cx, |view, cx| {
                        view.state.tabs.set_active(index);
                        cx.notify();
                    });
                })
                .on_close(move |_, cx| {
                    let _ = close_weak.update(cx, |view, cx| view.close_tab(index, cx));
                });
            tabs = tabs.item(doc_tab);
        }

        tabs = tabs.suffix(
            IconButton::new("open-tabs-new-request", IconName::Plus)
                .tooltip("New request")
                .on_click(move |_, window, cx| {
                    sidebar::open_new_request_dialog(weak.clone(), None, window, cx);
                }),
        );
        tabs.into_any_element()
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
        let background = cx.theme().background;

        let title_bar = self.render_title_bar(weak.clone(), window, cx);
        let error_banner = self.workspace_error.clone().map(|message| {
            let dismiss_weak = weak.clone();
            div().px_3().pt_2().child(
                InlineMessage::new(InlineMessageKind::Danger, message).action(
                    "Dismiss",
                    move |_, cx| {
                        let _ = dismiss_weak.update(cx, |view, cx| {
                            view.workspace_error = None;
                            cx.notify();
                        });
                    },
                ),
            )
        });
        let body = self.render_body(weak, window, cx);
        let status_bar = self.render_status_bar(cx);

        v_flex()
            .size_full()
            .bg(background)
            .on_action(cx.listener(Self::on_save_action))
            .on_action(cx.listener(Self::on_send_action))
            .on_action(cx.listener(Self::on_select_environment_1))
            .on_action(cx.listener(Self::on_select_environment_2))
            .on_action(cx.listener(Self::on_select_environment_3))
            .on_action(cx.listener(Self::on_select_environment_4))
            .on_action(cx.listener(Self::on_select_environment_5))
            .on_action(cx.listener(Self::on_select_environment_6))
            .on_action(cx.listener(Self::on_select_environment_7))
            .on_action(cx.listener(Self::on_select_environment_8))
            .on_action(cx.listener(Self::on_select_environment_9))
            .on_action(cx.listener(Self::on_select_no_environment))
            .child(title_bar)
            .children(error_banner)
            .child(body)
            .child(status_bar)
    }
}
