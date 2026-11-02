//! Golden geometry parity (Seam A): the `scene` projection of each JVM
//! golden view must match `fixtures/jvm-goldens/*.geometry.edn` exactly in
//! structure, with every finite number within 1e-6; NaN/Inf fail.

use crate::common;

use common::{assert_close, assert_golden_tolerance, read_edn as read, repo};
use edn::Value;
use edn::record::get;

fn focuses(golden: &Value) -> Vec<Value> {
    match get(golden, "views") {
        Some(Value::Vector(views)) => views
            .iter()
            .map(|v| get(v, "focus").unwrap().clone())
            .collect(),
        _ => vec![],
    }
}

#[test]
fn projection_matches_every_golden_view_package_class_and_size() {
    for lang in ["lua", "typescript", "rust", "python"] {
        let dir = repo().join("fixtures/jvm-goldens");
        let doc = scene::ir::read_document(
            &std::fs::read_to_string(dir.join(format!("{lang}.ir.edn"))).unwrap(),
        )
        .unwrap();
        let golden = read(&dir.join(format!("{lang}.geometry.edn")));
        for (view, focus) in match get(&golden, "views") {
            Some(Value::Vector(views)) => views
                .iter()
                .map(|v| (v.clone(), get(v, "focus").unwrap().clone()))
                .collect::<Vec<_>>(),
            _ => panic!("no views"),
        } {
            let projection = scene::cli::project(&doc, None, &focus, &Value::Map(vec![]));
            let label = format!("{lang} {focus:?}");
            // Packages: count and rects by index.
            let want_packages = get(&view, "packages").unwrap();
            let got_packages = get(&projection, "packages").unwrap();
            assert_close(
                want_packages,
                Some(got_packages),
                &format!("{label} packages"),
            );
            // Size fields.
            for k in ["w", "h", "min-x", "min-y"] {
                assert_close(
                    &get(get(&view, "size").unwrap(), k).unwrap().clone(),
                    Some(get(get(&projection, "size").unwrap(), k).unwrap()),
                    &format!("{label} size {k}"),
                );
            }
            // Classes: id equality and rects, in order.
            let want_classes = match get(&view, "classes").unwrap() {
                Value::Vector(v) => v.clone(),
                _ => panic!(),
            };
            let got_classes = match get(&projection, "classes").unwrap() {
                Value::Vector(v) => v.clone(),
                _ => panic!(),
            };
            assert_eq!(want_classes.len(), got_classes.len(), "{label} class count");
            for (i, (want, got)) in want_classes.iter().zip(&got_classes).enumerate() {
                assert_eq!(get(want, "id"), get(got, "id"), "{label} class[{i}] id");
                assert_close(
                    get(want, "rect").unwrap(),
                    Some(get(got, "rect").unwrap()),
                    &format!("{label} class[{i}] rect"),
                );
            }
        }
    }
}

#[test]
fn projection_matches_every_golden_edge_and_constrained_curve() {
    for lang in ["lua", "typescript", "rust", "python"] {
        let dir = repo().join("fixtures/jvm-goldens");
        let doc = scene::ir::read_document(
            &std::fs::read_to_string(dir.join(format!("{lang}.ir.edn"))).unwrap(),
        )
        .unwrap();
        let golden = read(&dir.join(format!("{lang}.geometry.edn")));
        for view in match get(&golden, "views") {
            Some(Value::Vector(views)) => views.clone(),
            _ => panic!(),
        } {
            let focus = get(&view, "focus").unwrap().clone();
            let projection = scene::cli::project(&doc, None, &focus, &Value::Map(vec![]));
            let label = format!("{lang} {focus:?}");
            let want = match get(&view, "edges").unwrap() {
                Value::Vector(v) => v.clone(),
                _ => panic!(),
            };
            let got = match get(&projection, "edges").unwrap() {
                Value::Vector(v) => v.clone(),
                _ => panic!(),
            };
            assert_eq!(want.len(), got.len(), "{label} edge count");
            for (i, (want, got)) in want.iter().zip(&got).enumerate() {
                for k in [
                    "from",
                    "to",
                    "kind",
                    "points",
                    "tip",
                    "behind",
                    "draw-bounds",
                    "curve",
                ] {
                    assert_close(
                        get(want, k).unwrap_or(&Value::Nil),
                        Some(get(got, k).unwrap_or(&Value::Nil)),
                        &format!("{label} edge[{i}] {k}"),
                    );
                }
            }
        }
    }
}

#[test]
fn every_golden_file_is_read_and_has_the_expected_view_count() {
    let dir = repo().join("fixtures/jvm-goldens");
    for (lang, views) in [("lua", 1), ("typescript", 2), ("rust", 3), ("python", 2)] {
        let golden = read(&dir.join(format!("{lang}.geometry.edn")));
        assert_eq!(views, focuses(&golden).len(), "{lang} view count");
        assert_golden_tolerance(&golden, lang);
    }
}
