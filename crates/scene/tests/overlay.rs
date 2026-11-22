use edn::record::{get, local_name, num};
use edn::{Value, parse};
use scene::Snapshot;

fn snap(path: &str, text: &str, mtime: Value) -> Snapshot {
    Snapshot::new(path, text, mtime)
}

fn classes(doc: &Value) -> Vec<Value> {
    match get(doc, "classes") {
        Some(Value::Vector(v)) => v.clone(),
        _ => vec![],
    }
}

fn overlaid(doc: &Value, files: &[Snapshot]) -> Vec<Value> {
    let state = scene::document::new_state(doc.clone(), None, None, None, files, None);
    classes(doc)
        .iter()
        .map(|c| {
            scene::class_card(&state, get(c, "id").unwrap())
                .unwrap()
                .class
        })
        .collect()
}

fn names(c: &Value) -> Vec<String> {
    match get(c, "ops") {
        Some(Value::Vector(ops)) => ops
            .iter()
            .filter_map(|op| get(op, "name"))
            .map(|v| local_name(v).unwrap_or_default().to_owned())
            .collect(),
        _ => vec![],
    }
}

#[test]
fn joins_typescript_modules_and_rust_crate_metrics_without_leaking() {
    let doc = parse(r#"{:hierarchical true :prefix "bookwriter" :classes [{:id :model :ns "bookwriter.model" :lang :typescript} {:id :pdf :ns "bookwriter.pdf" :lang :typescript} {:id :rust :ns "bookwriter.rust" :lang :rust :file "src-tauri/src/lib.rs"} {:id :rust.main :ns "bookwriter.rust.main" :lang :rust :file "src-tauri/src/main.rs"}]}"#).unwrap();
    let cs = overlaid(
        &doc,
        &[
            snap(
                ".metrics/crap.edn",
                r#"{:entries [
                    {:namespace "model" :name "slugify" :complexity 2 :coverage 100 :crap 2}
                    {:namespace "pdf" :name "styled" :complexity 4 :coverage 100 :crap 4}
                    {:namespace "pdf.Layout" :name "figure" :complexity 8 :coverage 100 :crap 8}
                    {:namespace "bookwriter" :name "read_text" :complexity 1 :coverage 100 :crap 1}
                    {:namespace "bookwriter" :name "main" :complexity 1 :coverage 0 :crap 2}
                    {:namespace "bookwriter::build" :name "main" :complexity 1 :coverage nil :crap nil}]}"#,
                Value::Integer(1),
            ),
            snap(
                ".metrics/mutate/model.edn",
                r#"{:namespace "model" :forms [{:id "defn/slugify" :killed 1 :survived 0 :uncovered 0 :sites 1}]}"#,
                Value::Integer(1),
            ),
        ],
    );
    assert_eq!(Some(2.0), num(get(&cs[0], "cc").unwrap()));
    assert_eq!(Some(1.0), num(get(&cs[0], "killed").unwrap()));
    assert_eq!(Some(1.0), num(get(&cs[0], "sites").unwrap()));
    assert_eq!(vec!["slugify"], names(&cs[0]));
    assert_eq!(Some(12.0), num(get(&cs[1], "cc").unwrap()));
    assert_eq!(vec!["styled", "figure"], names(&cs[1]));
    assert_eq!(Some(2.0), num(get(&cs[2], "cc").unwrap()));
    assert_eq!(vec!["read_text", "main"], names(&cs[2]));
    assert!(get(&cs[3], "crap").is_none());
    assert_eq!(Vec::<String>::new(), names(&cs[3]));
}

#[test]
fn newer_mutation_snapshot_wins_over_older_explicit_or_derived() {
    let doc = parse(r#"{:hierarchical true :classes [{:id :config :ns "demo.config"}]}"#).unwrap();
    for (worse_at, pool_at, killed, op) in [(2000, 1000, 1.0, "worse"), (2000, 3000, 2.0, "pool")] {
        let cs = overlaid(
            &doc,
            &[
                snap(
                    ".metrics/mutate/demo.config.edn",
                    r#"{:namespace "demo.config" :forms [{:id "defn/worse" :killed 1 :survived 9}]}"#,
                    Value::Integer(worse_at),
                ),
                snap(
                    ".metrics/mutate/demo/config.edn",
                    r#"{:source "src/demo/config.lua" :forms [{:id "defn/pool" :killed 2 :survived 0}]}"#,
                    Value::Integer(pool_at),
                ),
            ],
        );
        assert_eq!(Some(killed), num(get(&cs[0], "killed").unwrap()));
        assert_eq!(vec![op], names(&cs[0]));
    }
}

#[test]
fn derives_a_lua_namespace_with_underscores_from_its_source_path() {
    let doc =
        parse(r#"{:hierarchical true :classes [{:id :config :ns "demo.my_config"}]}"#).unwrap();
    let cs = overlaid(
        &doc,
        &[snap(
            ".metrics/mutate/config.edn",
            r#"{:source "src/demo/my_config.lua" :forms [{:id "defn/pool" :killed 2 :survived 0}]}"#,
            Value::Integer(1),
        )],
    );
    assert_eq!(Some(2.0), num(get(&cs[0], "killed").unwrap()));
    assert_eq!(vec!["pool"], names(&cs[0]));
}

#[test]
fn paints_private_forms_and_percent_coverage_onto_lua_snapshots() {
    let doc = parse(r#"{:hierarchical true :classes [{:id :calc :ns "calc.util" :ops [{:name "M.clamp" :text "authored signature"}]}]}"#).unwrap();
    let cs = overlaid(
        &doc,
        &[
            snap(
                ".metrics/crap.edn",
                r#"{:entries [{:namespace "calc.util" :name "M.clamp" :complexity 2 :coverage 83.3333 :crap 3}]}"#,
                Value::Integer(1),
            ),
            snap(
                ".metrics/mutate/calc.edn",
                r#"{:namespace "calc.util" :forms [{:id "defn/M.clamp" :killed 4 :survived 1 :uncovered 2} {:id "defn-/between" :killed 1 :survived 0 :sites 0}]}"#,
                Value::Integer(1),
            ),
        ],
    );
    let c = &cs[0];
    assert_eq!(Some(3.0), num(get(get(c, "crap").unwrap(), "mu").unwrap()));
    assert!((num(get(c, "coverage").unwrap()).unwrap() - 0.833333).abs() < 1e-9);
    assert_eq!(Some(5.0), num(get(c, "killed").unwrap()));
    assert_eq!(Some(1.0), num(get(c, "survived").unwrap()));
    assert_eq!(Some(7.0), num(get(c, "sites").unwrap()));
    let ops = match get(c, "ops").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    assert_eq!(
        Some(&Value::String("authored signature".into())),
        get(&ops[0], "text")
    );
    assert_eq!(Some(&Value::Bool(true)), get(&ops[1], "private"));
    assert_eq!(Some(0.0), num(get(&ops[1], "sites").unwrap()));
}

#[test]
fn assigns_rust_children_to_nearest_id_while_exact_namespace_wins() {
    let doc = parse(r#"{:hierarchical true :prefix :crate :classes [{:id :root :ns "root" :lang :rust} {:id :model :ns "my_model"} {:id :model.deep :ns "deep"} {:id :other :ns "model.deep.Exact"}]}"#).unwrap();
    let cs = overlaid(
        &doc,
        &[snap(
            ".metrics/crap.edn",
            r#"{:entries [{:namespace "my_model" :name "a" :complexity 1 :crap 1} {:namespace "model" :name "b" :complexity 2 :crap 2} {:namespace "model::deep::Child" :name "c" :complexity 3 :crap 3} {:namespace "model.deep.Exact" :name "d" :complexity 4 :crap 4} {:namespace "crate" :name "e" :complexity 5 :crap 5}]}"#,
            Value::Integer(1),
        )],
    );
    let ccs: Vec<f64> = cs
        .iter()
        .map(|c| num(get(c, "cc").unwrap()).unwrap())
        .collect();
    assert_eq!(vec![5.0, 3.0, 3.0, 4.0], ccs);
}

#[test]
fn newest_duplicate_namespace_wins_and_authored_fallback_is_kept() {
    let doc = parse(r#"{:hierarchical true :classes [{:id :calc :ns "calc.util" :ops [{:name "M.clamp" :text "authored signature"}]}]}"#).unwrap();
    let cs = overlaid(
        &doc,
        &[
            snap(
                ".metrics/mutate/new.edn",
                r#"{:source "src/ignored.lua" :namespace "calc.util" :forms [{:id "defn/f" :killed 7}]}"#,
                Value::String("10".into()),
            ),
            snap(
                ".metrics/mutate/old.edn",
                r#"{:namespace "calc.util" :forms [{:id "defn/f" :killed 1}]}"#,
                Value::String("9".into()),
            ),
        ],
    );
    assert_eq!(Some(7.0), num(get(&cs[0], "killed").unwrap()));
    let doc2 = parse(r#"{:hierarchical true :classes [{:id :a :ns "my_module"}]}"#).unwrap();
    let cs = overlaid(
        &doc2,
        &[snap(
            ".metrics/mutate/a.edn",
            r#"{:namespace "my_module" :source "src/wrong.lua" :forms [{:id "defn/f" :killed 2}]}"#,
            Value::Integer(1),
        )],
    );
    assert_eq!(Some(2.0), num(get(&cs[0], "killed").unwrap()));
}

#[test]
fn foreign_and_oval_classes_are_never_painted() {
    let doc = scene::ir::read_document(r#"{:diagrams [{:packages [{:label "P" :classes [{:id :a :ns "a" :crap 2 :killed 1 :survived 0} {:id :b :ns "b" :shape :oval}]}]} {:packages [{:label "Q" :classes [{:id :c :ns "c"}]}]}]}"#).unwrap();
    let cs = classes(&scene::document::compile_view(
        &doc,
        &[snap(
            ".metrics/crap.edn",
            r#"{:entries [{:namespace "b" :name "f" :complexity 5 :crap 5}]}"#,
            Value::Integer(1),
        )],
        &[],
        &Value::Map(vec![]),
    ));
    assert_eq!(
        Some(2.0),
        num(get(get(&cs[0], "crap").unwrap(), "mu").unwrap())
    );
    assert!(get(&cs[1], "crap").is_none());
    let raw = parse(r#"{:hierarchical true :classes [{:id :a}]}"#).unwrap();
    assert_eq!(classes(&raw), overlaid(&raw, &[]));
}
