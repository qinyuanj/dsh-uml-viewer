//! Frame-to-frame render cache (#143, spec #134 "渲染"): the class/edge draw
//! commands, their world bounds and the LOD-decimated edge polylines are
//! rebuilt only when the scene, selection, hover or LOD tier changes. Pan and
//! zoom only change the paint transform.

use edn::Value;
use scene::Rect;
use scene::document::ViewState;

use crate::commands::{self, Command, EdgeCommand};
use crate::lod::{self, Tier};

/// The inputs the cached geometry is a pure function of. The scene is
/// `compile_view(document, snapshots, focus, extra)`; every document or
/// snapshot change bumps the revision, and in-place recompiles (drill,
/// declutter, open layer) change `focus`/`extra`. Class and edge counts are a
/// cheap guard; a deep scene comparison costs ~10 ms per frame at 500 classes.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    revision: u64,
    focus: Vec<Value>,
    extra: Value,
    classes: usize,
    edges: usize,
    selected: Option<Value>,
    hover: Option<Value>,
}

fn count(scene: &Value, key: &str) -> usize {
    match edn::record::get(scene, key) {
        Some(Value::Vector(items)) => items.len(),
        _ => 0,
    }
}

/// One cached edge stroke: tier-decimated world points plus their bounds.
#[derive(Debug, Clone)]
pub struct Polyline {
    pub points: Vec<[f64; 2]>,
    pub bounds: Rect,
}

#[derive(Debug, Default)]
pub struct RenderCache {
    key: Option<Key>,
    /// Tier and mid-tier decimation step (quarter octaves of zoom; 0 outside
    /// `Frames`) the polylines were decimated for.
    lod: Option<(Tier, i32)>,
    commands: Vec<(Option<Rect>, Command)>,
    edges: Vec<EdgeCommand>,
    /// Per edge command: the decimated stroke for `Stroke`, None otherwise.
    polylines: Vec<Option<Polyline>>,
    builds: u64,
    command_builds: u64,
}

fn bounds_of(points: &[[f64; 2]]) -> Rect {
    let Some(first) = points.first() else {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first[0], first[1], first[0], first[1]);
    for p in points {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
    }
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Mid-tier zoom step: quarter octaves, so the decimation budget follows the
/// zoom within ±9 % while pan and small zooms reuse the built polylines.
fn decimation_step(zoom: f64, tier: Tier) -> i32 {
    if tier == Tier::Frames && zoom > 0.0 {
        (zoom.log2() * 4.0).round() as i32
    } else {
        0
    }
}

/// Chord tolerance of mid-tier strokes, in screen points.
const SIMPLIFY_PX: f64 = 0.5;

/// Mid-tier strokes drop points closer than `DECIMATE_PX` on screen at the
/// step's zoom (the #131 prototype's 8 px rule) and points within
/// `SIMPLIFY_PX` of the chord.
fn decimated(points: &[[f64; 2]], tier: Tier, step: i32) -> Vec<[f64; 2]> {
    if tier == Tier::Frames {
        let zoom = (step as f64 / 4.0).exp2();
        lod::simplify(points, lod::DECIMATE_PX / zoom, SIMPLIFY_PX / zoom)
    } else {
        points.to_vec()
    }
}

impl RenderCache {
    /// Bring the cache up to date for this frame's state and tier (the
    /// state's zoom only matters through the mid-tier decimation step).
    pub fn frame(&mut self, state: &ViewState, tier: Tier) {
        let key = Key {
            revision: state.revision,
            focus: state.focus.clone(),
            extra: state.extra.clone(),
            classes: count(&state.scene, "classes"),
            edges: count(&state.scene, "edges"),
            selected: state.selected.clone(),
            hover: state.hover.clone(),
        };
        let lod = (tier, decimation_step(state.zoom, tier));
        let scene_changed = self.key.as_ref() != Some(&key);
        if !scene_changed && self.lod == Some(lod) {
            return;
        }
        if scene_changed {
            let selected = state.selected.as_ref();
            let hover = state.hover.as_ref();
            self.commands = commands::commands(&state.scene, selected, hover)
                .into_iter()
                .map(|c| (c.bounds(), c))
                .collect();
            self.edges = commands::edge_commands(&state.scene, selected, hover);
            self.key = Some(key);
            self.command_builds += 1;
        }
        let (tier, step) = lod;
        self.polylines = self
            .edges
            .iter()
            .map(|e| match e {
                EdgeCommand::Stroke { points, .. } if points.len() >= 2 => {
                    let points = decimated(points, tier, step);
                    let bounds = bounds_of(&points);
                    Some(Polyline { points, bounds })
                }
                _ => None,
            })
            .collect();
        self.lod = Some(lod);
        self.builds += 1;
    }

    /// How many times the class/edge commands were rebuilt (scene,
    /// selection or hover change); LOD changes only redecimate edges.
    pub fn command_builds(&self) -> u64 {
        self.command_builds
    }

    /// How many times any cached geometry was rebuilt (invalidation evidence).
    pub fn builds(&self) -> u64 {
        self.builds
    }

    /// Class/package commands with their world-space culling bounds.
    pub fn commands(&self) -> &[(Option<Rect>, Command)] {
        &self.commands
    }

    /// Edge commands paired with the cached stroke polyline (strokes only).
    pub fn edges(&self) -> impl Iterator<Item = (&EdgeCommand, Option<&Polyline>)> {
        self.edges
            .iter()
            .zip(self.polylines.iter().map(Option::as_ref))
    }
}
