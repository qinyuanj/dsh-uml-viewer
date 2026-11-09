//! Declarative EDN driver scripts (Seam B, #139/#140): the viewer replays
//! input events and file/evidence assertions from a step list so the
//! behavioural drivers stay declarative and runtime-independent.
//!
//! ```text
//! {:version 1
//!  :steps [{:op :wait-frames :n 3}
//!          {:op :assert :evidence "scene-1.edn"
//!           :contains-any [":mu 1 " ":mu 1}"] :message "first scene lacks the crap-1 summary"}
//!          {:op :edit :project ".metrics/crap.edn" :from ":crap 1" :to ":crap 9"
//!           :same-length true}
//!          {:op :record-mtime :project "diagram.edn" :as :ir}
//!          {:op :wait-file :evidence "scene-2.edn"}
//!          {:op :assert :evidence "scene-2.edn" :differs-from "scene-1.edn"}
//!          {:op :write :evidence "metrics-pass.txt" :text "PASS ...\n"}
//!          {:op :quit :code 0}]}
//! ```
//!
//! Pacing: `:wait-frames` counts from the frame the step is reached, and the
//! runner yields after every input step, so the next step observes the state
//! the viewer produced from that input (the Lua drivers' one-action-per-frame
//! rhythm).
//!
//! Navigation assertions (#140) are relational, as in the Lua drivers:
//! `:record-view` stores a [`ViewRecord`] under a name and `:assert-view`
//! compares the current record against it (zoom step, pan by a screen delta
//! at the recorded zoom, cursor anchor fixed, unchanged fields).
//!
//! The runner is window-free: a `Host` supplies the view records, event
//! injection, glyph/text probes and the quit; file checks run here. The app
//! calls [`Runner::run`] once per frame.

use edn::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use crate::search_box::SearchRecord;
use crate::watch;

/// One replayed input event (view/canvas coordinates in points).
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Click {
        x: f64,
        y: f64,
    },
    DoubleClick {
        x: f64,
        y: f64,
    },
    MouseMove {
        x: f64,
        y: f64,
    },
    Drag {
        from: [f64; 2],
        to: [f64; 2],
    },
    Wheel {
        dx: f64,
        dy: f64,
        ctrl: bool,
        shift: bool,
    },
    Key {
        key: String,
        ctrl: bool,
        shift: bool,
    },
    /// Committed text (the textinput / IME-commit path).
    Text(String),
    /// Clipboard paste into the focused text field.
    Paste(String),
}

/// A snapshot of the navigation state. Ids are keyword names.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewRecord {
    pub focus: Vec<String>,
    pub zoom: f64,
    pub cam_x: f64,
    pub cam_y: f64,
    pub selected: Option<String>,
    pub detail: Option<String>,
    pub declutter: String,
    pub dragging: bool,
    /// World point under the requested screen anchor, when one was given.
    pub anchor: Option<(f64, f64)>,
    pub search: SearchRecord,
}

impl Default for ViewRecord {
    fn default() -> ViewRecord {
        ViewRecord {
            focus: Vec::new(),
            zoom: 1.0,
            cam_x: 0.0,
            cam_y: 0.0,
            selected: None,
            detail: None,
            declutter: "full".into(),
            dragging: false,
            anchor: None,
            search: SearchRecord::default(),
        }
    }
}

fn keyword_or_nil(id: &Option<String>) -> Value {
    id.as_deref().map_or(Value::Nil, Value::keyword)
}

impl ViewRecord {
    /// The `navigation-*.edn` evidence shape: `{:focus :zoom :cam-x :cam-y
    /// :selected ...}` plus the declutter mode and the search box.
    pub fn to_edn(&self) -> Value {
        let keywords =
            |items: &[String]| Value::Vector(items.iter().map(|s| Value::keyword(s)).collect());
        Value::Map(vec![
            (Value::keyword("focus"), keywords(&self.focus)),
            (Value::keyword("zoom"), Value::Float(self.zoom)),
            (Value::keyword("cam-x"), Value::Float(self.cam_x)),
            (Value::keyword("cam-y"), Value::Float(self.cam_y)),
            (Value::keyword("selected"), keyword_or_nil(&self.selected)),
            (Value::keyword("detail"), keyword_or_nil(&self.detail)),
            (Value::keyword("declutter"), Value::keyword(&self.declutter)),
            (Value::keyword("dragging"), Value::Bool(self.dragging)),
            (
                Value::keyword("search"),
                Value::Map(vec![
                    (Value::keyword("active"), Value::Bool(self.search.active)),
                    (
                        Value::keyword("query"),
                        Value::String(self.search.query.clone()),
                    ),
                    (Value::keyword("results"), keywords(&self.search.results)),
                    (
                        Value::keyword("selection"),
                        keyword_or_nil(&self.search.selection),
                    ),
                ]),
            ),
        ])
    }
}

/// Checks of one `:assert-view` step; every present check must hold.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewChecks {
    pub focus: Option<Vec<String>>,
    pub selected: Option<String>,
    pub detail: Option<String>,
    pub declutter: Option<String>,
    pub dragging: Option<bool>,
    /// The view equals a fresh fit of the usable canvas (host-side).
    pub fitted: Option<bool>,
    /// `[:name factor]`: zoom == clamp(recorded zoom × factor, 0.1, 10).
    pub zoom_times: Option<(String, f64)>,
    pub zoom_unchanged: Option<String>,
    pub cam_unchanged: Option<String>,
    pub focus_unchanged: Option<String>,
    pub selected_unchanged: Option<String>,
    /// `[[dx dy] :name]`: cam == recorded cam + (dx, dy) / recorded zoom.
    pub cam_shift: Option<([f64; 2], String)>,
    pub cam_y_not_below: Option<String>,
    /// Screen point whose world position `:anchor-fixed` compares.
    pub anchor: Option<[f64; 2]>,
    pub anchor_fixed: Option<String>,
}

const EPSILON: f64 = 1e-6;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < EPSILON
}

fn zoom_of(z: f64) -> f64 {
    if z == 0.0 { 1.0 } else { z }
}

/// Compare the current record with the checks and the named records.
pub fn check_view(
    current: &ViewRecord,
    records: &HashMap<String, ViewRecord>,
    checks: &ViewChecks,
) -> Result<(), String> {
    let named = |name: &str| {
        records
            .get(name)
            .ok_or_else(|| format!("no recorded view named :{name}"))
    };
    let fail = |what: &str| Err(what.to_string());
    if let Some(focus) = &checks.focus
        && &current.focus != focus
    {
        return fail("focus");
    }
    if let Some(id) = &checks.selected
        && current.selected.as_deref() != Some(id.as_str())
    {
        return fail("selected");
    }
    if let Some(id) = &checks.detail
        && current.detail.as_deref() != Some(id.as_str())
    {
        return fail("detail");
    }
    if let Some(mode) = &checks.declutter
        && &current.declutter != mode
    {
        return fail("declutter");
    }
    if let Some(dragging) = checks.dragging
        && current.dragging != dragging
    {
        return fail("dragging");
    }
    if let Some((name, factor)) = &checks.zoom_times {
        let before = named(name)?;
        let expected = (zoom_of(before.zoom) * factor).clamp(0.1, 10.0);
        if !near(current.zoom, expected) {
            return fail("zoom-times");
        }
    }
    if let Some(name) = &checks.zoom_unchanged
        && current.zoom != named(name)?.zoom
    {
        return fail("zoom-unchanged");
    }
    if let Some(name) = &checks.cam_unchanged {
        let before = named(name)?;
        if current.cam_x != before.cam_x || current.cam_y != before.cam_y {
            return fail("cam-unchanged");
        }
    }
    if let Some(name) = &checks.focus_unchanged
        && current.focus != named(name)?.focus
    {
        return fail("focus-unchanged");
    }
    if let Some(name) = &checks.selected_unchanged
        && current.selected != named(name)?.selected
    {
        return fail("selected-unchanged");
    }
    if let Some(([dx, dy], name)) = &checks.cam_shift {
        let before = named(name)?;
        let z = zoom_of(before.zoom);
        if !near(current.cam_x, before.cam_x + dx / z)
            || !near(current.cam_y, before.cam_y + dy / z)
        {
            return fail("cam-shift");
        }
    }
    if let Some(name) = &checks.cam_y_not_below
        && current.cam_y < named(name)?.cam_y - 1e-9
    {
        return fail("cam-y-not-below");
    }
    if let Some(name) = &checks.anchor_fixed {
        let before = named(name)?;
        match (before.anchor, current.anchor) {
            (Some((bx, by)), Some((ax, ay))) if near(ax, bx) && near(ay, by) => {}
            (Some(_), Some(_)) => return fail("anchor-fixed"),
            _ => return fail("anchor-fixed needs :anchor on both records"),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    WaitFrames(u64),
    WaitFile {
        evidence: String,
    },
    /// Wait until a project file exists and (when given) contains every
    /// needle: the driver's stand-in for a slow external step.
    WaitProject {
        project: String,
        contains: Vec<String>,
    },
    /// Copy a project file into the evidence directory.
    DumpProject {
        evidence: String,
        project: String,
    },
    /// Wall-clock timers: an external step is asynchronous, so a driver
    /// needs real elapsed time, not frames.
    StartTimer {
        name: String,
    },
    WaitTimer {
        name: String,
        ms: f64,
    },
    ReadEvidence {
        evidence: String,
        name: String,
    },
    Assert {
        evidence: String,
        /// Read the file from the project instead of the evidence directory.
        project: bool,
        contains: Vec<String>,
        contains_any: Vec<String>,
        differs_from: Option<String>,
        same_as: Option<String>,
        /// Keys compared away by `:same-as` (the schema keeps derived fields
        /// that a validator adds, e.g. `:proposal`).
        except: Vec<String>,
        message: String,
    },
    Edit {
        project: String,
        from: String,
        to: String,
        same_length: bool,
    },
    RecordMtime {
        project: String,
        name: String,
    },
    AssertMtime {
        project: String,
        name: String,
        message: String,
    },
    Input(Input),
    /// Layout the probe text upright and slanted through the real font atlas
    /// and compare the tessellated glyph vertices (the egui equivalent of the
    /// cimgui vertex-slant assertion).
    AssertItalicGlyphs {
        message: String,
    },
    View {
        zoom: Option<f64>,
        center: Option<[f64; 2]>,
    },
    Screenshot {
        evidence: String,
    },
    Write {
        evidence: String,
        text: String,
        /// Write into the project instead of the evidence dir.
        project: Option<String>,
    },
    DumpView {
        evidence: String,
    },
    RecordView {
        name: String,
        anchor: Option<[f64; 2]>,
    },
    AssertView {
        checks: Box<ViewChecks>,
        message: String,
    },
    AssertBlank {
        at: [f64; 2],
        message: String,
    },
    ClickClass {
        id: String,
        double: bool,
        /// Right-click instead of left (selects what it hits).
        secondary: bool,
    },
    ClickDeclutter,
    AssertFonts {
        message: String,
        /// Extra probe text whose glyphs every role must resolve.
        extra: Option<String>,
    },
    AssertTextFits {
        message: String,
    },
    DumpDisplay {
        evidence: String,
    },
    DumpQuality {
        evidence: String,
        id: String,
    },
    ResizeWindow {
        size: [f64; 2],
    },
    /// Move the egui pointer to a recorded widget (`:at` bounds key, centre
    /// plus `:offset`; `:prefix` adds the code-font width of that text from
    /// the left edge) or to `:xy`.
    GuiPoint(GuiTarget),
    /// Press or release an egui pointer button where the pointer is.
    GuiButton {
        secondary: bool,
        pressed: bool,
    },
    /// Point, press and release over three frames (a real egui click).
    GuiClick {
        target: GuiTarget,
        secondary: bool,
    },
    /// An egui key press and release (Ctrl+C also sends the copy event).
    GuiKey {
        key: String,
        ctrl: bool,
        shift: bool,
    },
    DumpGui {
        evidence: String,
    },
    AssertGui {
        path: Vec<Value>,
        check: GuiCheck,
        message: String,
    },
    /// Wait (wall clock) until a GUI record check passes; fail at the
    /// deadline. An external step is asynchronous, so the driver cannot
    /// count frames for it.
    WaitGui {
        path: Vec<Value>,
        check: GuiCheck,
        message: String,
        seconds: f64,
    },
    OpenSource {
        lang: String,
        file: String,
        name: Option<String>,
    },
    ResizeCard {
        size: [f64; 2],
    },
    /// Sample a screenshot inside a widget: the fill at its left inset and
    /// some pixel close to the ink color.
    AssertPixels {
        evidence: String,
        at: String,
        fill: Option<[u8; 3]>,
        ink: Option<[u8; 3]>,
        tolerance: u8,
        message: String,
    },
    Quit(u8),
}

/// Where a GUI pointer step goes.
#[derive(Debug, Clone, PartialEq)]
pub struct GuiTarget {
    pub at: Option<String>,
    pub xy: Option<[f64; 2]>,
    pub offset: [f64; 2],
    pub prefix: Option<String>,
}

/// One `:assert-gui` comparison against the value at `:path`.
#[derive(Debug, Clone, PartialEq)]
pub enum GuiCheck {
    Equals(Value),
    Gt(f64),
    Ge(f64),
    Lt(f64),
    Le(f64),
    Present(bool),
    Contains(String),
    /// The observed text must not contain this (a failure marker).
    NotContains(String),
    Count(usize),
}

/// Navigate a GUI record by keyword/string keys and vector indices.
pub fn at_path<'a>(record: &'a Value, path: &[Value]) -> Option<&'a Value> {
    let mut v = record;
    for key in path {
        v = match (v, key) {
            (Value::Vector(items), Value::Integer(i)) => items.get(*i as usize)?,
            (Value::Map(_), Value::Integer(_)) => return None,
            (Value::Map(_), k) => v.get(k)?,
            _ => return None,
        };
    }
    Some(v)
}

fn value_number(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(n) => Some(*n as f64),
        Value::Float(x) => Some(*x),
        _ => None,
    }
}

/// Evaluate a check; Err names the observed value.
pub fn check_gui(record: &Value, path: &[Value], check: &GuiCheck) -> Result<(), String> {
    let found = at_path(record, path).filter(|v| **v != Value::Nil);
    let shown = || found.map_or("nil".into(), |v| edn::encode(v).unwrap_or_default());
    let number = || found.and_then(value_number);
    let ok = match check {
        GuiCheck::Present(want) => found.is_some() == *want,
        GuiCheck::Equals(want) => match (found.and_then(value_number), value_number(want)) {
            (Some(a), Some(b)) => (a - b).abs() < EPSILON,
            _ => found.unwrap_or(&Value::Nil) == want,
        },
        GuiCheck::Gt(n) => number().is_some_and(|x| x > *n),
        GuiCheck::Ge(n) => number().is_some_and(|x| x >= *n - EPSILON),
        GuiCheck::Lt(n) => number().is_some_and(|x| x < *n),
        GuiCheck::Le(n) => number().is_some_and(|x| x <= *n + EPSILON),
        GuiCheck::Contains(text) => match found {
            Some(Value::String(s)) => s.contains(text.as_str()),
            Some(other) => edn::encode(other)
                .unwrap_or_default()
                .contains(text.as_str()),
            None => false,
        },
        GuiCheck::NotContains(text) => match found {
            Some(Value::String(s)) => !s.contains(text.as_str()),
            Some(other) => !edn::encode(other)
                .unwrap_or_default()
                .contains(text.as_str()),
            None => true,
        },
        GuiCheck::Count(n) => matches!(found, Some(Value::Vector(items)) if items.len() == *n),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("observed {}", shown()))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Script {
    pub steps: Vec<Step>,
}

fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    edn::record::get_raw(v, key)
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Keyword(k) | Value::Symbol(k) => Some(k.clone()),
        Value::Integer(n) => Some(n.to_string()),
        Value::Float(x) => Some(x.to_string()),
        _ => None,
    }
}

fn number(v: Option<&Value>) -> Option<f64> {
    v.and_then(edn::record::num)
}

fn boolean(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Vector(items)) => items.iter().filter_map(|x| text(Some(x))).collect(),
        Some(other) => text(Some(other)).into_iter().collect(),
        None => Vec::new(),
    }
}

fn pair(v: Option<&Value>) -> Option<[f64; 2]> {
    match v {
        Some(Value::Vector(items)) if items.len() == 2 => {
            Some([number(items.first())?, number(items.get(1))?])
        }
        _ => None,
    }
}

impl Script {
    pub fn parse(source: &str) -> Result<Script, String> {
        let value = edn::parse(source).map_err(|e| format!("driver script is not EDN: {e}"))?;
        let steps = match get(&value, "steps") {
            Some(Value::Vector(steps)) => steps,
            _ => return Err("driver script needs a :steps vector".into()),
        };
        let mut out = Vec::with_capacity(steps.len());
        for step in steps {
            out.push(parse_step(step)?);
        }
        Ok(Script { steps: out })
    }

    pub fn load(path: &Path) -> Result<Script, String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read driver script {}: {e}", path.display()))?;
        Script::parse(&source)
    }
}

fn required<'a>(step: &'a Value, key: &str) -> Result<&'a Value, String> {
    get(step, key).ok_or_else(|| {
        format!(
            "driver step needs :{key}: {}",
            edn::encode(step).unwrap_or_default()
        )
    })
}

fn evidence_name(step: &Value) -> Result<String, String> {
    text(get(step, "evidence")).ok_or_else(|| "driver step needs :evidence".into())
}

fn message(step: &Value, default: &str) -> String {
    text(get(step, "message")).unwrap_or_else(|| default.into())
}

fn input_of(step: &Value, op: &str) -> Result<Input, String> {
    let at = pair(get(step, "at"));
    let union = |name: &str| -> Result<[f64; 2], String> {
        pair(get(step, name)).ok_or_else(|| format!("{op} needs :{name} [x y]"))
    };
    Ok(match op {
        "click" => {
            let p = at.ok_or("click needs :at [x y]")?;
            Input::Click { x: p[0], y: p[1] }
        }
        "double-click" => {
            let p = at.ok_or("double-click needs :at [x y]")?;
            Input::DoubleClick { x: p[0], y: p[1] }
        }
        "mouse-move" => {
            let p = at.ok_or("mouse-move needs :at [x y]")?;
            Input::MouseMove { x: p[0], y: p[1] }
        }
        "drag" => Input::Drag {
            from: union("from")?,
            to: union("to")?,
        },
        "wheel" => Input::Wheel {
            dx: number(get(step, "dx")).unwrap_or(0.0),
            dy: number(get(step, "dy")).unwrap_or(0.0),
            ctrl: edn::record::is_true(step, "ctrl"),
            shift: edn::record::is_true(step, "shift"),
        },
        "key" => Input::Key {
            key: text(get(step, "key")).ok_or("key needs :key")?,
            ctrl: edn::record::is_true(step, "ctrl"),
            shift: edn::record::is_true(step, "shift"),
        },
        "text" => Input::Text(text(get(step, "text")).ok_or("text needs :text")?),
        "paste" => Input::Paste(text(get(step, "text")).ok_or("paste needs :text")?),
        other => return Err(format!("unknown driver op :{other}")),
    })
}

/// `[:name factor]`
fn named_factor(v: Option<&Value>) -> Result<Option<(String, f64)>, String> {
    match v {
        None => Ok(None),
        Some(Value::Vector(items)) if items.len() == 2 => Ok(Some((
            text(items.first()).ok_or(":zoom-times needs [:name factor]")?,
            number(items.get(1)).ok_or(":zoom-times needs [:name factor]")?,
        ))),
        Some(_) => Err(":zoom-times needs [:name factor]".into()),
    }
}

/// `[[dx dy] :name]`
fn named_shift(v: Option<&Value>) -> Result<Option<([f64; 2], String)>, String> {
    match v {
        None => Ok(None),
        Some(Value::Vector(items)) if items.len() == 2 => Ok(Some((
            pair(items.first()).ok_or(":cam-shift needs [[dx dy] :name]")?,
            text(items.get(1)).ok_or(":cam-shift needs [[dx dy] :name]")?,
        ))),
        Some(_) => Err(":cam-shift needs [[dx dy] :name]".into()),
    }
}

fn view_checks(step: &Value) -> Result<ViewChecks, String> {
    Ok(ViewChecks {
        focus: get(step, "focus").map(|f| strings(Some(f))),
        selected: text(get(step, "selected")),
        detail: text(get(step, "detail")),
        declutter: text(get(step, "declutter")),
        dragging: boolean(get(step, "dragging")),
        fitted: boolean(get(step, "fitted")),
        zoom_times: named_factor(get(step, "zoom-times"))?,
        zoom_unchanged: text(get(step, "zoom-unchanged")),
        cam_unchanged: text(get(step, "cam-unchanged")),
        focus_unchanged: text(get(step, "focus-unchanged")),
        selected_unchanged: text(get(step, "selected-unchanged")),
        cam_shift: named_shift(get(step, "cam-shift"))?,
        cam_y_not_below: text(get(step, "cam-y-not-below")),
        anchor: pair(get(step, "anchor")),
        anchor_fixed: text(get(step, "anchor-fixed")),
    })
}

fn rgb(v: Option<&Value>) -> Option<[u8; 3]> {
    match v {
        Some(Value::Vector(items)) if items.len() == 3 => Some([
            number(items.first())? as u8,
            number(items.get(1))? as u8,
            number(items.get(2))? as u8,
        ]),
        _ => None,
    }
}

fn gui_target(step: &Value) -> Result<GuiTarget, String> {
    let at = get(step, "at");
    let target = GuiTarget {
        at: if matches!(at, Some(Value::Vector(_))) {
            None
        } else {
            text(at)
        },
        xy: pair(get(step, "xy")).or_else(|| pair(at)),
        offset: pair(get(step, "offset")).unwrap_or([0.0, 0.0]),
        prefix: text(get(step, "prefix")),
    };
    if target.at.is_none() && target.xy.is_none() {
        return Err("a GUI pointer step needs :at \"bounds-key\" or :xy [x y]".into());
    }
    Ok(target)
}

fn gui_check(step: &Value) -> Result<GuiCheck, String> {
    if let Some(v) = get(step, "equals") {
        return Ok(GuiCheck::Equals(v.clone()));
    }
    type NumericCheck = (&'static str, fn(f64) -> GuiCheck);
    let numeric: [NumericCheck; 4] = [
        ("gt", GuiCheck::Gt),
        ("ge", GuiCheck::Ge),
        ("lt", GuiCheck::Lt),
        ("le", GuiCheck::Le),
    ];
    for (key, make) in numeric {
        if let Some(n) = number(get(step, key)) {
            return Ok(make(n));
        }
    }
    if let Some(b) = boolean(get(step, "present")) {
        return Ok(GuiCheck::Present(b));
    }
    if let Some(t) = text(get(step, "contains")) {
        return Ok(GuiCheck::Contains(t));
    }
    if let Some(t) = text(get(step, "not-contains")) {
        return Ok(GuiCheck::NotContains(t));
    }
    if let Some(n) = number(get(step, "count")) {
        return Ok(GuiCheck::Count(n as usize));
    }
    Err("assert-gui needs :equals, :gt, :ge, :lt, :le, :present, :contains, :not-contains or :count".into())
}

fn parse_step(step: &Value) -> Result<Step, String> {
    let op = text(get(step, "op")).ok_or("driver step needs :op")?;
    Ok(match op.as_str() {
        "click" | "double-click" | "mouse-move" | "drag" | "wheel" | "key" | "text" | "paste" => {
            Step::Input(input_of(step, &op)?)
        }
        "wait-frames" => Step::WaitFrames(
            number(get(step, "n"))
                .ok_or("wait-frames needs :n")?
                .max(0.0) as u64,
        ),
        "wait-project" => Step::WaitProject {
            project: text(Some(required(step, "project")?)).ok_or("wait-project needs :project")?,
            contains: strings(get(step, "contains")),
        },
        "dump-project" => Step::DumpProject {
            evidence: evidence_name(step)?,
            project: text(Some(required(step, "project")?)).ok_or("dump-project needs :project")?,
        },
        "start-timer" => Step::StartTimer {
            name: text(Some(required(step, "as")?)).ok_or("start-timer needs :as")?,
        },
        "wait-timer" => Step::WaitTimer {
            name: text(Some(required(step, "as")?)).ok_or("wait-timer needs :as")?,
            ms: number(get(step, "ms")).ok_or("wait-timer needs :ms")?,
        },
        "wait-file" => Step::WaitFile {
            evidence: evidence_name(step)?,
        },
        "read" => Step::ReadEvidence {
            evidence: evidence_name(step)?,
            name: text(Some(required(step, "as")?)).ok_or("read needs :as")?,
        },
        "assert" => Step::Assert {
            project: get(step, "project").is_some(),
            evidence: text(get(step, "project")).map_or_else(|| evidence_name(step), Ok)?,
            contains: strings(get(step, "contains")),
            contains_any: strings(get(step, "contains-any")),
            differs_from: text(get(step, "differs-from")),
            same_as: text(get(step, "same-as")),
            except: strings(get(step, "except")),
            message: message(step, "driver assertion failed"),
        },
        "edit" => Step::Edit {
            project: text(Some(required(step, "project")?)).ok_or("edit needs :project")?,
            from: text(Some(required(step, "from")?)).ok_or("edit needs :from")?,
            to: text(Some(required(step, "to")?)).ok_or("edit needs :to")?,
            same_length: edn::record::is_true(step, "same-length"),
        },
        "record-mtime" => Step::RecordMtime {
            project: text(Some(required(step, "project")?)).ok_or("record-mtime needs :project")?,
            name: text(Some(required(step, "as")?)).ok_or("record-mtime needs :as")?,
        },
        "assert-mtime" => Step::AssertMtime {
            project: text(Some(required(step, "project")?)).ok_or("assert-mtime needs :project")?,
            name: text(Some(required(step, "same-as")?)).ok_or("assert-mtime needs :same-as")?,
            message: message(step, "file mtime changed unexpectedly"),
        },
        "assert-italic-glyphs" => Step::AssertItalicGlyphs {
            message: message(step, "italic names must slant real glyphs"),
        },
        "view" => Step::View {
            zoom: number(get(step, "zoom")),
            center: pair(get(step, "center")),
        },
        "screenshot" => Step::Screenshot {
            evidence: evidence_name(step)?,
        },
        "write" => Step::Write {
            evidence: evidence_name(step)?,
            text: text(get(step, "text")).ok_or("write needs :text")?,
            project: None,
        },
        "write-project" => Step::Write {
            evidence: String::new(),
            text: text(get(step, "text")).ok_or("write-project needs :text")?,
            project: Some(
                text(Some(required(step, "project")?)).ok_or("write-project needs :project")?,
            ),
        },
        "dump-view" => Step::DumpView {
            evidence: evidence_name(step)?,
        },
        "record-view" => Step::RecordView {
            name: text(Some(required(step, "as")?)).ok_or("record-view needs :as")?,
            anchor: pair(get(step, "anchor")),
        },
        "assert-view" => Step::AssertView {
            checks: Box::new(view_checks(step)?),
            message: message(step, "view assertion failed"),
        },
        "assert-blank" => Step::AssertBlank {
            at: pair(get(step, "at")).ok_or("assert-blank needs :at [x y]")?,
            message: message(step, "the point is not blank canvas"),
        },
        "click-class" | "double-click-class" => Step::ClickClass {
            id: text(Some(required(step, "id")?)).ok_or("click-class needs :id")?,
            double: op == "double-click-class",
            secondary: text(get(step, "button")).as_deref() == Some("secondary"),
        },
        "click-declutter" => Step::ClickDeclutter,
        "assert-fonts" => Step::AssertFonts {
            message: message(step, "font lookup or glyph coverage failed"),
            extra: text(get(step, "extra")),
        },
        "assert-text-fits" => Step::AssertTextFits {
            message: message(step, "canvas text exceeds its slot"),
        },
        "dump-display" => Step::DumpDisplay {
            evidence: evidence_name(step)?,
        },
        "dump-quality" => Step::DumpQuality {
            evidence: evidence_name(step)?,
            id: text(Some(required(step, "id")?)).ok_or("dump-quality needs :id")?,
        },
        "resize-window" => Step::ResizeWindow {
            size: pair(get(step, "size")).ok_or("resize-window needs :size [w h]")?,
        },
        "gui-point" => Step::GuiPoint(gui_target(step)?),
        "gui-press" | "gui-release" => Step::GuiButton {
            secondary: text(get(step, "button")).as_deref() == Some("secondary"),
            pressed: op == "gui-press",
        },
        "gui-click" => Step::GuiClick {
            target: gui_target(step)?,
            secondary: text(get(step, "button")).as_deref() == Some("secondary"),
        },
        "gui-key" => Step::GuiKey {
            key: text(get(step, "key")).ok_or("gui-key needs :key")?,
            ctrl: edn::record::is_true(step, "ctrl"),
            shift: edn::record::is_true(step, "shift"),
        },
        "dump-gui" => Step::DumpGui {
            evidence: evidence_name(step)?,
        },
        "wait-gui" => Step::WaitGui {
            path: match get(step, "path") {
                Some(Value::Vector(items)) => items.clone(),
                _ => return Err("wait-gui needs :path [..]".into()),
            },
            check: gui_check(step)?,
            message: message(step, "GUI wait failed"),
            seconds: number(get(step, "seconds")).unwrap_or(60.0),
        },
        "assert-gui" => Step::AssertGui {
            path: match get(step, "path") {
                Some(Value::Vector(items)) => items.clone(),
                _ => return Err("assert-gui needs :path [..]".into()),
            },
            check: gui_check(step)?,
            message: message(step, "GUI assertion failed"),
        },
        "open-source" => Step::OpenSource {
            lang: text(Some(required(step, "lang")?)).ok_or("open-source needs :lang")?,
            file: text(Some(required(step, "file")?)).ok_or("open-source needs :file")?,
            name: text(get(step, "name")),
        },
        "resize-card" => Step::ResizeCard {
            size: pair(get(step, "size")).ok_or("resize-card needs :size [w h]")?,
        },
        "assert-pixels" => Step::AssertPixels {
            evidence: evidence_name(step)?,
            at: text(Some(required(step, "at")?)).ok_or("assert-pixels needs :at")?,
            fill: rgb(get(step, "fill")),
            ink: rgb(get(step, "ink")),
            tolerance: number(get(step, "tolerance")).unwrap_or(2.0) as u8,
            message: message(step, "widget colors differ"),
        },
        "quit" => Step::Quit(number(get(step, "code")).unwrap_or(0.0).max(0.0) as u8),
        other => return Err(format!("unknown driver op :{other}")),
    })
}

/// The app-side effects of a driver step.
pub trait Host {
    fn inject(&mut self, input: Input);
    /// Lay out `探针` upright and slanted and compare glyph vertices.
    fn assert_italic_glyphs(&mut self) -> Result<(), String>;
    /// Request a screenshot; false until it has been written.
    fn screenshot(&mut self, evidence: &str) -> bool;
    fn set_view(&mut self, zoom: Option<f64>, center: Option<[f64; 2]>);
    fn quit(&mut self, code: u8);
    /// The current navigation/search state; `anchor` is a screen point whose
    /// world position the record carries.
    fn view_record(&mut self, anchor: Option<[f64; 2]>) -> ViewRecord;
    /// Does the view equal a fresh fit of the usable canvas?
    fn is_fitted(&self) -> bool;
    /// No scene element under this screen point.
    fn is_blank(&self, x: f64, y: f64) -> bool;
    /// Click (or double-click, or right-click) inside a rendered class box.
    fn click_class(&mut self, id: &str, double: bool, secondary: bool) -> Result<(), String>;
    /// Click the centre of the declutter control through the input path.
    fn click_declutter(&mut self);
    /// Font source, role sizes and glyph coverage (env-driven expectations).
    fn assert_fonts(&mut self) -> Result<(), String>;
    /// Every canvas text command fits its vertical and horizontal slot.
    fn assert_text_fits(&mut self) -> Result<(), String>;
    /// `{:ui scale :window {:w :h} :fonts {...}}`
    fn display_record(&self) -> Value;
    /// One diagnosis line for the class's quality rendering.
    fn quality_line(&self, id: &str) -> Option<String>;
    fn resize_window(&mut self, w: f64, h: f64);
    /// Font check with an extra probe text (default: the plain check).
    fn assert_fonts_with(&mut self, extra: Option<&str>) -> Result<(), String> {
        let _ = extra;
        self.assert_fonts()
    }
    /// Queued GUI events not yet delivered; the runner waits for them.
    fn busy(&self) -> bool {
        false
    }
    fn gui_point(&mut self, target: &GuiTarget) -> Result<(), String> {
        let _ = target;
        Err("this host has no GUI".into())
    }
    fn gui_button(&mut self, secondary: bool, pressed: bool) {
        let _ = (secondary, pressed);
    }
    fn gui_click(&mut self, target: &GuiTarget, secondary: bool) -> Result<(), String> {
        let _ = (target, secondary);
        Err("this host has no GUI".into())
    }
    fn gui_key(&mut self, key: &str, ctrl: bool, shift: bool) {
        let _ = (key, ctrl, shift);
    }
    /// The inspector / card / source window record (`:dump-gui`).
    fn gui_record(&self) -> Value {
        Value::Nil
    }
    fn open_source(&mut self, lang: &str, file: &str, name: Option<&str>) -> Result<(), String> {
        let _ = (lang, file, name);
        Err("this host has no source windows".into())
    }
    fn resize_card(&mut self, w: f64, h: f64) {
        let _ = (w, h);
    }
    /// A widget's rect in screenshot pixels: [x0, y0, x1, y1].
    fn widget_pixels(&self, key: &str) -> Option<[f64; 4]> {
        let _ = key;
        None
    }
}

fn near_rgb(a: &[u8], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tolerance)
}

/// The fill at the widget's left inset and any pixel near the ink color.
fn check_pixels(
    png: &Path,
    rect: [f64; 4],
    fill: Option<[u8; 3]>,
    ink: Option<[u8; 3]>,
    tolerance: u8,
) -> Result<(), String> {
    let image = image::open(png)
        .map_err(|e| format!("cannot read {}: {e}", png.display()))?
        .to_rgb8();
    let [x0, y0, x1, y1] = rect.map(|v| v.round().max(0.0) as u32);
    let (x1, y1) = (x1.min(image.width()), y1.min(image.height()));
    if x0 + 4 >= x1 || y0 + 2 >= y1 {
        return Err("widget outside the screenshot".into());
    }
    if let Some(fill) = fill {
        let p = image.get_pixel(x0 + 3, (y0 + y1) / 2);
        if !near_rgb(&p.0, fill, tolerance) {
            return Err(format!("fill {:?} expected {fill:?}", p.0));
        }
    }
    if let Some(ink) = ink {
        let found =
            (y0..y1).any(|y| (x0..x1).any(|x| near_rgb(&image.get_pixel(x, y).0, ink, tolerance)));
        if !found {
            return Err(format!("no pixel near ink {ink:?}"));
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct Runner {
    script: Script,
    cursor: usize,
    /// (step index, frame) where the current `:wait-frames` began.
    waiting_since: Option<(usize, u64)>,
    values: HashMap<String, String>,
    records: HashMap<String, ViewRecord>,
    /// `:start-timer` origins, by name.
    timers: HashMap<String, std::time::Instant>,
}

/// Outcome of one frame's worth of driver work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Running,
    Done,
}

impl Runner {
    pub fn new(script: Script) -> Runner {
        Runner {
            script,
            ..Runner::default()
        }
    }

    pub fn is_done(&self) -> bool {
        self.cursor >= self.script.steps.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Advance as far as this frame allows. Failures carry the step's
    /// assertion message, which the caller writes to stderr before exiting 1.
    pub fn run(
        &mut self,
        frame: u64,
        evidence: Option<&Path>,
        project: &Path,
        host: &mut dyn Host,
    ) -> Result<Progress, String> {
        while let Some(step) = self.script.steps.get(self.cursor).cloned() {
            if host.busy() {
                return Ok(Progress::Running);
            }
            match step {
                Step::WaitFrames(n) => {
                    let since = match self.waiting_since {
                        Some((index, since)) if index == self.cursor => since,
                        _ => {
                            self.waiting_since = Some((self.cursor, frame));
                            frame
                        }
                    };
                    if frame.saturating_sub(since) < n {
                        return Ok(Progress::Running);
                    }
                }
                Step::WaitFile { evidence: name } => {
                    if !self.evidence_path(evidence, &name)?.exists() {
                        return Ok(Progress::Running);
                    }
                }
                Step::WaitProject {
                    project: rel,
                    contains,
                } => {
                    let path = project.join(&rel);
                    let ready = std::fs::read_to_string(&path)
                        .is_ok_and(|body| contains.iter().all(|needle| body.contains(needle)));
                    if !ready {
                        return Ok(Progress::Running);
                    }
                }
                Step::DumpProject {
                    evidence: name,
                    project: rel,
                } => {
                    let path = project.join(&rel);
                    let Ok(body) = std::fs::read_to_string(&path) else {
                        return Ok(Progress::Running);
                    };
                    self.write_evidence(evidence, &name, &body)?;
                }
                Step::StartTimer { name } => {
                    self.timers.insert(name, std::time::Instant::now());
                }
                Step::WaitTimer { name, ms } => {
                    let Some(started) = self.timers.get(&name) else {
                        return Err(format!("wait-timer :{name} has no start-timer"));
                    };
                    if started.elapsed().as_secs_f64() * 1000.0 < ms {
                        return Ok(Progress::Running);
                    }
                }
                Step::ReadEvidence {
                    evidence: name,
                    name: slot,
                } => {
                    let path = self.evidence_path(evidence, &name)?;
                    let Ok(body) = std::fs::read_to_string(&path) else {
                        return Ok(Progress::Running);
                    };
                    self.values.insert(slot, body);
                }
                Step::Assert {
                    evidence: name,
                    project: in_project,
                    contains,
                    contains_any,
                    differs_from,
                    same_as,
                    except,
                    message,
                } => {
                    let path = if in_project {
                        project.join(&name)
                    } else {
                        self.evidence_path(evidence, &name)?
                    };
                    let Ok(body) = std::fs::read_to_string(&path) else {
                        if in_project {
                            return Err(format!("{message}: cannot read {}", path.display()));
                        }
                        return Ok(Progress::Running);
                    };
                    for needle in &contains {
                        if !body.contains(needle) {
                            return Err(message);
                        }
                    }
                    if !contains_any.is_empty() && !contains_any.iter().any(|n| body.contains(n)) {
                        return Err(message);
                    }
                    if let Some(other) = differs_from {
                        let other_path = self.evidence_path(evidence, &other)?;
                        let Ok(other_body) = std::fs::read_to_string(&other_path) else {
                            return Ok(Progress::Running);
                        };
                        if other_body == body {
                            return Err(message);
                        }
                    }
                    if let Some(other) = same_as {
                        let other_path = self
                            .evidence_path(evidence, &other)
                            .or_else(|_| Ok::<_, String>(project.join(&other)))?;
                        let Ok(other_body) = std::fs::read_to_string(&other_path) else {
                            return Ok(Progress::Running);
                        };
                        let strip = |text: &str| -> Option<Value> {
                            let value = edn::parse(text).ok()?;
                            let kept: Vec<(Value, Value)> = match value {
                                Value::Map(entries) => entries
                                    .into_iter()
                                    .filter(|(k, _)| {
                                        !except.iter().any(|drop| Value::keyword(drop) == *k)
                                    })
                                    .collect(),
                                other => return Some(other),
                            };
                            Some(Value::Map(kept))
                        };
                        // EDN equality, not bytes: the two writers order
                        // maps independently and a validator may add derived
                        // fields named in `:except`.
                        match (strip(&body), strip(&other_body)) {
                            (Some(mine), Some(theirs)) if mine == theirs => {}
                            _ => {
                                eprintln!(
                                    "assert same-as: {} differs from {} ({:?} vs {:?})",
                                    path.display(),
                                    other_path.display(),
                                    strip(&body),
                                    strip(&other_body)
                                );
                                return Err(message);
                            }
                        }
                    }
                }
                Step::Edit {
                    project: rel,
                    from,
                    to,
                    same_length,
                } => {
                    let path = project.join(&rel);
                    let body = std::fs::read_to_string(&path)
                        .map_err(|e| format!("cannot edit {}: {e}", path.display()))?;
                    if !body.contains(&from) {
                        return Err(format!("{} does not contain {from:?}", path.display()));
                    }
                    let edited = body.replace(&from, &to);
                    if same_length && edited.len() != body.len() {
                        return Err(format!(
                            "the edit {} -> {to:?} must keep the byte length",
                            from
                        ));
                    }
                    std::fs::write(&path, edited)
                        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                }
                Step::RecordMtime { project: rel, name } => {
                    let path = project.join(&rel);
                    let stamp = watch::mtime(&path)
                        .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
                    self.values.insert(format!("mtime:{name}"), stamp);
                }
                Step::AssertMtime {
                    project: rel,
                    name,
                    message,
                } => {
                    let path = project.join(&rel);
                    let stamp = watch::mtime(&path)
                        .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
                    if self.values.get(&format!("mtime:{name}")) != Some(&stamp) {
                        return Err(message);
                    }
                }
                Step::Input(input) => {
                    host.inject(input);
                    return Ok(self.yield_after_input());
                }
                Step::AssertItalicGlyphs { message } => host
                    .assert_italic_glyphs()
                    .map_err(|reason| format!("{message}: {reason}"))?,
                Step::View { zoom, center } => host.set_view(zoom, center),
                Step::Screenshot { evidence: name } => {
                    let target = self.evidence_path(evidence, &name)?;
                    if !(target.exists() || host.screenshot(&name)) {
                        return Ok(Progress::Running);
                    }
                }
                Step::Write {
                    evidence: name,
                    text,
                    project: rel,
                } => match rel {
                    Some(rel) => {
                        let path = project.join(&rel);
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)
                                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
                        }
                        std::fs::write(&path, text)
                            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                    }
                    None => self.write_evidence(evidence, &name, &text)?,
                },
                Step::DumpView { evidence: name } => {
                    let record = host.view_record(None);
                    let body = edn::encode(&record.to_edn()).map_err(|e| e.to_string())?;
                    self.write_evidence(evidence, &name, &body)?;
                }
                Step::RecordView { name, anchor } => {
                    let record = host.view_record(anchor);
                    self.records.insert(name, record);
                }
                Step::AssertView { checks, message } => {
                    let current = host.view_record(checks.anchor);
                    // The step message is the failure, as with the Lua check();
                    // stderr names the failing check.
                    check_view(&current, &self.records, &checks).map_err(|what| {
                        eprintln!("assert-view: {what} does not hold");
                        message.clone()
                    })?;
                    if let Some(fitted) = checks.fitted
                        && host.is_fitted() != fitted
                    {
                        return Err(message);
                    }
                }
                Step::AssertBlank { at, message } => {
                    if !host.is_blank(at[0], at[1]) {
                        return Err(message);
                    }
                }
                Step::ClickClass {
                    id,
                    double,
                    secondary,
                } => {
                    host.click_class(&id, double, secondary)?;
                    return Ok(self.yield_after_input());
                }
                Step::ClickDeclutter => {
                    host.click_declutter();
                    return Ok(self.yield_after_input());
                }
                Step::AssertFonts { message, extra } => host
                    .assert_fonts_with(extra.as_deref())
                    .map_err(|reason| format!("{message}: {reason}"))?,
                Step::AssertTextFits { message } => host
                    .assert_text_fits()
                    .map_err(|reason| format!("{message}: {reason}"))?,
                Step::DumpDisplay { evidence: name } => {
                    let body = edn::encode(&host.display_record()).map_err(|e| e.to_string())?;
                    self.write_evidence(evidence, &name, &body)?;
                }
                Step::DumpQuality { evidence: name, id } => {
                    let line = host
                        .quality_line(&id)
                        .ok_or_else(|| format!("no class {id} on the scene"))?;
                    self.write_evidence(evidence, &name, &format!("{line}\n"))?;
                }
                Step::ResizeWindow { size } => host.resize_window(size[0], size[1]),
                Step::GuiPoint(target) => {
                    host.gui_point(&target)?;
                    return Ok(self.yield_after_input());
                }
                Step::GuiButton { secondary, pressed } => {
                    host.gui_button(secondary, pressed);
                    return Ok(self.yield_after_input());
                }
                Step::GuiClick { target, secondary } => {
                    host.gui_click(&target, secondary)?;
                    return Ok(self.yield_after_input());
                }
                Step::GuiKey { key, ctrl, shift } => {
                    host.gui_key(&key, ctrl, shift);
                    return Ok(self.yield_after_input());
                }
                Step::DumpGui { evidence: name } => {
                    let body = edn::encode(&host.gui_record()).map_err(|e| e.to_string())?;
                    self.write_evidence(evidence, &name, &body)?;
                }
                Step::AssertGui {
                    path,
                    check,
                    message,
                } => check_gui(&host.gui_record(), &path, &check).map_err(|seen| {
                    let at = edn::encode(&Value::Vector(path.clone())).unwrap_or_default();
                    eprintln!("assert-gui {at}: {seen}");
                    message.clone()
                })?,
                Step::WaitGui {
                    path,
                    check,
                    message,
                    seconds,
                } => {
                    if check_gui(&host.gui_record(), &path, &check).is_ok() {
                        // Passed: continue with the next step this frame.
                    } else {
                        let key = format!("wait-gui:{}", self.cursor);
                        let started = *self
                            .timers
                            .entry(key.clone())
                            .or_insert_with(std::time::Instant::now);
                        if started.elapsed().as_secs_f64() < seconds {
                            return Ok(Progress::Running);
                        }
                        self.timers.remove(&key);
                        let at = edn::encode(&Value::Vector(path.clone())).unwrap_or_default();
                        let seen = check_gui(&host.gui_record(), &path, &check)
                            .err()
                            .unwrap_or_default();
                        eprintln!("wait-gui {at}: {seen}");
                        return Err(message);
                    }
                }
                Step::OpenSource { lang, file, name } => {
                    host.open_source(&lang, &file, name.as_deref())?;
                    return Ok(self.yield_after_input());
                }
                Step::ResizeCard { size } => {
                    host.resize_card(size[0], size[1]);
                    return Ok(self.yield_after_input());
                }
                Step::AssertPixels {
                    evidence: name,
                    at,
                    fill,
                    ink,
                    tolerance,
                    message,
                } => {
                    let png = self.evidence_path(evidence, &name)?;
                    if !png.exists() {
                        return Ok(Progress::Running);
                    }
                    let rect = host
                        .widget_pixels(&at)
                        .ok_or_else(|| format!("{message}: no widget {at}"))?;
                    check_pixels(&png, rect, fill, ink, tolerance)
                        .map_err(|reason| format!("{message}: {reason}"))?;
                }
                Step::Quit(code) => {
                    host.quit(code);
                    self.cursor = self.script.steps.len();
                    return Ok(Progress::Done);
                }
            }
            self.cursor += 1;
        }
        Ok(Progress::Done)
    }

    /// Inputs end the frame's work so the next step observes their effect.
    fn yield_after_input(&mut self) -> Progress {
        self.cursor += 1;
        if self.is_done() {
            Progress::Done
        } else {
            Progress::Running
        }
    }

    fn write_evidence(
        &self,
        evidence: Option<&Path>,
        name: &str,
        text: &str,
    ) -> Result<(), String> {
        let target = self.evidence_path(evidence, name)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, text).map_err(|e| format!("cannot write {}: {e}", target.display()))
    }

    fn evidence_path(&self, evidence: Option<&Path>, name: &str) -> Result<PathBuf, String> {
        let dir = evidence.ok_or_else(|| format!("driver needs UML_VIEWER_EVIDENCE for {name}"))?;
        Ok(dir.join(name))
    }
}
