//! Scene compilation scales near-linearly (#137, ADR 0008).
//!
//! The Lua `prepare_scene` rebuilt the class index for every edge (O(E·N)):
//! a 2000-class scene took 532 s to compile. The Rust port indexes classes
//! once plus a spatial grid, so this must stay in the low seconds and scale
//! near-linearly. The scenes are the regenerable `:levels` scenes
//! (`common::levels`); the committed `bench/` scenes were deleted with the
//! standalone window (dsh-uml-viewer#30). Timing bounds are generous (CI
//! machines vary); the slope check is the near-linearity evidence.

use edn::Value;
use std::path::Path;
use std::time::{Duration, Instant};

/// mutator runs each mutant's tests in a slot under `target/mutation-workers/`
/// on shared cores; a timing bound there measures the machine, not the code,
/// and a 2000-class debug compile costs every mutant seconds the unit tests
/// and goldens already cover.
fn under_mutation() -> bool {
    std::env::var_os("CARGO_TARGET_DIR").is_some_and(|dir| {
        Path::new(&dir)
            .components()
            .any(|c| c.as_os_str() == "mutation-workers")
    })
}

fn compile(doc: &Value) -> (Duration, Value) {
    let started = Instant::now();
    let scene = scene::document::compile_view(doc, &[], &[], &Value::Map(vec![]));
    let elapsed = started.elapsed();
    (elapsed, scene)
}

use crate::common;

/// Sizes of the regenerated `:levels` scenes, a doubling apart.
const LEVEL_SIZES: [usize; 4] = [250, 500, 1000, 2000];

#[test]
fn a_levels_scene_has_the_calibrated_shape() {
    let text = common::levels::scene(1000);
    assert_eq!(text, common::levels::scene(1000), "regenerates identically");
    let doc = scene::ir::read_document(&text).unwrap();
    let diagram = &edn::record::seq(edn::record::get(&doc, "diagrams"))[0];
    let packages = edn::record::seq(edn::record::get(diagram, "packages"));
    assert_eq!(1, packages.len(), "one package, so the ovals sink");
    let classes = edn::record::seq(edn::record::get(&packages[0], "classes"));
    assert_eq!(1000, classes.len());
    let level = |id: &Value| {
        classes
            .iter()
            .find(|c| edn::record::get(c, "id") == Some(id))
            .and_then(|c| edn::record::get(c, "level").and_then(edn::record::num))
    };
    let edges = edn::record::seq(edn::record::get(diagram, "edges"));
    let foreign = edges
        .iter()
        .filter(|e| level(edn::record::get(e, "to").unwrap()).is_none())
        .count();
    let inner = edges
        .iter()
        .filter(|e| {
            let to = level(edn::record::get(e, "to").unwrap());
            to.is_some() && to == level(edn::record::get(e, "from").unwrap())
        })
        .count();
    let share = |n: usize| n as f64 / edges.len() as f64;
    assert!(
        (3000..=3200).contains(&edges.len()),
        "{} edges",
        edges.len()
    );
    assert!(
        (0.07..=0.09).contains(&share(foreign)),
        "foreign {}",
        share(foreign)
    );
    assert!(
        (0.5..=0.62).contains(&share(inner)),
        "inner {}",
        share(inner)
    );
}

#[test]
// Coverage instrumentation slows the build several-fold; time it uninstrumented.
#[cfg_attr(coverage, ignore)]
fn levels_scenes_compile_near_linearly_across_four_sizes() {
    if under_mutation() {
        return;
    }
    let docs: Vec<Value> = LEVEL_SIZES
        .iter()
        .map(|&n| scene::ir::read_document(&common::levels::scene(n)).unwrap())
        .collect();
    let mut best = vec![f64::INFINITY; docs.len()];
    for _ in 0..3 {
        for (b, doc) in best.iter_mut().zip(&docs) {
            *b = b.min(compile(doc).0.as_secs_f64());
        }
    }
    // Least-squares slope of log(time) against log(classes).
    let xs: Vec<f64> = LEVEL_SIZES.iter().map(|&n| (n as f64).ln()).collect();
    let ys: Vec<f64> = best.iter().map(|t| t.max(1e-6).ln()).collect();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (mx, my) = (mean(&xs), mean(&ys));
    let slope = xs
        .iter()
        .zip(&ys)
        .map(|(x, y)| (x - mx) * (y - my))
        .sum::<f64>()
        / xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>();
    for (n, t) in LEVEL_SIZES.iter().zip(&best) {
        eprintln!("scene: levels {n}: {t:.3}s");
    }
    eprintln!("scene: levels log-log slope {slope:.2}");
    // The same bound as the 500 -> 2000 ratio check: 10x for 4x data.
    assert!(
        slope < 10f64.ln() / 4f64.ln(),
        "levels scenes scale with slope {slope:.2}: not near-linear"
    );
    assert!(
        best[3] < 30.0,
        "2000-class levels compile took {:.1}s",
        best[3]
    );
}
