//! Route geometry goldens (#149, Seam A): representative routing scenarios
//! whose expected scene geometry in `fixtures/route-goldens/` is static data
//! produced once by the pre-#144 Lua/LOVE scene core; only `:strokes` was
//! re-recorded by the Rust core when flattening moved from 16 fixed steps
//! to a 0.25px tolerance (#187), and `tie`'s `:size :h` when package frames
//! came to hold the edges between their classes. Every number must
//! match within 1e-6 (the golden's `:absolute-tolerance`); edges are
//! compared in order, with their heads and strokes, so a hidden, faded,
//! re-headed or reordered edge fails.
//!
//! | scenario      | what it pins                                                  |
//! |---------------|---------------------------------------------------------------|
//! | `direct`      | unobstructed connection is one straight segment               |
//! | `obstacle`    | a box blocks every direct/track/hop candidate: first clear    |
//! |               | non-reversing detour, including the original's end sync       |
//! | `tie`         | same-rank peers with a box between: equal-score detours, the  |
//! |               | last one wins; two edges in one hop group (`i` = 0, 1)        |
//! | `rank-hop`    | a connection crossing a rank goes through the gap between    |
//! |               | that rank's boxes                                             |
//! | `attachments` | several ends on one class face are spread along it           |
//! | `curve-lr`    | left-to-right: curved corners, end tangents, inheritance     |
//! |               | heads, three ends on one face                                 |
//! | `route148`    | #148 regression: non-hierarchical classes keep their members |
//! |               | (`:hide-members false`), so LoanService -> Loan stays direct  |
//! | `proposal`    | hierarchical IR under `{:proposal true}`: layer packages,     |
//! |               | Unassigned, a foreign oval, detours ranked by the layers      |
//!
//! `:curve` is not part of the compiled scene: `compose::prepare_scene`
//! turns the constrained basis path into `:tip`/`:behind`/`:draw-bounds`/
//! `:strokes` and does not keep it. The only public output that carries it
//! is the diagram projection of `scene::cli::project` (the `uml-viewer
//! scene` command, also the JVM golden seam), and the test compares that
//! where it exists (`proposal`). Non-hierarchical documents compile to
//! `:sections`, whose projection passes the scene edges through without a
//! curve, so for them the test rebuilds it from `:points` with
//! `cli::edge_curve`, the call the diagram projection and `prepare_scene`
//! make.
//! The strokes, tip, behind and draw bounds compared above are the scene's
//! own output of that path.

use crate::common;

use common::{assert_close, assert_golden_tolerance, items, read_edn};
use edn::record::get;
use edn::{Value, parse};
use scene::Rect;
use scene::events::{self, Dims};
use std::path::PathBuf;

const SCENARIOS: [&str; 8] = [
    "direct",
    "obstacle",
    "tie",
    "rank-hop",
    "attachments",
    "curve-lr",
    "route148",
    "proposal",
];

fn fixture(name: &str) -> PathBuf {
    common::fixture("route-goldens", name)
}

/// The view each golden was produced for (`compile_view`'s `extra`).
fn extra(scenario: &str) -> Value {
    match scenario {
        "proposal" => parse("{:proposal true}").unwrap(),
        _ => Value::Map(vec![]),
    }
}

fn document(scenario: &str) -> Value {
    let text = std::fs::read_to_string(fixture(&format!("{scenario}.ir.edn"))).unwrap();
    scene::ir::read_document(&text).unwrap()
}

fn compiled(scenario: &str) -> Value {
    scene::document::compile_view(&document(scenario), &[], &[], &extra(scenario))
}

/// Each edge's `:curve` (by edge index), from the projection when it
/// carries one; see the module comment for the sectioned case.
fn curves(scenario: &str, scene: &Value) -> Vec<Value> {
    let projection = scene::cli::project(
        &document(scenario),
        None,
        &Value::Vector(vec![]),
        &extra(scenario),
    );
    let edges = items(get(scene, "edges"));
    if get(&projection, "sections").is_none() {
        let projected = items(get(&projection, "edges"));
        assert_eq!(edges.len(), projected.len(), "{scenario}: projected edges");
        for (i, (e, p)) in edges.iter().zip(&projected).enumerate() {
            for k in ["from", "to", "points"] {
                assert_eq!(get(e, k), get(p, k), "{scenario} edge[{i}]: projected {k}");
            }
        }
        return projected
            .iter()
            .map(|p| get(p, "curve").cloned().expect("projected :curve"))
            .collect();
    }
    let rect_of = |id: Option<&Value>| {
        items(get(scene, "classes"))
            .iter()
            .find(|c| get(c, "id") == id)
            .and_then(|c| get(c, "rect"))
            .and_then(Rect::from_value)
    };
    edges
        .iter()
        .map(|e| {
            scene::cli::edge_curve(
                &items(get(e, "points")),
                rect_of(get(e, "from")),
                rect_of(get(e, "to")),
            )
        })
        .collect()
}

fn expected(scenario: &str) -> Value {
    read_edn(&fixture(&format!("{scenario}.geometry.edn")))
}

/// Size, class rects and every edge (in order) against the golden; each
/// edge's curve against the projection of the same view.
fn assert_matches_original(scenario: &str) {
    let scene = compiled(scenario);
    let want = expected(scenario);
    assert_golden_tolerance(&want, scenario);
    assert_close(
        get(&want, "size").unwrap(),
        get(&scene, "size"),
        &format!("{scenario} size"),
    );
    let (want_classes, got_classes) = (items(get(&want, "classes")), items(get(&scene, "classes")));
    assert_eq!(
        want_classes.len(),
        got_classes.len(),
        "{scenario}: class count"
    );
    for (i, (w, g)) in want_classes.iter().zip(&got_classes).enumerate() {
        assert_eq!(get(w, "id"), get(g, "id"), "{scenario}: class[{i}] id");
        assert_close(
            get(w, "rect").unwrap(),
            get(g, "rect"),
            &format!("{scenario} class[{i}] rect"),
        );
    }
    let (want_edges, got_edges) = (items(get(&want, "edges")), items(get(&scene, "edges")));
    let curves = curves(scenario, &scene);
    assert_eq!(want_edges.len(), got_edges.len(), "{scenario}: edge count");
    for (i, (w, g)) in want_edges.iter().zip(&got_edges).enumerate() {
        let at = format!("{scenario} edge[{i}]");
        for k in [
            "from",
            "to",
            "kind",
            "head",
            "points",
            "tip",
            "behind",
            "draw-bounds",
            "strokes",
        ] {
            let want_k = get(w, k).unwrap_or_else(|| panic!("{at}: golden has no {k}"));
            assert_close(want_k, get(g, k), &format!("{at} {k}"));
        }
        assert_close(
            get(w, "curve").unwrap(),
            Some(&curves[i]),
            &format!("{at} curve"),
        );
    }
}

fn edge(scene: &Value, from: &str, to: &str) -> Value {
    items(get(scene, "edges"))
        .into_iter()
        .find(|e| {
            get(e, "from")
                .map(|v| edn::record::local_name(v).unwrap_or_default().to_owned())
                .as_deref()
                == Some(from)
                && get(e, "to")
                    .map(|v| edn::record::local_name(v).unwrap_or_default().to_owned())
                    .as_deref()
                    == Some(to)
        })
        .unwrap_or_else(|| panic!("edge {from} -> {to}"))
}

fn rect(scene: &Value, id: &str) -> Rect {
    items(get(scene, "classes"))
        .into_iter()
        .find(|c| {
            get(c, "id")
                .map(|v| edn::record::local_name(v).unwrap_or_default().to_owned())
                .as_deref()
                == Some(id)
        })
        .and_then(|c| get(&c, "rect").and_then(Rect::from_value))
        .unwrap_or_else(|| panic!("class {id}"))
}

fn points(e: &Value) -> Vec<[f64; 2]> {
    items(get(e, "points"))
        .iter()
        .map(|p| {
            let p = items(Some(p));
            [
                edn::record::num(&p[0]).unwrap(),
                edn::record::num(&p[1]).unwrap(),
            ]
        })
        .collect()
}

#[test]
fn unobstructed_direct_connection_matches_the_original() {
    assert_matches_original("direct");
    let scene = compiled("direct");
    let e = edge(&scene, "d0-order", "d0-line");
    assert_eq!(2, points(&e).len(), "one straight segment");
    assert_eq!(1, items(get(&e, "strokes")).len(), "edge is drawn");
}

#[test]
fn connection_blocked_by_one_class_box_matches_the_original() {
    assert_matches_original("obstacle");
    let scene = compiled("obstacle");
    let e = edge(&scene, "d0-source", "d0-sink");
    let filter = rect(&scene, "d0-filter");
    let ps = points(&e);
    assert!(ps.len() > 2, "detours instead of the straight line");
    assert!(
        ps.iter().any(|p| p[0] < filter.x),
        "leaves on the left of the blocking Filter box"
    );
}

#[test]
fn equal_score_candidates_pick_the_last_as_in_the_original() {
    assert_matches_original("tie");
    let scene = compiled("tie");
    let below = |from: &str, to: &str| {
        let r = rect(&scene, from);
        points(&edge(&scene, from, to))[1][1] - r.bottom()
    };
    // Three clear right-angle detours score alike; the last (the rank
    // track, 20 + 10i below the row) wins, not the first (10 + 10i).
    assert_eq!(20.0, below("d0-left", "d0-right"));
    assert_eq!(30.0, below("d0-right", "d0-left"));
}

#[test]
fn connection_hopping_a_rank_matches_the_original() {
    assert_matches_original("rank-hop");
    let scene = compiled("rank-hop");
    let ps = points(&edge(&scene, "d0-gateway", "d0-repo"));
    let (auth, limit) = (rect(&scene, "d0-auth"), rect(&scene, "d0-limit"));
    assert_eq!(4, ps.len(), "start, two hop points, end");
    let (top, bottom) = (auth.y.min(limit.y), auth.bottom().max(limit.bottom()));
    assert!(
        ps[1][1] < top,
        "first hop point {:?} above the middle rank",
        ps[1]
    );
    assert!(
        ps[2][1] > bottom,
        "second hop point {:?} below the middle rank",
        ps[2]
    );
}

#[test]
fn several_attachments_on_one_face_match_the_original() {
    assert_matches_original("attachments");
    let scene = compiled("attachments");
    let broker = rect(&scene, "d0-broker");
    let mut xs: Vec<f64> = ["d0-orders", "d0-billing", "d0-shipping"]
        .iter()
        .map(|to| points(&edge(&scene, "d0-broker", to))[0])
        .inspect(|p| assert_eq!(broker.bottom(), p[1], "leaves the bottom face"))
        .map(|p| p[0])
        .collect();
    xs.dedup();
    assert_eq!(3, xs.len(), "three distinct attachment points");
}

#[test]
fn curved_corners_and_end_tangents_left_to_right_match_the_original() {
    assert_matches_original("curve-lr");
    let scene = compiled("curve-lr");
    let e = edge(&scene, "d0-exporter", "d0-shape");
    let at = items(get(&scene, "edges"))
        .iter()
        .position(|x| *x == e)
        .unwrap();
    let curve = curves("curve-lr", &scene).swap_remove(at);
    assert!(
        items(get(&curve, "ops"))
            .iter()
            .any(|op| get(op, "op").and_then(edn::record::local_name) == Some("cubic")),
        "corners are curved"
    );
    let (tip, behind) = (point(get(&e, "tip")), point(get(&e, "behind")));
    let shape = rect(&scene, "d0-shape");
    assert_eq!(shape.right(), tip[0], "arrow tip on Shape's right face");
    assert!(
        behind[0] > tip[0],
        "arrives from the right, outside the box"
    );
}

/// #148: the original keeps members on every non-hierarchical class (`IR`
/// normalizes an absent `:hide-members` to `false`), which keeps the boxes
/// at their full size and LoanService -> Loan one straight segment.
#[test]
fn unobstructed_dependency_with_member_boxes_is_direct_as_in_the_original() {
    assert_matches_original("route148");
    let scene = compiled("route148");
    let e = edge(&scene, "d0-loan-service", "d0-loan");
    assert_eq!(2, points(&e).len(), "no right-angle detour");
    assert_eq!(1, items(get(&e, "strokes")).len(), "edge is drawn");
    assert!(
        rect(&scene, "d0-loan").h > 42.0,
        "member lines are laid out, not the bare 120x42 name box"
    );
}

#[test]
fn proposal_view_of_a_hierarchical_document_matches_the_original() {
    assert_matches_original("proposal");
    let scene = compiled("proposal");
    let ids: Vec<String> = items(get(&scene, "packages"))
        .iter()
        .filter_map(|p| {
            get(p, "id").map(|v| edn::record::local_name(v).unwrap_or_default().to_owned())
        })
        .collect();
    assert_eq!(
        vec![
            "proposal.unassigned".to_string(),
            "proposal.edge".to_string(),
            "proposal.domain".to_string()
        ],
        ids,
        "proposal layers, not the namespace tree"
    );
    assert!(
        points(&edge(&scene, "core", "util")).len() > 2,
        "Core -> Util detours around the Edge layer"
    );
}

fn point(v: Option<&Value>) -> [f64; 2] {
    let p = items(v);
    [
        edn::record::num(&p[0]).unwrap(),
        edn::record::num(&p[1]).unwrap(),
    ]
}

#[test]
fn repeated_compilation_gives_identical_geometry() {
    // Attachment, hop and endpoint groups live in per-call hash maps with
    // fresh seeds; every compilation must still produce the same scene.
    for scenario in SCENARIOS {
        let first = compiled(scenario);
        for _ in 0..16 {
            assert_eq!(first, compiled(scenario), "{scenario}");
        }
    }
}

#[test]
fn zoom_pan_and_window_size_leave_world_routes_unchanged() {
    let jvm = common::fixture("jvm-goldens", "rust.ir.edn");
    let mut docs: Vec<(Value, Value)> = SCENARIOS.iter().map(|s| (document(s), extra(s))).collect();
    docs.push((
        scene::ir::read_document(&std::fs::read_to_string(jvm).unwrap()).unwrap(),
        Value::Map(vec![]),
    ));
    for (doc, extra) in docs {
        let start = scene::document::new_state(doc.clone(), None, None, Some(&extra), &[], None);
        let routes = get(&start.scene, "edges").cloned();
        let mut s = start.clone();
        for (w, h) in [(1500.0, 920.0), (640.0, 480.0), (3840.0, 2160.0)] {
            let dims = Dims {
                window_w: w,
                window_h: h,
                ..Dims::default()
            };
            s = events::fit(&s, &dims);
            s = events::zoom_at(&s, 3.7, w / 3.0, h / 4.0);
            s = events::on_wheel(&s, 0.0, -4.0, &dims);
            s = events::on_wheel(&s, 2.0, 0.0, &dims);
            let ctrl = Dims {
                control: true,
                pointer: Some((10.0, 20.0)),
                ..dims.clone()
            };
            s = events::on_wheel(&s, 0.0, -3.0, &ctrl);
            assert_eq!(
                routes,
                get(&s.scene, "edges").cloned(),
                "camera changed routes"
            );
        }
        assert_ne!(
            (start.cam_x, start.cam_y, start.zoom),
            (s.cam_x, s.cam_y, s.zoom)
        );
        let fresh = scene::document::compile_view(&doc, &[], &[], &extra);
        assert_eq!(routes, get(&fresh, "edges").cloned(), "recompiled routes");
    }
}
