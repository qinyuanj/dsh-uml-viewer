use edn::{Value, parse};
use frame::detail;
use frame::strings::Lang;
use scene::Snapshot;

fn doc() -> Value {
    parse(
        r#"{:hierarchical true :prefix "uml" :classes [
             {:id :scene.a :ns "uml.scene.a" :lang :rust :file "src/a.rs"
              :ops [{:name "f1" :text "f1()"} {:name "f2" :text "f2()"}]}
             {:id :scene.b :ns "uml.scene.b" :lang :rust :file "src/b.rs"
              :ops [{:name "g" :text "g()"}]}]}"#,
    )
    .unwrap()
}

fn dry() -> Snapshot {
    Snapshot::new(
        ".metrics/dry.edn",
        r#"{:candidates [
             {:score 0.91 :left {:file "src/a.rs" :start-line 1 :end-line 5 :namespace "scene::a" :name "f1"}
              :right {:file "src/b.rs" :start-line 2 :end-line 6 :namespace "scene::b" :name "g"}}
             {:score 0.83 :left {:file "src/a.rs" :start-line 1 :end-line 5 :namespace "scene::a" :name "f1"}
              :right {:file "src/a.rs" :start-line 9 :end-line 13 :namespace "scene::a" :name "f2"}}]}"#,
        Value::Integer(1),
    )
}

fn state(snapshots: &[Snapshot]) -> scene::document::ViewState {
    scene::document::new_state(doc(), None, None, None, snapshots, None)
}

#[test]
fn the_class_card_shows_overlap_none_and_unmeasured_as_distinct_rows() {
    let with = state(&[dry()]);
    let model = detail::for_state(&with, &Value::keyword("scene.a")).unwrap();
    let rows = detail::rows(&model, Lang::En);
    let line = rows.iter().find(|row| row.kind == "dry").unwrap();
    assert_eq!(
        line.text, "Duplicates: 2 functions, 2 pairs, weakest 0.83",
        "the additive row carries numbers but no quality column"
    );
    assert!(line.cells.crap_s.is_none());
    let member = rows
        .iter()
        .find(|row| row.op_name.as_deref() == Some("f1"))
        .unwrap();
    assert!(member.text.contains("duplicate 0.83"));
    let plain = rows
        .iter()
        .find(|row| row.op_name.as_deref() == Some("f2"))
        .unwrap();
    assert!(plain.text.contains("duplicate 0.83"));

    let empty = state(&[Snapshot::new(
        ".metrics/dry.edn",
        "{:candidates []}",
        Value::Integer(1),
    )]);
    let model = detail::for_state(&empty, &Value::keyword("scene.a")).unwrap();
    let rows = detail::rows(&model, Lang::Zh);
    let line = rows.iter().find(|row| row.kind == "dry").unwrap();
    assert_eq!(line.text, "重复结构：已测无重复");

    let absent = state(&[]);
    let model = detail::for_state(&absent, &Value::keyword("scene.a")).unwrap();
    let rows = detail::rows(&model, Lang::En);
    let line = rows.iter().find(|row| row.kind == "dry").unwrap();
    assert_eq!(
        line.text, "Duplicates: unmeasured",
        "no snapshot says so, instead of leaving the class silent"
    );
}

#[test]
fn the_frame_carries_a_bounded_cluster_list_and_unattributed_count() {
    let with = state(&[dry()]);
    let frame = frame::frame::build(&with, 1200.0, 800.0, Lang::En);
    let json = frame.to_json();
    let dry = json["dry"].as_object().expect("dry payload");
    assert_eq!(dry["candidates"], 2);
    assert_eq!(dry["clusters"], 1);
    assert_eq!(dry["shownClusters"], 1);
    assert_eq!(dry["truncated"], false);
    let cluster = &dry["clusterList"][0];
    assert_eq!(cluster["weakest"], 0.83);
    assert_eq!(cluster["members"][0]["file"], "src/a.rs");
    assert_eq!(cluster["members"][0]["startLine"], 1);
    assert_eq!(cluster["members"][0]["endLine"], 5);
    assert_eq!(cluster["classes"][0], ":scene.a");
    assert_eq!(cluster["classes"][1], ":scene.b");

    let absent = state(&[]);
    let frame = frame::frame::build(&absent, 1200.0, 800.0, Lang::En);
    assert!(
        frame.to_json()["dry"].is_null(),
        "no snapshot is unmeasured"
    );
}
