//! Seam A through the real binary (#137): `uml-viewer scene` parses argv,
//! reads IR EDN and writes a geometry projection that matches the JVM
//! goldens within 1e-6.

use edn::{Value, parse};
use std::path::{Path, PathBuf};
use std::process::Output;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> Output {
    super::binary(std::iter::once("scene").chain(args.iter().copied()))
}

fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Integer(n)) => *n as f64,
        Some(Value::Float(x)) => *x,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(&Value::keyword(key))
}

fn items(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Vector(items)) => items,
        _ => &[],
    }
}

#[test]
fn exports_golden_geometry_for_each_focus_to_stdout() {
    let ir = repo().join("fixtures/jvm-goldens/lua.ir.edn");
    let output = run(&["--ir", ir.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = parse(&String::from_utf8_lossy(&output.stdout)).unwrap();
    assert_eq!(Some(&Value::Integer(1)), get(&out, "version"));
    let views = items(get(&out, "views"));
    assert_eq!(1, views.len());
    let golden = parse(
        &std::fs::read_to_string(repo().join("fixtures/jvm-goldens/lua.geometry.edn")).unwrap(),
    )
    .unwrap();
    let want = &items(get(&golden, "views"))[0];
    let class_want = &items(get(want, "classes"))[0];
    let class_got = &items(get(&views[0], "classes"))[0];
    assert_eq!(get(class_want, "id"), get(class_got, "id"));
    for k in ["x", "y", "w", "h"] {
        let a = num(get(get(class_want, "rect").unwrap(), k));
        let b = num(get(get(class_got, "rect").unwrap(), k));
        assert!((a - b).abs() <= 1e-6, "{k}: {a} vs {b}");
    }
    for k in ["w", "h", "min-x", "min-y"] {
        let a = num(get(get(want, "size").unwrap(), k));
        let b = num(get(get(&views[0], "size").unwrap(), k));
        assert!((a - b).abs() <= 1e-6, "size {k}: {a} vs {b}");
    }
}

#[test]
fn repeated_focus_flags_export_one_view_each_and_write_to_output() {
    let ir = repo().join("fixtures/jvm-goldens/rust.ir.edn");
    let out_path = std::env::temp_dir().join(format!("scene-cli-{}.edn", std::process::id()));
    let output = run(&[
        "--ir",
        ir.to_str().unwrap(),
        "--focus",
        "[]",
        "--focus",
        "[:rust]",
        "--focus",
        "[:rust :proto]",
        "--output",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "output went to the file");
    let out = parse(&std::fs::read_to_string(&out_path).unwrap()).unwrap();
    let views = items(get(&out, "views"));
    assert_eq!(3, views.len());
    assert_eq!(Some(&parse("[]").unwrap()), get(&views[0], "focus"));
    assert_eq!(Some(&parse("[:rust]").unwrap()), get(&views[1], "focus"));
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn a_metrics_root_overlays_quality_but_not_golden_geometry() {
    // lua-fixture has real `.metrics` snapshots; geometry must be unchanged
    // because quality only paints, it does not move boxes.
    let ir = repo().join("fixtures/jvm-goldens/lua.ir.edn");
    let root = repo().join("lua-fixture");
    let plain = run(&["--ir", ir.to_str().unwrap()]);
    let overlaid = run(&[
        "--ir",
        ir.to_str().unwrap(),
        "--metrics-root",
        root.to_str().unwrap(),
    ]);
    assert!(overlaid.status.success());
    let a = parse(&String::from_utf8_lossy(&plain.stdout)).unwrap();
    let b = parse(&String::from_utf8_lossy(&overlaid.stdout)).unwrap();
    let rects = |v: &Value| {
        items(get(&items(get(v, "views"))[0], "classes"))
            .iter()
            .map(|c| get(c, "rect").unwrap().clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(rects(&a), rects(&b));
}

#[test]
fn usage_errors_exit_2_and_bad_ir_exits_1() {
    let output = run(&["--focus", "[]"]);
    assert_eq!(Some(2), output.status.code());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage"));
    let output = run(&["--ir", "missing.edn"]);
    assert_eq!(Some(1), output.status.code());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing.edn"));
    let output = run(&["--ir", "/dev/null"]);
    assert_eq!(Some(1), output.status.code());
    let output = run(&["--ir", "x.edn", "--focus", ":not-a-vector"]);
    assert_eq!(Some(2), output.status.code());
}
