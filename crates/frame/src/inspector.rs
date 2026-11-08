//! Inspector sidebar model (#141): port of `application/inspector.lua` and
//! the inspector-width rules of `domain/display.lua`. Widths are points
//! (the Lua DPI-independent view units); egui applies the display scale.

use edn::Value;
use edn::record::{get, local_name, num};
use scene::document::ViewState;

use crate::theme::Role;
use crate::{detail, strings};

/// One inspector summary line, shared by native and panel renderers.
/// Metric segments wrap between whole statistics, never within one.
#[derive(Debug, Clone, PartialEq)]
pub enum SummaryLine {
    Label { text: String, color: Role },
    Segments { parts: Vec<String>, color: Role },
}

fn label(text: impl Into<String>, color: Role) -> SummaryLine {
    SummaryLine::Label {
        text: text.into(),
        color,
    }
}

fn mutation_line(c: &Value) -> Option<SummaryLine> {
    let (k, s) = (
        get(c, "killed").and_then(num),
        get(c, "survived").and_then(num),
    );
    if k.is_none() && s.is_none() {
        return None;
    }
    let mut parts = vec![format!(
        "{} killed / {} survived",
        k.unwrap_or(0.0) as i64,
        s.unwrap_or(0.0) as i64
    )];
    if let Some(u) = get(c, "uncovered").and_then(num).filter(|u| *u > 0.0) {
        parts.push(format!("{} uncovered", u as i64));
    }
    Some(SummaryLine::Segments {
        parts,
        color: Role::Muted,
    })
}

fn crap_summary(crap: &Value) -> Option<SummaryLine> {
    let mu = get(crap, "mu").and_then(num)?;
    let max = get(crap, "max").and_then(num).unwrap_or(mu);
    let sigma = get(crap, "sigma").and_then(num).unwrap_or(0.0);
    let parts = strings::CRAP_LINE
        .split_inclusive("%s")
        .zip([mu, max, sigma])
        .map(|(part, x)| part.trim().replace("%s", &format!("{x:.1}")))
        .collect();
    Some(SummaryLine::Segments {
        parts,
        color: Role::Accent,
    })
}

/// Selected class identity, quality and member text, in display order.
/// The no-data line preserves the inspector's absence-of-CRAP behavior.
pub fn class_summary(model: &detail::Model) -> Vec<SummaryLine> {
    let c = &model.class;
    let text = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Integer(n)) => n.to_string(),
        Some(Value::Float(x)) => x.to_string(),
        Some(other) => local_name(other).unwrap_or_default().to_owned(),
        None => String::new(),
    };
    let mut out = vec![label(text(get(c, "name")), Role::Ink)];
    if let Some(package) = model.package_label() {
        out.push(label(format!("{}{package}", strings::PACKAGE), Role::Muted));
    }
    if let Some(level) = get(c, "level").filter(|v| **v != Value::Nil) {
        out.push(label(
            format!("{}{}", strings::LEVEL, text(Some(level))),
            Role::Muted,
        ));
    }
    if let Some(stereo) = get(c, "stereotype") {
        out.push(label(
            format!("«{}»", local_name(stereo).unwrap_or_default()),
            Role::Muted,
        ));
    }
    let crap = get(c, "crap");
    let mut measured = false;
    if let Some(n) = crap.and_then(num) {
        out.push(label(
            strings::CRAP_SINGLE.replace("%s", &format!("{n:.1}")),
            Role::Accent,
        ));
        measured = true;
    } else if let Some(line) = crap.and_then(crap_summary) {
        out.push(line);
        measured = true;
    }
    if let Some(line) = mutation_line(c) {
        out.push(line);
    }
    for line in edn::record::seq(get(c, "lines")) {
        let kind = get(line, "kind").and_then(local_name).unwrap_or_default();
        if kind == "field" || kind == "op" {
            out.push(label(text(get(line, "text")), Role::Ink));
        }
    }
    if !measured {
        out.push(label(strings::NO_DATA, Role::NoData));
    }
    out
}

/// Package quality and the number of classes belonging to it in this view.
/// A missing package produces no lines.
pub fn package_summary(scene: &Value, id: &Value) -> Vec<SummaryLine> {
    let Some(p) = scene::hit::package_by_id(scene, id) else {
        return Vec::new();
    };
    let title = match get(p, "label") {
        Some(Value::String(s)) => s.clone(),
        _ => local_name(id).unwrap_or_default().to_owned(),
    };
    let mut out = vec![label(title, Role::Ink)];
    let crap = get(p, "crap");
    if let Some(line) = crap.and_then(crap_summary) {
        out.push(line);
    }
    if let Some(line) = mutation_line(p) {
        out.push(line);
    }
    let count = edn::record::seq(get(scene, "classes"))
        .iter()
        .filter(|c| get(c, "package") == Some(id))
        .count();
    out.push(label(
        format!("{}{count}", strings::CLASS_COUNT),
        Role::Muted,
    ));
    if crap.is_none() {
        out.push(label(strings::NO_DATA, Role::NoData));
    }
    out
}

/// The original sidebar width, its floor, and the canvas it must leave.
pub const DEFAULT_WIDTH: f64 = 280.0;
pub const MIN_WIDTH: f64 = 200.0;
pub const CANVAS_MIN: f64 = 240.0;
/// The left edge drags within this many points either side.
pub const EDGE_GRIP: f64 = 4.0;

pub fn clamp_width(width: Option<f64>, window_w: f64) -> f64 {
    let w = width.filter(|w| *w > 0.0).unwrap_or(DEFAULT_WIDTH);
    let most = MIN_WIDTH.max(window_w - CANVAS_MIN);
    w.min(most).max(MIN_WIDTH)
}

pub fn canvas_width(window_w: f64, width: f64) -> f64 {
    (window_w - width).max(0.0)
}

pub fn on_edge(x: f64, window_w: f64, width: f64) -> bool {
    (x - canvas_width(window_w, width)).abs() <= EDGE_GRIP
}

/// The width a drag of the left edge to `x` sets.
pub fn drag_width(x: f64, window_w: f64) -> f64 {
    clamp_width(Some(window_w - x), window_w)
}

/// One row of the diagram list: the source diagram (`id` None) or a proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: Option<Value>,
    pub label: String,
    pub active: bool,
}

pub fn entries(state: &ViewState) -> Vec<Entry> {
    let active = get(&state.extra, "proposal-id").filter(|v| **v != Value::Nil);
    let doc = state.document.clone().unwrap_or(Value::Map(vec![]));
    let title = match get(&doc, "title") {
        Some(Value::String(t)) => t.clone(),
        _ => strings::REAL_DIAGRAM.to_string(),
    };
    let mut out = vec![Entry {
        id: None,
        label: title,
        active: active.is_none(),
    }];
    for p in scene::proposals(&doc) {
        let id = get(&p, "id").cloned().unwrap_or(Value::Nil);
        let label = match get(&p, "name") {
            Some(Value::String(n)) => n.clone(),
            _ => local_name(&id).unwrap_or_default().to_owned(),
        };
        out.push(Entry {
            active: active == Some(&id),
            id: Some(id),
            label,
        });
    }
    out
}

/// A diagram choice: the new view and the `:context` mail it implies.
#[derive(Debug, Clone)]
pub struct Choice {
    pub state: ViewState,
    pub context: Value,
}

pub fn choose(state: &ViewState, id: Option<&Value>) -> Choice {
    let kw = Value::keyword;
    let context = match id {
        None => Value::Map(vec![(kw("context"), kw("real"))]),
        Some(id) => {
            let label = entries(state)
                .into_iter()
                .find(|e| e.id.as_ref() == Some(id))
                .map(|e| e.label)
                .unwrap_or_else(|| local_name(id).unwrap_or_default().to_owned());
            Value::Map(vec![
                (kw("context"), kw("proposal")),
                (kw("proposal-id"), id.clone()),
                (kw("name"), Value::String(label)),
            ])
        }
    };
    Choice {
        state: scene::events::show_proposal(state, id),
        context,
    }
}

/// The help text shown while nothing is selected.
pub fn empty_explanation(state: &ViewState) -> Option<&'static str> {
    state.selected.is_none().then_some(strings::HELP)
}

pub fn declutter_label(state: &ViewState) -> &'static str {
    let mode = get(&state.extra, "declutter")
        .map(|v| local_name(v).unwrap_or_default().to_owned())
        .unwrap_or_else(|| "full".into());
    strings::declutter_label(&mode)
}

/// The id whose card a wheel over the card scrolls.
pub fn card_id(state: &ViewState) -> Option<Value> {
    state
        .detail_id
        .clone()
        .or_else(|| state.selected.as_ref().and_then(|s| get(s, "id").cloned()))
}

/// Card wheel input: `delta` notches, clamped by the card's rows and the
/// viewport height the window reported.
pub fn scroll(state: &ViewState, delta: f64, view_h: f64) -> ViewState {
    let rows = card_id(state)
        .and_then(|id| detail::for_state(state, &id))
        .map(|m| detail::rows(&m))
        .unwrap_or_default();
    scroll_rows(state, &rows, delta, view_h)
}

/// [`scroll`] over card rows the caller already holds (the gui card cache).
pub fn scroll_rows(state: &ViewState, rows: &[detail::Row], delta: f64, view_h: f64) -> ViewState {
    let mut next = state.clone();
    next.detail_scroll = detail::scroll(rows, view_h, state.detail_scroll, delta);
    next
}
