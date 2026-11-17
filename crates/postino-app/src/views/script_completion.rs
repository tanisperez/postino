//! The autocompletion of the `::: pre` and `::: post` code editors: the script API, variable
//! and header names, and the keys of the last JSON response.
//!
//! telaio's editor asks [`script_items`] for rows while the user types (when [`script_trigger`]
//! says so) and on `Ctrl+Space`, and draws its own menu, with the arrow keys, `Enter`, `Tab` and
//! `Escape`. What the rows are comes from `state::script_completion`; the rows only read the
//! [`ScriptCompletionData`] the request editor refreshes, so typing never rescans the response.

use std::rc::Rc;
use std::sync::Arc;

use rust_i18n::t;
use telaio_editor::CompletionItem;

use crate::state::header_completion::HEADER_NAMES;
use crate::state::script_completion::{
    ItemKind, JsonShape, ScriptContext, ScriptKind, completions, is_trigger,
};
use crate::state::variable_hint::{self, VariableContext};

/// What the script editors complete besides the API: the variables and the last response of
/// the active tab. Shared between the request editor (which refreshes it) and the providers.
#[derive(Default)]
pub struct ScriptCompletionData {
    pub variables: Rc<VariableContext>,
    pub response: Option<Arc<JsonShape>>,
}

/// The rows of a `kind` script editor for the cursor at byte `offset` of `text`, from `data`.
/// `explicit` when asked with `Ctrl+Space`, which lists the globals even with nothing typed.
pub fn script_items(
    kind: ScriptKind,
    data: &ScriptCompletionData,
    text: &str,
    offset: usize,
    explicit: bool,
) -> Vec<CompletionItem> {
    let variables = variable_hint::completions(&data.variables, false);
    let context = ScriptContext {
        kind,
        variables: &variables,
        header_names: HEADER_NAMES,
        response: data.response.as_deref(),
    };
    completions(text, offset, &context, explicit)
        .into_iter()
        .map(|item| CompletionItem {
            detail: match (item.kind, item.detail) {
                (ItemKind::Variable, None) => Some("set by this script".into()),
                (ItemKind::Variable, Some(value)) if value.is_empty() => {
                    Some(t!("request.variable.empty").into_owned().into())
                }
                (_, detail) => detail.map(Into::into),
            },
            documentation: item.documentation.map(Into::into),
            label: item.label.into(),
            replace: item.replace,
            insert: item.insert,
        })
        .collect()
}

/// Whether the text just typed in a script editor opens its menu.
pub fn script_trigger(typed: &str) -> bool {
    is_trigger(typed)
}
