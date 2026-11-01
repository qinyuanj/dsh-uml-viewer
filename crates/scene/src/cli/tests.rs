//! `uml-viewer scene` argument parsing and the projection it writes.

use crate::cli::{Options, execute, parse_args};
use edn::Value;
use edn::record::{get, seq};
use std::ffi::OsString;
use std::path::PathBuf;

fn parse(args: &[&str]) -> Result<Options, String> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    parse_args(&args)
}

fn error(args: &[&str]) -> String {
    parse(args).err().expect("parse error")
}

fn fixture(name: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/jvm-goldens");
    dir.join(name).to_string_lossy().into_owned()
}

#[test]
fn the_ir_alone_compiles_the_root_view_in_the_default_layout() {
    let opts = parse(&["--ir", "a.edn"]).unwrap();
    assert_eq!(Some(PathBuf::from("a.edn")), opts.ir);
    assert_eq!(vec![Value::Vector(vec![])], opts.focuses);
    assert_eq!(
        (None, None, false, None),
        (opts.metrics_root, opts.output, opts.wide, opts.aspect)
    );
}

#[test]
fn every_option_lands_and_focuses_keep_their_order() {
    let opts = parse(&[
        "--focus",
        "[:a]",
        "--ir",
        "a.edn",
        "--metrics-root",
        "m",
        "--output",
        "o.edn",
        "--focus",
        "[:a :b]",
        "--wide",
        "--aspect",
        "1.5",
    ])
    .unwrap();
    assert_eq!(
        vec![edn::parse("[:a]").unwrap(), edn::parse("[:a :b]").unwrap()],
        opts.focuses
    );
    assert_eq!(Some(PathBuf::from("m")), opts.metrics_root);
    assert_eq!(Some(PathBuf::from("o.edn")), opts.output);
    assert!(opts.wide);
    assert_eq!(Some(1.5), opts.aspect);
}

#[test]
fn a_missing_ir_value_or_unknown_option_is_a_usage_error() {
    assert_eq!("--ir is required", error(&[]));
    assert_eq!("--ir is required", error(&["--wide"]));
    assert_eq!("--ir requires a value", error(&["--ir"]));
    assert_eq!(
        "--output requires a value",
        error(&["--ir", "a", "--output"])
    );
    assert_eq!(
        "unknown option --zoom",
        error(&["--ir", "a", "--zoom", "2"])
    );
}

#[test]
fn a_focus_is_an_edn_vector() {
    assert_eq!(
        "--focus must be an EDN vector of keywords",
        error(&["--ir", "a", "--focus", ":a"])
    );
    assert!(error(&["--ir", "a", "--focus", "[:a"]).starts_with("--focus: "));
}

#[test]
fn an_aspect_is_a_positive_number_of_a_wide_layout() {
    for bad in ["0", "-1", "wide", "NaN", "inf"] {
        assert_eq!(
            "--aspect must be a positive number",
            error(&["--ir", "a", "--wide", "--aspect", bad]),
            "{bad}"
        );
    }
    assert_eq!(
        "--aspect requires --wide",
        error(&["--ir", "a", "--aspect", "2"])
    );
}

#[test]
fn execute_writes_one_projected_view_per_focus() {
    let out = std::env::temp_dir().join(format!("uml-scene-cli-{}.edn", std::process::id()));
    let opts = parse(&[
        "--ir",
        &fixture("rust.ir.edn"),
        "--focus",
        "[]",
        "--focus",
        "[:rust]",
        "--output",
        &out.to_string_lossy(),
    ])
    .unwrap();
    execute(&opts).unwrap();
    let written = edn::parse(&std::fs::read_to_string(&out).unwrap()).unwrap();
    std::fs::remove_file(&out).unwrap();
    assert_eq!(Some(&Value::Integer(1)), get(&written, "version"));
    let views = seq(get(&written, "views"));
    assert_eq!(2, views.len());
    assert_eq!(Some(&Value::Vector(vec![])), get(&views[0], "focus"));
    assert_eq!(
        Some(&edn::parse("[:rust]").unwrap()),
        get(&views[1], "focus")
    );
    let doc = crate::ir::read_document(&std::fs::read_to_string(fixture("rust.ir.edn")).unwrap())
        .unwrap();
    for (view, focus) in views.iter().zip(&opts.focuses) {
        assert_eq!(
            &crate::cli::project(&doc, None, focus, &Value::Map(vec![])),
            view
        );
    }
}

#[test]
fn execute_reports_an_unreadable_or_invalid_ir() {
    let missing = parse(&["--ir", "/nonexistent/uml-scene.edn"]).unwrap();
    assert!(execute(&missing).unwrap_err().starts_with("cannot read "));
    let invalid = parse(&["--ir", &fixture("README.md")]).unwrap();
    assert!(execute(&invalid).unwrap_err().starts_with("invalid IR "));
}
