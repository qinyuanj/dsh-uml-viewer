//! Go, Java and Clojure mutation sites and forms against the pinned original.
//!
//! `fixtures/mutate/sites/go-java-clojure.json` is mutator f772a83's own
//! `sites_in_file`, `form_spans` and `form_digests` output
//! (scripts/mutate_goldens_go_java_clojure.py) for a corpus built from its
//! `test_sites.py` cases plus edge cases (Clojure reader macros, metadata,
//! malformed forms, Go and Java operators), and for the three mutation fixture
//! projects. Every field is compared: byte offsets, original and mutant text,
//! category, owner, form id and mutation id.
use quality_core::serde_json::{self, Value};
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate")
}

fn base(label: &str) -> PathBuf {
    match label {
        "gjc" => fixture().join("sites").join("gjc"),
        language => fixture().join(language).join("project"),
    }
}

fn inventory() -> serde_json::Map<String, Value> {
    let text = std::fs::read_to_string(fixture().join("sites/go-java-clojure.json")).unwrap();
    match serde_json::from_str(&text).unwrap() {
        Value::Object(map) => map,
        _ => panic!("go-java-clojure.json is an object"),
    }
}

fn discovered(label: &str, relative: &str) -> (Value, Value) {
    let root = base(label);
    let path = root.join(relative);
    // The original reads the bytes as UTF-8 without newline translation.
    let source = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
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
fn go_java_and_clojure_sites_and_forms_match_the_pinned_original() {
    let inventory = inventory();
    // Exact: a lost file or site must fail.
    assert_eq!(inventory.len(), FILES);
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
    assert_eq!(sites, SITES);
    assert_eq!(
        languages.len(),
        4,
        "go, java, clj and cljc are all represented"
    );
}

/// The reader against the grammar on real code: `MUTATOR_CLOJURE_CORPUS` is a
/// directory of Clojure sources and `MUTATOR_CLOJURE_RAW` the original's
/// `discover_raw` for each (`scripts/mutate_goldens_go_java_clojure.py
/// --clojure-corpus DIR --raw-output FILE`).
#[test]
#[ignore = "needs an external corpus: set MUTATOR_CLOJURE_CORPUS and MUTATOR_CLOJURE_RAW"]
fn clojure_reader_matches_the_grammar_on_an_external_corpus() {
    let corpus = PathBuf::from(std::env::var_os("MUTATOR_CLOJURE_CORPUS").unwrap());
    let raw = std::fs::read_to_string(std::env::var_os("MUTATOR_CLOJURE_RAW").unwrap()).unwrap();
    let Value::Object(expected) = serde_json::from_str(&raw).unwrap() else {
        panic!("the raw sites are an object");
    };
    let mut differ = Vec::new();
    let mut total = 0;
    for (relative, sites) in &expected {
        let path = corpus.join(relative);
        let source = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        let actual: Vec<Value> =
            mutator::sites::raw_sites(quality_core::language::Language::Clojure, &source, &path)
                .into_iter()
                .map(|(line, start, end, original, mutant, category)| {
                    serde_json::json!([line, start, end, original, mutant, category])
                })
                .collect();
        total += actual.len();
        if &Value::Array(actual) != sites {
            differ.push(relative.clone());
        }
    }
    eprintln!("{} files, {total} sites", expected.len());
    assert!(differ.is_empty(), "sites differ in {differ:?}");
}

const FILES: usize = 21;
const SITES: usize = 288;
