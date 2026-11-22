use crate::geom::{self, Face, Rect, face};
use edn::Value;
use edn::record::name;
use std::collections::HashMap;

fn horiz(f: Face) -> bool {
    f == Face::Left || f == Face::Right
}

fn port(r: Rect, f: Face, t: f64) -> [f64; 2] {
    let t = t.clamp(0.12, 0.88);
    match f {
        Face::Top => [r.x + t * r.w, r.y],
        Face::Bottom => [r.x + t * r.w, r.bottom()],
        Face::Left => [r.x, r.y + t * r.h],
        Face::Right => [r.right(), r.y + t * r.h],
    }
}

type Path = Vec<[f64; 2]>;

fn collapse(ps: &[[f64; 2]]) -> Path {
    let mut out: Path = Vec::new();
    for &p in ps {
        let n = out.len();
        let b = n.checked_sub(1).map(|i| out[i]);
        let a = n.checked_sub(2).map(|i| out[i]);
        match (a, b) {
            (_, Some(b)) if p[0] == b[0] && p[1] == b[1] => {}
            (Some(a), Some(b))
                if ((a[0] - b[0]).abs() < 0.51 && (b[0] - p[0]).abs() < 0.51
                    || (a[1] - b[1]).abs() < 0.51 && (b[1] - p[1]).abs() < 0.51)
                    && b[0] >= a[0].min(p[0])
                    && b[0] <= a[0].max(p[0])
                    && b[1] >= a[1].min(p[1])
                    && b[1] <= a[1].max(p[1]) =>
            {
                out.pop();
                if a[0] != p[0] || a[1] != p[1] {
                    out.push(p);
                }
            }
            _ => out.push(p),
        }
    }
    out
}

fn flow(a: Rect, b: Rect, lr: bool) -> (Face, Face) {
    if lr {
        if a.cx() <= b.cx() {
            return (Face::Right, Face::Left);
        }
        (Face::Left, Face::Right)
    } else {
        if a.cy() <= b.cy() {
            return (Face::Bottom, Face::Top);
        }
        (Face::Top, Face::Bottom)
    }
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

fn via_track(s: [f64; 2], e: [f64; 2], track: f64, lr: bool) -> Path {
    if lr {
        collapse(&[s, [track, s[1]], [track, e[1]], e])
    } else {
        collapse(&[s, [s[0], track], [e[0], track], e])
    }
}

#[derive(Clone)]
struct Node {
    r: Rect,
    id: String,
    rank: f64,
    dummy: bool,
}

struct RouteEdge {
    edge: Value,
    a: usize,
    b: usize,
    ft: f64,
    tt: f64,
    i: usize,
    n: usize,
    ps: Path,
    lane: Option<Path>,
    fallback: bool,
}

fn stub_point(p: [f64; 2], f: Face, row: Rect) -> [f64; 2] {
    match f {
        Face::Left => [(p[0] - 20.0).min(row.x - 20.0), p[1]],
        Face::Right => [(p[0] + 20.0).max(row.right() + 20.0), p[1]],
        Face::Top => [p[0], (p[1] - 20.0).min(row.y - 20.0)],
        Face::Bottom => [p[0], (p[1] + 20.0).max(row.bottom() + 20.0)],
    }
}

fn settle(mut ps: Path, a: Rect, b: Rect) -> Path {
    if ps.len() >= 2 {
        let s = slide(a, ps[0], ps[1]);
        let e = slide(b, *ps.last().unwrap(), ps[ps.len() - 2]);
        sync(&mut ps, true, s, face(a, s));
        sync(&mut ps, false, e, face(b, e));
    }
    ps
}

fn through_lanes(
    a: &Node,
    b: &Node,
    (ft, tt): (f64, f64),
    lane: &[[f64; 2]],
    rows: (Rect, Rect),
    lr: bool,
) -> Path {
    let (ff, tf) = flow(a.r, b.r, lr);
    let (s, e) = (port(a.r, ff, ft), port(b.r, tf, tt));
    let mut ps = vec![s, stub_point(s, ff, rows.0)];
    ps.extend_from_slice(lane);
    ps.push(stub_point(e, tf, rows.1));
    ps.push(e);
    collapse(&ps)
}

#[allow(clippy::too_many_arguments)]
fn candidates(
    a: &Node,
    b: &Node,
    ft: f64,
    tt: f64,
    i: usize,
    n: usize,
    rank_boxes: &HashMap<String, Rect>,
    lr: bool,
) -> Vec<Path> {
    let (ar, br) = (a.r, b.r);
    let stacked = if lr {
        br.x - ar.right() > 8.0 || ar.x - br.right() > 8.0
    } else {
        br.y - ar.bottom() > 8.0 || ar.y - br.bottom() > 8.0
    };
    let mut result: Vec<Path> = Vec::new();
    if !stacked {
        let (ff, tf) = if lr {
            if ar.cy() < br.cy() {
                (Face::Bottom, Face::Top)
            } else {
                (Face::Top, Face::Bottom)
            }
        } else if ar.cx() < br.cx() {
            (Face::Right, Face::Left)
        } else {
            (Face::Left, Face::Right)
        };
        let (s, e) = (port(ar, ff, ft), port(br, tf, tt));
        let (lo, hi) = if lr {
            (ar.x.max(br.x), ar.right().min(br.right()))
        } else {
            (ar.y.max(br.y), ar.bottom().min(br.bottom()))
        };
        if lo + 1.0 < hi {
            if lr {
                result.push(collapse(&[
                    [lo + ft * (hi - lo), s[1]],
                    [lo + tt * (hi - lo), e[1]],
                ]));
            } else {
                result.push(collapse(&[
                    [s[0], lo + ft * (hi - lo)],
                    [e[0], lo + tt * (hi - lo)],
                ]));
            }
        } else if lr {
            result.push(collapse(&[
                s,
                [s[0], (s[1] + e[1]) / 2.0],
                [e[0], (s[1] + e[1]) / 2.0],
                e,
            ]));
        } else {
            result.push(via_track(s, e, (s[0] + e[0]) / 2.0, true));
        }
        let bb = union_rect(ar, br);
        let (outer, inner) = if lr {
            (Face::Right, Face::Left)
        } else {
            (Face::Bottom, Face::Top)
        };
        for f in [outer, inner] {
            let (ps, pe) = (port(ar, f, ft), port(br, f, tt));
            let pad = 10.0 + i as f64 * 10.0;
            let track = match f {
                Face::Right => bb.right() + pad,
                Face::Left => bb.x - pad,
                Face::Bottom => bb.bottom() + pad,
                Face::Top => bb.y - pad,
            };
            result.push(via_track(ps, pe, track, lr));
        }
        let key = rank_key(a.rank);
        let bb = rank_boxes.get(&key).copied().unwrap_or(ar);
        let track = (if lr { bb.right() } else { bb.bottom() }) + 20.0 + i as f64 * 10.0;
        result.push(via_track(
            port(ar, outer, ft),
            port(br, outer, tt),
            track,
            lr,
        ));
    } else {
        let (ff, tf) = flow(ar, br, lr);
        let (s, e) = (port(ar, ff, ft), port(br, tf, tt));
        let lo = if lr {
            ar.right().min(br.right())
        } else {
            ar.bottom().min(br.bottom())
        };
        let hi = if lr { ar.x.max(br.x) } else { ar.y.max(br.y) };
        let step = (10.0_f64).max(hi - lo) / (n.max(1) as f64 + 1.0);
        result.push(via_track(s, e, lo + (i as f64 + 1.0) * step, lr));
        let pad = 20.0 + i as f64 * 10.0;
        if lr {
            let (top, bot) = (ar.y.min(br.y) - pad, ar.bottom().max(br.bottom()) + pad);
            let (st, sb) = (port(ar, Face::Top, ft), port(ar, Face::Bottom, ft));
            result.push(collapse(&[st, [st[0], top], [e[0], top], e]));
            result.push(collapse(&[sb, [sb[0], bot], [e[0], bot], e]));
        } else {
            let (left, rgt) = (ar.x.min(br.x) - pad, ar.right().max(br.right()) + pad);
            let (sl, sr) = (port(ar, Face::Left, ft), port(ar, Face::Right, ft));
            result.push(collapse(&[sl, [left, sl[1]], [left, e[1]], e]));
            result.push(collapse(&[sr, [rgt, sr[1]], [rgt, e[1]], e]));
        }
        let mut hops: Path = vec![s];
        let dir: f64 = if a.rank < b.rank { 1.0 } else { -1.0 };
        let mut rank = a.rank + dir;
        while (dir == 1.0 && rank < b.rank) || (dir == -1.0 && rank > b.rank) {
            if let Some(bb) = rank_boxes.get(&rank_key(rank)).copied() {
                if lr {
                    let y = ((s[1] + e[1]) / 2.0).clamp(s[1].min(e[1]), s[1].max(e[1]));
                    hops.push([bb.x - 10.0 - i as f64 * 10.0, y]);
                    hops.push([bb.right() + 10.0 + i as f64 * 10.0, y]);
                } else {
                    let x = ((s[0] + e[0]) / 2.0).clamp(s[0].min(e[0]), s[0].max(e[0]));
                    let top = (bb.y - 10.0 - i as f64 * 10.0).clamp(s[1].min(e[1]), s[1].max(e[1]));
                    let bot = (bb.bottom() + 10.0 + i as f64 * 10.0)
                        .clamp(s[1].min(e[1]), s[1].max(e[1]));
                    hops.push([x, top]);
                    if (top - bot).abs() >= 1.0 {
                        hops.push([x, bot]);
                    }
                }
            }
            rank += dir;
        }
        if hops.len() > 1 {
            hops.push(e);
            result.push(collapse(&hops));
        }
        result.push(via_track(s, e, lo + (n as f64 - i as f64) * step, lr));
        result.push(collapse(&[s, e]));
    }
    result
}

fn rank_key(rank: f64) -> String {
    format!("{rank:?}")
}

fn reverses(ps: &[[f64; 2]]) -> bool {
    if ps.len() < 3 {
        return false;
    }
    let (s, e) = (ps[0], *ps.last().unwrap());
    let axis = if (e[0] - s[0]).abs() >= (e[1] - s[1]).abs() {
        0
    } else {
        1
    };
    let dir = e[axis] - s[axis];
    for w in ps.windows(2) {
        let (a, b) = (w[0][axis], w[1][axis]);
        if (dir > 0.0 && a > e[axis] + 12.0 && b < a - 1.0)
            || (dir < 0.0 && a < e[axis] - 12.0 && b > a + 1.0)
        {
            return true;
        }
    }
    false
}

fn hits(ps: &[[f64; 2]], a: usize, b: usize, nodes: &[Node]) -> bool {
    for w in ps.windows(2) {
        let (p, q) = (w[0], w[1]);
        for (ci, c) in nodes.iter().enumerate() {
            if ci != a && ci != b && !c.dummy {
                let r = c.r;
                if p[0].min(q[0]) < r.right()
                    && p[0].max(q[0]) > r.x
                    && p[1].min(q[1]) < r.bottom()
                    && p[1].max(q[1]) > r.y
                {
                    return true;
                }
            }
        }
    }
    false
}

fn stub(r: Rect, p: [f64; 2], q: [f64; 2]) -> bool {
    let h = horiz(face(r, p));
    let (dx, dy) = ((q[0] - p[0]).abs(), (q[1] - p[1]).abs());
    if h { dx + 1.0 >= dy } else { dy + 1.0 >= dx }
}

fn score(ps: &[[f64; 2]]) -> f64 {
    let mut best: f64 = 1.0;
    for i in 1..ps.len().saturating_sub(1) {
        let (a, b, c) = (ps[i - 1], ps[i], ps[i + 1]);
        let (x, y) = (b[0] - a[0], b[1] - a[1]);
        let (u, v) = (c[0] - b[0], c[1] - b[1]);
        let (la, lb) = ((x * x + y * y).sqrt(), (u * u + v * v).sqrt());
        if la >= 1e-9 && lb >= 1e-9 {
            best = best.min((x * u + y * v) / (la * lb));
        }
    }
    best
}

fn choose(paths: Vec<Path>, a: usize, b: usize, nodes: &[Node]) -> Path {
    let first_any = paths.first().cloned().unwrap_or_default();
    let mut clear: Vec<Path> = Vec::new();
    let mut first_ok: Option<Path> = None;
    let mut best: Option<Path> = None;
    let mut bestscore = f64::NEG_INFINITY;
    for p in paths {
        if p.is_empty() || hits(&p, a, b, nodes) {
            continue;
        }
        if !reverses(&p) {
            if first_ok.is_none() {
                first_ok = Some(p.clone());
            }
            if p.len() > 1
                && stub(nodes[a].r, p[0], p[1])
                && stub(nodes[b].r, *p.last().unwrap(), p[p.len() - 2])
            {
                let sc = score(&p);
                if sc >= bestscore {
                    best = Some(p.clone());
                    bestscore = sc;
                }
            }
        }
        clear.push(p);
    }
    if let Some(best) = best {
        return best;
    }
    if let Some(first) = first_ok {
        return first;
    }
    clear.into_iter().next().unwrap_or(first_any)
}

fn sync(ps: &mut Path, start: bool, p: [f64; 2], f: Face) {
    let n = ps.len();
    let at = if start { 0 } else { n - 1 };
    ps[at] = p;
    if n >= 3 {
        let i = if start { 1 } else { n - 2 };
        let mut inner = ps[i];
        inner = if horiz(f) {
            [inner[0], p[1]]
        } else {
            [p[0], inner[1]]
        };
        let (dx, dy) = (inner[0] - p[0], inner[1] - p[1]);
        let l = (dx * dx + dy * dy).sqrt();
        if l >= 1e-9 {
            let scale = (20.0 / l).max(1.0);
            inner = [p[0] + dx * scale, p[1] + dy * scale];
        }
        ps[i] = inner;
    }
}

fn slide(r: Rect, p: [f64; 2], q: [f64; 2]) -> [f64; 2] {
    let f = face(r, p);
    let (mut x, mut y) = (p[0], p[1]);
    if horiz(f) {
        let lim = (x - q[0]).abs().max(1.0);
        y = (q[1] - lim).max((q[1] + lim).min(y));
        y = y.clamp(r.y + 0.12 * r.h, r.bottom() - 0.12 * r.h);
    } else {
        let lim = (y - q[1]).abs().max(1.0);
        x = (q[0] - lim).max((q[0] + lim).min(x));
        x = x.clamp(r.x + 0.12 * r.w, r.right() - 0.12 * r.w);
    }
    [x, y]
}

fn stable_sort_by<T: Clone, F: Fn(&T) -> f64>(xs: &mut [T], scorefn: F) {
    let mut decorated: Vec<(usize, f64, T)> = xs
        .iter()
        .enumerate()
        .map(|(i, x)| (i, scorefn(x), x.clone()))
        .collect();
    decorated.sort_by(|a, b| {
        if a.1 == b.1 {
            a.0.cmp(&b.0)
        } else {
            a.1.partial_cmp(&b.1).unwrap()
        }
    });
    for (i, (_, _, x)) in decorated.into_iter().enumerate() {
        xs[i] = x;
    }
}

pub fn route(scene: &mut Value) {
    let diagram = edn::record::get(scene, "diagram")
        .cloned()
        .unwrap_or(Value::Nil);
    let direction = edn::record::get(&diagram, "direction").map(name);
    let lr = direction.as_deref() == Some("lr") || direction.as_deref() == Some("rl");
    let mut lanes: HashMap<usize, Path> = HashMap::new();
    for l in edn::record::seq(edn::record::get(scene, "lanes")) {
        let ei = edn::record::get(l, "edge")
            .and_then(edn::record::num)
            .unwrap() as usize;
        let ps = edn::record::seq(edn::record::get(l, "points"))
            .iter()
            .filter_map(geom::vec2)
            .collect();
        lanes.insert(ei, ps);
    }
    edn::record::remove(scene, "lanes");
    let mut nodes: Vec<Node> = Vec::new();
    let mut idx: HashMap<String, usize> = HashMap::new();
    let mut rows: HashMap<String, Rect> = HashMap::new();
    if let Some(Value::Vector(classes)) = edn::record::get(scene, "classes") {
        for c in classes {
            let Some(r) = edn::record::get(c, "rect").and_then(Rect::from_value) else {
                continue;
            };
            let id = edn::encode(edn::record::get(c, "id").unwrap()).unwrap();
            if let Some(rank) = edn::record::get(c, "rank").and_then(edn::record::num) {
                rows.entry(rank_key(rank))
                    .and_modify(|bb| *bb = union_rect(*bb, r))
                    .or_insert(r);
            }
            idx.insert(id.clone(), nodes.len());
            nodes.push(Node {
                r,
                id,
                rank: edn::record::get(c, "rank")
                    .and_then(edn::record::num)
                    .unwrap_or(0.0),
                dummy: edn::record::flag(c, "dummy?"),
            });
        }
    }
    let mut edges: Vec<RouteEdge> = Vec::new();
    let mut from: HashMap<String, Vec<usize>> = HashMap::new();
    let mut to: HashMap<String, Vec<usize>> = HashMap::new();
    let mut hops: HashMap<String, Vec<usize>> = HashMap::new();
    if let Some(Value::Vector(diagram_edges)) = edn::record::get(
        edn::record::get(scene, "diagram").unwrap_or(&Value::Nil),
        "edges",
    ) {
        for (dei, e) in diagram_edges.iter().enumerate() {
            let f = edn::record::get(e, "from").and_then(|v| idx.get(&edn::encode(v).unwrap()));
            let t = edn::record::get(e, "to").and_then(|v| idx.get(&edn::encode(v).unwrap()));
            if let (Some(&f), Some(&t)) = (f, t) {
                let ei = edges.len();
                edges.push(RouteEdge {
                    edge: e.clone(),
                    a: f,
                    b: t,
                    ft: 0.0,
                    tt: 0.0,
                    i: 0,
                    n: 0,
                    ps: Vec::new(),
                    lane: lanes.remove(&dei),
                    fallback: false,
                });
                from.entry(nodes[f].id.clone()).or_default().push(ei);
                to.entry(nodes[t].id.clone()).or_default().push(ei);
                let key = format!(
                    "{}:{}",
                    rank_key(nodes[f].rank.min(nodes[t].rank)),
                    rank_key(nodes[f].rank.max(nodes[t].rank))
                );
                hops.entry(key).or_default().push(ei);
            }
        }
    }
    for group in from.values_mut() {
        stable_sort_by(group, |&ei| {
            if lr {
                nodes[edges[ei].b].r.cy()
            } else {
                nodes[edges[ei].b].r.cx()
            }
        });
        let n = group.len();
        for (i, &ei) in group.iter().enumerate() {
            edges[ei].ft = (i + 1) as f64 / (n + 1) as f64;
        }
    }
    for group in to.values_mut() {
        stable_sort_by(group, |&ei| {
            if lr {
                nodes[edges[ei].a].r.cy()
            } else {
                nodes[edges[ei].a].r.cx()
            }
        });
        let n = group.len();
        for (i, &ei) in group.iter().enumerate() {
            edges[ei].tt = (i + 1) as f64 / (n + 1) as f64;
        }
    }
    for group in hops.values_mut() {
        group.sort_by(|&a, &b| {
            let (ea, eb) = (&edges[a], &edges[b]);
            let (fa, fb) = (&nodes[ea.a].id, &nodes[eb.a].id);
            if fa == fb {
                nodes[ea.b].id.cmp(&nodes[eb.b].id)
            } else {
                fa.cmp(fb)
            }
        });
        let n = group.len();
        for (i, &ei) in group.iter().enumerate() {
            edges[ei].i = i;
            edges[ei].n = n;
        }
    }
    let mut rank_boxes: HashMap<String, Rect> = HashMap::new();
    for c in &nodes {
        let key = rank_key(c.rank);
        rank_boxes
            .entry(key)
            .and_modify(|bb| *bb = union_rect(*bb, c.r))
            .or_insert(c.r);
    }
    let mut atts: HashMap<String, Vec<(usize, bool, [f64; 2])>> = HashMap::new();
    #[allow(clippy::needless_range_loop)]
    for ei in 0..edges.len() {
        let (a, b, ft, tt, i, n) = {
            let e = &edges[ei];
            (e.a, e.b, e.ft, e.tt, e.i, e.n)
        };
        let row = |c: &Node| rows.get(&rank_key(c.rank)).copied().unwrap_or(c.r);
        let ends = (row(&nodes[a]), row(&nodes[b]));
        let through = edges[ei]
            .lane
            .as_ref()
            .map(|lane| through_lanes(&nodes[a], &nodes[b], (ft, tt), lane, ends, lr))
            .filter(|ps| !hits(ps, a, b, &nodes));
        edges[ei].fallback = edges[ei].lane.is_some() && through.is_none();
        let settle = |ps: Path| settle(ps, nodes[a].r, nodes[b].r);
        let mut ps = settle(through.unwrap_or_else(|| {
            choose(
                candidates(&nodes[a], &nodes[b], ft, tt, i, n, &rank_boxes, lr),
                a,
                b,
                &nodes,
            )
        }));
        if ps.is_empty() {
            continue;
        }
        if nodes[a].rank != nodes[b].rank && hits(&ps, a, b, &nodes) {
            let rows_apart = settle(through_lanes(&nodes[a], &nodes[b], (ft, tt), &[], ends, lr));
            if !hits(&rows_apart, a, b, &nodes) {
                ps = rows_apart;
            }
        }
        edges[ei].ps = ps;
        for start in [true, false] {
            let c = if start { a } else { b };
            let p = if start {
                edges[ei].ps[0]
            } else {
                *edges[ei].ps.last().unwrap()
            };
            let f = face(nodes[c].r, p);
            atts.entry(format!("{}{:?}", nodes[c].id, f))
                .or_default()
                .push((ei, start, p));
        }
    }
    let mut spread: Vec<bool> = vec![false; edges.len()];
    for group in atts.values() {
        if group.len() > 1 {
            let ei0 = group[0].0;
            let c = if group[0].1 {
                edges[ei0].a
            } else {
                edges[ei0].b
            };
            let r = nodes[c].r;
            let f = face(r, group[0].2);
            let mut sorted: Vec<(usize, bool, [f64; 2])> = group.clone();
            stable_sort_by(&mut sorted, |a| if horiz(f) { a.2[1] } else { a.2[0] });
            let (lo, hi) = if horiz(f) {
                (r.y + 0.12 * r.h, r.bottom() - 0.12 * r.h)
            } else {
                (r.x + 0.12 * r.w, r.right() - 0.12 * r.w)
            };
            for (i, a) in sorted.iter().enumerate() {
                let v = lo + (i + 1) as f64 * (hi - lo) / (sorted.len() + 1) as f64;
                let p = if horiz(f) { [a.2[0], v] } else { [v, a.2[1]] };
                if p != a.2 {
                    spread[a.0] = true;
                    sync(&mut edges[a.0].ps, a.1, p, f);
                }
            }
        }
    }
    for (e, _) in edges.iter_mut().zip(&spread).filter(|(_, s)| **s) {
        if e.ps.len() < 2 || !hits(&e.ps, e.a, e.b, &nodes) {
            continue;
        }
        let (s, t) = (e.ps[0], *e.ps.last().unwrap());
        let (ra, rb) = (nodes[e.a].r, nodes[e.b].r);
        let (ft, tt, i, n) = (e.ft, e.tt, e.i, e.n);
        let clear = candidates(&nodes[e.a], &nodes[e.b], ft, tt, i, n, &rank_boxes, lr)
            .into_iter()
            .filter(|c| c.len() >= 2)
            .map(|c| {
                let mut c = settle(c, ra, rb);
                if face(ra, c[0]) == face(ra, s) {
                    sync(&mut c, true, s, face(ra, s));
                }
                if face(rb, *c.last().unwrap()) == face(rb, t) {
                    sync(&mut c, false, t, face(rb, t));
                }
                c
            })
            .find(|c| !hits(c, e.a, e.b, &nodes));
        if let Some(clear) = clear {
            e.ps = clear;
        }
    }
    let mut out: Vec<Value> = Vec::new();
    for e in &edges {
        if e.ps.is_empty() {
            continue;
        }
        let mut edge = e.edge.clone();
        let points: Vec<Value> = e.ps.iter().map(|p| geom::point(*p)).collect();
        edn::record::put(&mut edge, "points", Value::Vector(points));
        let head = match edn::record::get(&edge, "kind").map(name) {
            Some(k) if k == "inheritance" || k == "implements" => "triangle",
            Some(k) if k == "composition" => "diamond-fill",
            Some(k) if k == "aggregation" => "diamond",
            _ => "open",
        };
        edn::record::put(&mut edge, "head", Value::keyword(head));
        if e.fallback {
            edn::record::put(&mut edge, "lane-fallback", Value::Bool(true));
        }
        out.push(edge);
    }
    edn::record::put(scene, "edges", Value::Vector(out));
}

#[cfg(test)]
mod tests;
