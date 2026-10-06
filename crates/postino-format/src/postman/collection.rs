//! Mapping a Postman collection (v2.1, v2.0 accepted when it parses the same) into an
//! [`ImportPlan`]. The mapping is described in `docs/postman-import.md`.

use postino_core::{Body, KeyValue, Method, Request};

use super::error::PostmanError;
use super::model::{
    RawAuth, RawAuthParam, RawBody, RawCollection, RawEvent, RawItem, RawRequest, RawUrl,
    RawUrlDetailed,
};
use super::value::value_to_text;

/// The fixed first line written on top of every imported script: Postman's `pm.*`
/// API has no equivalent in Postino, so the script body is commented out rather than translated.
const IMPORT_SCRIPT_HEADER: &str =
    "// Imported from Postman. The pm.* API is not supported, adapt it.";

/// The result of importing a Postman collection: a tree of folders and requests, plus the
/// collection's own variables and anything that could not be mapped faithfully. Pure in-memory
/// data, ready for `postino-workspace` to write to disk.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    /// The collection name (`info.name`), used as the name of the new top-level folder and of
    /// the environment created from the collection's own variables.
    pub collection_name: String,
    /// The root folder of the imported tree. Its own name is not meaningful, only its `folders`
    /// and `requests` fields are used.
    pub root: ImportFolder,
    /// The collection's own `variable` array, to become the environment named after the
    /// collection (`environments/<collection name>.env`).
    pub variables: Vec<KeyValue>,
    /// Everything that could not be mapped faithfully, in the order it was found.
    pub warnings: Vec<String>,
}

/// One folder of an [`ImportPlan`], mirroring a Postman collection folder.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportFolder {
    /// The folder name, as it appeared in the collection.
    pub name: String,
    /// Nested folders, in collection order.
    pub folders: Vec<ImportFolder>,
    /// Requests directly inside this folder, as `(name, Request)` pairs, in collection order.
    pub requests: Vec<(String, Request)>,
}

/// Parses the text of a Postman collection export into an [`ImportPlan`].
pub fn parse_collection(text: &str) -> Result<ImportPlan, PostmanError> {
    let raw: RawCollection =
        serde_json::from_str(text).map_err(PostmanError::InvalidCollectionJson)?;
    let mut warnings = Vec::new();
    warn_about_dropped_events(
        raw.event.as_deref(),
        "collection",
        &raw.info.name,
        &mut warnings,
    );
    let (folders, requests) =
        map_items(&raw.item, raw.auth.as_ref(), &raw.info.name, &mut warnings);
    let mut variables = Vec::new();
    for variable in raw.variable {
        let Some(key) = variable.key else { continue };
        let context = format!("variable \"{key}\"");
        let value = value_to_text(variable.value.as_ref(), &context, &mut warnings);
        variables.push(KeyValue::new(key, value));
    }
    Ok(ImportPlan {
        collection_name: raw.info.name,
        root: ImportFolder {
            name: String::new(),
            folders,
            requests,
        },
        variables,
        warnings,
    })
}

/// Maps one `item` array (the collection's own, or a folder's) into its child folders and
/// requests, resolving auth inheritance as it goes.
///
/// `inherited_auth` is the auth block that applies to a request in this list when the request
/// itself has no auth of its own or explicitly uses `"inherit"`. `path` is a human readable
/// location, used to prefix warnings.
fn map_items(
    items: &[RawItem],
    inherited_auth: Option<&RawAuth>,
    path: &str,
    warnings: &mut Vec<String>,
) -> (Vec<ImportFolder>, Vec<(String, Request)>) {
    let mut folders = Vec::new();
    let mut requests = Vec::new();

    for item in items {
        let name = item.name.clone().unwrap_or_default();
        let item_path = format!("{path}/{name}");

        if let Some(children) = &item.item {
            let folder_auth = item.auth.as_ref().or(inherited_auth);
            warn_about_dropped_events(item.event.as_deref(), "folder", &name, warnings);
            let (nested_folders, nested_requests) =
                map_items(children, folder_auth, &item_path, warnings);
            folders.push(ImportFolder {
                name,
                folders: nested_folders,
                requests: nested_requests,
            });
        } else if let Some(request) = &item.request {
            let auth = effective_auth(request.auth.as_ref(), inherited_auth);
            let events = item.event.as_deref().unwrap_or(&[]);
            let mapped = map_request(request, events, auth, &item_path, warnings);
            requests.push((name, mapped));
        }
        // An item with neither `item` nor `request` is malformed and has nothing to map.
    }

    (folders, requests)
}

/// Warns about a `prerequest`/`test` event found at the collection or folder level.
///
/// Only per-request scripts are imported: a collection- or
/// folder-level script has no single request to attach to, so it is never imported. This only
/// records that it was dropped, naming `level` (`"collection"` or `"folder"`) and `name`, so the
/// user knows some code was left behind.
fn warn_about_dropped_events(
    events: Option<&[RawEvent]>,
    level: &str,
    name: &str,
    warnings: &mut Vec<String>,
) {
    for event in events.into_iter().flatten() {
        let Some(kind) = event.listen.as_deref() else {
            continue;
        };
        if kind == "prerequest" || kind == "test" {
            warnings.push(format!(
                "{level} \"{name}\": a {kind} script is not imported at the {level} level, only per request"
            ));
        }
    }
}

/// Resolves which auth block applies to a request: its own, unless it is absent or explicitly
/// `"inherit"`, in which case the collection/folder level auth applies instead.
/// Collection and folder level auth is inherited by requests that use `inherit` or no auth.
fn effective_auth<'a>(
    own_auth: Option<&'a RawAuth>,
    inherited_auth: Option<&'a RawAuth>,
) -> Option<&'a RawAuth> {
    match own_auth {
        Some(auth) if auth.kind != "inherit" => Some(auth),
        Some(_) => inherited_auth,
        None => inherited_auth,
    }
}

/// Maps one Postman request into a Postino [`Request`].
fn map_request(
    request: &RawRequest,
    events: &[RawEvent],
    auth: Option<&RawAuth>,
    path: &str,
    warnings: &mut Vec<String>,
) -> Request {
    let method = parse_method(request.method.as_deref().unwrap_or("GET"));
    let (url, query) = map_url(request.url.as_ref(), path, warnings);

    let mut headers = Vec::new();
    for header in request.header.iter().flatten() {
        let context = format!("{path}: header \"{}\"", header.key);
        headers.push(KeyValue {
            key: header.key.clone(),
            value: value_to_text(header.value.as_ref(), &context, warnings),
            enabled: !header.disabled,
        });
    }
    if let Some(auth) = auth {
        headers.extend(auth_headers(auth, path, warnings));
    }

    let body = map_body(request.body.as_ref(), path, warnings);
    let docs = request
        .description
        .as_ref()
        .map(super::model::RawDescription::as_text)
        .unwrap_or_default();

    Request {
        method,
        url,
        headers,
        query,
        body,
        pre_script: map_script(events, "prerequest"),
        post_script: map_script(events, "test"),
        docs,
    }
}

/// Parses a Postman method token, which is always one of the standard uppercase methods in
/// practice. [`Method::from_str`] never fails, an unrecognized token simply becomes
/// [`Method::Custom`].
fn parse_method(token: &str) -> Method {
    match token.parse::<Method>() {
        Ok(method) => method,
        // `Method::from_str`'s error type is `Infallible`: this branch can never run.
        Err(never) => match never {},
    }
}

/// Maps a `url`, splitting the query string out of `raw` and combining it with the structured
/// `query` array: `url.raw` without its query string, plus `url.query`.
fn map_url(
    url: Option<&RawUrl>,
    path: &str,
    warnings: &mut Vec<String>,
) -> (String, Vec<KeyValue>) {
    let Some(url) = url else {
        warnings.push(format!(
            "{path}: request has no URL, imported with an empty URL"
        ));
        return (String::new(), Vec::new());
    };
    match url {
        RawUrl::Raw(raw) => {
            let (base, query_string) = split_query_string(raw);
            (base, parse_query_string(&query_string))
        }
        RawUrl::Detailed(detailed) => map_detailed_url(detailed, path, warnings),
    }
}

/// Maps the detailed object form of a `url`.
fn map_detailed_url(
    detailed: &RawUrlDetailed,
    path: &str,
    warnings: &mut Vec<String>,
) -> (String, Vec<KeyValue>) {
    let raw = detailed.raw.clone().unwrap_or_default();
    let (base, query_string) = split_query_string(&raw);
    if detailed.query.is_empty() {
        (base, parse_query_string(&query_string))
    } else {
        let mut query = Vec::new();
        for param in &detailed.query {
            let key = param.key.clone().unwrap_or_default();
            let context = format!("{path}: query \"{key}\"");
            let value = value_to_text(param.value.as_ref(), &context, warnings);
            query.push(KeyValue {
                key,
                value,
                enabled: !param.disabled,
            });
        }
        (base, query)
    }
}

/// Splits a raw URL at its first `?`, returning `(url_without_query, query_string)`. When there
/// is no `?`, the whole string is the URL and the query string is empty.
fn split_query_string(raw: &str) -> (String, String) {
    match raw.split_once('?') {
        Some((base, query)) => (base.to_string(), query.to_string()),
        None => (raw.to_string(), String::new()),
    }
}

/// Parses a `key=value&key2=value2` query string into [`KeyValue`] entries. Used only for the
/// plain string form of a `url` (or when the detailed form omits its `query` array), since the
/// object form's `query` array already carries this information.
fn parse_query_string(query: &str) -> Vec<KeyValue> {
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => KeyValue::new(key, value),
            None => KeyValue::new(pair, ""),
        })
        .collect()
}

/// Maps an `auth` block into the headers it implies: bearer,
/// basic and apikey in header location become headers.
fn auth_headers(auth: &RawAuth, path: &str, warnings: &mut Vec<String>) -> Vec<KeyValue> {
    match auth.kind.as_str() {
        "bearer" => {
            let context = format!("{path}: auth bearer \"token\"");
            let token = find_param(&auth.bearer, "token", &context, warnings).unwrap_or_default();
            vec![KeyValue::new("Authorization", format!("Bearer {token}"))]
        }
        "basic" => {
            let username_context = format!("{path}: auth basic \"username\"");
            let password_context = format!("{path}: auth basic \"password\"");
            let username = find_param(&auth.basic, "username", &username_context, warnings)
                .unwrap_or_default();
            let password = find_param(&auth.basic, "password", &password_context, warnings)
                .unwrap_or_default();
            let encoded = postino_core::functions::base64_encode(&format!("{username}:{password}"));
            vec![KeyValue::new("Authorization", format!("Basic {encoded}"))]
        }
        "apikey" => {
            let key_context = format!("{path}: auth apikey \"key\"");
            let value_context = format!("{path}: auth apikey \"value\"");
            let in_context = format!("{path}: auth apikey \"in\"");
            let key = find_param(&auth.apikey, "key", &key_context, warnings).unwrap_or_default();
            let value =
                find_param(&auth.apikey, "value", &value_context, warnings).unwrap_or_default();
            let location = find_param(&auth.apikey, "in", &in_context, warnings)
                .unwrap_or_else(|| "header".to_string());
            if location == "header" {
                vec![KeyValue::new(key, value)]
            } else {
                warnings.push(format!(
                    "{path}: apikey auth in \"{location}\" is not supported, only header, skipped"
                ));
                Vec::new()
            }
        }
        "noauth" | "" => Vec::new(),
        other => {
            warnings.push(format!(
                "{path}: auth type \"{other}\" is not supported, skipped"
            ));
            Vec::new()
        }
    }
}

/// Finds the auth param named `key`, returning its value converted to text, or `None` if there
/// is no such param at all (used to tell "absent" apart from "present but empty", which matters
/// for the apikey `"in"` location default).
fn find_param(
    params: &[RawAuthParam],
    key: &str,
    context: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    params
        .iter()
        .find(|param| param.key == key)
        .map(|param| value_to_text(param.value.as_ref(), context, warnings))
}

/// Maps a `body` object into a Postino [`Body`].
fn map_body(body: Option<&RawBody>, path: &str, warnings: &mut Vec<String>) -> Body {
    let Some(body) = body else {
        return Body::None;
    };
    match body.mode.as_deref() {
        None => Body::None,
        Some("raw") => map_raw_body(body),
        Some("urlencoded") => {
            let mut fields = Vec::new();
            for param in &body.urlencoded {
                let context = format!("{path}: form field \"{}\"", param.key);
                fields.push(KeyValue {
                    key: param.key.clone(),
                    value: value_to_text(param.value.as_ref(), &context, warnings),
                    enabled: !param.disabled,
                });
            }
            Body::Form(fields)
        }
        Some(mode @ ("formdata" | "file" | "graphql")) => {
            warnings.push(format!(
                "{path}: {mode} body is not supported, imported as empty"
            ));
            Body::None
        }
        Some(other) => {
            warnings.push(format!(
                "{path}: unknown body mode \"{other}\", imported as empty"
            ));
            Body::None
        }
    }
}

/// Maps a `mode: "raw"` body, dispatching on `options.raw.language` (default `text`).
fn map_raw_body(body: &RawBody) -> Body {
    let text = body.raw.clone().unwrap_or_default();
    let language = body
        .options
        .as_ref()
        .and_then(|options| options.raw.as_ref())
        .and_then(|raw_options| raw_options.language.as_deref())
        .unwrap_or("text");
    match language {
        "json" => Body::Json(text),
        "xml" => Body::Xml(text),
        _ => Body::Text(text),
    }
}

/// Builds the `pre` or `post` script for the events whose `listen` matches `kind` (`"prerequest"`
/// or `"test"`), commented out line by line with the fixed header. Returns an empty string when
/// there is no such event, or its script has no lines, so the section is omitted when the request
/// is serialized.
fn map_script(events: &[RawEvent], kind: &str) -> String {
    let lines: Vec<String> = events
        .iter()
        .filter(|event| event.listen.as_deref() == Some(kind))
        .filter_map(|event| event.script.as_ref())
        .filter_map(|script| script.exec.as_ref())
        .flat_map(super::model::RawExec::lines)
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut commented = vec![IMPORT_SCRIPT_HEADER.to_string()];
    commented.extend(lines.iter().map(|line| format!("// {line}")));
    commented.join("\n")
}
