use crate::common;

use edn::Value;
use edn::record::{flag, get, local_name, num, seq};
use scene::document::ViewState;
use scene::{Dims, Rect, vec2};
use std::collections::HashMap;

const FULL_ZOOM: f64 = 0.4;

fn rect(v: &Value) -> Option<Rect> {
    get(v, "rect").and_then(Rect::from_value)
}

fn key(v: &Value) -> String {
    edn::encode(v).unwrap()
}

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

fn crossings(scene: &Value) -> Vec<String> {
    let classes: Vec<(&Value, Rect)> = seq(get(scene, "classes"))
        .iter()
        .filter(|c| !flag(c, "dummy?"))
        .filter_map(|c| Some((get(c, "id")?, rect(c)?)))
        .collect();
    let mut crossed = Vec::new();
    for e in seq(get(scene, "edges")) {
        let ps: Vec<[f64; 2]> = seq(get(e, "points")).iter().filter_map(vec2).collect();
        let ends = [get(e, "from"), get(e, "to")];
        for (id, r) in &classes {
            if ends.contains(&Some(*id)) {
                continue;
            }
            let crosses = ps.windows(2).any(|w| {
                let (p, q) = (w[0], w[1]);
                p[0].min(q[0]) < r.right()
                    && p[0].max(q[0]) > r.x
                    && p[1].min(q[1]) < r.bottom()
                    && p[1].max(q[1]) > r.y
            });
            if crosses {
                crossed.push(format!("{:?} -> {:?} across {id:?}", ends[0], ends[1]));
            }
        }
    }
    crossed
}

#[test]
fn no_wide_relationship_crosses_a_box_or_oval_it_does_not_end_at() {
    for (focus, scene) in common::self_hosted_views_with(&common::wide()) {
        let crossings = crossings(&scene);
        assert!(crossings.is_empty(), "{focus:?}: {crossings:#?}");
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
    for (focus, scene) in common::self_hosted_views_with(&common::wide()) {
        let edges = seq(get(&scene, "edges"));
        let linked = |id: &Value| {
            edges
                .iter()
                .any(|e| get(e, "from") == Some(id) || get(e, "to") == Some(id))
        };
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
    assert!(drawn > 0, "the sweep draws indicators");
}

#[test]
fn the_wide_layout_depends_on_its_inputs_only() {
    let wide = common::self_hosted_views_with(&common::wide());
    let again = common::self_hosted_views_with(&common::wide());
    for ((focus, a), (_, b)) in wide.iter().zip(&again) {
        assert_eq!(a, b, "{focus:?} placed twice");
    }
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
