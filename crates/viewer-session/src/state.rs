//! The session state as data (#17): what the plugin host persists with its
//! session so a reopened session or a restarted DeepSeek Harness returns to
//! the same view and discussion context. Schema
//! `dsh-uml-viewer/session-state`, version 1 (`docs/resident-protocol.md`).
//!
//! It carries the same restoration dimensions as the window's restart
//! snapshot (`frame::session::snapshot`) minus the window chrome and the
//! mail cursor, which belong to the egui window and the companion mail.
//! Ids and hits stay EDN text, as in the scene frame.

use edn::Value;
use edn::record::get;
use scene::document::ViewState;
use serde_json::{Value as Json, json};

pub const SCHEMA: &str = "dsh-uml-viewer/session-state";
pub const VERSION: i64 = 1;

fn edn_text(value: &Value) -> Json {
    edn::encode(value).map_or(Json::Null, Json::String)
}

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !matches!(v, Value::Nil))
}

fn optional(value: Option<&Value>) -> Json {
    present(value).map_or(Json::Null, edn_text)
}

/// A discussion context as JSON: `kind` (`real` or `proposal`), the
/// proposal's `proposalId` and `name`, and `edn`, the `:context` map itself
/// (what a panel request quotes, and what a restore reads).
pub fn context_json(context: &Value) -> Json {
    let kind = get(context, "context")
        .and_then(edn::record::local_name)
        .unwrap_or("real");
    let mut out = json!({"kind": kind, "edn": edn_text(context)});
    if let Some(id) = present(get(context, "proposal-id")) {
        out["proposalId"] = edn_text(id);
    }
    if let Some(Value::String(name)) = get(context, "name") {
        out["name"] = json!(name);
    }
    out
}

/// The state a host saves for `state`.
pub fn to_json(state: &ViewState) -> Json {
    json!({
        "schema": SCHEMA,
        "version": VERSION,
        // A waiting canvas has displayed nothing yet: its path is only the
        // project default, which a restore must not show unasked.
        "diagram": if state.waiting { None } else { state.path.as_deref() },
        "context": state.discussion_context.as_ref().map_or(Json::Null, context_json),
        "focus": state.focus.iter().map(edn_text).collect::<Vec<_>>(),
        "proposalId": optional(get(&state.extra, "proposal-id")),
        "openLayer": optional(get(&state.extra, "open-layer")),
        "declutter": optional(get(&state.extra, "declutter")),
        "camera": {"x": state.cam_x, "y": state.cam_y, "zoom": state.zoom},
        "selected": optional(state.selected.as_ref()),
        "detail": optional(state.detail_id.as_ref()),
    })
}

fn parse_edn(json: &Json, key: &str) -> Result<Option<Value>, String> {
    match json.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(text)) => edn::parse(text)
            .map(Some)
            .map_err(|e| format!("state {key}: {e}")),
        Some(other) => Err(format!("state {key} must be EDN text, not {other}")),
    }
}

/// The diagram a saved state names, if any.
pub fn diagram(json: &Json) -> Option<String> {
    json.get("diagram")
        .and_then(Json::as_str)
        .map(str::to_owned)
}

/// A saved state as the restart snapshot `frame::session::restore` applies.
/// Unknown keys are ignored; another schema or version is refused.
pub fn snapshot(json: &Json) -> Result<Value, String> {
    if json.get("schema").and_then(Json::as_str) != Some(SCHEMA) {
        return Err(format!("state schema must be {SCHEMA}"));
    }
    if json.get("version").and_then(Json::as_i64) != Some(VERSION) {
        return Err(format!("state version must be {VERSION}"));
    }
    let kw = Value::keyword;
    let mut m = Value::Map(vec![]);
    if let Some(path) = diagram(json) {
        m.assoc(kw("path"), Value::String(path));
    }
    if let Some(context) = json.get("context").filter(|c| !c.is_null())
        && let Some(value) = parse_edn(context, "edn")?
    {
        m.assoc(kw("discussion-context"), value);
    }
    let focus = match json.get("focus") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| match item {
                Json::String(text) => edn::parse(text).map_err(|e| format!("state focus: {e}")),
                other => Err(format!("state focus entries are EDN text, not {other}")),
            })
            .collect::<Result<_, _>>()?,
        Some(other) => return Err(format!("state focus must be an array, not {other}")),
    };
    m.assoc(kw("focus"), Value::Vector(focus));
    if let Some(id) = parse_edn(json, "proposalId")? {
        m.assoc(kw("proposal-id"), id);
        m.assoc(kw("proposal"), Value::Bool(true));
    }
    for (key, edn_key) in [
        ("openLayer", "open-layer"),
        ("declutter", "declutter"),
        ("selected", "selected"),
        ("detail", "detail-id"),
    ] {
        if let Some(value) = parse_edn(json, key)? {
            m.assoc(kw(edn_key), value);
        }
    }
    let camera = json.get("camera").cloned().unwrap_or(Json::Null);
    for (key, edn_key) in [("x", "cam-x"), ("y", "cam-y"), ("zoom", "zoom")] {
        if let Some(n) = camera.get(key).and_then(Json::as_f64) {
            m.assoc(kw(edn_key), Value::Float(n));
        }
    }
    Ok(m)
}
