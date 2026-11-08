//! Python and TypeScript mutation sites and forms against the pinned original.
//!
//! `fixtures/mutate/sites/sites.json` is mutator f772a83's own `sites_in_file`,
//! `form_spans` and `form_digests` output (scripts/mutate_goldens_python_ts.py)
//! for a corpus built from its `test_sites.py` cases plus edge cases, and for
//! the two mutation fixture projects. Every field is compared: byte offsets,
//! original and mutant text, category, owner, form id and mutation id.
use quality_core::serde_json::{self, Value};
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate")
}

fn base(label: &str) -> PathBuf {
    match label {
        "sites" => fixture().join("sites"),
        language => fixture().join(language).join("project"),
    }
}

fn inventory() -> serde_json::Map<String, Value> {
    let text = std::fs::read_to_string(fixture().join("sites/sites.json")).unwrap();
    match serde_json::from_str(&text).unwrap() {
        Value::Object(map) => map,
        _ => panic!("sites.json is an object"),
    }
}

fn discovered(label: &str, relative: &str) -> (Value, Value) {
    let root = base(label);
    let path = root.join(relative);
    let source = std::fs::read_to_string(&path).unwrap();
    let (sites, mut forms) = mutator::sites::discover(&source, &path, &root, relative).unwrap();
    let sites: Vec<Value> = sites
        .iter()
        .map(|site| {
            serde_json::json!({
                "file": site.file, "namespace": site.namespace, "form_id": site.form_id,
                "name": site.name, "line": site.line, "start": site.start, "end": site.end,
                "original": site.original, "mutant": site.mutant, "category": site.category,
                "mutation_id": site.mutation_id,
            })
        })
        .collect();
    forms.sort_by(|a, b| (a.line, &a.id).cmp(&(b.line, &b.id)));
    let forms: Vec<Value> = forms
        .iter()
        .map(|form| {
            serde_json::json!({
                "namespace": form.namespace, "id": form.id, "line": form.line,
                "end_line": form.end_line, "name": form.name, "private": form.private,
                "hash": form.digest,
            })
        })
        .collect();
    (Value::Array(sites), Value::Array(forms))
}

fn sorted_forms(forms: &Value) -> Value {
    let mut forms = forms.as_array().unwrap().clone();
    forms.sort_by_key(|form| {
        (
            form["line"].as_u64(),
            form["id"].as_str().map(str::to_owned),
        )
    });
    Value::Array(forms)
}

#[test]
fn python_and_typescript_sites_and_forms_match_the_pinned_original() {
    let inventory = inventory();
    // Exact: the corpus is 30 files and 176 sites; a lost file must fail.
    assert_eq!(inventory.len(), 30);
    let mut sites = 0;
    let mut languages = std::collections::BTreeSet::new();
    for (key, expected) in &inventory {
        let (label, relative) = key.split_once('/').unwrap();
        languages.insert(Path::new(relative).extension().unwrap().to_owned());
        let (actual_sites, actual_forms) = discovered(label, relative);
        assert_eq!(actual_sites, expected["sites"], "sites of {key}");
        assert_eq!(
            actual_forms,
            sorted_forms(&expected["forms"]),
            "forms of {key}"
        );
        sites += expected["sites"].as_array().unwrap().len();
    }
    assert_eq!(sites, 176);
    assert_eq!(languages.len(), 3, "py, ts and tsx are all represented");
}

/// Ported from the original `test_sites.py` assertions that state an exact
/// expectation independent of the implementation's tables.
#[test]
fn original_site_cases_hold_for_python_and_typescript() {
    let pairs = |relative: &str| -> Vec<(String, String, String)> {
        let (sites, _) = discovered("sites", relative);
        sites
            .as_array()
            .unwrap()
            .iter()
            .map(|site| {
                (
                    site["original"].as_str().unwrap().to_owned(),
                    site["mutant"].as_str().unwrap().to_owned(),
                    site["category"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };
    let has = |relative: &str, original: &str, mutant: &str| {
        pairs(relative)
            .iter()
            .any(|(o, m, _)| o == original && m == mutant)
    };
    // test_python_typescript_and_rust_use_the_java_decisions
    assert!(has("src/demo/app.py", "and", "or"));
    assert!(has("src/demo/app.py", "not", ""));
    assert!(!has("src/demo/app.py", "True", "False"));
    assert!(has("src/ts/app.ts", "===", "!=="));
    assert!(has("src/ts/app.ts", "&&", "||"));
    // test_bitwise_shift_and_remainder_sites
    let bitwise: Vec<_> = [
        ("^", "|", "bitwise"),
        ("&", "|", "bitwise"),
        ("|", "&", "bitwise"),
        ("<<", ">>", "bitwise"),
        (">>", "<<", "bitwise"),
        ("%", "*", "arithmetic"),
    ]
    .iter()
    .map(|(o, m, c)| (o.to_string(), m.to_string(), c.to_string()))
    .collect();
    assert_eq!(pairs("src/demo/bitwise.py"), bitwise);
    assert_eq!(pairs("src/ts/bitwise.ts"), bitwise);
    // test_typescript_unsigned_shift_has_a_single_site, for .ts and .tsx
    for relative in ["src/ts/shift.ts", "src/ts/shift.tsx"] {
        let shift = vec![(">>>".into(), "<<".into(), "bitwise".into())];
        assert_eq!(pairs(relative), shift, "{relative}");
        let root = base("sites");
        let source = std::fs::read_to_string(root.join(relative)).unwrap();
        let (sites, _) =
            mutator::sites::discover(&source, &root.join(relative), &root, relative).unwrap();
        assert_eq!(
            mutator::sites::apply_site(&source, &sites[0]),
            "export function mix(a: number, b: number) { return a << b; }\n"
        );
    }
    // Compound assignments, strings, comments and type unions are not sites.
    for relative in [
        "src/demo/assign.py",
        "src/ts/assign.ts",
        "src/demo/strings.py",
        "src/demo/annotation.py",
        "src/ts/union.ts",
        "src/demo/identity.py",
        "src/ts/identity.ts",
    ] {
        assert_eq!(pairs(relative), Vec::new(), "{relative}");
    }
    // test_python_union_annotation_does_not_hide_runtime_bitwise_site
    assert_eq!(
        pairs("src/demo/union_runtime.py"),
        vec![("|".into(), "&".into(), "bitwise".into())]
    );
    // test_new_operator_offsets_are_utf8_bytes
    let root = base("sites");
    let source = std::fs::read_to_string(root.join("src/demo/utf8.py")).unwrap();
    let (sites, _) = mutator::sites::discover(
        &source,
        &root.join("src/demo/utf8.py"),
        &root,
        "src/demo/utf8.py",
    )
    .unwrap();
    assert_eq!(sites.len(), 1);
    assert_eq!(
        mutator::sites::apply_site(&source, &sites[0]),
        "def mask(a, b):\n    label = \"雪\"\n    return a | b\n"
    );
    // test_python_private_and_class_namespace
    let (board, _) = discovered("sites", "src/demo/board.py");
    let board = board.as_array().unwrap();
    assert!(
        board
            .iter()
            .all(|site| site["namespace"] == "demo.board.Board")
    );
    assert!(
        board
            .iter()
            .any(|site| site["name"] == "_hide" && site["form_id"] == "defn-/_hide")
    );
    assert!(
        board
            .iter()
            .any(|site| site["name"] == "place" && site["form_id"] == "defn/place")
    );
    // test_literal_returns_use_existing_sites_without_body_replacement
    let literal = |relative: &str| -> Vec<(String, String)> {
        pairs(relative)
            .into_iter()
            .map(|(o, m, _)| (o, m))
            .collect()
    };
    assert_eq!(
        literal("src/demo/literals.py"),
        [("0", "1"), ("1", "0"), ("True", "False"), ("False", "True")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
    );
    assert_eq!(
        literal("src/ts/literals.ts"),
        [("0", "1"), ("1", "0"), ("true", "false"), ("false", "true")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
    );
}
