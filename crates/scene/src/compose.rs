//! Scene compilation (port of `love/uml/engine/compose.lua`): layout, route,
//! fit, then `prepare_scene`. The Lua original rebuilt the class index per
//! edge (O(E·N)); here the index and a spatial grid over class rects are
//! built once per scene, so preparation is near-linear. Geometry is
//! unchanged: the grid yields the same obstacle set and `gap_segment`
//! merges intervals independent of rect order. Strokes are bit-for-bit
//! those of clipping every segment against every overlapping rect (#186).

use crate::curve;
use crate::geom::{self, Rect};
use edn::Value;
use edn::record::{flag, get, name, numval, put, seq};
use std::collections::HashMap;

/// Class rectangles keyed by encoded id, plus the obstacle grid. Rects of
/// dummy boxes are tracked but never clip strokes.
struct ClassIndex {
    rect_by_id: HashMap<String, Rect>,
    grid: Grid,
    obstacles: Vec<(Value, Rect, bool)>, // (id, rect, dummy?)
    seen: Vec<u32>,                      // per obstacle: the last query that saw it
    query: u32,
}

const CELL: f64 = 256.0;

/// Farthest a stroke strays from its curve, in world px (kurbo's and
/// Skia's 0.25; #187).
const FLATTEN_TOLERANCE: f64 = 0.25;

/// Inclusive cell span `(x0, y0, x1, y1)` of a rect.
fn span(r: Rect) -> (i64, i64, i64, i64) {
    let cell = |v: f64| (v / CELL).floor() as i64;
    (cell(r.x), cell(r.y), cell(r.right()), cell(r.bottom()))
}

/// Most cells a dense grid may hold; a wider spread scans the obstacles
/// instead (16 MiB of empty cells, 64 times a 2000-class scene).
const MAX_CELLS: i64 = 1 << 20;

/// Obstacle slots per cell, dense over the cells the obstacles cover. An
/// empty grid (no obstacles, or more than [`MAX_CELLS`]) covers no cell.
#[derive(Default)]
struct Grid {
    x0: i64,
    y0: i64,
    cols: i64,
    rows: i64,
    cells: Vec<Vec<usize>>,
}

impl Grid {
    fn new(rects: &[Rect]) -> Grid {
        let spans: Vec<_> = rects.iter().map(|r| span(*r)).collect();
        let Some(&(x0, y0, _, _)) = spans.first() else {
            return Grid::default();
        };
        let (mut lo, mut hi) = ((x0, y0), (x0, y0));
        for &(a, b, c, d) in &spans {
            lo = (lo.0.min(a), lo.1.min(b));
            hi = (hi.0.max(c), hi.1.max(d));
        }
        let (cols, rows) = (hi.0 - lo.0 + 1, hi.1 - lo.1 + 1);
        if cols.saturating_mul(rows) > MAX_CELLS {
            return Grid::default();
        }
        let mut grid = Grid {
            x0: lo.0,
            y0: lo.1,
            cols,
            rows,
            cells: vec![Vec::new(); (cols * rows) as usize],
        };
        for (slot, &(x0, y0, x1, y1)) in spans.iter().enumerate() {
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    let at = grid.at(cx, cy).unwrap();
                    grid.cells[at].push(slot);
                }
            }
        }
        grid
    }

    fn at(&self, cx: i64, cy: i64) -> Option<usize> {
        let (i, j) = (cx - self.x0, cy - self.y0);
        ((0..self.cols).contains(&i) && (0..self.rows).contains(&j))
            .then(|| (i * self.rows + j) as usize)
    }
}

impl ClassIndex {
    fn new(scene: &Value) -> ClassIndex {
        let mut rect_by_id = HashMap::new();
        let mut obstacles = Vec::new();
        for c in seq(get(scene, "classes")) {
            let (Some(id), Some(rect)) = (get(c, "id"), get(c, "rect").and_then(Rect::from_value))
            else {
                continue;
            };
            rect_by_id.insert(edn::encode(id).unwrap(), rect);
            obstacles.push((id.clone(), rect, flag(c, "dummy?")));
        }
        let rects: Vec<Rect> = obstacles.iter().map(|o| o.1).collect();
        ClassIndex {
            rect_by_id,
            grid: Grid::new(&rects),
            seen: vec![0; obstacles.len()],
            obstacles,
            query: 0,
        }
    }

    fn rect_of(&self, id: &Value) -> Option<Rect> {
        self.rect_by_id.get(&edn::encode(id).unwrap()).copied()
    }

    /// Obstacle rects overlapping the probe, excluding dummy boxes and the
    /// edge's own endpoints. A probe spanning more cells than there are
    /// obstacles (a long edge) scans the obstacles directly; the order of
    /// the rects differs, which `gap_segment` does not see.
    fn obstacles_in(&mut self, probe: Rect, from: Option<&Value>, to: Option<&Value>) -> Vec<Rect> {
        let (x0, y0, x1, y1) = span(probe);
        let admits = |(id, rect, dummy): &(Value, Rect, bool)| {
            probe.overlaps(*rect) && !*dummy && Some(id) != from && Some(id) != to
        };
        let cells = (x1 - x0 + 1).saturating_mul(y1 - y0 + 1);
        if self.grid.cells.is_empty() || cells > self.obstacles.len() as i64 {
            return self
                .obstacles
                .iter()
                .filter(|o| admits(o))
                .map(|o| o.1)
                .collect();
        }
        self.query += 1;
        let mut out = Vec::new();
        for cx in x0..=x1 {
            for cy in y0..=y1 {
                let Some(at) = self.grid.at(cx, cy) else {
                    continue;
                };
                for &slot in &self.grid.cells[at] {
                    if self.seen[slot] == self.query {
                        continue;
                    }
                    self.seen[slot] = self.query;
                    if admits(&self.obstacles[slot]) {
                        out.push(self.obstacles[slot].1);
                    }
                }
            }
        }
        out
    }
}

/// The constrained basis path of a routed edge, when it has ≥ 2 points.
fn path(scene_edge: &Value, index: &ClassIndex) -> Option<Value> {
    let ps = match get(scene_edge, "points") {
        Some(Value::Vector(items)) if items.len() >= 2 => items,
        _ => return None,
    };
    let a = get(scene_edge, "from").and_then(|id| index.rect_of(id));
    let b = get(scene_edge, "to").and_then(|id| index.rect_of(id));
    Some(curve::constrain_ends(&curve::basis_path(ps), a, b))
}

fn shift_rect(x: &mut Value, dx: f64, dy: f64) {
    if let Some(r) = get(x, "rect").and_then(Rect::from_value) {
        put(x, "rect", r.translate(dx, dy).to_value());
    }
}

fn translate(scene: &mut Value, dx: f64, dy: f64) {
    for key in ["classes", "packages"] {
        if let Some(Value::Vector(items)) = get(scene, key).cloned() {
            let mut moved = Vec::with_capacity(items.len());
            for mut x in items.into_iter() {
                shift_rect(&mut x, dx, dy);
                // Port pills hang off their class and move with it (#189).
                for k in ["in-ports", "out-ports"] {
                    if let Some(Value::Vector(mut ports)) = get(&x, k).cloned() {
                        for p in ports.iter_mut() {
                            shift_rect(p, dx, dy);
                        }
                        put(&mut x, k, Value::Vector(ports));
                    }
                }
                moved.push(x);
            }
            put(scene, key, Value::Vector(moved));
        }
    }
    if let Some(Value::Vector(edges)) = get(scene, "edges").cloned() {
        let mut shifted = Vec::with_capacity(edges.len());
        for mut e in edges.into_iter() {
            if let Some(Value::Vector(points)) = get(&e, "points") {
                let ps: Vec<Value> = points
                    .iter()
                    .map(|p| {
                        let p = geom::vec2(p).unwrap();
                        geom::point([p[0] + dx, p[1] + dy])
                    })
                    .collect();
                put(&mut e, "points", Value::Vector(ps));
            }
            shifted.push(e);
        }
        put(scene, "edges", Value::Vector(shifted));
    }
}

fn bounds(scene: &Value, index: &ClassIndex) -> Rect {
    let mut rects: Vec<Option<Rect>> = Vec::new();
    for key in ["classes", "packages"] {
        for c in seq(get(scene, key)) {
            rects.push(get(c, "rect").and_then(Rect::from_value));
        }
    }
    for e in seq(get(scene, "edges")) {
        if let Some(p) = path(e, index) {
            rects.push(curve::path_bounds(&p));
        }
    }
    geom::union(&rects)
        .map(|b| b.inflate(16.0))
        .unwrap_or(Rect::new(0.0, 0.0, 400.0, 300.0))
}

fn size(scene: &mut Value) {
    // Rebuild the index: rects moved since the caller's index was built.
    let index = ClassIndex::new(scene);
    let b = bounds(scene, &index);
    put(
        scene,
        "size",
        Value::Map(vec![
            (Value::keyword("w"), numval(b.right() + 40.0)),
            (Value::keyword("h"), numval(b.bottom() + 40.0)),
            (Value::keyword("min-x"), numval((b.x - 40.0).min(0.0))),
            (Value::keyword("min-y"), numval((b.y - 40.0).min(0.0))),
        ]),
    );
}

/// Margin between a frame and the farthest edge it holds, as between a frame
/// and its boxes (`layout`).
const FRAME_PAD: f64 = 12.0;

/// Layout fixes a package frame before routing places the detour tracks of
/// the edges between its classes, so a frame side grows wherever such an
/// edge's points pass it. The basis curve stays in the hull of its points.
/// Roll-up boxes keep their rect: they show only when no class does.
fn hold_inner_edges(scene: &mut Value) {
    let mut owner: HashMap<String, String> = HashMap::new();
    for c in seq(get(scene, "classes")) {
        if let (Some(id), Some(p)) = (get(c, "id"), get(c, "package"))
            && !flag(c, "dummy?")
            && !flag(c, "foreign")
        {
            owner.insert(edn::encode(id).unwrap(), edn::encode(p).unwrap());
        }
    }
    let package =
        |e: &Value, k: &str| get(e, k).and_then(|id| owner.get(&edn::encode(id).unwrap()));
    let mut reach: HashMap<&String, Vec<[f64; 2]>> = HashMap::new();
    for e in seq(get(scene, "edges")) {
        match (package(e, "from"), package(e, "to")) {
            (Some(a), Some(b)) if a == b => reach
                .entry(a)
                .or_default()
                .extend(seq(get(e, "points")).iter().filter_map(geom::vec2)),
            _ => {}
        }
    }
    let Some(Value::Vector(mut packages)) = get(scene, "packages").cloned() else {
        return;
    };
    for p in packages.iter_mut() {
        let (Some(id), Some(r)) = (get(p, "id"), get(p, "rect").and_then(Rect::from_value)) else {
            continue;
        };
        let Some(points) = reach
            .get(&edn::encode(id).unwrap())
            .filter(|ps| !ps.is_empty())
        else {
            continue;
        };
        let far = |axis: usize, pick: fn(f64, f64) -> f64| {
            points.iter().map(|q| q[axis]).reduce(pick).unwrap()
        };
        // A side the points pass moves to the farthest one, plus the margin.
        let lo = |side: f64, reach: f64| {
            if reach < side {
                reach - FRAME_PAD
            } else {
                side
            }
        };
        let hi = |side: f64, reach: f64| {
            if reach > side {
                reach + FRAME_PAD
            } else {
                side
            }
        };
        let (x0, y0) = (lo(r.x, far(0, f64::min)), lo(r.y, far(1, f64::min)));
        let (x1, y1) = (
            hi(r.right(), far(0, f64::max)),
            hi(r.bottom(), far(1, f64::max)),
        );
        put(p, "rect", Rect::new(x0, y0, x1 - x0, y1 - y0).to_value());
    }
    put(scene, "packages", Value::Vector(packages));
}

fn fit(scene: &mut Value) {
    let index = ClassIndex::new(scene);
    let b = bounds(scene, &index);
    translate(scene, (40.0 - b.x).max(0.0), (40.0 - b.y).max(0.0));
    size(scene);
}

/// `prepare_scene`: per edge, clip the flattened curve against every
/// overlapping class box and record `:tip`, `:behind`, `:draw-bounds` and
/// `:strokes`; then index classes and packages by id.
pub fn prepare_scene(scene: &mut Value) {
    let mut index = ClassIndex::new(scene);
    let mut edges: Vec<Value> = Vec::new();
    for raw in seq(get(scene, "edges")) {
        let mut e = raw.clone();
        if let Some(p) = path(&e, &index) {
            let bb = curve::path_bounds(&p).unwrap();
            let probe = bb.inflate(13.0);
            let rects = index.obstacles_in(probe, get(&e, "from"), get(&e, "to"));
            let tangent = curve::end_tangent(&p);
            put(&mut e, "behind", tangent[0].clone());
            put(&mut e, "tip", tangent[1].clone());
            put(&mut e, "draw-bounds", bb.to_value());
            let flat = curve::flatten_path(&p, FLATTEN_TOLERANCE);
            let strokes = geom::gap_polyline(&flat, &rects, 5.0);
            put(
                &mut e,
                "strokes",
                Value::Vector(
                    strokes
                        .iter()
                        .map(|s| Value::Vector(s.iter().map(|p| geom::point(*p)).collect()))
                        .collect(),
                ),
            );
        }
        edges.push(e);
    }
    put(scene, "edges", Value::Vector(edges));
    for (list, key) in [("classes", "class-by-id"), ("packages", "package-by-id")] {
        let mut idx = Value::Map(vec![]);
        for c in seq(get(scene, list)) {
            if let Some(id) = get(c, "id") {
                idx.assoc(id.clone(), c.clone());
            }
        }
        put(scene, key, idx);
    }
}

/// One diagram: layout, route, fit, prepare; `:hide-edges` swaps strokes for
/// dependency indicators.
pub fn compile_diagram(diagram: &Value) -> Value {
    let mut scene = crate::layout::layout(diagram);
    crate::route::route(&mut scene);
    hold_inner_edges(&mut scene);
    fit(&mut scene);
    prepare_scene(&mut scene);
    if flag(diagram, "hide-edges") {
        let indicators = crate::hit::dep_indicators(&scene);
        put(&mut scene, "dep-indicators", indicators);
    }
    scene
}

/// Rename a scene's ids with a diagram prefix, so several diagrams can share
/// one merged scene without id collisions.
fn prefix_ids(scene: &mut Value, prefix: &str) {
    let q = |id: &Value| Value::keyword(&format!("{}{}", prefix, name(id)));
    for key in ["packages", "classes"] {
        if let Some(Value::Vector(items)) = get(scene, key).cloned() {
            let mut renamed = Vec::with_capacity(items.len());
            for mut c in items.into_iter() {
                if let Some(id) = get(&c, "id").cloned() {
                    put(&mut c, "id", q(&id));
                }
                if let Some(pkg) = get(&c, "package").cloned() {
                    put(&mut c, "package", q(&pkg));
                }
                renamed.push(c);
            }
            put(scene, key, Value::Vector(renamed));
        }
    }
    if let Some(Value::Vector(items)) = get(scene, "edges").cloned() {
        let mut renamed = Vec::with_capacity(items.len());
        for mut e in items.into_iter() {
            if let Some(from) = get(&e, "from").cloned() {
                put(&mut e, "from", q(&from));
            }
            if let Some(to) = get(&e, "to").cloned() {
                put(&mut e, "to", q(&to));
            }
            renamed.push(e);
        }
        put(scene, "edges", Value::Vector(renamed));
    }
}

/// A document compiled as one or more sections, stacked vertically with
/// `d<i>-` id prefixes so the sections cannot collide.
pub fn compile_document(doc: &Value) -> Value {
    let (mut sections, mut classes, mut packages, mut edges) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut y = 24.0;
    let diagrams = match get(doc, "diagrams") {
        Some(Value::Vector(ds)) => ds.clone(),
        _ => vec![doc.clone()],
    };
    for (i, d) in diagrams.iter().enumerate() {
        let mut scene = crate::layout::layout(d);
        crate::route::route(&mut scene);
        hold_inner_edges(&mut scene);
        fit(&mut scene);
        let h = get(&scene, "size")
            .and_then(|s| get(s, "h"))
            .and_then(edn::record::num)
            .unwrap();
        let prefix = format!("d{}-", i);
        prefix_ids(&mut scene, &prefix);
        translate(&mut scene, 40.0, y + 40.0);
        put(
            &mut scene,
            "title",
            get(d, "title").cloned().unwrap_or(Value::Nil),
        );
        put(
            &mut scene,
            "crap",
            get(d, "crap").cloned().unwrap_or(Value::Nil),
        );
        put(&mut scene, "title-y", numval(y));
        for key in ["classes", "packages", "edges"] {
            match key {
                "classes" => classes.extend(seq(get(&scene, key)).iter().cloned()),
                "packages" => packages.extend(seq(get(&scene, key)).iter().cloned()),
                _ => edges.extend(seq(get(&scene, key)).iter().cloned()),
            }
        }
        sections.push(scene);
        y += 40.0 + h + 64.0;
    }
    let title = get(doc, "title").cloned().unwrap_or(Value::Nil);
    let mut scene = Value::Map(vec![
        (Value::keyword("title"), title.clone()),
        (
            Value::keyword("diagram"),
            Value::Map(vec![(Value::keyword("title"), title)]),
        ),
        (Value::keyword("sections"), Value::Vector(sections)),
        (Value::keyword("classes"), Value::Vector(classes)),
        (Value::keyword("packages"), Value::Vector(packages)),
        (Value::keyword("edges"), Value::Vector(edges)),
    ]);
    let index = ClassIndex::new(&scene);
    let b = bounds(&scene, &index);
    let dx = (40.0 - b.x).max(0.0);
    if dx > 0.0 {
        translate(&mut scene, dx, 0.0);
    }
    size(&mut scene);
    if let Some(Value::Map(entries)) = edn::record::get_raw(&scene, "size").cloned() {
        let mut sz = Value::Map(entries);
        sz.assoc(Value::keyword("h"), numval(y));
        put(&mut scene, "size", sz);
    }
    prepare_scene(&mut scene);
    scene
}

#[cfg(test)]
mod tests;
