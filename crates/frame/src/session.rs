//! Restart session snapshots (port of `love/uml/application/session.lua` and
//! `adapters/session.lua`): the snapshot schema the LOVE viewer writes on
//! `:quit-for-restart` and this viewer reads on `--restart`. The file itself
//! is EDN under `.uml-viewer/session.edn`; reads and writes go through the
//! app's atomic write boundary.

use std::path::{Path, PathBuf};

use edn::Value;
use edn::record::{get, num};
use scene::document::{self, ViewState};

/// Default window in points (the LOVE viewer's 1500x920 view units); egui
/// multiplies points by pixels-per-point, so a scaled display keeps it.
pub const WINDOW_SIZE: [f32; 2] = [1500.0, 920.0];
pub const WINDOW_MIN: [f32; 2] = [640.0, 480.0];

/// Window chrome travels beside the view: window size (LOVE's window px =
/// this viewer's logical points at scale 1) and the DPI-independent
/// inspector width (the Lua `chrome_keys`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Chrome {
    pub window_w: Option<f64>,
    pub window_h: Option<f64>,
    pub inspector_w: Option<f64>,
}

fn size(saved: Option<f64>, default: f64, minimum: f64, ui: f64) -> f64 {
    let target = saved.filter(|s| *s > 0.0).unwrap_or(default * ui);
    target.max(minimum * ui)
}

/// The window the saved (or absent) chrome opens at (port of
/// `Display.window_size`): saved sizes restore as-is, never below
/// 640x480 view units.
pub fn window_size(chrome: &Chrome, ui: f64) -> [f64; 2] {
    let ui = if ui > 0.0 { ui } else { 1.0 };
    [
        size(
            chrome.window_w,
            WINDOW_SIZE[0] as f64,
            WINDOW_MIN[0] as f64,
            ui,
        ),
        size(
            chrome.window_h,
            WINDOW_SIZE[1] as f64,
            WINDOW_MIN[1] as f64,
            ui,
        ),
    ]
}

fn saved_number(snapshot: &Value, key: &str) -> Option<f64> {
    get(snapshot, key).and_then(num).filter(|v| *v > 0.0)
}

/// The chrome fields of a snapshot; a legacy snapshot without them yields
/// defaults.
pub fn chrome(snapshot: Option<&Value>) -> Chrome {
    match snapshot {
        Some(s @ Value::Map(_)) => Chrome {
            window_w: saved_number(s, "window-w"),
            window_h: saved_number(s, "window-h"),
            inspector_w: saved_number(s, "inspector-width"),
        },
        _ => Chrome::default(),
    }
}

/// `Session.snapshot`: every restoration dimension in the LOVE schema.
pub fn snapshot(state: &ViewState, chrome: &Chrome) -> Value {
    let mut m = Value::Map(vec![]);
    let kw = Value::keyword;
    let numval = |x: f64| {
        if x.fract() == 0.0 && x.abs() < 9.0e15 {
            Value::Integer(x as i64)
        } else {
            Value::Float(x)
        }
    };
    if let Some(w) = chrome.window_w {
        m.assoc(kw("window-w"), numval(w));
    }
    if let Some(h) = chrome.window_h {
        m.assoc(kw("window-h"), numval(h));
    }
    if let Some(w) = chrome.inspector_w {
        m.assoc(kw("inspector-width"), numval(w));
    }
    if let Some(path) = &state.path {
        m.assoc(kw("path"), Value::String(path.clone()));
    }
    if let Some(context) = &state.discussion_context {
        m.assoc(kw("discussion-context"), context.clone());
    }
    m.assoc(kw("mail-seen"), Value::Integer(state.mail_seen));
    m.assoc(kw("focus"), Value::Vector(state.focus.clone()));
    for key in ["proposal-id", "proposal", "open-layer", "declutter"] {
        if let Some(value) = get(&state.extra, key) {
            m.assoc(kw(key), value.clone());
        }
    }
    m.assoc(kw("cam-x"), numval(state.cam_x));
    m.assoc(kw("cam-y"), numval(state.cam_y));
    m.assoc(kw("zoom"), numval(state.zoom));
    if let Some(selected) = &state.selected {
        m.assoc(kw("selected"), selected.clone());
    }
    if let Some(detail) = &state.detail_id {
        m.assoc(kw("detail-id"), detail.clone());
    }
    m
}

/// View options of every view this viewer compiles: source-tree views take
/// the wide layout (#191). A snapshot does not carry them; [`restore`] keeps
/// the loaded state's.
pub fn view_options() -> Value {
    Value::Map(vec![(Value::keyword("wide"), Value::Bool(true))])
}

/// `Session.path`: explicit path wins, then the saved one, then the default.
pub fn path(explicit: Option<&Path>, snapshot: Option<&Value>, default: &Path) -> PathBuf {
    if let Some(explicit) = explicit {
        return explicit.to_path_buf();
    }
    if let Some(Value::String(saved)) = snapshot.and_then(|s| get(s, "path")) {
        return PathBuf::from(saved);
    }
    default.to_path_buf()
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

pub fn file_path(root: &Path) -> PathBuf {
    root.join(".uml-viewer/session.edn")
}

/// `SessionFile.read`: the parsed map, or `None` for missing/malformed files.
pub fn read(root: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(file_path(root)).ok()?;
    match edn::parse(&text) {
        Ok(v @ Value::Map(_)) => Some(v),
        _ => None,
    }
}

/// `SessionFile.save`: atomic replace (temp + rename), no trailing newline,
/// exactly like the LOVE writer.
pub fn save(root: &Path, snapshot: &Value) -> std::io::Result<()> {
    let text = edn::encode(snapshot).map_err(|e| std::io::Error::other(e.to_string()))?;
    crate::project_files::write_atomic(&file_path(root), &text)
}
