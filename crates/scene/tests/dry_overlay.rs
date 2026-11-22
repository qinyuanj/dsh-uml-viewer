use edn::record::{get, local_name, num, seq};
use edn::{Value, parse};
use scene::Snapshot;

fn snap(path: &str, text: &str, mtime: i64) -> Snapshot {
    Snapshot::new(path, text, Value::Integer(mtime))
}

fn card(doc: &Value, files: &[Snapshot], id: &str) -> (Value, Option<Value>) {
    let state = scene::document::new_state(doc.clone(), None, None, None, files, None);
    let card = scene::class_card(&state, &Value::keyword(id)).expect("class is in the document");
    (card.class, card.dry)
}

fn op(class: &Value, name: &str) -> Value {
    seq(get(class, "ops"))
        .iter()
        .find(|o| get(o, "name").and_then(local_name) == Some(name))
        .unwrap_or_else(|| panic!("no op {name}"))
        .clone()
}

fn dry(class: &Value) -> Value {
    get(class, "dry")
        .cloned()
        .expect("class carries the marker")
}

fn state(class: &Value) -> String {
    get(&dry(class), "state")
        .and_then(local_name)
        .unwrap_or_default()
        .to_owned()
}

const DOC: &str = r#"{:hierarchical true :prefix "uml" :classes [
  {:id :scene.overlay :ns "uml.scene.overlay" :lang :rust :file "crates/scene/src/overlay.rs"
   :ops [{:name "paint" :text "paint"} {:name "load" :text "load"}]}
  {:id :scene.card :ns "uml.scene.card" :lang :rust :file "crates/scene/src/card.rs"
   :ops [{:name "class_card" :text "class_card"}]}
  {:id :scene.dry :ns "uml.scene.dry" :lang :rust :file "crates/scene/src/dry.rs"}]}"#;

const DRY: &str = r#"{:candidates [
  {:score 0.9
   :language "rust"
   :left {:file "crates/scene/src/overlay.rs" :start-line 10 :end-line 20 :namespace "scene::overlay" :name "paint"}
   :right {:file "crates/scene/src/card.rs" :start-line 30 :end-line 40 :namespace "scene::card" :name "class_card"}
   :left-nodes 10
   :right-nodes 10}
  {:score 0.82
   :language "rust"
   :left {:file "crates/scene/src/overlay.rs" :start-line 50 :end-line 60 :namespace "scene::overlay" :name "load"}
   :right {:file "crates/scene/tests/thing.rs" :start-line 1 :end-line 9 :namespace "scene::tests::thing" :name "t"}
   :left-nodes 5
   :right-nodes 5}]}"#;

#[test]
fn duplicate_structure_reaches_the_class_record_with_its_three_states() {
    let doc = parse(DOC).unwrap();
    let (overlay, document_summary) =
        card(&doc, &[snap(".metrics/dry.edn", DRY, 1)], "scene.overlay");

    assert_eq!("overlap", state(&overlay));
    let marker = dry(&overlay);
    assert_eq!(Some(2.0), num(get(&marker, "functions").unwrap()));
    assert_eq!(Some(2.0), num(get(&marker, "pairs").unwrap()));
    let weakest = num(get(&marker, "weakest").unwrap()).unwrap();
    assert!((weakest - 0.82).abs() < 1e-9, "{weakest}");
    assert_eq!(
        vec![Value::keyword("scene.card")],
        seq(get(&marker, "others"))
    );
    assert_eq!(Some(1.0), num(get(&marker, "unattributed").unwrap()));

    assert!(get(&overlay, "crap").is_none());
    assert!(get(&overlay, "cc").is_none());
    assert!(get(&overlay, "coverage").is_none());

    let paint = dry(&op(&overlay, "paint"));
    let pair = &seq(get(&paint, "pairs"))[0];
    assert_eq!(Some(0.9), num(get(pair, "score").unwrap()));
    assert_eq!(Some(&Value::keyword("scene.card")), get(pair, "class"));
    assert_eq!(
        Some(&Value::String("crates/scene/src/card.rs".into())),
        get(pair, "file")
    );
    assert_eq!(Some(30.0), num(get(pair, "start-line").unwrap()));
    assert_eq!(Some(40.0), num(get(pair, "end-line").unwrap()));
    assert_eq!(Some(&Value::String("class_card".into())), get(pair, "name"));
    let load = dry(&op(&overlay, "load"));
    let pair = &seq(get(&load, "pairs"))[0];
    assert!(
        get(pair, "class").is_none(),
        "the counterpart is off the graph"
    );
    assert_eq!(
        Some(&Value::String("crates/scene/tests/thing.rs".into())),
        get(pair, "file")
    );

    let (card_class, _) = card(&doc, &[snap(".metrics/dry.edn", DRY, 1)], "scene.card");
    assert_eq!("overlap", state(&card_class));
    let (clean, _) = card(&doc, &[snap(".metrics/dry.edn", DRY, 1)], "scene.dry");
    assert_eq!("none", state(&clean));
    assert!(get(&dry(&clean), "functions").is_none());

    let (unmeasured, absent_summary) = card(&doc, &[], "scene.overlay");
    assert!(get(&unmeasured, "dry").is_none());
    assert!(absent_summary.is_none());

    let summary = document_summary.expect("the summary is there when a snapshot is");
    assert_eq!(Some(2.0), num(get(&summary, "candidates").unwrap()));
    assert_eq!(Some(1.0), num(get(&summary, "attributed").unwrap()));
    assert_eq!(Some(1.0), num(get(&summary, "unattributed").unwrap()));
    assert_eq!(Some(2.0), num(get(&summary, "clusters").unwrap()));
    assert_eq!(Some(4.0), num(get(&summary, "functions").unwrap()));

    let empty = snap(".metrics/dry.edn", "{:candidates []}", 1);
    let (none, empty_summary) = card(&doc, &[empty], "scene.overlay");
    assert_eq!("none", state(&none));
    let summary = empty_summary.unwrap();
    assert_eq!(Some(0.0), num(get(&summary, "candidates").unwrap()));
    assert_eq!(Some(0.0), num(get(&summary, "clusters").unwrap()));
}

#[test]
fn the_snapshot_summary_exposes_bounded_cluster_members() {
    let doc = parse(DOC).unwrap();
    let (_, summary) = card(&doc, &[snap(".metrics/dry.edn", DRY, 1)], "scene.overlay");
    let summary = summary.expect("the summary is there when a snapshot is");
    assert_eq!(Some(2.0), num(get(&summary, "shown-clusters").unwrap()));
    assert_eq!(
        Some(Value::Bool(false)),
        get(&summary, "truncated").cloned()
    );
    let clusters = seq(get(&summary, "cluster-list"));
    assert_eq!(2, clusters.len());
    let worst = &clusters[0];
    assert_eq!(Some(0.82), num(get(worst, "weakest").unwrap()));
    let members = seq(get(worst, "members"));
    assert_eq!(2, members.len());
    assert_eq!(
        Some(&Value::String("crates/scene/src/overlay.rs".into())),
        get(&members[0], "file")
    );
    assert_eq!(
        Some(&Value::keyword("scene.overlay")),
        get(&members[0], "class")
    );
    assert!(
        get(&members[1], "class").is_none(),
        "the other side joins no class, so it says so instead of guessing"
    );
    assert_eq!(
        vec![Value::keyword("scene.overlay")],
        seq(get(worst, "classes"))
    );
    let strong = &clusters[1];
    assert_eq!(Some(0.9), num(get(strong, "weakest").unwrap()));
    assert_eq!(
        vec![
            Value::keyword("scene.overlay"),
            Value::keyword("scene.card")
        ],
        seq(get(strong, "classes"))
    );
}

#[test]
fn counts_functions_rather_than_pairs_and_a_self_pair_once() {
    let doc = parse(
        r#"{:hierarchical true :prefix "uml" :classes [
             {:id :scene.a :ns "uml.scene.a" :lang :rust :file "src/a.rs"
              :ops [{:name "f1"} {:name "f2"} {:name "f3"}]}]}"#,
    )
    .unwrap();
    let pairs = r#"{:candidates [
      {:score 0.9 :left {:file "src/a.rs" :start-line 1 :end-line 4 :namespace "scene::a" :name "f1"}
       :right {:file "src/a.rs" :start-line 5 :end-line 8 :namespace "scene::a" :name "f2"}}
      {:score 0.8 :left {:file "src/a.rs" :start-line 5 :end-line 8 :namespace "scene::a" :name "f2"}
       :right {:file "src/a.rs" :start-line 9 :end-line 12 :namespace "scene::a" :name "f3"}}]}"#;
    let (class, _) = card(&doc, &[snap(".metrics/dry.edn", pairs, 1)], "scene.a");
    let marker = dry(&class);
    assert_eq!(Some(3.0), num(get(&marker, "functions").unwrap()));
    assert_eq!(Some(2.0), num(get(&marker, "pairs").unwrap()));
    assert_eq!(
        vec![Value::keyword("scene.a")],
        seq(get(&marker, "others")),
        "the class is its own counterpart, once"
    );
    let weakest = num(get(&marker, "weakest").unwrap()).unwrap();
    assert!((weakest - 0.8).abs() < 1e-9, "{weakest}");
    assert_eq!(2, seq(get(&dry(&op(&class, "f2")), "pairs")).len());
    assert_eq!(1, seq(get(&dry(&op(&class, "f1")), "pairs")).len());
}

#[test]
fn the_file_fallback_marks_the_class_and_refuses_an_ambiguous_file() {
    let doc = parse(
        r#"{:hierarchical true :prefix "uml" :classes [
             {:id :scene.a :ns "uml.scene.a" :lang :rust :file "src/a.rs" :ops [{:name "f1"}]}
             {:id :scene.one :ns "uml.scene.one" :lang :rust :file "src/shared.rs"}
             {:id :scene.two :ns "uml.scene.two" :lang :rust :file "src/shared.rs"}]}"#,
    )
    .unwrap();
    let stale = r#"{:candidates [
      {:score 0.95 :left {:file "src/a.rs" :start-line 1 :end-line 6}
       :right {:file "src/a.rs" :start-line 9 :end-line 14}}
      {:score 0.86 :left {:file "src/shared.rs" :start-line 1 :end-line 6}
       :right {:file "src/a.rs" :start-line 20 :end-line 25}}]}"#;
    let (class, summary) = card(&doc, &[snap(".metrics/dry.edn", stale, 1)], "scene.a");
    let marker = dry(&class);
    assert_eq!("overlap", state(&class));
    assert_eq!(Some(3.0), num(get(&marker, "functions").unwrap()));
    assert_eq!(Some(2.0), num(get(&marker, "pairs").unwrap()));
    assert_eq!(Some(1.0), num(get(&marker, "unattributed").unwrap()));
    assert_eq!(
        vec![Value::keyword("scene.a")],
        seq(get(&marker, "others")),
        "the shared file joins neither class, so the only counterpart is the class itself"
    );
    assert!(
        get(&op(&class, "f1"), "dry").is_none(),
        "no name, no member marker"
    );

    let summary = summary.unwrap();
    assert_eq!(Some(2.0), num(get(&summary, "candidates").unwrap()));
    assert_eq!(Some(1.0), num(get(&summary, "attributed").unwrap()));
    assert_eq!(Some(1.0), num(get(&summary, "unattributed").unwrap()));
}

#[test]
fn a_dry_snapshot_never_joins_the_mutation_data() {
    let doc =
        parse(r#"{:hierarchical true :classes [{:id :demo.config :ns "demo.config"}]}"#).unwrap();
    let (class, _) = card(
        &doc,
        &[
            snap(
                ".metrics/mutate/demo.config.edn",
                r#"{:namespace "demo.config" :forms [{:id "defn/pool" :killed 2 :survived 0 :sites 2}]}"#,
                1000,
            ),
            snap(
                ".metrics/dry.edn",
                r#"{:namespace "demo.config" :candidates []}"#,
                2000,
            ),
        ],
        "demo.config",
    );
    assert_eq!(Some(2.0), num(get(&class, "killed").unwrap()));
    assert_eq!(vec!["pool"], names(&class));
    assert_eq!("none", state(&class));
}

fn names(class: &Value) -> Vec<String> {
    seq(get(class, "ops"))
        .iter()
        .filter_map(|op| get(op, "name"))
        .map(|v| local_name(v).unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn foreign_and_missing_classes_are_not_measurement_objects() {
    let doc = parse(
        r#"{:hierarchical true :prefix "uml" :classes [
             {:id :scene.a :ns "uml.scene.a" :lang :rust :file "src/a.rs" :ops [{:name "f1"}]}
             {:id :eframe :name "eframe" :foreign true :file "src/foreign.rs"}
             {:id :scene.gone :ns "uml.scene.gone" :lang :rust :file "src/missing.rs" :missing true}
             {:id :scene.never :ns "uml.scene.never" :lang :rust :file "src/never.rs"}]}"#,
    )
    .unwrap();
    let pairs = r#"{:candidates [
      {:score 0.9 :left {:file "src/a.rs" :start-line 1 :end-line 5}
       :right {:file "src/foreign.rs" :start-line 1 :end-line 5}}
      {:score 0.88 :left {:file "src/a.rs" :start-line 9 :end-line 13}
       :right {:file "src/missing.rs" :start-line 1 :end-line 5}}
      {:score 0.86 :left {:file "src/never.rs" :start-line 1 :end-line 5}
       :right {:file "src/never.rs" :start-line 9 :end-line 13}}]}"#;
    let files = [snap(".metrics/dry.edn", pairs, 1)];
    let (foreign, summary) = card(&doc, &files, "eframe");
    assert!(
        get(&foreign, "dry").is_none(),
        "a foreign package is not measured"
    );
    let (missing, _) = card(&doc, &files, "scene.gone");
    assert!(
        get(&missing, "dry").is_none(),
        "a class with no source is not measured"
    );
    let (never, _) = card(&doc, &files, "scene.never");
    assert_eq!("overlap", state(&never));
    let summary = summary.expect("the summary is there whenever a snapshot is");
    assert_eq!(Some(3.0), num(get(&summary, "candidates").unwrap()));
    assert_eq!(Some(2.0), num(get(&summary, "unattributed").unwrap()));
}
