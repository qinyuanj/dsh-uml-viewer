use crate::geom::Rect;
use edn::Value;
use edn::record::numval;

fn length(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

fn pt(p: [f64; 2]) -> Value {
    Value::Vector(vec![numval(p[0]), numval(p[1])])
}

fn v2(v: &Value) -> [f64; 2] {
    crate::geom::vec2(v).unwrap()
}

pub fn round_corners(ps: &[[f64; 2]], radius: f64) -> Vec<[f64; 2]> {
    if ps.len() < 3 {
        return ps.to_vec();
    }
    let mut out = vec![ps[0]];
    for i in 1..ps.len() - 1 {
        let (a, b, c) = (ps[i - 1], ps[i], ps[i + 1]);
        let (ax, ay) = (b[0] - a[0], b[1] - a[1]);
        let (bx, by) = (c[0] - b[0], c[1] - b[1]);
        let (la, lb) = (length(ax, ay), length(bx, by));
        let r = radius.min(0.45 * la).min(0.45 * lb);
        let mut turn = 0.0;
        if la >= 1e-9 && lb >= 1e-9 {
            turn = (ax * bx + ay * by) / (la * lb);
        }
        if r < 4.0 || turn > 0.92 {
            out.push(b);
        } else {
            out.push([b[0] - ax / la * r, b[1] - ay / la * r]);
            out.push([b[0] + bx / lb * r, b[1] + by / lb * r]);
        }
    }
    out.push(*ps.last().unwrap());
    out
}

fn cone(dx: f64, dy: f64, r: Rect, p: [f64; 2], arriving: bool) -> (f64, f64) {
    let face = crate::geom::face(r, p);
    let (mut nx, mut ny) = match face {
        crate::geom::Face::Left => (-1.0, 0.0),
        crate::geom::Face::Right => (1.0, 0.0),
        crate::geom::Face::Top => (0.0, -1.0),
        crate::geom::Face::Bottom => (0.0, 1.0),
    };
    if arriving {
        nx = -nx;
        ny = -ny;
    }
    let l = length(dx, dy);
    if l < 1e-6 {
        return (nx, ny);
    }
    let (ux, uy) = (dx / l, dy / l);
    let along = ux * nx + uy * ny;
    let (px, py) = (ux - along * nx, uy - along * ny);
    let pl = length(px, py);
    if along > 0.0 && pl <= along {
        return (dx, dy);
    }
    let (sx, sy) = if pl < 1e-9 {
        (-ny, nx)
    } else {
        (px / pl, py / pl)
    };
    let s = 1.0 / 2.0_f64.sqrt();
    (l * (s * nx + s * sx), l * (s * ny + s * sy))
}

fn line_op(p: [f64; 2]) -> Value {
    Value::Map(vec![
        (Value::keyword("op"), Value::keyword("line")),
        (Value::keyword("p"), pt(p)),
    ])
}

fn cubic_op(c1: [f64; 2], c2: [f64; 2], p: [f64; 2]) -> Value {
    Value::Map(vec![
        (Value::keyword("op"), Value::keyword("cubic")),
        (Value::keyword("c1"), pt(c1)),
        (Value::keyword("c2"), pt(c2)),
        (Value::keyword("p"), pt(p)),
    ])
}

pub fn basis_path(points: &[Value]) -> Value {
    let raw: Vec<[f64; 2]> = points.iter().map(v2).collect();
    let ps = round_corners(&raw, 24.0);
    let mut ops: Vec<Value> = Vec::new();
    if ps.len() == 2 {
        ops.push(line_op(ps[1]));
    } else if ps.len() >= 3 {
        let (mut a, mut b) = (ps[0], ps[1]);
        ops.push(line_op([
            (5.0 * a[0] + b[0]) / 6.0,
            (5.0 * a[1] + b[1]) / 6.0,
        ]));
        for c in &ps[2..] {
            ops.push(cubic_op(
                [(2.0 * a[0] + b[0]) / 3.0, (2.0 * a[1] + b[1]) / 3.0],
                [(a[0] + 2.0 * b[0]) / 3.0, (a[1] + 2.0 * b[1]) / 3.0],
                [
                    (a[0] + 4.0 * b[0] + c[0]) / 6.0,
                    (a[1] + 4.0 * b[1] + c[1]) / 6.0,
                ],
            ));
            a = b;
            b = *c;
        }
        if let Some(Value::Map(entries)) = ops.last_mut() {
            for (k, v) in entries.iter_mut() {
                if *k == Value::keyword("p") {
                    *v = pt(*ps.last().unwrap());
                }
            }
        }
    }
    Value::Map(vec![
        (
            Value::keyword("start"),
            ps.first().map(|p| pt(*p)).unwrap_or_else(|| pt([0.0, 0.0])),
        ),
        (Value::keyword("ops"), Value::Vector(ops)),
    ])
}

fn op_get<'a>(op: &'a Value, key: &str) -> Option<&'a Value> {
    edn::record::get(op, key)
}

fn op_put(op: &mut Value, key: &str, v: [f64; 2]) {
    edn::record::put(op, key, pt(v));
}

pub fn constrain_ends(path: &Value, from: Option<Rect>, to: Option<Rect>) -> Value {
    let mut out = path.clone();
    let s = v2(edn::record::get(path, "start").unwrap());
    if let Value::Map(entries) = &mut out {
        for (k, v) in entries.iter_mut() {
            if *k == Value::keyword("ops")
                && let Value::Vector(ops) = v
            {
                if ops.len() > 1
                    && let Some(from) = from
                {
                    let op = &mut ops[0];
                    let key = if op_get(op, "c1").is_some() {
                        "c1"
                    } else {
                        "p"
                    };
                    let p = v2(op_get(op, key).unwrap());
                    let (dx, dy) = cone(p[0] - s[0], p[1] - s[1], from, s, false);
                    op_put(op, key, [s[0] + dx, s[1] + dy]);
                }
                if !ops.is_empty()
                    && let Some(to) = to
                {
                    let op = ops.last_mut().unwrap();
                    if op_get(op, "c2").is_some() {
                        let p = v2(op_get(op, "p").unwrap());
                        let c = v2(op_get(op, "c2").unwrap());
                        let (dx, dy) = cone(p[0] - c[0], p[1] - c[1], to, p, true);
                        op_put(op, "c2", [p[0] - dx, p[1] - dy]);
                    }
                }
            }
        }
    }
    out
}

pub fn path_bounds(path: &Value) -> Option<Rect> {
    let mut rects = Vec::new();
    let s = v2(edn::record::get(path, "start")?);
    rects.push(Some(Rect::new(s[0], s[1], 0.0, 0.0)));
    if let Value::Vector(ops) = edn::record::get(path, "ops")? {
        for op in ops {
            for k in ["p", "c1", "c2"] {
                if let Some(q) = op_get(op, k) {
                    let q = v2(q);
                    rects.push(Some(Rect::new(q[0], q[1], 0.0, 0.0)));
                }
            }
        }
    }
    crate::geom::union(&rects)
}

pub fn end_tangent(path: &Value) -> [Value; 2] {
    let mut cur = v2(edn::record::get(path, "start").unwrap());
    let mut behind = cur;
    if let Value::Vector(ops) = edn::record::get(path, "ops").unwrap() {
        for op in ops {
            behind = op_get(op, "c2").map(v2).unwrap_or(cur);
            cur = v2(op_get(op, "p").unwrap());
        }
    }
    [pt(behind), pt(cur)]
}

const MAX_DEPTH: u32 = 16;

fn flatten_cubic(
    [a, c1, c2, d]: [[f64; 2]; 4],
    tolerance: f64,
    depth: u32,
    out: &mut Vec<[f64; 2]>,
) {
    let flat = crate::geom::point_seg_dist(c1, a, d) <= tolerance
        && crate::geom::point_seg_dist(c2, a, d) <= tolerance;
    if flat || depth >= MAX_DEPTH {
        out.push(d);
        return;
    }
    let mid = |p: [f64; 2], q: [f64; 2]| [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
    let (ab, bc, cd) = (mid(a, c1), mid(c1, c2), mid(c2, d));
    let (abc, bcd) = (mid(ab, bc), mid(bc, cd));
    let m = mid(abc, bcd);
    flatten_cubic([a, ab, abc, m], tolerance, depth + 1, out);
    flatten_cubic([m, bcd, cd, d], tolerance, depth + 1, out);
}

pub fn flatten_path(path: &Value, tolerance: f64) -> Vec<[f64; 2]> {
    assert!(tolerance > 0.0, "positive flattening tolerance required");
    let mut cur = v2(edn::record::get(path, "start").unwrap());
    let mut out = vec![cur];
    if let Value::Vector(ops) = edn::record::get(path, "ops").unwrap() {
        for op in ops {
            let p = v2(op_get(op, "p").unwrap());
            match (op_get(op, "c1"), op_get(op, "c2")) {
                (Some(c1), Some(c2)) => {
                    flatten_cubic([cur, v2(c1), v2(c2), p], tolerance, 0, &mut out)
                }
                _ => out.push(p),
            }
            cur = p;
        }
    }
    out
}

#[cfg(test)]
mod tests;
