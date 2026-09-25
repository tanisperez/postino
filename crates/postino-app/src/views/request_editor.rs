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
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{
    Editor, EditorState, Input, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use postino_core::{Body, KeyValue, Method, Request};

use crate::state::request_edit::{self, BodyKind};
use crate::state::ui_tabs::RequestTab;

use super::root::AppView;

/// The standard HTTP methods offered by the method selector. A request whose method is a custom
/// token (`plans/mvp.md`, section 3.2) still displays and sends correctly; picking one of these
/// simply replaces it, there is no way to type a custom method in this editor yet.
const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

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
    let key = cx.new(|cx| InputState::new(window, cx).placeholder("Key"));
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

    let value = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
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
    /// URL without changing the build key, see [`BuildKey`]'s docs).
    fn sync(
        &mut self,
        tab_id: &str,
        request: &Request,
        window: &mut Window,
        cx: &mut Context<AppView>,
    ) {
        let key = BuildKey::of(tab_id, request);
        if self.built_for.as_ref() != Some(&key) {
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
        f(&mut tab.request);
        self.state.tabs.mark_dirty(index);
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

    /// Renders the active tab's request editor, or a placeholder when no tab is open.
    pub(crate) fn render_request_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab) = self.state.tabs.active() else {
            self.request_editor = RequestEditorEntities::default();
            return placeholder(cx, "Open a request from the sidebar to edit it here.");
        };
        let tab_id = tab.id.clone();
        let request = tab.request.clone();
        self.request_editor.sync(&tab_id, &request, window, cx);

        v_flex()
            .size_full()
            .child(self.render_method_url_bar(&tab_id, &request, cx))
            .child(self.render_request_tab_bar(cx))
            .child(
                div()
                    .id("request-editor-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .child(self.render_request_tab_content(&tab_id, &request, cx)),
            )
            .into_any_element()
    }

    /// Renders the method selector, URL input and Send/Cancel controls.
    fn render_method_url_bar(
        &self,
        tab_id: &str,
        request: &Request,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let method_label = request.method.to_string();
        let method_tab_id = tab_id.to_string();
        let method_weak = weak.clone();
        let method_button = Button::new("method-select")
            .small()
            .label(method_label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for name in METHODS {
                    let select_weak = method_weak.clone();
                    let tab_id = method_tab_id.clone();
                    menu = menu.item(PopupMenuItem::new(name).on_click(move |_, _, cx| {
                        // `Method::from_str` never fails (`Err = Infallible`): an unrecognized
                        // token simply becomes `Method::Custom`, so this match is exhaustive.
                        let Ok(method) = name.parse::<Method>();
                        let _ = select_weak.update(cx, |view, cx| {
                            view.edit_active_request(&tab_id, cx, |request| {
                                request.method = method
                            });
                        });
                    }));
                }
                menu
            });

        let url_input = self.request_editor.url.clone();
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
                        .label("Send")
                        .loading(true)
                        .disabled(true),
                )
                .child(
                    Button::new("cancel-send")
                        .ghost()
                        .small()
                        .label("Cancel")
                        .on_click(move |_, _, cx| {
                            let _ = cancel_weak.update(cx, |view, cx| view.cancel_send(cx));
                        }),
                )
        } else {
            h_flex().child(
                Button::new("send")
                    .primary()
                    .small()
                    .icon(Icon::new(IconName::Play).small())
                    .label("Send")
                    .tooltip("Send (Ctrl+Enter)")
                    .on_click(move |_, _, cx| {
                        let _ = send_weak.update(cx, |view, cx| view.send_active_tab(cx));
                    }),
            )
        };

        h_flex()
            .gap_2()
            .items_center()
            .p_2()
            .child(method_button)
            .children(url_input.map(|input| div().flex_1().child(Input::new(&input))))
            .child(controls)
            .into_any_element()
    }

    /// Renders the Params/Headers/Body/Pre-request/Post-response/Docs tab bar.
    fn render_request_tab_bar(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let active = self.active_request_tab;
        let selected_index = RequestTab::ALL
            .iter()
            .position(|tab| *tab == active)
            .unwrap_or(0);
        let mut bar = TabBar::new("request-tabs").selected_index(selected_index);
        for tab in RequestTab::ALL {
            let select_weak = weak.clone();
            bar = bar.child(Tab::new().label(tab.label()).on_click(move |_, _, cx| {
                let _ = select_weak.update(cx, |view, cx| {
                    view.active_request_tab = tab;
                    cx.notify();
                });
            }));
        }
        bar.into_any_element()
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
            RequestTab::Pre => render_editor_or_placeholder(&self.request_editor.pre_editor),
            RequestTab::Post => render_editor_or_placeholder(&self.request_editor.post_editor),
            RequestTab::Docs => match &self.request_editor.docs {
                Some(docs) => Textarea::new(docs).h(px(320.)).into_any_element(),
                None => div().into_any_element(),
            },
        }
    }

    /// Renders the Body tab: the body type selector plus either a code editor (JSON/Text/XML) or
    /// the Form key-value table.
    fn render_body_tab(&self, tab_id: &str, request: &Request, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let tab_id_owned = tab_id.to_string();
        let current_kind = BodyKind::of(&request.body);
        let type_button = Button::new("body-type")
            .small()
            .label(current_kind.label())
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for kind in BodyKind::ALL {
                    let select_weak = weak.clone();
                    let tab_id = tab_id_owned.clone();
                    menu = menu.item(
                        PopupMenuItem::new(kind.label())
                            .checked(kind == current_kind)
                            .on_click(move |_, _, cx| {
                                let _ = select_weak.update(cx, |view, cx| {
                                    view.edit_active_request(&tab_id, cx, |request| {
                                        request_edit::set_body_kind(request, kind);
                                    });
                                });
                            }),
                    );
                }
                menu
            });

        let body_content = match &request.body {
            Body::None => div()
                .p_2()
                .text_color(cx.theme().muted_foreground)
                .child("This request has no body.")
                .into_any_element(),
            Body::Form(rows) => self.render_key_value_table(
                tab_id,
                RowKind::Form,
                rows,
                &self.request_editor.form,
                cx,
            ),
            Body::Json(_) | Body::Text(_) | Body::Xml(_) => {
                render_editor_or_placeholder(&self.request_editor.body_editor)
            }
        };

        v_flex()
            .gap_2()
            .child(type_button)
            .child(body_content)
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
        let mut list = v_flex().gap_1();
        for (index, (row, row_entities)) in rows.iter().zip(entities.rows.iter()).enumerate() {
            let toggle_weak = weak.clone();
            let toggle_tab_id = tab_id.to_string();
            let remove_weak = weak.clone();
            let remove_tab_id = tab_id.to_string();
            list = list.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Checkbox::new(("row-enabled", index))
                            .checked(row.enabled)
                            .on_click(move |enabled, _, cx| {
                                let enabled = *enabled;
                                let _ = toggle_weak.update(cx, |view, cx| {
                                    view.edit_row(&toggle_tab_id, kind, index, cx, |row| {
                                        row.enabled = enabled
                                    });
                                });
                            }),
                    )
                    .child(Input::new(&row_entities.key).w(px(180.)))
                    .child(Input::new(&row_entities.value).flex_1())
                    .child(
                        Button::new(("remove-row", index))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Delete).small())
                            .tooltip("Remove")
                            .on_click(move |_, _, cx| {
                                let _ = remove_weak.update(cx, |view, cx| {
                                    view.remove_row(&remove_tab_id, kind, index, cx);
                                });
                            }),
                    ),
            );
        }

        let add_weak = weak;
        let add_tab_id = tab_id.to_string();
        list.child(
            Button::new("add-row")
                .ghost()
                .small()
                .icon(Icon::new(IconName::Plus).small())
                .label("Add")
                .on_click(move |_, _, cx| {
                    let _ = add_weak.update(cx, |view, cx| view.add_row(&add_tab_id, kind, cx));
                }),
        )
        .into_any_element()
    }
}

/// Renders a code editor entity if present, or an empty placeholder (only possible transiently,
/// before the first [`RequestEditorEntities::sync`] call).
fn render_editor_or_placeholder(editor: &Option<Entity<EditorState>>) -> AnyElement {
    match editor {
        Some(editor) => Editor::new(editor).h(px(320.)).into_any_element(),
        None => div().into_any_element(),
    }
}

/// Renders a centered, muted placeholder message filling the panel.
fn placeholder(cx: &Context<AppView>, message: &str) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(message.to_string()),
        )
        .into_any_element()
}
