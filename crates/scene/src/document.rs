//! The scene-core facade (port of `love/uml/application/document.lua`).
//!
//! `compile_view` is the public entry point the viewer crate consumes: an IR
//! document, optional quality snapshots, a namespace focus and view options
//! in; a prepared scene out. Proposal write-back takes an injected
//! [`Boundary`] so this module never touches the filesystem itself.

use crate::compose;
use crate::hierarchy as H;
use crate::ir;
use crate::overlay::{self, Snapshot};
use edn::Value;
use edn::record::{drop_keys, get, local_name, name, numval};
use std::collections::HashMap;

/// Filesystem capability for proposal write-back, supplied by the caller.
pub trait Boundary {
    fn exists(&self, path: &str) -> bool;
    fn read(&self, path: &str) -> Option<String>;
    fn write(&self, path: &str, text: &str) -> bool;
}

fn vmap(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(k, v)| (Value::keyword(k), v))
            .collect(),
    )
}

/// Resolve the explicit proposal, or the first one for a legacy proposal view.
pub(crate) fn active_proposal_id(proposals: &[Value], extra: &Value) -> Option<Value> {
    get(extra, "proposal-id")
        .filter(|p| !matches!(p, Value::Nil))
        .cloned()
        .or_else(|| {
            let proposed =
                edn::record::flag(extra, "proposal") || matches!(extra, Value::Bool(true));
            if proposed {
                proposals.first().and_then(|p| get(p, "id").cloned())
            } else {
                None
            }
        })
}

/// Quality snapshots, focus and view options in; a scene out.
///
/// * `snapshots` — quality records; `None`/empty leaves the document alone.
/// * `focus` — namespace path keywords (`[]` is the root).
/// * `extra` — `:proposal-id`, `:proposal`, `:open-layer`, `:declutter`,
///   and `:wide` (with an optional `:aspect`, default [`WIDE_ASPECT`]) for
///   the wide layout of a source-tree view (#191).
pub fn compile_view(doc: &Value, snapshots: &[Snapshot], focus: &[Value], extra: &Value) -> Value {
    let doc = if snapshots.is_empty() {
        doc.clone()
    } else {
        overlay::apply(doc, snapshots)
    };
    if edn::record::flag(&doc, "hierarchical") {
        let ps = H::named_proposals(&doc);
        let pid = active_proposal_id(&ps, extra);
        let layer = get(extra, "open-layer")
            .filter(|l| !matches!(l, Value::Nil))
            .cloned();
        let mode = get(extra, "declutter");
        let view = if let (Some(pid), Some(layer)) = (&pid, &layer)
            && focus.is_empty()
        {
            H::layer_view(&doc, Some(pid), layer)
        } else if let Some(pid) = &pid
            && focus.is_empty()
            && !ps.is_empty()
        {
            H::proposal_view(&doc, Some(pid))
        } else {
            with_wide_layout(H::view_at(&doc, focus), extra)
        };
        let mut mode = mode.cloned();
        if layer.is_some() && mode.as_ref().and_then(local_name) == Some("classes") {
            mode = Some(Value::keyword("elements"));
        }
        // Authored ids survive: the viewer selects, clicks and menus by them.
        return compose::compile_diagram(&H::apply_declutter(&view, mode.as_ref()));
    }
    compose::compile_document(&doc)
}

/// Width over height of the canvas the wide layout fits by default: the
/// viewer's 1500×920 window less its inspector and chrome (1220×884).
pub const WIDE_ASPECT: f64 = 1220.0 / 884.0;

/// A source-tree view laid out wide (#191) when `extra` asks for it: ranks
/// run left to right and fold toward the target aspect; the levels, ports
/// and indicators stay as they are top to bottom.
fn with_wide_layout(mut view: Value, extra: &Value) -> Value {
    if edn::record::flag(extra, "wide") {
        let aspect = get(extra, "aspect")
            .and_then(edn::record::num)
            .unwrap_or(WIDE_ASPECT);
        edn::record::put(&mut view, "direction", Value::keyword("lr"));
        edn::record::put(&mut view, "wide", numval(aspect));
    }
    view
}

/// A fresh companion launch shows a blank canvas until the agent sends
/// `:display`; the path is retained so `R` can still load a diagram.
pub fn waiting_state(path: Option<String>, mail_seen: i64) -> ViewState {
    ViewState {
        document: None,
        scene: vmap(vec![
            ("classes", Value::Vector(vec![])),
            ("packages", Value::Vector(vec![])),
            ("edges", Value::Vector(vec![])),
            ("sections", Value::Vector(vec![])),
            (
                "size",
                vmap(vec![("w", Value::Integer(800)), ("h", Value::Integer(600))]),
            ),
            (
                "diagram",
                vmap(vec![("title", Value::String("UML".into()))]),
            ),
        ]),
        focus: vec![],
        extra: Value::Map(vec![]),
        mail_seen,
        path,
        waiting: true,
        ..ViewState::default()
    }
}

/// Navigation and document state. Fields mirror the Lua record; the viewer
/// crate owns camera/zoom, which live here as plain numbers.
#[derive(Debug, Clone)]
pub struct ViewState {
    pub document: Option<Value>,
    pub scene: Value,
    pub mtime: Option<Value>,
    pub metrics_stamp: Option<Value>,
    pub snapshots: Vec<Snapshot>,
    pub focus: Vec<Value>,
    pub extra: Value,
    pub revision: u64,
    pub path: Option<String>,
    pub project_root: Option<String>,
    pub waiting: bool,
    pub mail_seen: i64,
    pub cam_x: f64,
    pub cam_y: f64,
    pub zoom: f64,
    pub fit_pending: bool,
    pub selected: Option<Value>,
    pub hover: Option<Value>,
    pub pointer: Option<(f64, f64)>,
    pub detail_id: Option<Value>,
    pub detail_scroll: f64,
    pub discussion_context: Option<Value>,
    pub drag: Option<Drag>,
}

/// An in-flight canvas drag (press position, camera at press, moved flag).
#[derive(Debug, Clone)]
pub struct Drag {
    pub x: f64,
    pub y: f64,
    pub cam_x: f64,
    pub cam_y: f64,
    pub moved: bool,
}

impl Default for ViewState {
    fn default() -> ViewState {
        ViewState {
            document: None,
            scene: Value::Map(vec![]),
            mtime: None,
            metrics_stamp: None,
            snapshots: Vec::new(),
            focus: Vec::new(),
            extra: Value::Map(vec![]),
            revision: 0,
            path: None,
            project_root: None,
            waiting: false,
            mail_seen: 0,
            cam_x: 0.0,
            cam_y: 0.0,
            zoom: 1.0,
            fit_pending: false,
            selected: None,
            hover: None,
            pointer: None,
            detail_id: None,
            detail_scroll: 0.0,
            discussion_context: None,
            drag: None,
        }
    }
}

/// A new loaded state (`document.new`): compile immediately at revision 1.
pub fn new_state(
    doc: Value,
    mtime: Option<Value>,
    focus: Option<&[Value]>,
    extra: Option<&Value>,
    snapshots: &[Snapshot],
    metrics_stamp: Option<Value>,
) -> ViewState {
    let focus = focus.map(|f| f.to_vec()).unwrap_or_default();
    let extra = extra.cloned().unwrap_or_else(|| Value::Map(vec![]));
    let scene = compile_view(&doc, snapshots, &focus, &extra);
    ViewState {
        document: Some(doc),
        mtime,
        metrics_stamp,
        snapshots: snapshots.to_vec(),
        focus,
        extra,
        revision: 1,
        zoom: 1.0,
        scene,
        ..ViewState::default()
    }
}

fn scene_classes(scene: &Value) -> &[Value] {
    match get(scene, "classes") {
        Some(Value::Vector(v)) => v,
        _ => &[],
    }
}

fn has_id(items: Option<&Value>, id: &Value) -> Option<Value> {
    match items {
        Some(Value::Vector(items)) => items
            .iter()
            .find(|item| get(item, "id") == Some(id))
            .cloned(),
        _ => None,
    }
}

/// Does a class card exist for this id in the loaded document? Resolved
/// against the scene plus the document's own classes (the detail model's
/// complete scene); rendering rows is #141's job.
fn card_target(s: &ViewState, id: &Value) -> bool {
    if crate::hit::class_by_id(&s.scene, id).is_some() {
        return true;
    }
    let Some(doc) = &s.document else {
        return false;
    };
    let in_doc = |c: &Value| get(c, "id") == Some(id);
    match get(doc, "classes") {
        Some(Value::Vector(cs)) if cs.iter().any(&in_doc) => return true,
        _ => {}
    }
    match get(doc, "packages") {
        Some(Value::Vector(ps)) => ps.iter().any(|p| match get(p, "classes") {
            Some(Value::Vector(cs)) => cs.iter().any(&in_doc),
            _ => false,
        }),
        _ => false,
    }
}

fn selected_target(s: &ViewState, sel: &Value) -> bool {
    let Some(kind) = get(sel, "kind").map(name) else {
        // A bare id (or a record without :kind) is a card target itself.
        return card_target(s, sel);
    };
    let Some(id) = get(sel, "id") else {
        return false;
    };
    match kind.as_str() {
        "class" => card_target(s, id),
        "package" => has_id(get(&s.scene, "packages"), id).is_some(),
        "edge" => match get(&s.scene, "edges") {
            Some(Value::Vector(es)) => es
                .iter()
                .any(|e| get(e, "from") == get(sel, "from") && get(e, "to") == get(sel, "to")),
            _ => false,
        },
        "child" | "port" => {
            let parent = has_id(
                Some(&Value::Vector(scene_classes(&s.scene).to_vec())),
                get(sel, "parent").unwrap_or(&Value::Nil),
            );
            ["contents", "in-ports", "out-ports"]
                .iter()
                .any(|key| has_id(parent.as_ref().and_then(|p| get(p, key)), id).is_some())
        }
        "dep" => match get(&s.scene, "dep-indicators") {
            Some(Value::Vector(inds)) => inds
                .iter()
                .any(|d| get(d, "id") == Some(id) && get(d, "dir") == get(sel, "dir")),
            _ => false,
        },
        _ => card_target(s, id),
    }
}

/// Drop selection/detail targets that no longer resolve.
pub(crate) fn drop_missing_detail(mut s: ViewState) -> ViewState {
    if let Some(id) = s.detail_id.clone()
        && !card_target(&s, &id)
    {
        s.detail_id = None;
    }
    if let Some(sel) = s.selected.clone()
        && !selected_target(&s, &sel)
    {
        s.selected = None;
    }
    s
}

/// Reload a changed IR file; a rejected reload keeps the last usable scene.
pub fn reload(
    state: &ViewState,
    text: &str,
    mtime: Option<Value>,
    snapshots: &[Snapshot],
    metrics_stamp: Option<Value>,
) -> (ViewState, bool, Option<String>) {
    if mtime == state.mtime && metrics_stamp == state.metrics_stamp {
        return (state.clone(), false, None);
    }
    match ir::read_document(text) {
        Ok(doc) => {
            let mut s = state.clone();
            s.document = Some(doc);
            s.mtime = mtime;
            s.metrics_stamp = metrics_stamp;
            s.snapshots = snapshots.to_vec();
            let s = restore_session(&s, state);
            let mut s = s;
            s.revision = state.revision + 1;
            (s, true, None)
        }
        Err(err) => (state.clone(), false, Some(err)),
    }
}

/// Reapply only valid view targets after a same-canonical-diagram display.
pub fn restore_session(state: &ViewState, snap: &ViewState) -> ViewState {
    let Some(doc) = state.document.clone() else {
        return state.clone();
    };
    let mut s = state.clone();
    let mut focus = snap.focus.clone();
    if edn::record::flag(&doc, "hierarchical") && !focus.is_empty() {
        // Reduce the retained focus only while it has no visible boxes; the
        // root always stops the walk.
        loop {
            let present = match get(&H::view_at(&doc, &focus), "packages") {
                Some(Value::Vector(ps)) => ps.iter().any(|p| match get(p, "classes") {
                    Some(Value::Vector(cs)) => !cs.is_empty(),
                    _ => false,
                }),
                _ => false,
            };
            if present || focus.is_empty() {
                break;
            }
            focus.pop();
        }
    }
    let mut extra = snap.extra.clone();
    let pid = get(&extra, "proposal-id").cloned();
    let pid = match pid {
        Some(pid) if !matches!(pid, Value::Nil) && has_named(&doc, &pid) => Some(pid),
        _ => None,
    };
    let layer = get(&extra, "open-layer").cloned();
    let layer = match (&pid, layer) {
        (Some(pid), Some(layer)) if !matches!(layer, Value::Nil) => {
            let in_view = has_id(get(&H::proposal_view(&doc, Some(pid)), "packages"), &layer);
            let nested = H::find_nested_group(&doc, Some(pid), &layer);
            if in_view.is_some() || nested.is_some() {
                Some(layer)
            } else {
                None
            }
        }
        _ => None,
    };
    extra = match &pid {
        Some(pid) => {
            let mut e = extra.clone();
            e.assoc(Value::keyword("proposal-id"), pid.clone());
            e
        }
        None => drop_keys(&extra, &["proposal-id"]),
    };
    extra.assoc(Value::keyword("proposal"), Value::Bool(pid.is_some()));
    extra = match &layer {
        Some(layer) => {
            let mut e = extra.clone();
            e.assoc(Value::keyword("open-layer"), layer.clone());
            e
        }
        None => drop_keys(&extra, &["open-layer"]),
    };
    s.focus = focus;
    s.extra = extra;
    s.scene = compile_view(&doc, &s.snapshots, &s.focus.clone(), &s.extra.clone());
    s.cam_x = snap.cam_x;
    s.cam_y = snap.cam_y;
    s.zoom = if snap.zoom == 0.0 { 1.0 } else { snap.zoom };
    s.fit_pending = false;
    s.hover = None;
    s.pointer = None;
    s.selected = snap.selected.clone();
    s.detail_id = snap.detail_id.clone();
    s.discussion_context = snap.discussion_context.clone();
    drop_missing_detail(s)
}

/// The proposal list of an IR or policy document: `:proposals` in order,
/// then the legacy singular `:proposal`.
pub fn proposals(doc: &Value) -> Vec<Value> {
    crate::policy::named_proposals(doc)
}

fn has_named(doc: &Value, pid: &Value) -> bool {
    H::named_proposals(doc)
        .iter()
        .any(|p| get(p, "id") == Some(pid))
}

/// IO capabilities for display commands (resolve/canonical/load).
pub trait MailBoundary {
    fn resolve(&self, path: &str, root: Option<&str>) -> String;
    fn canonical(&self, path: &str) -> String;
    fn load(&self, path: &str) -> Option<ViewState>;
}

/// Apply one scene command for the viewer session. Unknown operations only
/// advance the cursor; panel requests are handled by the host.
pub fn apply_mail(state: &ViewState, cmd: &Value, boundary: &dyn MailBoundary) -> ViewState {
    let mut s = state.clone();
    s.mail_seen = get(cmd, "id").and_then(edn::record::num).unwrap_or(0.0) as i64;
    let op = get(cmd, "op")
        .and_then(local_name)
        .unwrap_or_default()
        .to_owned();
    if op == "display"
        && let Some(Value::String(path)) = get(cmd, "path")
    {
        let path = boundary.resolve(path, state.project_root.as_deref());
        let Some(loaded) = boundary.load(&path) else {
            return s;
        };
        let same = match &state.path {
            Some(old) if !state.waiting => boundary.canonical(old) == boundary.canonical(&path),
            _ => false,
        };
        s = if same {
            restore_session(&loaded, state)
        } else {
            loaded
        };
        s.fit_pending = !same;
        if !same {
            // A different diagram keeps declutter and a surviving proposal.
            if let Some(old) = get(&state.extra, "declutter") {
                s.extra.assoc(Value::keyword("declutter"), old.clone());
            }
            let pid = get(&state.extra, "proposal-id").cloned();
            if let Some(pid) = pid
                && !matches!(pid, Value::Nil)
                && s.document.as_ref().is_some_and(|d| has_named(d, &pid))
            {
                s.extra.assoc(Value::keyword("proposal-id"), pid);
                s.extra.assoc(Value::keyword("proposal"), Value::Bool(true));
            }
            s.scene = compile_view(
                s.document.as_ref().unwrap(),
                &s.snapshots,
                &s.focus.clone(),
                &s.extra.clone(),
            );
        }
        s.path = Some(path);
        s.project_root = state.project_root.clone();
        s.mail_seen = get(cmd, "id").and_then(edn::record::num).unwrap_or(0.0) as i64;
        s.discussion_context = state.discussion_context.clone();
        s.waiting = false;
        if s.zoom == 0.0 {
            s.zoom = 1.0;
        }
        s.revision = state.revision + 1;
    } else if op == "context" {
        let mut ctx = Value::Map(vec![]);
        for key in ["context", "proposal-id", "name"] {
            if let Some(v) = edn::record::get_raw(cmd, key) {
                ctx.assoc(Value::keyword(key), v.clone());
            }
        }
        s.discussion_context = Some(ctx);
    }
    s
}

/// Sibling `<stem>.policy.edn` of an IR path, when the predicate agrees it
/// exists. Filesystem access stays in the caller.
pub fn policy_path_for(edn_path: &str, is_file: &dyn Fn(&str) -> bool) -> Option<String> {
    let stem = edn_path.strip_suffix(".edn")?;
    let path = format!("{stem}.policy.edn");
    if is_file(&path) { Some(path) } else { None }
}

/// Authored proposal records for write-back: view annotations (`:notice`,
/// `:proposal`) are dropped, everything else is preserved in order.
pub(crate) fn proposal_records(doc: &Value) -> Value {
    let records: Vec<Value> = H::named_proposals(doc)
        .iter()
        .map(|p| drop_keys(p, &["notice", "proposal"]))
        .collect();
    Value::Vector(records)
}

/// The `love ir` file header. IR emission itself belongs to #136; the
/// write-back path here only needs the same framing.
const IR_HEADER: &str = ";; Generated by uml ir from the policy file. Do not edit.\n";

/// Write named proposals to the IR and, when present, its sibling policy
/// file. Other policy content is preserved; comments are not (unchanged).
pub fn write_proposals(edn_path: Option<&str>, doc: &Value, boundary: &dyn Boundary) -> Value {
    let records = proposal_records(doc);
    let mut out = doc.clone();
    out.assoc(Value::keyword("proposals"), records.clone());
    let policy_path = match get(doc, "policy-file") {
        Some(Value::String(p)) => Some(p.clone()),
        _ => edn_path.and_then(|p| policy_path_for(p, &|f| boundary.exists(f))),
    };
    if let Some(path) = edn_path {
        let text = format!("{IR_HEADER}{}\n", edn::encode(&out).unwrap());
        boundary.write(path, &text);
    }
    if let Some(path) = policy_path
        && boundary.exists(&path)
        && let Some(text) = boundary.read(&path)
        && let Ok(policy) = edn::parse(&text)
        && matches!(policy, Value::Map(_))
    {
        let mut policy = policy;
        policy.assoc(Value::keyword("proposals"), records);
        let policy = drop_keys(&policy, &["proposal"]);
        boundary.write(&path, &format!("{}\n", edn::encode(&policy).unwrap()));
    }
    out
}

/// In-memory boundary used by tests and callers that own their own files.
#[derive(Default)]
pub struct MemoryBoundary {
    pub files: std::sync::Mutex<HashMap<String, String>>,
}

impl Boundary for MemoryBoundary {
    fn exists(&self, path: &str) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn read(&self, path: &str) -> Option<String> {
        self.files.lock().unwrap().get(path).cloned()
    }

    fn write(&self, path: &str, text: &str) -> bool {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), text.to_owned());
        true
    }
}
