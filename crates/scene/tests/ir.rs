use edn::record::get;
use edn::{Value, parse};
use scene::ir::{normalize, read_document};

#[test]
fn rejects_duplicate_class_ids_unknown_endpoints_and_bad_ranges() {
    for text in [
        "{:packages [{:label \"P\" :classes [{:id :a} {:id :a}]}]}",
        "{:packages [{:label \"P\" :classes [{:id :a}]}] :edges [{:from :a :to :absent}]}",
        "{:packages [{:label \"P\" :classes [{:id :a :coverage 2}]}]}",
    ] {
        assert!(read_document(text).is_err(), "{text} must fail");
    }
}

#[test]
fn golden_ir_files_load_without_loss_or_duplication() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/jvm-goldens");
    for lang in ["lua", "typescript", "rust", "python"] {
        let path = dir.join(format!("{lang}.ir.edn"));
        let text = std::fs::read_to_string(&path).unwrap();
        let raw = parse(&text).unwrap();
        assert_eq!(raw, read_document(&text).unwrap(), "{lang}");
        let classes = edn::record::seq(get(&raw, "classes"));
        let edges = edn::record::seq(get(&raw, "edges"));
        assert!(!classes.is_empty() && !edges.is_empty(), "{lang}");
        let round = parse(&edn::encode(&raw).unwrap()).unwrap();
        assert_eq!(raw, round, "{lang} round trip");
    }
}

#[test]
fn hierarchical_documents_pass_through_unchanged() {
    let raw = parse("{:hierarchical true :classes [] :edges []}").unwrap();
    assert_eq!(
        raw,
        read_document("{:hierarchical true :classes [] :edges []}").unwrap()
    );
}

#[test]
fn string_members_become_text_records_and_private_ops_keep_the_flag() {
    let d = read_document(
        "{:packages [{:label \"P\" :classes [{:id :a :ops [\"public\" {:text \"secret\" :private true}]}]}]}",
    )
    .unwrap();
    let diagrams = match get(&d, "diagrams").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    let packages = match get(&diagrams[0], "packages").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    let classes = match get(&packages[0], "classes").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    let ops = match get(&classes[0], "ops").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    assert_eq!(Some(&Value::String("public".into())), get(&ops[0], "text"));
    assert_eq!(Some(&Value::Bool(true)), get(&ops[1], "private"));
}

#[test]
fn string_ids_normalize_to_dashed_lowercase_keywords() {
    let d =
        normalize(&parse("{:packages [{:label \"P\" :classes [{:id \"My Class\"}]}]}").unwrap())
            .unwrap();
    let packages = match get(&d, "packages").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    let classes = match get(&packages[0], "classes").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    assert_eq!(Some(&Value::keyword("my-class")), get(&classes[0], "id"));
}

#[test]
fn numeric_crap_becomes_a_summary_map() {
    let d = normalize(&parse("{:packages [{:label \"P\" :classes [{:id :a :crap 3}]}]}").unwrap())
        .unwrap();
    let packages = match get(&d, "packages").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    let classes = match get(&packages[0], "classes").unwrap() {
        Value::Vector(v) => v,
        _ => panic!(),
    };
    assert_eq!(
        parse("{:mu 3 :max 3 :sigma 0}").unwrap(),
        get(&classes[0], "crap").unwrap().clone()
    );
}
