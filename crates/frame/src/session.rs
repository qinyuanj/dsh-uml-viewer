//! View snapshots shared by frame consumers and resident session restore.
//! The schema retains compatibility with historical LOVE snapshots; the
//! resident returns these values to the host for session persistence.

use edn::Value;
use edn::record::{get, num};
use scene::document::{self, ViewState};

/// View options of every view the resident compiles: source-tree views take
/// the wide layout (#191). A snapshot does not carry them; [`restore`] keeps
/// the loaded state's.
pub fn view_options() -> Value {
    Value::Map(vec![(Value::keyword("wide"), Value::Bool(true))])
}

/// `Session.restore`: reapply a saved view. Invalid focus/proposal/layer/
/// selection targets fall back exactly as a same-diagram display (the scene
/// `restore_session` validator); a snapshot without a discussion context
/// keeps the current one, and its mail cursor stamps `mail_seen` last.
pub fn restore(state: &ViewState, snapshot: &Value) -> ViewState {
    let mut extra = Value::Map(vec![]);
    for key in ["wide", "aspect"] {
        if let Some(value) = get(&state.extra, key) {
            extra.assoc(Value::keyword(key), value.clone());
        }
    }
    for key in ["proposal-id", "proposal", "open-layer", "declutter"] {
        if let Some(value) = get(snapshot, key) {
            extra.assoc(Value::keyword(key), value.clone());
        }
    }
    let mut shaped = ViewState {
        focus: match get(snapshot, "focus") {
            Some(Value::Vector(v)) => v.clone(),
            _ => Vec::new(),
        },
        extra,
        cam_x: get(snapshot, "cam-x").and_then(num).unwrap_or(0.0),
        cam_y: get(snapshot, "cam-y").and_then(num).unwrap_or(0.0),
        zoom: get(snapshot, "zoom").and_then(num).unwrap_or(1.0),
        selected: get(snapshot, "selected").cloned(),
        detail_id: get(snapshot, "detail-id").cloned(),
        ..ViewState::default()
    };
    if let Some(context) = get(snapshot, "discussion-context") {
        shaped.discussion_context = Some(context.clone());
    }
    let mut restored = document::restore_session(state, &shaped);
    if get(snapshot, "discussion-context").is_none() {
        restored.discussion_context = state.discussion_context.clone();
    }
    restored.mail_seen = get(snapshot, "mail-seen")
        .and_then(num)
        .map(|n| n as i64)
        .unwrap_or(state.mail_seen);
    restored
}
