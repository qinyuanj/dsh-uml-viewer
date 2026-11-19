//! The wide layout (#191) of the self-hosted diagram: each view fits the
//! viewer canvas at a zoom that draws members, keeps the architecture
//! levels of the default layout in reading order, and routes no
//! relationship that keeps to one shelf across a box or oval it does not
//! end at (#89 added the shelves).

use crate::common;

use edn::Value;
use edn::record::{flag, get, local_name, num, seq};
use scene::document::ViewState;
use scene::{Dims, Rect, vec2};
use std::collections::HashMap;

/// The viewer's members threshold (`frame::lod::FULL_ZOOM`).
const FULL_ZOOM: f64 = 0.4;

fn rect(v: &Value) -> Option<Rect> {
    get(v, "rect").and_then(Rect::from_value)
}

fn key(v: &Value) -> String {
    edn::encode(v).unwrap()
}

/// The historical 1500×920 window with the inspector it once had: a 1220px
/// canvas.
fn canvas() -> Dims {
    Dims {
        window_w: 1500.0,
        window_h: 920.0,
        view_w: Some(1220.0),
        ..Dims::default()
    }
}

fn fit_zoom(scene: &Value) -> f64 {
    let state = ViewState {
        scene: scene.clone(),
        ..ViewState::default()
    };
    scene::events::fit(&state, &canvas()).zoom
}

#[test]
fn every_wide_view_fits_at_members_zoom_and_no_smaller_than_the_default() {
    let default = common::self_hosted_views();
    let wide = common::self_hosted_views_with(&common::wide());
    for ((focus, d), (_, w)) in default.iter().zip(&wide) {
        if [":edn", ":main"].contains(focus) {
            continue;
        }
        let (zd, zw) = (fit_zoom(d), fit_zoom(w));
        assert!(zw >= FULL_ZOOM, "{focus:?} wide zoom {zw:.3}");
        assert!(
            zw >= zd,
            "{focus:?} wide zoom {zw:.3} below default {zd:.3}"
        );
    }
}

/// The shelf each class sits on: the wide layout stacks its columns into
/// shelves separated by a gutter, and the boxes of one shelf all cross the
/// height of that shelf (#89).
fn shelves(scene: &Value) -> HashMap<String, usize> {
    let mut boxes: Vec<(f64, f64, String)> = seq(get(scene, "classes"))
        .iter()
        .filter(|c| !flag(c, "dummy?"))
        .filter_map(|c| {
            let r = rect(c)?;
            Some((r.y, r.bottom(), key(get(c, "id")?)))
        })
        .collect();
    boxes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut out = HashMap::new();
    let (mut shelf, mut bottom) = (0, f64::NEG_INFINITY);
    for (y, b, id) in boxes {
        if y > bottom {
            shelf += 1;
            bottom = b;
        } else {
            bottom = bottom.max(b);
        }
        out.insert(id, shelf);
    }
    out
}

/// Points of a routed edge's polyline crossing a box it does not end at,
/// split into the edges that stay on one shelf and the edges that wrap.
fn crossings(scene: &Value) -> (Vec<String>, Vec<String>) {
    let shelf = shelves(scene);
    let classes: Vec<(&Value, Rect)> = seq(get(scene, "classes"))
        .iter()
        .filter(|c| !flag(c, "dummy?"))
        .filter_map(|c| Some((get(c, "id")?, rect(c)?)))
        .collect();
    let (mut inside, mut wrapped) = (Vec::new(), Vec::new());
    for e in seq(get(scene, "edges")) {
        let ps: Vec<[f64; 2]> = seq(get(e, "points")).iter().filter_map(vec2).collect();
        let ends = [get(e, "from"), get(e, "to")];
        let wraps = match (ends[0], ends[1]) {
            (Some(f), Some(t)) => shelf.get(&key(f)) != shelf.get(&key(t)),
            _ => false,
        };
        for (id, r) in &classes {
            if ends.contains(&Some(*id)) {
                continue;
            }
            let crossed = ps.windows(2).any(|w| {
                let (p, q) = (w[0], w[1]);
                p[0].min(q[0]) < r.right()
                    && p[0].max(q[0]) > r.x
                    && p[1].min(q[1]) < r.bottom()
                    && p[1].max(q[1]) > r.y
            });
            if crossed {
                let at = format!("{:?} -> {:?} across {id:?}", ends[0], ends[1]);
                if wraps {
                    wrapped.push(at);
                } else {
                    inside.push(at);
                }
            }
        }
    }
    (inside, wrapped)
}

/// Ceilings on the crossings a wrapped relationship may make (#94). A lane
/// is reserved through the columns of one shelf only, so a relationship
/// whose ends wrap onto different shelves keeps none (#89), and
/// `route::candidates` assumes the rank order is the x order, which the
/// strip satisfies and the wrapped columns do not (#94). These are the
/// counts the real tree measures today: they may go down, never up, and a
/// view that is not listed here needs its own line.
const WRAPPED_CROSSINGS: &[(&str, usize)] = &[
    ("", 12),
    (":companion", 0),
    (":edn", 0),
    (":ir", 0),
    (":main", 0),
    (":scene", 0),
    (":viewer", 0),
];

#[test]
fn no_wide_relationship_crosses_a_box_or_oval_it_does_not_end_at() {
    // A lane is reserved through the columns of one shelf, and a relationship
    // whose ends wrap onto different shelves loses it (#89). Every crossing
    // must belong to such a wrapped relationship: the guarantee stays whole
    // for every relationship that keeps to a shelf, and the ones that lose
    // their lane are counted against a ceiling that only goes down (#94).
    for (focus, scene) in common::self_hosted_views_with(&common::wide()) {
        let (inside, wrapped) = crossings(&scene);
        assert!(
            inside.is_empty(),
            "{focus:?}: {inside:#?} (wrapped: {wrapped:#?})"
        );
        let Some((_, ceiling)) = WRAPPED_CROSSINGS.iter().find(|(v, _)| *v == focus) else {
            panic!("{focus:?} has no ceiling in WRAPPED_CROSSINGS; add one");
        };
        assert!(
            wrapped.len() <= *ceiling,
            "{focus:?}: {} cross-shelf crossings, ceiling {} - tighten the ceiling instead \
             of raising it, and see #94 for the gutter channel model that removes them: {wrapped:#?}",
            wrapped.len(),
            ceiling,
        );
    }
}

#[test]
fn wide_views_keep_every_level_and_violation_of_the_default() {
    let default = common::self_hosted_views();
    let wide = common::self_hosted_views_with(&common::wide());
    let levels = |s: &Value| -> HashMap<String, Option<f64>> {
        seq(get(s, "classes"))
            .iter()
            .map(|c| (key(get(c, "id").unwrap()), get(c, "level").and_then(num)))
            .collect()
    };
    let violations = |s: &Value| -> HashMap<String, bool> {
        seq(get(s, "edges"))
            .iter()
            .map(|e| {
                let k = format!("{:?}->{:?}", get(e, "from"), get(e, "to"));
                (k, flag(e, "violating"))
            })
            .collect()
    };
    for ((focus, d), (_, w)) in default.iter().zip(&wide) {
        assert_eq!(levels(d), levels(w), "{focus:?} levels");
        assert_eq!(violations(d), violations(w), "{focus:?} violations");
    }
}

#[test]
fn wide_levels_run_from_the_outermost_on_the_left_to_level_zero_then_the_ovals() {
    // "Left to right" became the layout's reading order once the strip wraps
    // into shelves (#89): the column rank, highest at the outermost level and
    // lowest at the oval sink. On one shelf that is the old order of x; a
    // shelf below starts again at the left, so x alone no longer says it.
    for (focus, scene) in common::self_hosted_views_with(&common::wide()) {
        let edges = seq(get(&scene, "edges"));
        let linked = |id: &Value| {
            edges
                .iter()
                .any(|e| get(e, "from") == Some(id) || get(e, "to") == Some(id))
        };
        // level -> (leftmost rank, rightmost rank) of its boxes with an edge
        let mut spans: Vec<(f64, f64, f64)> = Vec::new();
        let mut ovals: Vec<f64> = Vec::new();
        for c in seq(get(&scene, "classes")) {
            let (Some(id), Some(rank)) = (get(c, "id"), get(c, "rank").and_then(num)) else {
                continue;
            };
            if flag(c, "foreign") {
                ovals.push(rank);
                continue;
            }
            let Some(level) = get(c, "level").and_then(num) else {
                continue;
            };
            if !linked(id) {
                continue;
            }
            match spans.iter_mut().find(|s| s.0 == level) {
                Some(s) => (s.1, s.2) = (s.1.min(rank), s.2.max(rank)),
                None => spans.push((level, rank, rank)),
            }
        }
        spans.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        for pair in spans.windows(2) {
            assert!(
                pair[0].1 > pair[1].2,
                "{focus:?} level {} reaches rank {} past level {} at {}",
                pair[0].0,
                pair[0].1,
                pair[1].0,
                pair[1].2
            );
        }
        if let Some(last) = spans.last() {
            for rank in &ovals {
                assert!(
                    *rank < last.1,
                    "{focus:?} oval at rank {rank} before level {} at {}",
                    last.0,
                    last.1
                );
            }
        }
    }
}

#[test]
fn wide_indicators_stay_above_and_below_their_box() {
    let mut extra = common::wide();
    extra.assoc(Value::keyword("declutter"), Value::keyword("triangles"));
    let mut drawn = 0;
    for (focus, scene) in common::self_hosted_views_with(&extra) {
        let rects: HashMap<String, Rect> = seq(get(&scene, "classes"))
            .iter()
            .filter_map(|c| Some((key(get(c, "id")?), rect(c)?)))
            .collect();
        let indicators = seq(get(&scene, "dep-indicators"));
        drawn += indicators.len();
        for ind in indicators {
            let r = rects[&key(get(ind, "id").unwrap())];
            let ys: Vec<f64> = seq(get(ind, "triangle"))
                .iter()
                .filter_map(vec2)
                .map(|p| p[1])
                .collect();
            match get(ind, "dir").and_then(local_name) {
                Some("in") => assert!(ys.iter().all(|y| *y <= r.y), "{focus:?} {ind:?}"),
                _ => assert!(ys.iter().all(|y| *y >= r.bottom()), "{focus:?} {ind:?}"),
            }
        }
    }
    // Indicators stand for edges; a ports-only view (`:main`) draws none.
    assert!(drawn > 0, "the sweep draws indicators");
}

#[test]
fn the_wide_layout_depends_on_its_inputs_only() {
    let wide = common::self_hosted_views_with(&common::wide());
    let again = common::self_hosted_views_with(&common::wide());
    for ((focus, a), (_, b)) in wide.iter().zip(&again) {
        assert_eq!(a, b, "{focus:?} placed twice");
    }
    // Fitting another window moves the camera, never the boxes.
    let (_, scene) = wide.iter().find(|(focus, _)| *focus == ":scene").unwrap();
    let state = ViewState {
        scene: scene.clone(),
        ..ViewState::default()
    };
    for (w, h) in [(800.0, 600.0), (2560.0, 1440.0)] {
        let dims = Dims {
            window_w: w,
            window_h: h,
            ..Dims::default()
        };
        assert_eq!(scene, &scene::events::fit(&state, &dims).scene);
    }
}

#[test]
fn wide_columns_keep_the_rank_order_of_the_default_rows() {
    // Both layouts number ranks so that the first row (top) or column
    // (left) is the highest: levels, their sub-rows, the oval sink and the
    // edgeless grid keep one order, folds only add columns within a rank.
    let rank = |s: &Value| -> HashMap<String, f64> {
        seq(get(s, "classes"))
            .iter()
            .filter(|c| !flag(c, "dummy?"))
            .filter_map(|c| Some((key(get(c, "id")?), get(c, "rank").and_then(num)?)))
            .collect()
    };
    let default = common::self_hosted_views();
    let wide = common::self_hosted_views_with(&common::wide());
    for ((focus, d), (_, w)) in default.iter().zip(&wide) {
        let (rd, rw) = (rank(d), rank(w));
        assert_eq!(
            rd.keys().collect::<std::collections::BTreeSet<_>>(),
            rw.keys().collect(),
            "{focus:?} ranked classes"
        );
        for (a, ra) in &rd {
            for (b, rb) in &rd {
                if ra > rb {
                    assert!(rw[a] > rw[b], "{focus:?} {a} before {b}");
                }
            }
        }
    }
}
