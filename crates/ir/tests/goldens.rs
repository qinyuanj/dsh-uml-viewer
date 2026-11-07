//! Golden parity (#136, Seam A): for each fixture project the `ir` output is
//! structurally equal to the frozen JVM golden, and byte-identical across
//! repeated runs. Comparison parses both sides with the same EDN codec, so
//! golden pretty-printing and key order do not matter.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn generate(fixture: &str, output: &Path) -> std::process::ExitCode {
    let root = repo().join(fixture);
    let args: Vec<OsString> = [
        "--root",
        root.to_str().unwrap(),
        "--policy",
        "policy.edn",
        "--output",
        output.to_str().unwrap(),
    ]
    .iter()
    .map(OsString::from)
    .collect();
    ir::run(&args)
}

/// A private output file, removed when dropped. Parallel test runs (mutation
/// workers) share the temp directory, so a fixed name would let one run read
/// another's output.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "uml-ir-test-{tag}-{}-{}.edn",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn parsed(path: &Path) -> edn::Value {
    edn::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn lua_fixture_matches_the_jvm_golden() {
    let scratch = Scratch::new("lua");
    let output = &scratch.0;
    assert_eq!(
        generate("lua-fixture", output),
        std::process::ExitCode::SUCCESS
    );
    assert_eq!(
        parsed(output),
        parsed(&repo().join("fixtures/jvm-goldens/lua.ir.edn"))
    );
}

fn items(document: &edn::Value, key: &str) -> Vec<edn::Value> {
    match document.get(&edn::Value::keyword(key)) {
        Some(edn::Value::Vector(items)) => items.clone(),
        other => panic!("{key}: {other:?}"),
    }
}

fn touches(item: &edn::Value, ids: &[edn::Value]) -> bool {
    ["id", "from", "to"]
        .iter()
        .any(|k| matches!(item.get(&edn::Value::keyword(k)), Some(v) if ids.contains(v)))
}

/// The document with `key` replaced by its items for which `keep` holds.
fn filtered(document: &edn::Value, key: &str, keep: impl Fn(&edn::Value) -> bool) -> edn::Value {
    let edn::Value::Map(entries) = document else {
        panic!("not a map");
    };
    let kept: Vec<edn::Value> = items(document, key).into_iter().filter(keep).collect();
    edn::Value::Map(
        entries
            .iter()
            .map(|(k, v)| {
                if *k == edn::Value::keyword(key) {
                    (k.clone(), edn::Value::Vector(kept.clone()))
                } else {
                    (k.clone(), v.clone())
                }
            })
            .collect(),
    )
}

#[test]
fn rust_fixture_matches_the_jvm_golden() {
    // The JVM oracle predates the platform modules (#201 story 7) and also
    // feeds the frozen scene geometry, so it stays untouched: the platform
    // modules are taken out here and pinned literally below.
    let scratch = Scratch::new("rust");
    let output = &scratch.0;
    assert_eq!(
        generate("rust-fixture", output),
        std::process::ExitCode::SUCCESS
    );
    let platform = [
        edn::parse(":rust.console").unwrap(),
        edn::parse(":rust.signals").unwrap(),
    ];
    // Past eight edges the IR keeps Clojure's hash-map order (see
    // `clojure_group_by_order_holds_past_the_array_map_boundary`), so the two
    // extra edges reorder the vector: edges compare as a multiset here, and
    // `fixtures/ir-bytes/rust-fixture.edn` pins their exact order.
    let document = parsed(output);
    let golden = parsed(&repo().join("fixtures/jvm-goldens/rust.ir.edn"));
    let edges: Vec<edn::Value> = items(&document, "edges")
        .into_iter()
        .filter(|e| !touches(e, &platform))
        .collect();
    assert_same_items(&items(&golden, "edges"), &edges);
    let document = filtered(&document, "classes", |c| !touches(c, &platform));
    let document = filtered(&document, "edges", |_| false);
    let golden = filtered(&golden, "edges", |_| false);
    assert_eq!(golden, document);
}

fn assert_same_items(want: &[edn::Value], got: &[edn::Value]) {
    let mut rest = got.to_vec();
    for item in want {
        let at = rest
            .iter()
            .position(|g| g == item)
            .unwrap_or_else(|| panic!("missing {item:?} in {got:?}"));
        rest.remove(at);
    }
    assert!(rest.is_empty(), "unexpected {rest:?}");
}

#[test]
fn rust_cfg_platform_modules_are_classes_on_every_host() {
    // `#[cfg(windows)]` and `#[cfg(unix)]` modules are source, not build
    // configuration: both are classes whichever host generates the IR.
    let scratch = Scratch::new("rust-cfg");
    let output = &scratch.0;
    assert_eq!(
        generate("rust-fixture", output),
        std::process::ExitCode::SUCCESS
    );
    let platform = [
        edn::parse(":rust.console").unwrap(),
        edn::parse(":rust.signals").unwrap(),
    ];
    let document = parsed(output);
    let classes: Vec<edn::Value> = items(&document, "classes")
        .into_iter()
        .filter(|c| touches(c, &platform))
        .collect();
    let edges: Vec<edn::Value> = items(&document, "edges")
        .into_iter()
        .filter(|e| touches(e, &platform))
        .collect();
    assert_eq!(
        edn::parse(
            "[{:ns \"bookwriter.rust.console\" :name \"console.rs\" :file \"src/console.rs\"
               :ops [{:name \"attach\" :text \"attach\"}] :lang :rust :id :rust.console}
              {:ns \"bookwriter.rust.signals\" :name \"signals.rs\" :file \"src/signals.rs\"
               :ops [{:name \"install\" :text \"install\"}] :lang :rust :id :rust.signals}]"
        )
        .unwrap(),
        edn::Value::Vector(classes)
    );
    let edn::Value::Vector(want) = edn::parse(
        "[{:from :rust :to :rust.console :kind :dependency}
          {:from :rust :to :rust.signals :kind :dependency}
          {:from :rust.console :to :std :kind :dependency}
          {:from :rust.signals :to :std :kind :dependency}]",
    )
    .unwrap() else {
        unreachable!()
    };
    assert_same_items(&want, &edges);
}

#[test]
fn typescript_fixture_matches_the_jvm_golden() {
    let scratch = Scratch::new("typescript");
    let output = &scratch.0;
    assert_eq!(
        generate("typescript-fixture", output),
        std::process::ExitCode::SUCCESS
    );
    assert_eq!(
        parsed(output),
        parsed(&repo().join("fixtures/jvm-goldens/typescript.ir.edn"))
    );
}

#[test]
fn python_fixture_matches_the_jvm_golden() {
    let scratch = Scratch::new("python");
    let output = &scratch.0;
    assert_eq!(
        generate("python-fixture", output),
        std::process::ExitCode::SUCCESS
    );
    assert_eq!(
        parsed(output),
        parsed(&repo().join("fixtures/jvm-goldens/python.ir.edn"))
    );
}

#[test]
fn identical_input_produces_byte_identical_output() {
    let scratch = Scratch::new("stable");
    let output = &scratch.0;
    let mut runs = std::collections::HashSet::new();
    for _ in 0..4 {
        assert_eq!(
            generate("lua-fixture", output),
            std::process::ExitCode::SUCCESS
        );
        runs.insert(std::fs::read(output).unwrap());
    }
    assert_eq!(1, runs.len(), "IR bytes changed between identical runs");
}

#[test]
fn lua_namespaces_match_the_crapper_and_mutator_naming() {
    // The quality overlay joins snapshots by the class :ns, and crapper and
    // mutator name a module by its project-relative path. For the fixture
    // (:src "src" :prefix "calc") the scanned namespaces must equal the
    // snapshot namespaces exactly.
    let scratch = Scratch::new("lua-ns");
    let output = &scratch.0;
    assert_eq!(
        generate("lua-fixture", output),
        std::process::ExitCode::SUCCESS
    );
    let document = parsed(output);
    let namespaces: Vec<&str> = document
        .get(&edn::Value::keyword("classes"))
        .and_then(|c| match c {
            edn::Value::Vector(items) => Some(items),
            _ => None,
        })
        .unwrap()
        .iter()
        .filter_map(|class| match class.get(&edn::Value::keyword("ns")) {
            Some(edn::Value::String(ns)) => Some(ns.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(vec!["calc", "calc.util"], namespaces);
    let crap = parsed(&repo().join("lua-fixture/.metrics/crap.edn"));
    let snapshot_namespaces: std::collections::HashSet<&str> = crap
        .get(&edn::Value::keyword("entries"))
        .and_then(|e| match e {
            edn::Value::Vector(items) => Some(items),
            _ => None,
        })
        .unwrap()
        .iter()
        .filter_map(|entry| match entry.get(&edn::Value::keyword("namespace")) {
            Some(edn::Value::String(ns)) => Some(ns.as_str()),
            _ => None,
        })
        .collect();
    for ns in &namespaces {
        assert!(
            snapshot_namespaces.contains(ns),
            "class :ns {ns} missing from the crapper snapshot"
        );
    }
}
