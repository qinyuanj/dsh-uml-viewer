//! Hit testing over a prepared scene (port of `love/uml/engine/hit.lua`).

use crate::geom::{self, Rect};
use edn::Value;
use edn::record::{record, seq};

/// `{:from :to :violating}` records of an edge: its collapsed `:deps`, else a
/// single leaf record built from the endpoints.
pub fn deps_of(e: &Value) -> Option<Value> {
    let deps = edn::record::get(e, "deps");
    if let Some(Value::Vector(items)) = deps
        && !items.is_empty()
    {
        return Some(Value::Vector(items.clone()));
    }
    let (from, to) = (edn::record::get(e, "from"), edn::record::get(e, "to"));
    if let (Some(from), Some(to)) = (from, to) {
        return Some(Value::Vector(vec![Value::Map(vec![
            (Value::keyword("from"), from.clone()),
            (Value::keyword("to"), to.clone()),
            (
                Value::keyword("violating"),
                Value::Bool(edn::record::flag(e, "violating")),
            ),
        ])]));
    }
    None
}

pub fn class_by_id<'a>(scene: &'a Value, id: &Value) -> Option<&'a Value> {
    seq(edn::record::get(scene, "classes"))
        .iter()
        .find(|&c| edn::record::get(c, "id") == Some(id))
        .map(|v| v as _)
}

pub fn package_by_id<'a>(scene: &'a Value, id: &Value) -> Option<&'a Value> {
    seq(edn::record::get(scene, "packages"))
        .iter()
        .find(|&c| edn::record::get(c, "id") == Some(id))
        .map(|v| v as _)
}

#[cfg(test)]
pub(crate) fn connected_edges(scene: &Value, id: &Value) -> Vec<Value> {
    seq(edn::record::get(scene, "edges"))
        .iter()
        .filter(|e| {
            edn::record::get(e, "from") == Some(id) || edn::record::get(e, "to") == Some(id)
        })
        .cloned()
        .collect()
}

/// Collapsed dependency triangles above/below each class box.
pub fn dep_indicators(scene: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let es = seq(edn::record::get(scene, "edges"));
    for c in seq(edn::record::get(scene, "classes")) {
        let Some(r) = edn::record::get(c, "rect").and_then(Rect::from_value) else {
            continue;
        };
        for dir in ["in", "out"] {
            let endpoint = if dir == "in" { "to" } else { "from" };
            let (mut bx, mut leaf): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
            let id = edn::record::get(c, "id").unwrap();
            for e in es {
                if let Some(Value::Vector(deps)) = deps_of(e) {
                    for dep in &deps {
                        if edn::record::get(e, endpoint) == Some(id) {
                            bx.push(dep.clone());
                        }
                        if edn::record::get(dep, endpoint) == Some(id) {
                            leaf.push(dep.clone());
                        }
                    }
                }
            }
            let deps = if !leaf.is_empty() { leaf } else { bx };
            if deps.is_empty() {
                continue;
            }
            let (x, y, b) = (r.cx(), r.y, r.bottom());
            let h = 12.0 * 0.8660254037844386;
            let tri = if dir == "in" {
                vec![[x, y - h], [x - 6.0, y], [x + 6.0, y]]
            } else {
                vec![[x - 6.0, b], [x + 6.0, b], [x, b + h]]
            };
            let violating = deps.iter().any(|dep| edn::record::flag(dep, "violating"));
            out.push(Value::Map(vec![
                (Value::keyword("id"), id.clone()),
                (Value::keyword("dir"), Value::keyword(dir)),
                (
                    Value::keyword("triangle"),
                    Value::Vector(tri.iter().map(|p| geom::point(*p)).collect()),
                ),
                (Value::keyword("deps"), Value::Vector(deps)),
                (Value::keyword("violating"), Value::Bool(violating)),
            ]));
        }
    }
    Value::Vector(out)
}

pub(crate) fn indicator_at(scene: &Value, x: f64, y: f64, pad: f64) -> Option<Value> {
    let inds = seq(edn::record::get(scene, "dep-indicators"));
    for a in inds.iter().rev() {
        let tri: Vec<[f64; 2]> = seq(edn::record::get(a, "triangle"))
            .iter()
            .filter_map(geom::vec2)
            .collect();
        if tri.len() == 3 && geom::near_triangle([x, y], &[tri[0], tri[1], tri[2]], pad) {
            return Some(record(vec![
                ("kind", Value::keyword("dep")),
                (
                    "dir",
                    edn::record::get(a, "dir").cloned().unwrap_or(Value::Nil),
                ),
                (
                    "id",
                    edn::record::get(a, "id").cloned().unwrap_or(Value::Nil),
                ),
                (
                    "deps",
                    edn::record::get(a, "deps").cloned().unwrap_or(Value::Nil),
                ),
                (
                    "violating",
                    edn::record::get(a, "violating")
                        .cloned()
                        .unwrap_or(Value::Nil),
                ),
            ]));
        }
    }
    None
}

pub(crate) fn edge_at(scene: &Value, x: f64, y: f64, pad: f64) -> Option<Value> {
    let es = seq(edn::record::get(scene, "edges"));
    for e in es.iter().rev() {
        let strokes = seq(edn::record::get(e, "strokes"));
        let fallback;
        let paths: &[Value] = if !strokes.is_empty() {
            strokes
        } else {
            let ps = edn::record::get(e, "points");
            fallback = match ps {
                Some(Value::Vector(items)) if items.len() > 1 => {
                    vec![Value::Vector(items.clone())]
                }
                _ => vec![],
            };
            &fallback
        };
        for path in paths {
            let pts: Vec<[f64; 2]> = seq(Some(path)).iter().filter_map(geom::vec2).collect();
            if geom::near_polyline([x, y], &pts, pad) {
                return Some(record(vec![
                    ("kind", Value::keyword("edge")),
                    (
                        "from",
                        edn::record::get(e, "from").cloned().unwrap_or(Value::Nil),
                    ),
                    (
                        "to",
                        edn::record::get(e, "to").cloned().unwrap_or(Value::Nil),
                    ),
                    ("deps", deps_of(e).unwrap_or(Value::Vector(vec![]))),
                ]));
            }
        }
    }
    None
}

/// Full hit test: indicators, ports, child rows, class boxes, edges, then
/// package frames; later elements sit on top.
pub fn at(scene: &Value, x: f64, y: f64) -> Option<Value> {
    if let Some(ind) = indicator_at(scene, x, y, 5.0) {
        return Some(ind);
    }
    let cs = seq(edn::record::get(scene, "classes"));
    let visible = cs.iter().any(|c| !edn::record::flag(c, "dummy?"));
    for c in cs.iter().rev() {
        if visible && edn::record::flag(c, "dummy?") {
            continue;
        }
        for dir in ["in", "out"] {
            for p in seq(edn::record::get(c, &format!("{dir}-ports"))) {
                let hit = edn::record::get(p, "rect")
                    .and_then(Rect::from_value)
                    .is_some_and(|r| r.contains(x, y));
                if hit {
                    return Some(record(vec![
                        ("kind", Value::keyword("port")),
                        (
                            "id",
                            edn::record::get(p, "id").cloned().unwrap_or(Value::Nil),
                        ),
                        (
                            "parent",
                            edn::record::get(c, "id").cloned().unwrap_or(Value::Nil),
                        ),
                        ("dir", Value::keyword(dir)),
                    ]));
                }
            }
        }
        if let Some(r) = edn::record::get(c, "rect").and_then(Rect::from_value)
            && r.contains(x, y)
        {
            let mut ly = r.y + 16.0;
            for line in seq(edn::record::get(c, "lines")) {
                if edn::record::local_name(edn::record::get(line, "kind").unwrap())
                    .unwrap_or_default()
                    == "rule"
                {
                    ly += 12.0;
                    continue;
                }
                if y >= ly
                    && y <= ly + 17.0
                    && edn::record::local_name(edn::record::get(line, "kind").unwrap())
                        .unwrap_or_default()
                        == "child"
                {
                    return Some(record(vec![
                        ("kind", Value::keyword("child")),
                        (
                            "id",
                            edn::record::get(line, "id").cloned().unwrap_or(Value::Nil),
                        ),
                        (
                            "parent",
                            edn::record::get(c, "id").cloned().unwrap_or(Value::Nil),
                        ),
                        ("drill?", Value::Bool(edn::record::flag(line, "drill?"))),
                    ]));
                }
                ly += 18.0;
            }
            return Some(record(vec![
                ("kind", Value::keyword("class")),
                (
                    "id",
                    edn::record::get(c, "id").cloned().unwrap_or(Value::Nil),
                ),
                ("drill?", Value::Bool(edn::record::flag(c, "drill?"))),
            ]));
        }
    }
    if let Some(edge) = edge_at(scene, x, y, 8.0) {
        return Some(edge);
    }
    let ps = seq(edn::record::get(scene, "packages"));
    for p in ps.iter().rev() {
        let hit = edn::record::get(p, "rect")
            .and_then(Rect::from_value)
            .is_some_and(|r| r.contains(x, y));
        if hit {
            return Some(record(vec![
                ("kind", Value::keyword("package")),
                (
                    "id",
                    edn::record::get(p, "id").cloned().unwrap_or(Value::Nil),
                ),
            ]));
        }
    }
    None
}

#[cfg(test)]
mod tests;
