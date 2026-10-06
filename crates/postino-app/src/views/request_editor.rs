//! The request editor panel: method + URL bar, Send/Cancel, and the Params, Headers, Body,
//! Pre-request, Post-response and Docs tabs (`plans/mvp.md`, Phase 9).
//!
//! `gpui` entities for the editable fields (the URL input, the key-value table rows, the body
//! and script code editors, the docs textarea) are expensive to keep in sync by hand on every
//! keystroke, so [`RequestEditorEntities`] rebuilds them only when what they need to show
//! actually changes shape: a different tab becomes active, a row is added or removed, or the
//! body's type (and so its editor language) changes. Typing into an existing field never
//! triggers a rebuild, which is what lets it keep focus and cursor position while the user
//! types; see [`RequestEditorEntities::sync`].

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    Editor, EditorState, Input, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::{Body, KeyValue, Method, Request, variable_spans};

use crate::state::request_edit::{self, BodyKind};
use crate::state::script_heuristics;
use crate::state::ui_tabs::RequestTab;
use crate::theme::PaletteExt;
use crate::theme::metrics::{RADIUS_MD, SEND_BUTTON_HEIGHT, SEND_BUTTON_MIN_WIDTH};
use crate::views::components::{
    GhostButton, KeyValueRow, KeyValueTable, PrimaryButton, SegmentedControl, SegmentedItem,
    UnderlineTabItem, UnderlineTabs, UrlBar, edit_menu,
};

use super::root::AppView;

/// The Send button's key hint: `Cmd \u{21b5}` on macOS, `Ctrl \u{21b5}` elsewhere, matching
/// `main.rs`'s key bindings (same local-constant pattern as `views/env_picker.rs`'s
/// `MODIFIER_KEY`).
#[cfg(target_os = "macos")]
const SEND_KEY_HINT: &str = "Cmd+\u{21b5}";
#[cfg(not(target_os = "macos"))]
const SEND_KEY_HINT: &str = "Ctrl+\u{21b5}";

/// Which key-value table a row belongs to: routes an edit to the right field of the active tab's
/// [`Request`], and, only for [`RowKind::Query`], triggers the Params/URL sync afterward (see
/// `state::request_edit::sync_url_from_query`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowKind {
    /// The header list.
    Headers,
    /// The `::: query` section.
    Query,
    /// A `Body::Form`'s fields.
    Form,
}

/// Which script field a script code editor edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptField {
    /// The `::: pre` section.
    Pre,
    /// The `::: post` section.
    Post,
}

/// Everything about the active tab's shape that requires [`RequestEditorEntities`] to be
/// rebuilt. Row and script *content* is deliberately not part of this key: editing content must
/// never rebuild the entities, or the field being typed into would lose focus on every
/// keystroke. Only counts (which change when a row is added or removed) and the body's kind
/// (which changes the body editor's language) are tracked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildKey {
    tab_id: String,
    headers_len: usize,
    query_len: usize,
    form_len: usize,
    body_kind: BodyKind,
}

impl BuildKey {
    fn of(tab_id: &str, request: &Request) -> Self {
        let form_len = match &request.body {
            Body::Form(rows) => rows.len(),
            _ => 0,
        };
        Self {
            tab_id: tab_id.to_string(),
            headers_len: request.headers.len(),
            query_len: request.query.len(),
            form_len,
            body_kind: BodyKind::of(&request.body),
        }
    }
}

/// One key-value row's live `gpui` input entities.
struct RowEntities {
    key: Entity<InputState>,
    value: Entity<InputState>,
}

/// The live `gpui` entities behind one key-value table (Params, Headers or Form body), aligned
/// index-for-index with the `Request`'s own `Vec<KeyValue>`.
#[derive(Default)]
struct KeyValueTableEntities {
    rows: Vec<RowEntities>,
}

impl KeyValueTableEntities {
    fn build(
        tab_id: &str,
        rows: &[KeyValue],
        kind: RowKind,
        window: &mut Window,
        cx: &mut Context<AppView>,
    ) -> Self {
        let rows = rows
            .iter()
            .enumerate()
            .map(|(index, row)| build_row(tab_id, kind, index, row, window, cx))
            .collect();
        Self { rows }
    }
}

/// Builds one row's key and value inputs, pre-filled with `row`'s current content, and
/// subscribes each to write typed changes back into the active tab's request.
fn build_row(
    tab_id: &str,
    kind: RowKind,
    index: usize,
    row: &KeyValue,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> RowEntities {
    let key = cx.new(|cx| InputState::new(window, cx).placeholder(t!("request.kv.key")));
    key.update(cx, |state, cx| state.set_value(row.key.clone(), window, cx));
    {
        let tab_id = tab_id.to_string();
        cx.subscribe(&key, move |view, entity, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let value = entity.read(cx).value().to_string();
            view.edit_row(&tab_id, kind, index, cx, |row| row.key = value);
        })
        .detach();
    }

    let value = cx.new(|cx| InputState::new(window, cx).placeholder(t!("common.value")));
    value.update(cx, |state, cx| {
        state.set_value(row.value.clone(), window, cx)
    });
    {
        let tab_id = tab_id.to_string();
        cx.subscribe(&value, move |view, entity, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let new_value = entity.read(cx).value().to_string();
            view.edit_row(&tab_id, kind, index, cx, |row| row.value = new_value);
        })
        .detach();
    }

    RowEntities { key, value }
}

/// The body editor's language for a request's current body, and the raw text to load into it.
/// `None` for a body kind that has no code editor of its own (`None` or `Form`, which uses the
/// key-value table instead).
fn body_language(body: &Body) -> Option<(&str, &'static str)> {
    match body {
        Body::Json(text) => Some((text.as_str(), "json")),
        Body::Text(text) => Some((text.as_str(), "text")),
        // gpui-component 0.6.6 ships no dedicated XML grammar; "html" is the closest available
        // and still highlights tags and attributes reasonably well for a preview (see this
        // crate's Cargo.toml for the exact tree-sitter features enabled).
        Body::Xml(text) => Some((text.as_str(), "html")),
        Body::None | Body::Form(_) => None,
    }
}

/// Builds a JavaScript code editor for a `::: pre` or `::: post` section, subscribed to write
/// typed changes back into the active tab's request.
fn make_script_editor(
    tab_id: &str,
    text: &str,
    field: ScriptField,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> Entity<EditorState> {
    let editor = cx.new(|cx| EditorState::new(window, cx).language("javascript"));
    editor.update(cx, |state, cx| {
        state.set_value(text.to_string(), window, cx)
    });
    let tab_id = tab_id.to_string();
    cx.subscribe(&editor, move |view, entity, event: &InputEvent, cx| {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let text = entity.read(cx).value().to_string();
        view.edit_active_request(&tab_id, cx, |request| match field {
            ScriptField::Pre => request.pre_script = text,
            ScriptField::Post => request.post_script = text,
        });
    })
    .detach();
    editor
}

/// The `gpui` entities behind the request editor's editable fields, for whichever tab is
/// currently active. See the module docs for when they are rebuilt versus reused.
#[derive(Default)]
pub(crate) struct RequestEditorEntities {
    built_for: Option<BuildKey>,
    url: Option<Entity<InputState>>,
    /// The inline `Input` for typing a custom method token (`plans/ui-redesign.md` phase 5,
    /// reviewer fix item 6), shown by [`UrlBar`] in place of the dropdown while
    /// [`AppView::editing_method`] is `true`. Its value is seeded once, when editing begins
    /// ([`AppView::begin_editing_custom_method`]), not resynced here on every render: unlike the
    /// URL, `request.method` deliberately does not change on every keystroke while editing (see
    /// that method's doc comment), so an unconditional resync would fight the user's typing.
    method_input: Option<Entity<InputState>>,
    headers: KeyValueTableEntities,
    query: KeyValueTableEntities,
    form: KeyValueTableEntities,
    body_editor: Option<Entity<EditorState>>,
    pre_editor: Option<Entity<EditorState>>,
    post_editor: Option<Entity<EditorState>>,
    docs: Option<Entity<TextareaState>>,
}

impl RequestEditorEntities {
    /// Rebuilds every entity when `request`'s shape (see [`BuildKey`]) differs from what they
    /// were last built for, then, unconditionally, keeps the URL input's displayed text in sync
    /// with `request.url` (needed even without a rebuild: editing the Params table rewrites the
    /// URL without changing the build key, see [`BuildKey`]'s docs). Returns whether a rebuild
    /// happened, so the caller can reset [`AppView::editing_method`] (a fresh `method_input`
    /// entity would otherwise show empty while still claiming to be "being edited").
    fn sync(
        &mut self,
        tab_id: &str,
        request: &Request,
        window: &mut Window,
        cx: &mut Context<AppView>,
    ) -> bool {
        let key = BuildKey::of(tab_id, request);
        let rebuilt = self.built_for.as_ref() != Some(&key);
        if rebuilt {
            self.rebuild(tab_id, request, window, cx);
            self.built_for = Some(key);
        }

        if let Some(url) = &self.url {
            let current = url.read(cx).value();
            if current.as_ref() != request.url.as_str() {
                let new_value = request.url.clone();
                url.update(cx, |state, cx| state.set_value(new_value, window, cx));
            }
        }

        // The body editor's text can also change without a rebuild: the "Format" button
        // rewrites `request.body`'s text directly (not by typing into the editor), the same
        // reason the URL needs the unconditional resync above.
        if let Some(editor) = &self.body_editor
            && let Some((text, _language)) = body_language(&request.body)
        {
            let current = editor.read(cx).value();
            if current.as_ref() != text {
                let new_value = text.to_string();
                editor.update(cx, |state, cx| state.set_value(new_value, window, cx));
            }
        }

        rebuilt
    }

    fn rebuild(
        &mut self,
        tab_id: &str,
        request: &Request,
        window: &mut Window,
        cx: &mut Context<AppView>,
    ) {
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://..."));
        url.update(cx, |state, cx| {
            state.set_value(request.url.clone(), window, cx)
        });
        {
            let tab_id = tab_id.to_string();
            cx.subscribe(&url, move |view, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let value = entity.read(cx).value().to_string();
                view.edit_active_request(&tab_id, cx, |request| request.url = value);
            })
            .detach();
        }
        self.url = Some(url);

        let method_input = cx.new(|cx| InputState::new(window, cx).placeholder("METHOD"));
        {
            let tab_id = tab_id.to_string();
            cx.subscribe(
                &method_input,
                move |view, entity, event: &InputEvent, cx| match event {
                    InputEvent::PressEnter { .. } => {
                        let text = entity.read(cx).value().to_string();
                        view.commit_custom_method(&tab_id, text, false, cx);
                    }
                    InputEvent::Blur => {
                        let text = entity.read(cx).value().to_string();
                        view.commit_custom_method(&tab_id, text, true, cx);
                    }
                    _ => {}
                },
            )
            .detach();
        }
        self.method_input = Some(method_input);

        self.headers =
            KeyValueTableEntities::build(tab_id, &request.headers, RowKind::Headers, window, cx);
        self.query =
            KeyValueTableEntities::build(tab_id, &request.query, RowKind::Query, window, cx);
        self.form = match &request.body {
            Body::Form(rows) => {
                KeyValueTableEntities::build(tab_id, rows, RowKind::Form, window, cx)
            }
            _ => KeyValueTableEntities::default(),
        };

        self.body_editor = body_language(&request.body).map(|(text, language)| {
            let editor = cx.new(|cx| EditorState::new(window, cx).language(language));
            editor.update(cx, |state, cx| {
                state.set_value(text.to_string(), window, cx)
            });
            let tab_id = tab_id.to_string();
            cx.subscribe(&editor, move |view, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = entity.read(cx).value().to_string();
                view.edit_active_request(&tab_id, cx, |request| {
                    request_edit::set_body_text(request, text)
                });
            })
            .detach();
            editor
        });

        self.pre_editor = Some(make_script_editor(
            tab_id,
            &request.pre_script,
            ScriptField::Pre,
            window,
            cx,
        ));
        self.post_editor = Some(make_script_editor(
            tab_id,
            &request.post_script,
            ScriptField::Post,
            window,
            cx,
        ));

        let docs = cx.new(|cx| TextareaState::new(window, cx));
        docs.update(cx, |state, cx| {
            state.set_value(request.docs.clone(), window, cx)
        });
        {
            let tab_id = tab_id.to_string();
            cx.subscribe(&docs, move |view, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = entity.read(cx).value().to_string();
                view.edit_active_request(&tab_id, cx, |request| request.docs = text);
            })
            .detach();
        }
        self.docs = Some(docs);
    }
}

/// Returns the `Vec<KeyValue>` of `request` that `kind` edits, or `None` for `RowKind::Form`
/// when the body currently is not `Body::Form` (which should not happen while its table is
/// shown, but is handled instead of panicking).
fn rows_for_mut(request: &mut Request, kind: RowKind) -> Option<&mut Vec<KeyValue>> {
    match kind {
        RowKind::Headers => Some(&mut request.headers),
        RowKind::Query => Some(&mut request.query),
        RowKind::Form => match &mut request.body {
            Body::Form(rows) => Some(rows),
            _ => None,
        },
    }
}

impl AppView {
    /// Applies `f` to the request of the tab with id `tab_id`, marks it dirty, and re-renders.
    /// The single place every request edit funnels through, so typing, a checkbox and a row
    /// button all mark the tab dirty the same way (`plans/mvp.md` Phase 9: "any edit marks the
    /// tab dirty").
    pub(crate) fn edit_active_request(
        &mut self,
        tab_id: &str,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Request),
    ) {
        let Some(index) = self.state.tabs.index_of(tab_id) else {
            return;
        };
        let Some(tab) = self.state.tabs.get_mut(index) else {
            return;
        };
        let Some(request) = tab.request_mut() else {
            return;
        };
        f(request);
        self.state.tabs.mark_dirty(index);
        cx.notify();
    }

    /// Enters custom-method edit mode: seeds `method_input` with `prefill` (the current custom
    /// token, or empty for a fresh "Custom..." pick, see [`UrlBar`]'s own "Custom..." handler)
    /// and shows it in place of the method dropdown. Deliberately does not write `prefill` into
    /// `request.method` yet: only a valid, confirmed edit does that (`commit_custom_method`), so
    /// opening the editor and clicking away without typing anything never leaves an empty custom
    /// method behind (`plans/ui-redesign.md` phase 5, reviewer fix item 6).
    pub(crate) fn begin_editing_custom_method(
        &mut self,
        prefill: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = &self.request_editor.method_input {
            input.update(cx, |state, cx| state.set_value(prefill, window, cx));
        }
        self.editing_method = true;
        cx.notify();
    }

    /// Commits a custom-method edit: if `text` (trimmed) is a valid method token
    /// (`state::request_edit::is_valid_custom_method_token`, the same rule `postino-format` uses
    /// when reading a request line), writes it into `request.method` and exits edit mode.
    /// Otherwise, on `Blur` (`exit_on_invalid`) exits edit mode anyway, discarding the invalid
    /// edit; on `PressEnter`, stays in edit mode so the user can keep fixing it.
    pub(crate) fn commit_custom_method(
        &mut self,
        tab_id: &str,
        text: String,
        exit_on_invalid: bool,
        cx: &mut Context<Self>,
    ) {
        let trimmed = text.trim().to_string();
        if request_edit::is_valid_custom_method_token(&trimmed) {
            self.edit_active_request(tab_id, cx, |request| {
                request.method = Method::Custom(trimmed);
            });
            self.editing_method = false;
        } else if exit_on_invalid {
            self.editing_method = false;
        }
        cx.notify();
    }

    /// Applies `f` to one row of a key-value table, then, for [`RowKind::Query`], re-syncs the
    /// URL from the updated table.
    fn edit_row(
        &mut self,
        tab_id: &str,
        kind: RowKind,
        index: usize,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut KeyValue),
    ) {
        self.edit_active_request(tab_id, cx, |request| {
            if let Some(rows) = rows_for_mut(request, kind)
                && let Some(row) = rows.get_mut(index)
            {
                f(row);
            }
            if kind == RowKind::Query {
                request_edit::sync_url_from_query(request);
            }
        });
    }

    /// Appends a new, empty row to a key-value table.
    fn add_row(&mut self, tab_id: &str, kind: RowKind, cx: &mut Context<Self>) {
        self.edit_active_request(tab_id, cx, |request| {
            if let Some(rows) = rows_for_mut(request, kind) {
                request_edit::add_row(rows);
            }
            if kind == RowKind::Query {
                request_edit::sync_url_from_query(request);
            }
        });
    }

    /// Removes a row from a key-value table.
    fn remove_row(&mut self, tab_id: &str, kind: RowKind, index: usize, cx: &mut Context<Self>) {
        self.edit_active_request(tab_id, cx, |request| {
            if let Some(rows) = rows_for_mut(request, kind) {
                request_edit::remove_row(rows, index);
            }
            if kind == RowKind::Query {
                request_edit::sync_url_from_query(request);
            }
        });
    }

    /// Applies the current language to every input placeholder the request editor (and the open
    /// Define variable dialog) cached when it built its inputs. Call it after a language change.
    pub(crate) fn relocalize_request_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = &self.request_editor;
        for table in [&editor.headers, &editor.query, &editor.form] {
            for row in &table.rows {
                row.key.update(cx, |input, cx| {
                    input.set_placeholder(t!("request.kv.key"), window, cx);
                });
                row.value.update(cx, |input, cx| {
                    input.set_placeholder(t!("common.value"), window, cx);
                });
            }
        }
        self.relocalize_define_variable(window, cx);
    }

    /// Renders the active tab's request editor, or a placeholder when no tab is open.
    pub(crate) fn render_request_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab) = self.state.tabs.active() else {
            self.request_editor = RequestEditorEntities::default();
            return placeholder(cx, t!("request.empty.open_request"));
        };
        let tab_id = tab.id.clone();
        let Some(request) = tab.request().cloned() else {
            self.request_editor = RequestEditorEntities::default();
            return placeholder(cx, t!("request.empty.open_request"));
        };
        if self.request_editor.sync(&tab_id, &request, window, cx) {
            // A fresh `method_input` (built empty) would otherwise show as "being edited" with
            // nothing in it: a rebuild happens on a tab switch or a row/body-kind change, none
            // of which should leave a stale custom-method editor open.
            self.editing_method = false;
        }
        // See `plans/ui-redesign.md` phase 5 item 2: computed fresh on every render rather than
        // from a separate `InputState` subscription, since every edit that could change it
        // (URL, query, headers, body) already goes through `edit_active_request`, which calls
        // `cx.notify()` and so triggers exactly this render. Names the pre script sets with
        // `vars.set(...)` are treated as defined too (reviewer fix item 4a): `preview` never
        // runs the script, so it cannot see them resolve for real the way an actual send would.
        let known_from_script = script_heuristics::vars_set_names(&request.pre_script);
        let unknown_names = self
            .current_preview()
            .map(|preview| {
                preview
                    .unknown_variables
                    .into_iter()
                    .map(|unknown| unknown.name)
                    .filter(|name| !known_from_script.contains(name))
                    .collect()
            })
            .unwrap_or_default();

        // The Body tab's code editor (JSON/Text/XML) scrolls its own content and needs a real,
        // determinate height to fill (`plans/ui-redesign.md` phase 5, reviewer fix item B): an
        // ancestor `overflow_y_scroll()` container instead measures its child's intrinsic
        // height, which collapses a `flex_1` editor to a couple of lines. Params/Headers/Form
        // (a `KeyValueTable`), Pre/Post (a fixed-height editor, unaffected either way) and Docs
        // are plain content with no scrolling of their own, so they still need it here.
        let body_uses_code_editor = self.active_request_tab == RequestTab::Body
            && matches!(request.body, Body::Json(_) | Body::Text(_) | Body::Xml(_));
        let content = div().id("request-editor-content").flex_1().min_h_0();
        let content = if body_uses_code_editor {
            content
        } else {
            content.overflow_y_scroll()
        };

        v_flex()
            .size_full()
            .child(self.render_method_url_bar(&tab_id, &request, unknown_names, cx))
            .child(self.render_request_tab_bar(&request, cx))
            .child(
                content
                    .p_2()
                    .child(self.render_request_tab_content(&tab_id, &request, cx)),
            )
            .into_any_element()
    }

    /// Renders the method selector, URL input (joined in one `UrlBar`) and Send/Cancel controls.
    fn render_method_url_bar(
        &self,
        tab_id: &str,
        request: &Request,
        unknown_names: std::collections::HashSet<String>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let Some(url_input) = self.request_editor.url.clone() else {
            return div().into_any_element();
        };

        let method_tab_id = tab_id.to_string();
        let method_weak = weak.clone();
        let editing_method_input = if self.editing_method {
            self.request_editor.method_input.clone()
        } else {
            None
        };
        let app_focus_handle = self.focus_handle.clone();
        let chip_click_weak = weak.clone();
        let chip_unknown_names = unknown_names.clone();
        let url_bar = UrlBar::new(
            request.method.clone(),
            request.url.clone(),
            variable_spans(&request.url),
            unknown_names,
            url_input,
        )
        .editing_method(editing_method_input)
        .on_escape(move |window, cx| app_focus_handle.focus(window, cx))
        .on_method_change(move |method, window, cx| {
            let _ = method_weak.update(cx, |view, cx| {
                if let Method::Custom(prefill) = method {
                    // Only ever reached via `UrlBar`'s "Custom..." menu item: opens the inline
                    // editor instead of committing an empty method right away (see
                    // `begin_editing_custom_method`'s doc comment).
                    view.begin_editing_custom_method(prefill, window, cx);
                } else {
                    view.edit_active_request(&method_tab_id, cx, |request| request.method = method);
                }
            });
        })
        // Opens the Define dialog for a danger (unknown) chip, per `plans/ui-redesign.md`
        // phase 5 item 4 and phase 7 item 3. A defined (accent) chip's click is a no-op: only a
        // chip actually in `unknown_names` (the same set that colors it danger) opens anything.
        .on_chip_click(move |name, window, cx| {
            if chip_unknown_names.contains(&name) {
                let _ = chip_click_weak.update(cx, |view, cx| {
                    view.open_define_variable_dialog(name, window, cx)
                });
            }
        });

        let sending = self.is_sending(tab_id);
        let send_weak = weak.clone();
        let cancel_weak = weak;

        let controls = if sending {
            h_flex()
                .gap_1()
                .child(
                    Button::new("send")
                        .primary()
                        .small()
                        .label(t!("request.send"))
                        .loading(true)
                        .disabled(true),
                )
                .child(
                    Button::new("cancel-send")
                        .ghost()
                        .small()
                        .label(t!("common.cancel"))
                        .on_click(move |_, _, cx| {
                            let _ = cancel_weak.update(cx, |view, cx| view.cancel_send(cx));
                        }),
                )
        } else {
            h_flex().child(
                PrimaryButton::new("send", t!("request.send"))
                    .height(SEND_BUTTON_HEIGHT)
                    .min_width(SEND_BUTTON_MIN_WIDTH)
                    .key_hint(SEND_KEY_HINT)
                    .on_click(move |_, _, cx| {
                        let _ = send_weak.update(cx, |view, cx| view.send_active_tab(cx));
                    }),
            )
        };

        h_flex()
            .gap_2()
            .items_center()
            .pt(px(12.0))
            .pb(px(8.0))
            .px(px(14.0))
            .child(div().flex_1().child(url_bar))
            .child(controls)
            .into_any_element()
    }

    /// Renders the Params/Headers/Body/Pre-request/Post-response/Docs tab bar, with the enabled
    /// row count next to Params and Headers (`plans/ui-redesign.md` phase 5 item 1), and a
    /// "Code" ghost button at the right that opens the snippet dialog (phase 7 item 2).
    fn render_request_tab_bar(&self, request: &Request, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let active = self.active_request_tab;
        let enabled_params = request.query.iter().filter(|row| row.enabled).count();
        let enabled_headers = request.headers.iter().filter(|row| row.enabled).count();

        let mut bar = UnderlineTabs::new("request-tabs");
        for tab in RequestTab::ALL {
            let select_weak = weak.clone();
            let mut item = UnderlineTabItem::new(tab.label()).selected(tab == active);
            match tab {
                RequestTab::Params if enabled_params > 0 => {
                    item = item.count(enabled_params.to_string());
                }
                RequestTab::Headers if enabled_headers > 0 => {
                    item = item.count(enabled_headers.to_string());
                }
                _ => {}
            }
            item = item.on_click(move |_, cx| {
                let _ = select_weak.update(cx, |view, cx| {
                    view.active_request_tab = tab;
                    cx.notify();
                });
            });
            bar = bar.item(item);
        }
        let code_weak = weak;
        bar = bar.suffix(
            GhostButton::new("request-code-snippet", t!("common.code"))
                .icon(gpui_kit::assets::IconName::Code)
                .on_click(move |_, window, cx| {
                    let _ = code_weak.update(cx, |view, cx| view.open_snippet_dialog(window, cx));
                }),
        );
        div().px(px(14.0)).child(bar).into_any_element()
    }

    /// Renders the content of whichever request editor tab is active.
    fn render_request_tab_content(
        &self,
        tab_id: &str,
        request: &Request,
        cx: &Context<Self>,
    ) -> AnyElement {
        match self.active_request_tab {
            RequestTab::Params => self.render_key_value_table(
                tab_id,
                RowKind::Query,
                &request.query,
                &self.request_editor.query,
                cx,
            ),
            RequestTab::Headers => self.render_key_value_table(
                tab_id,
                RowKind::Headers,
                &request.headers,
                &self.request_editor.headers,
                cx,
            ),
            RequestTab::Body => self.render_body_tab(tab_id, request, cx),
            RequestTab::Pre => {
                render_editor_or_placeholder(&self.request_editor.pre_editor, false, cx)
            }
            RequestTab::Post => {
                render_editor_or_placeholder(&self.request_editor.post_editor, false, cx)
            }
            RequestTab::Docs => match &self.request_editor.docs {
                Some(docs) => {
                    let palette = cx.palette();
                    Textarea::new(docs)
                        .context_menu(edit_menu(docs, cx))
                        .h(px(320.0))
                        .border_1()
                        .border_color(palette.border)
                        .rounded(px(RADIUS_MD))
                        .bg(palette.raised)
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
        }
    }

    /// Renders the Body tab: the body type selector, a "Format" button (JSON bodies only) and
    /// either a code editor (JSON/Text/XML) or the Form key-value table.
    fn render_body_tab(&self, tab_id: &str, request: &Request, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let current_kind = BodyKind::of(&request.body);

        let mut type_selector = SegmentedControl::new("body-type");
        for kind in BodyKind::ALL {
            let select_weak = weak.clone();
            let tab_id_owned = tab_id.to_string();
            type_selector = type_selector.item(
                SegmentedItem::new(body_type_display_label(kind))
                    .selected(kind == current_kind)
                    .on_click(move |_, cx| {
                        let _ = select_weak.update(cx, |view, cx| {
                            view.edit_active_request(&tab_id_owned, cx, |request| {
                                request_edit::set_body_kind(request, kind);
                            });
                        });
                    }),
            );
        }

        let format_weak = weak;
        let format_tab_id = tab_id.to_string();
        let is_json = matches!(request.body, Body::Json(_));
        let format_button = GhostButton::new("format-body", t!("request.body.format"))
            .icon(gpui_kit::assets::IconName::WandSparkles)
            .disabled(!is_json)
            .on_click(move |_, _, cx| {
                let _ = format_weak.update(cx, |view, cx| {
                    view.edit_active_request(&format_tab_id, cx, |request| {
                        if let Body::Json(text) = &request.body
                            && let Some(pretty) = request_edit::format_json_body(text)
                        {
                            request.body = Body::Json(pretty);
                        }
                    });
                });
            });

        let body_content = match &request.body {
            Body::None => {
                let palette = cx.palette();
                div()
                    .p_2()
                    .text_color(palette.fg_muted)
                    .child(t!("request.body.empty"))
                    .into_any_element()
            }
            Body::Form(rows) => self.render_key_value_table(
                tab_id,
                RowKind::Form,
                rows,
                &self.request_editor.form,
                cx,
            ),
            Body::Json(_) | Body::Text(_) | Body::Xml(_) => {
                render_editor_or_placeholder(&self.request_editor.body_editor, true, cx)
            }
        };

        v_flex()
            .size_full()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .py(px(8.0))
                    .child(type_selector)
                    .child(format_button),
            )
            .child(div().flex_1().min_h_0().child(body_content))
            .into_any_element()
    }

    /// Renders an editable key-value table (Params, Headers or Form body): one row per entry,
    /// each with an enable checkbox, a key input, a value input and a remove button, plus an
    /// "Add" button at the end.
    fn render_key_value_table(
        &self,
        tab_id: &str,
        kind: RowKind,
        rows: &[KeyValue],
        entities: &KeyValueTableEntities,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let mut table = KeyValueTable::new(("kv-table", kind as u8 as usize));
        for (index, (row, row_entities)) in rows.iter().zip(entities.rows.iter()).enumerate() {
            let toggle_weak = weak.clone();
            let toggle_tab_id = tab_id.to_string();
            let remove_weak = weak.clone();
            let remove_tab_id = tab_id.to_string();
            table = table.row(
                KeyValueRow::with_elements(
                    Input::new(&row_entities.key)
                        .context_menu(edit_menu(&row_entities.key, cx))
                        .w_full(),
                    Input::new(&row_entities.value)
                        .context_menu(edit_menu(&row_entities.value, cx))
                        .w_full(),
                )
                .enabled(row.enabled)
                .on_toggle(move |enabled, _, cx| {
                    let _ = toggle_weak.update(cx, |view, cx| {
                        view.edit_row(&toggle_tab_id, kind, index, cx, |row| row.enabled = enabled);
                    });
                })
                .on_delete(move |_, cx| {
                    let _ = remove_weak.update(cx, |view, cx| {
                        view.remove_row(&remove_tab_id, kind, index, cx);
                    });
                }),
            );
        }

        let add_weak = weak;
        let add_tab_id = tab_id.to_string();
        table
            .on_add(move |_, cx| {
                let _ = add_weak.update(cx, |view, cx| view.add_row(&add_tab_id, kind, cx));
            })
            .into_any_element()
    }
}

/// The body type segmented control's display label: `BodyKind::label()`'s own text, except for
/// `Form`, where the design's segmented control shows the bare word "Form" rather than
/// `BodyKind::label()`'s fuller `"Form (urlencoded)"` (used elsewhere, for example the old body
/// type menu this phase replaced). A display-only override, not a change to `BodyKind::label()`
/// itself (`plans/ui-redesign.md` phase 5, reviewer fix item 7).
fn body_type_display_label(kind: BodyKind) -> String {
    match kind {
        BodyKind::Form => t!("request.body.form").into_owned(),
        other => other.label(),
    }
}

/// Renders a code editor entity if present, or an empty placeholder (only possible transiently,
/// before the first [`RequestEditorEntities::sync`] call), boxed in a bordered, `raised`
/// container with line numbers (`plans/ui-redesign.md` phase 5 item 1). `fill` makes it grow to the rest of the pane's height (the Body tab, reviewer
/// fix item B); otherwise (Pre/Post) it keeps the fixed 320 px height used before this phase.
///
/// `fill` uses `Editor::h(relative(1.0))`, not the generic `Styled::flex_1()`/`min_h_0()`:
/// `Editor` has its own inherent `h(impl Into<DefiniteLength>)` (a `gpui-component` widget that
/// needs a concrete height, in pixels or a percentage of its parent, to lay out its line-based
/// content and gutter), applied before `refine_style` inside its own `RenderOnce` impl.
/// `flex_1()` alone leaves that field `None`, and the editor then sizes to its content instead
/// of the space its `flex_1`/`min_h_0` *wrapper* (see the two call sites) makes available,
/// matching the pattern `gpui-component`'s own `Editor::new(...).h(relative(1.))` call sites use
/// (`gpui-component-0.6.6/src/inspector.rs`).
fn render_editor_or_placeholder(
    editor: &Option<Entity<EditorState>>,
    fill: bool,
    cx: &Context<AppView>,
) -> AnyElement {
    let palette = cx.palette();
    match editor {
        Some(editor) => {
            let editor = Editor::new(editor)
                .context_menu(edit_menu(editor, cx))
                .border_1()
                .border_color(palette.border)
                .rounded(px(RADIUS_MD))
                .bg(palette.raised);
            if fill {
                editor.h(relative(1.0)).into_any_element()
            } else {
                editor.h(px(320.0)).into_any_element()
            }
        }
        None => div().into_any_element(),
    }
}

/// Renders a centered, muted placeholder message filling the panel (`plans/ui-redesign.md`
/// phase 5 item 5, 13/400 `fg_muted`).
fn placeholder(cx: &Context<AppView>, message: impl Into<SharedString>) -> AnyElement {
    let palette = cx.palette();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(div().text_color(palette.fg_muted).child(message.into()))
        .into_any_element()
}
