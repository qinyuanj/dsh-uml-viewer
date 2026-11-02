//! A package frame holds the edges inside it in every view of the
//! self-hosted diagram, default and wide (#191): an edge between two of its
//! classes never leaves it, and an edge to a foreign oval leaves it only
//! through the face that looks at the oval, never along another side as if
//! it ran outside the package.

use crate::common;

use edn::Value;
use edn::record::{flag, get, seq};
use scene::{Rect, vec2};
use std::collections::HashMap;

/// Strokes are flattened curves; a point this close to the frame is on it.
const TOLERANCE: f64 = 1.0;

fn rect(v: &Value) -> Option<Rect> {
    get(v, "rect").and_then(Rect::from_value)
}

fn key(v: &Value) -> String {
    edn::encode(v).unwrap()
}

fn stroke_points(e: &Value) -> Vec<[f64; 2]> {
    seq(get(e, "strokes"))
        .iter()
        .flat_map(|s| seq(Some(s)).iter().filter_map(vec2))
        .collect()
}

fn inside(r: Rect, p: [f64; 2]) -> bool {
    r.inflate(TOLERANCE).contains(p[0], p[1])
}

/// Where a point outside `frame` may lie on its way to `oval`: beyond the
/// frame face nearest the oval, i.e. in the half-plane the oval sits in.
fn beyond_facing_face(frame: Rect, oval: Rect, p: [f64; 2]) -> bool {
    let gaps = [
        (oval.y - frame.bottom(), p[1] >= frame.bottom() - TOLERANCE),
        (frame.y - oval.bottom(), p[1] <= frame.y + TOLERANCE),
        (oval.x - frame.right(), p[0] >= frame.right() - TOLERANCE),
        (frame.x - oval.right(), p[0] <= frame.x + TOLERANCE),
    ];
    // The face with the widest gap to the oval is the one that faces it.
    gaps.iter()
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .unwrap()
        .1
}

#[test]
fn every_edge_stays_in_its_package_frame_until_it_heads_for_an_oval() {
    let mut failures = Vec::new();
    for (focus, scene) in common::self_hosted_views_both() {
        let frames: HashMap<String, Rect> = seq(get(&scene, "packages"))
            .iter()
            .filter_map(|p| Some((key(get(p, "id")?), rect(p)?)))
            .collect();
        // class id -> (package frame, rect, foreign?)
        let mut classes: HashMap<String, (Option<Rect>, Rect, bool)> = HashMap::new();
        for c in seq(get(&scene, "classes")) {
            if flag(c, "dummy?") {
                continue;
            }
            let frame = get(c, "package").and_then(|p| frames.get(&key(p)).copied());
            classes.insert(
                key(get(c, "id").unwrap()),
                (frame, rect(c).unwrap(), flag(c, "foreign")),
            );
        }
        for e in seq(get(&scene, "edges")) {
            let end = |k: &str| get(e, k).and_then(|id| classes.get(&key(id)).copied());
            let (Some(a), Some(b)) = (end("from"), end("to")) else {
                continue;
            };
            let points = stroke_points(e);
            let label = format!("{focus:?} {:?} -> {:?}", get(e, "from"), get(e, "to"));
            match (a, b) {
                ((Some(fa), _, false), (Some(fb), _, false)) if fa == fb => {
                    let out = points.iter().filter(|p| !inside(fa, **p)).count();
                    if out > 0 {
                        failures.push(format!("{label}: {out} points outside {fa:?}"));
                    }
                }
                ((Some(frame), _, false), (_, oval, true))
                | ((_, oval, true), (Some(frame), _, false)) => {
                    let stray = points
                        .iter()
                        .filter(|p| !inside(frame, **p) && !beyond_facing_face(frame, oval, **p))
                        .count();
                    if stray > 0 {
                        failures.push(format!(
                            "{label}: {stray} points beside {frame:?} on the way to {oval:?}"
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} edges leave their frame:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
