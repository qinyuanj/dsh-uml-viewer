//! The 2000-class bench scene compiles in seconds (#137, ADR 0008).
//!
//! The Lua `prepare_scene` rebuilt the class index for every edge (O(E·N)):
//! the 2000-class scene took 532 s to compile. The Rust port indexes classes
//! once plus a spatial grid, so this must stay in the low seconds and scale
//! near-linearly. Timing bounds are generous (CI machines vary); the ratio
//! check is the near-linearity evidence.

use edn::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A bench scene with its class and edge counts (classes are nested in
/// packages, so count through the normalized diagram).
fn scene_of(name: &str) -> (Value, usize, usize) {
    let path = repo().join("bench/scenes").join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let doc = scene::ir::read_document(&text).unwrap();
    let diagrams = edn::record::seq(edn::record::get(&doc, "diagrams"));
    let diagram = &diagrams[0];
    let classes: usize = edn::record::seq(edn::record::get(diagram, "packages"))
        .iter()
        .map(|p| edn::record::seq(edn::record::get(p, "classes")).len())
        .sum();
    let edges = edn::record::seq(edn::record::get(diagram, "edges")).len();
    (doc, classes, edges)
}

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

#[test]
fn the_bench_scenes_load_with_the_expected_size() {
    let (_, classes, edges) = scene_of("synthetic-2000.edn");
    assert_eq!(2000, classes);
    assert!(edges >= 6000, "expected ~6495 edges, got {edges}");
    let (_, classes, edges) = scene_of("synthetic-500.edn");
    assert_eq!(500, classes);
    assert!(edges >= 1500, "expected ~1617 edges, got {edges}");
}

#[test]
// Coverage instrumentation slows the build several-fold; time it uninstrumented.
#[cfg_attr(coverage, ignore)]
fn the_2000_class_scene_compiles_in_seconds() {
    if under_mutation() {
        return;
    }
    let (doc, _, _) = scene_of("synthetic-2000.edn");
    let (elapsed, scene) = compile(&doc);
    let classes = edn::record::seq(edn::record::get(&scene, "classes")).len();
    let edges = edn::record::seq(edn::record::get(&scene, "edges")).len();
    eprintln!(
        "scene: 2000-class compile {:.3}s ({} classes, {} edges)",
        elapsed.as_secs_f64(),
        classes,
        edges
    );
    assert_eq!(2000, classes);
    assert!(edges >= 6000);
    // The Lua baseline was 532 s; allow a wide margin for slow CI machines.
    assert!(
        elapsed < Duration::from_secs(30),
        "2000-class compile took {elapsed:?}"
    );
}

#[test]
// Coverage instrumentation slows the build several-fold; time it uninstrumented.
#[cfg_attr(coverage, ignore)]
fn prepare_scene_scales_near_linearly_from_500_to_2000_classes() {
    if under_mutation() {
        return;
    }
    let (small, _, _) = scene_of("synthetic-500.edn");
    let (large, _, _) = scene_of("synthetic-2000.edn");
    // Warm up both once so allocator/page-cache effects do not dominate.
    let _ = compile(&small);
    let _ = compile(&large);
    let mut small_times = Vec::new();
    let mut large_times = Vec::new();
    for _ in 0..3 {
        small_times.push(compile(&small).0.as_secs_f64());
        large_times.push(compile(&large).0.as_secs_f64());
    }
    let best = |xs: &[f64]| xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let ratio = best(&large_times) / best(&small_times).max(1e-6);
    eprintln!(
        "scene: 500-class {:.3}s, 2000-class {:.3}s, ratio {ratio:.1}x (4x data)",
        best(&small_times),
        best(&large_times)
    );
    // O(E·N) would be ~16x for 4x data; near-linear stays well below that.
    assert!(
        ratio < 10.0,
        "4x data compiled {ratio:.1}x slower: not near-linear"
    );
}

#[test]
fn the_exported_projection_covers_every_bench_class_and_edge() {
    if under_mutation() {
        return;
    }
    let (doc, classes, edges) = scene_of("synthetic-2000.edn");
    let view = scene::cli::project(&doc, None, &Value::Vector(vec![]), &Value::Map(vec![]));
    // The synthetic scenes are flat diagrams, so the projection stacks one
    // section per diagram.
    let sections = edn::record::seq(edn::record::get(&view, "sections"));
    assert_eq!(1, sections.len());
    let section = &sections[0];
    assert_eq!(
        classes,
        edn::record::seq(edn::record::get(section, "classes")).len()
    );
    let exported = edn::record::seq(edn::record::get(section, "edges"));
    assert_eq!(edges, exported.len());
    // Section edges are routed but not clipped (prepare runs once on the
    // stacked scene), so the export carries finite routed points per section.
    for e in exported {
        let points = edn::record::seq(edn::record::get(e, "points"));
        assert!(points.len() >= 2);
        for p in points {
            assert!(edn::record::num(p.as_vector_first().unwrap()).is_some());
        }
    }
}

/// Tiny helper for the first element of an EDN vector.
trait FirstVector {
    fn as_vector_first(&self) -> Option<&Value>;
}

impl FirstVector for Value {
    fn as_vector_first(&self) -> Option<&Value> {
        match self {
            Value::Vector(items) => items.first(),
            _ => None,
        }
    }
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
