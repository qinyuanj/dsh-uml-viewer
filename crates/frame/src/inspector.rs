//! Inspector sidebar model (#141): port of `application/inspector.lua` and
//! the inspector-width rules of `domain/display.lua`. Widths are points
//! (the Lua DPI-independent view units); egui applies the display scale.

use edn::Value;
use edn::record::{get, local_name};
use scene::document::ViewState;

use crate::{detail, strings};

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
