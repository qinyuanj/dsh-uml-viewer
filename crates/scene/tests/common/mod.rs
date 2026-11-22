#![allow(dead_code)]

pub mod levels;

use edn::record::get;
use edn::{Value, parse};
use std::path::{Path, PathBuf};

pub const TOLERANCE: f64 = 1e-6;

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixture(dir: &str, name: &str) -> PathBuf {
    repo().join("fixtures").join(dir).join(name)
}

pub fn self_hosted_views() -> Vec<(&'static str, Value)> {
    self_hosted_views_with(&Value::Map(vec![]))
}

pub fn wide() -> Value {
    Value::Map(vec![(Value::keyword("wide"), Value::Bool(true))])
}

pub fn self_hosted_views_both() -> Vec<(String, Value)> {
    let default = self_hosted_views()
        .into_iter()
        .map(|(focus, scene)| (focus.to_owned(), scene));
    let wide = self_hosted_views_with(&wide())
        .into_iter()
        .map(|(focus, scene)| (format!("wide {focus}"), scene));
    default.chain(wide).collect()
}

pub fn self_hosted_views_with(extra: &Value) -> Vec<(&'static str, Value)> {
    let doc = self_hosted_document();
    [
        "",
        ":companion",
        ":scene",
        ":viewer",
        ":ir",
        ":edn",
        ":main",
    ]
    .into_iter()
    .map(|focus| {
        let path: Vec<Value> = (!focus.is_empty())
            .then(|| Value::keyword(&focus[1..]))
            .into_iter()
            .collect();
        let scene = scene::document::compile_view(doc, &[], &path, extra);
        (focus, scene)
    })
    .collect()
}

fn self_hosted_document() -> &'static Value {
    static DOC: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    DOC.get_or_init(|| {
        let root = repo();
        ir::document(
            "examples/uml-viewer.policy.edn",
            root.to_str().expect("the repository path is UTF-8"),
        )
        .expect("the self-hosted policy compiles")
    })
}

pub fn read_edn(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    parse(&text).unwrap()
}

pub fn items(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Vector(v)) => v.clone(),
        _ => vec![],
    }
}

pub fn assert_golden_tolerance(golden: &Value, label: &str) {
    assert_eq!(
        Some(&Value::Float(TOLERANCE)),
        get(golden, "absolute-tolerance"),
        "{label}: :absolute-tolerance"
    );
}

pub fn assert_close(expected: &Value, actual: Option<&Value>, path: &str) {
    match expected {
        Value::Integer(_) | Value::Float(_) => {
            let a = edn::record::num(expected).unwrap();
            let Some(b) = actual.and_then(edn::record::num) else {
                panic!("{path}: expected {a}, actual missing or non-numeric");
            };
            assert!(b.is_finite(), "{path}: actual not finite ({b})");
            assert!(
                (a - b).abs() <= TOLERANCE,
                "{path}: expected {a} actual {b} (delta {})",
                (a - b).abs()
            );
        }
        Value::Map(entries) => {
            let Some(Value::Map(other)) = actual else {
                panic!("{path}: expected a map, got {actual:?}");
            };
            assert_eq!(entries.len(), other.len(), "{path}: map size");
            let actual = actual.unwrap();
            for (k, v) in entries {
                assert!(
                    other.iter().any(|(k2, _)| k2 == k),
                    "{path}: missing key {k:?}"
                );
                assert_close(v, actual.get(k), &format!("{path}{k:?}"));
            }
        }
        Value::Vector(items) => {
            let Some(Value::Vector(other)) = actual else {
                panic!("{path}: expected a vector, got {actual:?}");
            };
            assert_eq!(items.len(), other.len(), "{path}: vector length");
            for (i, v) in items.iter().enumerate() {
                assert_close(v, Some(&other[i]), &format!("{path}[{i}]"));
            }
        }
        _ => assert_eq!(Some(expected), actual, "{path}: scalar"),
    }
}
