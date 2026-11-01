//! Pure application transitions (port of `love/uml/application/events.lua`):
//! navigation, camera, selection, search location and proposal CRUD.

use crate::document::{self, Boundary, Drag, ViewState};
use crate::geom;
use crate::hierarchy as H;
use crate::hit;
use edn::Value;
use edn::record::{drop_keys, get, local_name, name, num};

/// Canvas geometry for camera math: window size, usable view width and the
/// optional pointer/control/shift modifiers.
#[derive(Debug, Clone, Default)]
pub struct Dims {
    pub window_w: f64,
    pub window_h: f64,
    pub view_w: Option<f64>,
    pub pointer: Option<(f64, f64)>,
    pub control: bool,
    pub shift: bool,
    pub horizontal: bool,
    pub key_code: Option<i64>,
    pub raw_key: Option<String>,
    pub count: Option<f64>,
}

impl Dims {
    fn vw(&self) -> f64 {
        self.view_w.unwrap_or(if self.window_w > 0.0 {
            self.window_w
        } else {
            1500.0
        })
    }

    fn vh(&self) -> f64 {
        if self.window_h > 0.0 {
            self.window_h
        } else {
            900.0
        }
    }
}

pub(crate) const ZOOM_MIN: f64 = 0.1;
pub(crate) const ZOOM_MAX: f64 = 10.0;
pub const ZOOM_STEP: f64 = 1.1;
pub(crate) const DRAG_THRESHOLD: f64 = 3.0;

fn clamp_zoom(z: f64) -> f64 {
    z.clamp(ZOOM_MIN, ZOOM_MAX)
}

fn rebuild(mut s: ViewState) -> ViewState {
    if let Some(doc) = s.document.clone()
        && edn::record::flag(&doc, "hierarchical")
    {
        s.scene = document::compile_view(&doc, &s.snapshots, &s.focus.clone(), &s.extra.clone());
        // Recompiling geometry is not navigation: keep camera/zoom and valid
        // selection targets (ADR 0002).
        s = document::drop_missing_detail(s);
    }
    s
}

fn navigate(s: ViewState) -> ViewState {
    let mut s = rebuild(s);
    s.cam_x = 0.0;
    s.cam_y = 0.0;
    s.selected = None;
    s.hover = None;
    s.pointer = None;
    s.fit_pending = true;
    s
}

/// Switch to a named proposal, or back to the source tree with `None`.
pub fn show_proposal(s: &ViewState, id: Option<&Value>) -> ViewState {
    if let Some(id) = id
        && !matches!(id, Value::Nil)
        && let Some(doc) = &s.document
        && !H::named_proposals(doc)
            .iter()
            .any(|p| get(p, "id") == Some(id))
    {
        return s.clone();
    }
    let mut n = s.clone();
    n.focus = vec![];
    // `E.assoc` removes a key when the value is EDN nil (the Lua `put` path
    // writes `nil_value`, which `E.get` reports as absent).
    n.extra = match &id {
        Some(id) => {
            let mut extra = n.extra.clone();
            extra.assoc(Value::keyword("proposal-id"), (*id).clone());
            extra
        }
        None => drop_keys(&n.extra, &["proposal-id"]),
    };
    n.extra
        .assoc(Value::keyword("proposal"), Value::Bool(id.is_some()));
    n.extra = drop_keys(&n.extra, &["open-layer"]);
    navigate(n)
}

fn with_proposals(doc: &Value, proposals: Value) -> Value {
    let mut out = doc.clone();
    out.assoc(Value::keyword("proposals"), proposals);
    out
}

fn persist(s: ViewState, doc: Value, boundary: Option<&dyn Boundary>) -> ViewState {
    let doc = match boundary {
        Some(b) => document::write_proposals(s.path.as_deref(), &doc, b),
        None => doc,
    };
    let mut n = s;
    n.document = Some(doc);
    rebuild(n)
}

/// Create a proposal with id `p-<now>[-n]` and the caller's `name`. The
/// front end reads the clock and formats the default local-time name.
pub fn add_proposal(
    s: &ViewState,
    now: i64,
    name: &str,
    boundary: Option<&dyn Boundary>,
) -> ViewState {
    let Some(doc) = s.document.clone() else {
        return s.clone();
    };
    let base = format!("p-{now}");
    let mut id = Value::keyword(&base);
    let mut n = 0;
    while crate::policy::proposal_by_id(&doc, Some(&id)).is_some() {
        n += 1;
        id = Value::keyword(&format!("{base}-{n}"));
    }
    let mut proposals: Vec<Value> = H::named_proposals(&doc);
    proposals.push(Value::Map(vec![
        (Value::keyword("id"), id.clone()),
        (Value::keyword("name"), Value::String(name.to_owned())),
        (Value::keyword("layers"), Value::Vector(vec![])),
        (
            Value::keyword("notice"),
            Value::String(crate::policy::PROPOSAL_NOTICE.into()),
        ),
    ]));
    let mut state = s.clone();
    state.focus = vec![];
    state.extra.assoc(Value::keyword("proposal-id"), id);
    state
        .extra
        .assoc(Value::keyword("proposal"), Value::Bool(true));
    persist(
        state,
        with_proposals(&doc, Value::Vector(proposals)),
        boundary,
    )
}

/// Rename a proposal; blank names are ignored.
pub fn rename_proposal(
    s: &ViewState,
    id: Option<&Value>,
    new_name: &str,
    boundary: Option<&dyn Boundary>,
) -> ViewState {
    let (Some(id), Some(doc)) = (id, s.document.as_ref()) else {
        return s.clone();
    };
    if new_name.trim().is_empty() {
        return s.clone();
    }
    let proposals: Vec<Value> = H::named_proposals(doc)
        .into_iter()
        .map(|p| {
            let mut out = p.clone();
            if get(&p, "id") == Some(id) {
                edn::record::put(&mut out, "name", Value::String(new_name.to_owned()));
            }
            out
        })
        .collect();
    persist(
        s.clone(),
        with_proposals(doc, Value::Vector(proposals)),
        boundary,
    )
}

pub fn delete_proposal(
    s: &ViewState,
    id: Option<&Value>,
    boundary: Option<&dyn Boundary>,
) -> ViewState {
    let (Some(id), Some(doc)) = (id, s.document.as_ref()) else {
        return s.clone();
    };
    let proposals: Vec<Value> = H::named_proposals(doc)
        .into_iter()
        .filter(|p| get(p, "id") != Some(id))
        .collect();
    let mut state = s.clone();
    if get(&s.extra, "proposal-id") == Some(id) {
        state.extra = drop_keys(&state.extra, &["proposal-id"]);
        state
            .extra
            .assoc(Value::keyword("proposal"), Value::Bool(false));
    }
    persist(
        state,
        with_proposals(doc, Value::Vector(proposals)),
        boundary,
    )
}

pub fn cycle_declutter(s: &ViewState) -> ViewState {
    let mut n = s.clone();
    let next = H::next_declutter(get(&s.extra, "declutter"));
    n.extra.assoc(Value::keyword("declutter"), next);
    rebuild(n)
}

/// Enter a namespace, a proposal package or a nested proposal group.
pub fn drill(s: &ViewState, id: &Value) -> ViewState {
    let mut n = s.clone();
    n.focus = vec![];
    let proposals = s
        .document
        .as_ref()
        .map(H::named_proposals)
        .unwrap_or_default();
    let pid = document::active_proposal_id(&proposals, &s.extra);
    if pid.is_some()
        && (H::proposal_package_id(Some(id))
            || s.document
                .as_ref()
                .is_some_and(|d| H::find_nested_group(d, pid.as_ref(), id).is_some()))
    {
        n.extra.assoc(Value::keyword("open-layer"), id.clone());
    } else {
        n.focus = s.focus.clone();
        let last = name(id);
        n.focus
            .push(Value::keyword(last.rsplit('.').next().unwrap_or("")));
    }
    navigate(n)
}

/// Leave one level: close the open proposal layer, else shorten the focus.
pub fn back(s: &ViewState) -> ViewState {
    let layer = get(&s.extra, "open-layer").filter(|l| !matches!(l, Value::Nil));
    if layer.is_none() && s.focus.is_empty() {
        return s.clone();
    }
    let mut n = s.clone();
    if layer.is_some() {
        n.extra = drop_keys(&n.extra, &["open-layer"]);
    } else {
        n.focus = s.focus[..s.focus.len() - 1].to_vec();
    }
    navigate(n)
}

pub fn zoom_of(s: &ViewState) -> f64 {
    if s.zoom == 0.0 { 1.0 } else { s.zoom }
}

pub fn on_scroll(s: &ViewState, amount: f64, dims: &Dims) -> ViewState {
    let mut n = s.clone();
    let z = zoom_of(s);
    let size = get(&s.scene, "size");
    let horizontal = dims.horizontal;
    let (minimum, span) = if horizontal {
        (
            size.and_then(|s| get(s, "min-x"))
                .and_then(num)
                .unwrap_or(0.0),
            size.and_then(|s| get(s, "w"))
                .and_then(num)
                .unwrap_or(800.0),
        )
    } else {
        (
            size.and_then(|s| get(s, "min-y"))
                .and_then(num)
                .unwrap_or(0.0),
            size.and_then(|s| get(s, "h"))
                .and_then(num)
                .unwrap_or(600.0),
        )
    };
    let visible = if horizontal { dims.vw() } else { dims.vh() };
    let maximum = minimum.max(span - visible / z);
    let current = if horizontal { s.cam_x } else { s.cam_y };
    let value = (current + amount * 48.0 / z).max(minimum).min(maximum);
    if horizontal {
        n.cam_x = value;
    } else {
        n.cam_y = value;
    }
    n
}

pub fn world_xy(s: &ViewState, x: f64, y: f64) -> (f64, f64) {
    let z = zoom_of(s);
    (s.cam_x + x / z, s.cam_y + y / z)
}

/// Scale zoom by `factor`, keeping the world point under (ax, ay) fixed.
pub fn zoom_at(s: &ViewState, factor: f64, ax: f64, ay: f64) -> ViewState {
    let mut n = s.clone();
    let old = zoom_of(s);
    n.zoom = clamp_zoom(old * factor);
    n.cam_x = s.cam_x + ax / old - ax / n.zoom;
    n.cam_y = s.cam_y + ay / old - ay / n.zoom;
    n
}

/// The pointer anchors zooming when it is on the canvas, else the centre.
fn anchor(dims: &Dims, s: &ViewState) -> (f64, f64) {
    if let Some(p) = dims.pointer.or(s.pointer) {
        return p;
    }
    (dims.vw() / 2.0, dims.vh() / 2.0)
}

/// Screen points at the top of the canvas kept for the title or scope line.
pub const CHROME_INSET: f64 = 36.0;

/// Fit the whole scene into the usable canvas, centred; small diagrams stay
/// at 100% instead of being magnified.
pub fn fit(s: &ViewState, dims: &Dims) -> ViewState {
    let mut n = s.clone();
    n.fit_pending = false;
    let size = get(&s.scene, "size");
    let x0 = size
        .and_then(|s| get(s, "min-x"))
        .and_then(num)
        .unwrap_or(0.0);
    let y0 = size
        .and_then(|s| get(s, "min-y"))
        .and_then(num)
        .unwrap_or(0.0);
    let w = (size
        .and_then(|s| get(s, "w"))
        .and_then(num)
        .unwrap_or(800.0)
        - x0)
        .max(1.0);
    let h = (size
        .and_then(|s| get(s, "h"))
        .and_then(num)
        .unwrap_or(600.0)
        - y0)
        .max(1.0);
    let vw = dims.vw();
    let vh = dims.vh();
    // The scope line owns the top strip: the scene centres below it.
    let usable = (vh - CHROME_INSET).max(1.0);
    n.zoom = clamp_zoom((vw / w).min(usable / h).min(1.0));
    n.cam_x = x0 + w / 2.0 - vw / (2.0 * n.zoom);
    n.cam_y = y0 + h / 2.0 - usable / (2.0 * n.zoom) - CHROME_INSET / n.zoom;
    n
}

pub fn settle(s: &ViewState, dims: &Dims) -> ViewState {
    if s.fit_pending {
        fit(s, dims)
    } else {
        s.clone()
    }
}

/// Wheel input: control zooms around the cursor, otherwise pan (shift or a
/// horizontal wheel pans sideways).
pub fn on_wheel(s: &ViewState, dx: f64, dy: f64, dims: &Dims) -> ViewState {
    if dims.control {
        if dy == 0.0 {
            return s.clone();
        }
        let (ax, ay) = anchor(dims, s);
        return zoom_at(s, ZOOM_STEP.powf(dy), ax, ay);
    }
    let mut d = dims.clone();
    d.horizontal = dx != 0.0 || dims.shift;
    on_scroll(s, if dx != 0.0 { -dx } else { -dy }, &d)
}

/// `+`/`-`/`0` zoom directions (with control).
pub fn zoom_dir(key: Option<&str>, dims: &Dims) -> Option<&'static str> {
    let key = key?;
    let c = dims.key_code;
    let raw = dims.raw_key.as_deref().and_then(|r| r.chars().next());
    if key == "+"
        || key == "="
        || key == "kp+"
        || c == Some(61)
        || c == Some(107)
        || c == Some(521)
        || raw == Some('+')
        || raw == Some('=')
    {
        return Some("in");
    }
    if key == "-"
        || key == "_"
        || key == "minus"
        || key == "subtract"
        || key == "kp-"
        || c == Some(45)
        || c == Some(109)
        || raw == Some('-')
        || raw == Some('_')
        || raw == Some('\u{1f}')
    {
        return Some("out");
    }
    if key == "0" || key == "kp0" || c == Some(48) || c == Some(96) || raw == Some('0') {
        return Some("reset");
    }
    None
}

fn key_name(key: Option<&Value>) -> Option<String> {
    match key? {
        Value::String(s) => Some(s.clone()),
        Value::Keyword(_) => Some(name(key?)),
        Value::Integer(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn on_key(s: &ViewState, key: Option<&Value>, dims: &Dims) -> ViewState {
    let key = key_name(key);
    let dir = if dims.control {
        zoom_dir(key.as_deref(), dims)
    } else {
        None
    };
    if let Some(dir) = dir {
        match dir {
            "reset" => return fit(s, dims),
            "in" => {
                let (ax, ay) = anchor(dims, s);
                return zoom_at(s, ZOOM_STEP, ax, ay);
            }
            _ => {
                let (ax, ay) = anchor(dims, s);
                return zoom_at(s, 1.0 / ZOOM_STEP, ax, ay);
            }
        }
    }
    let mut d = dims.clone();
    match key.as_deref() {
        Some("left") | Some("right") => {
            d.horizontal = true;
            on_scroll(
                s,
                if key.as_deref() == Some("left") {
                    -2.0
                } else {
                    2.0
                },
                &d,
            )
        }
        Some("up") | Some("down") => {
            d.horizontal = false;
            on_scroll(
                s,
                if key.as_deref() == Some("up") {
                    -2.0
                } else {
                    2.0
                },
                &d,
            )
        }
        Some("escape") | Some("esc") => escape(s),
        Some("p") | Some("P") => show_proposal(s, None),
        Some("r") => {
            let mut n = s.clone();
            n.waiting = false;
            n.mtime = None;
            n.metrics_stamp = None;
            n
        }
        _ => s.clone(),
    }
}

/// The layer a hit drills into (drillable class, child row, proposal package).
pub fn layer_id(hit_value: Option<&Value>) -> Option<Value> {
    let hit_value = hit_value?;
    let kind = get(hit_value, "kind").map(name)?;
    match kind.as_str() {
        "class" if edn::record::flag(hit_value, "drill?") => get(hit_value, "id").cloned(),
        "child" => get(hit_value, "parent").cloned(),
        "package" if H::proposal_package_id(get(hit_value, "id")) => get(hit_value, "id").cloned(),
        _ => None,
    }
}

/// Detail target identity of a class, child row or port hit.
pub(crate) fn detail_id_for(hit_value: Option<&Value>) -> Option<Value> {
    let hit_value = hit_value?;
    match get(hit_value, "kind").map(name).as_deref() {
        Some("class") | Some("child") | Some("port") => get(hit_value, "id").cloned(),
        _ => None,
    }
}

pub fn select(s: &ViewState, hit_value: Option<&Value>) -> ViewState {
    let mut n = s.clone();
    n.selected = hit_value.cloned();
    if let Some(id) = detail_id_for(hit_value)
        && s.detail_id.as_ref() != Some(&id)
    {
        n.detail_scroll = 0.0;
    }
    n
}

pub(crate) fn open_detail(s: &ViewState, hit_value: Option<&Value>) -> ViewState {
    let mut n = select(s, hit_value);
    if let Some(id) = detail_id_for(hit_value) {
        n.detail_id = Some(id);
    }
    n
}

/// Right-click opens the element menu for classes, child rows and proposal
/// packages.
pub fn element_menu(selection: Option<&Value>) -> bool {
    let Some(selection) = selection else {
        return false;
    };
    let kind = get(selection, "kind")
        .and_then(local_name)
        .unwrap_or_default()
        .to_owned();
    kind == "class" || kind == "child" || layer_id(Some(selection)).is_some()
}

/// Right-button press: select what it hits (blank clears), so the element
/// menu that opens on the release names the element under the pointer.
pub fn on_secondary_press(s: &ViewState, x: f64, y: f64) -> ViewState {
    let (wx, wy) = world_xy(s, x, y);
    select(s, hit::at(&s.scene, wx, wy).as_ref())
}

pub fn on_move(s: &ViewState, x: f64, y: f64) -> ViewState {
    let mut n = s.clone();
    let (wx, wy) = world_xy(s, x, y);
    n.hover = hit::at(&s.scene, wx, wy);
    n.pointer = Some((x, y));
    n
}

pub fn on_leave(s: &ViewState) -> ViewState {
    if s.hover.is_none() && s.pointer.is_none() {
        return s.clone();
    }
    let mut n = s.clone();
    n.hover = None;
    n.pointer = None;
    n
}

fn on_back_hotspot(s: &ViewState, x: f64, y: f64) -> bool {
    let layer = get(&s.extra, "open-layer").is_some_and(|l| !matches!(l, Value::Nil));
    (layer || !s.focus.is_empty()) && y < 44.0 && x < 320.0
}

pub fn on_press(s: &ViewState, x: f64, y: f64) -> ViewState {
    if on_back_hotspot(s, x, y) {
        return back(s);
    }
    let (wx, wy) = world_xy(s, x, y);
    let hit_value = hit::at(&s.scene, wx, wy);
    match hit_value
        .as_ref()
        .and_then(|h| get(h, "kind"))
        .map(name)
        .as_deref()
    {
        Some("child") | Some("port") => open_detail(s, hit_value.as_ref()),
        _ => select(s, hit_value.as_ref()),
    }
}

/// Left-button lifecycle: a press on a class/child/port/triangle clicks;
/// elsewhere it arms a pan drag past the threshold.
pub fn on_left_press(s: &ViewState, x: f64, y: f64) -> ViewState {
    if on_back_hotspot(s, x, y) {
        return back(s);
    }
    let (wx, wy) = world_xy(s, x, y);
    let hit_value = hit::at(&s.scene, wx, wy);
    let grabs = matches!(
        hit_value
            .as_ref()
            .and_then(|h| get(h, "kind"))
            .map(name)
            .as_deref(),
        Some("class" | "child" | "port" | "dep")
    );
    if grabs {
        return on_press(s, x, y);
    }
    let mut n = s.clone();
    n.drag = Some(Drag {
        x,
        y,
        cam_x: s.cam_x,
        cam_y: s.cam_y,
        moved: false,
    });
    n
}

pub fn on_drag(s: &ViewState, x: f64, y: f64) -> ViewState {
    let Some(d) = s.drag.clone() else {
        return on_move(s, x, y);
    };
    let (dx, dy) = (x - d.x, y - d.y);
    if !d.moved && dx.abs() < DRAG_THRESHOLD && dy.abs() < DRAG_THRESHOLD {
        return s.clone();
    }
    let mut n = s.clone();
    let z = zoom_of(s);
    n.drag = Some(Drag {
        moved: true,
        ..d.clone()
    });
    n.cam_x = d.cam_x - dx / z;
    n.cam_y = d.cam_y - dy / z;
    n.pointer = Some((x, y));
    n
}

pub fn on_release(s: &ViewState, _x: f64, _y: f64) -> ViewState {
    let Some(d) = s.drag.clone() else {
        return s.clone();
    };
    let mut n = s.clone();
    n.drag = None;
    if d.moved {
        return n;
    }
    on_press(&n, d.x, d.y)
}

pub fn on_double_press(s: &ViewState, x: f64, y: f64) -> ViewState {
    if on_back_hotspot(s, x, y) {
        return back(s);
    }
    let (wx, wy) = world_xy(s, x, y);
    let hit_value = hit::at(&s.scene, wx, wy);
    if let Some(layer) = layer_id(hit_value.as_ref()) {
        return drill(s, &layer);
    }
    open_detail(s, hit_value.as_ref())
}

/// Escape: back one level, else clear the selection.
pub fn escape(s: &ViewState) -> ViewState {
    let layer = get(&s.extra, "open-layer").is_some_and(|l| !matches!(l, Value::Nil));
    if layer || !s.focus.is_empty() {
        return back(s);
    }
    let mut n = s.clone();
    n.selected = None;
    n
}

/// Centre a class with the same canvas math as fit/zoom.
pub fn center(s: &ViewState, id: &Value, dims: &Dims) -> ViewState {
    let Some(c) = hit::class_by_id(&s.scene, id) else {
        return s.clone();
    };
    let Some(r) = get(c, "rect").and_then(geom::Rect::from_value) else {
        return s.clone();
    };
    let mut n = s.clone();
    let z = zoom_of(s);
    n.cam_x = r.cx() - dims.vw() / (2.0 * z);
    n.cam_y = r.cy() - dims.vh() / (2.0 * z);
    n.fit_pending = false;
    n
}

/// Activate a search target: enter its namespace (or proposal layer),
/// select it, open its card and centre it. Unknown targets are no-ops.
pub fn locate(s: &ViewState, id: &Value, dims: &Dims) -> ViewState {
    let present = {
        let in_doc = |c: &Value| get(c, "id") == Some(id);
        let mut present = s
            .document
            .as_ref()
            .is_some_and(|d| match get(d, "classes") {
                Some(Value::Vector(cs)) => cs.iter().any(&in_doc),
                _ => false,
            });
        if !present {
            present = s
                .document
                .as_ref()
                .is_some_and(|d| match get(d, "packages") {
                    Some(Value::Vector(ps)) => ps.iter().any(|p| match get(p, "classes") {
                        Some(Value::Vector(cs)) => cs.iter().any(&in_doc),
                        _ => false,
                    }),
                    _ => false,
                });
        }
        present
    };
    let hierarchical = s
        .document
        .as_ref()
        .is_some_and(|d| edn::record::flag(d, "hierarchical"));
    if !present && !hierarchical {
        return s.clone();
    }
    if !present && hierarchical {
        let doc = s.document.clone().unwrap();
        let id_name = name(id);
        let parts: Vec<&str> = id_name.split('.').collect();
        if parts.len() < 2 {
            return s.clone();
        }
        let parent: Vec<Value> = parts[..parts.len() - 1]
            .iter()
            .map(|p| Value::keyword(p))
            .collect();
        let candidate = document::compile_view(&doc, &s.snapshots, &parent, &Value::Map(vec![]));
        let found = hit::class_by_id(&candidate, id)
            .map(|c| edn::record::flag(c, "drill?"))
            .unwrap_or(false);
        if !found {
            return s.clone();
        }
    }
    let mut n = s.clone();
    n.focus = vec![];
    let compile = |n: &mut ViewState, layer: Option<Value>| -> bool {
        n.extra
            .assoc(Value::keyword("open-layer"), layer.unwrap_or(Value::Nil));
        let doc = n.document.clone().unwrap();
        n.scene = document::compile_view(&doc, &n.snapshots, &n.focus.clone(), &n.extra.clone());
        hit::class_by_id(&n.scene, id)
            .map(|c| !edn::record::flag(c, "dummy?") && !edn::record::flag(c, "proposal-group?"))
            .unwrap_or(false)
    };
    let pid = get(&n.extra, "proposal-id").cloned();

    let found = match pid.filter(|p| !matches!(p, Value::Nil)) {
        Some(pid) => {
            let mut found = compile(&mut n, None);
            if !found {
                let doc = n.document.clone().unwrap();
                let proposals = H::named_proposals(&doc);
                'outer: for p in proposals {
                    if get(&p, "id") != Some(&pid) {
                        continue;
                    }
                    for layer in edn::record::seq(get(&p, "layers")) {
                        if !found {
                            found = compile(
                                &mut n,
                                Some(Value::keyword(&format!(
                                    "proposal.{}",
                                    local_name(get(layer, "id").unwrap())
                                        .unwrap_or_default()
                                        .to_owned()
                                ))),
                            );
                        }
                        if !found {
                            found = locate_groups(&mut n, get(layer, "nses"), &compile);
                        }
                        if found {
                            break 'outer;
                        }
                    }
                }
            }
            return finish_locate(s, n, found, id, dims);
        }
        None => {
            let id_name = name(id);
            let parts: Vec<&str> = id_name.split('.').collect();
            for p in &parts[..parts.len().saturating_sub(1)] {
                n.focus.push(Value::keyword(p));
            }
            compile(&mut n, None)
        }
    };
    finish_locate(s, n, found, id, dims)
}

fn finish_locate(
    s: &ViewState,
    mut n: ViewState,
    found: bool,
    id: &Value,
    dims: &Dims,
) -> ViewState {
    if !found {
        return s.clone();
    }
    n.hover = None;
    n.pointer = None;
    n.drag = None;
    let mut n = open_detail(&n, Some(&parse_hit(id)));
    n = center(&n, id, dims);
    n
}

fn parse_hit(id: &Value) -> Value {
    Value::Map(vec![
        (Value::keyword("kind"), Value::keyword("class")),
        (Value::keyword("id"), id.clone()),
    ])
}

fn locate_groups(
    n: &mut ViewState,
    nses: Option<&Value>,
    compile: &dyn Fn(&mut ViewState, Option<Value>) -> bool,
) -> bool {
    for x in edn::record::seq(nses) {
        if !matches!(x, Value::Map(_)) {
            continue;
        }
        if compile(n, get(x, "id").cloned()) {
            return true;
        }
        if locate_groups(n, get(x, "nses"), compile) {
            return true;
        }
    }
    false
}

/// Close the detail card (its own close control).
pub fn close_detail(s: &ViewState) -> ViewState {
    let mut n = s.clone();
    n.detail_id = None;
    n
}
