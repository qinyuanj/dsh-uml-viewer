use edn::Value;
use edn::record::{get, num};
use scene::Snapshot;
use scene::{class_card, class_card_in};

fn kw(s: &str) -> Value {
    Value::keyword(s)
}

const DEEP: &str = "{:hierarchical true :classes [{:id :alpha.deep.parser :ns \"alpha.deep.parser\"} {:id :beta.parser}] :edges [{:from :alpha.deep.parser :to :beta.parser :kind :dependency}]}";

fn deep_state(focus: &[Value]) -> scene::document::ViewState {
    let doc = scene::ir::read_document(DEEP).unwrap();
    let crap = Snapshot::new(
        ".metrics/crap.edn",
        r#"{:entries [{:namespace "alpha.deep.parser" :name "parse" :complexity 3 :coverage 100 :crap 3}]}"#,
        Value::Integer(1),
    );
    scene::document::new_state(doc, None, Some(focus), None, &[crap], None)
}

#[test]
fn a_root_card_keeps_the_aggregate_relationship() {
    let card = class_card(&deep_state(&[]), &kw("alpha")).unwrap();
    assert_eq!(card.relations.len(), 1);
    let rel = &card.relations[0];
    assert_eq!(rel.other, kw("beta"));
    assert!(rel.outgoing);
    assert_eq!(rel.kind, Some(kw("dependency")));
}

#[test]
fn a_leaf_card_keeps_its_leaf_identity_relationship_and_quality() {
    let card = class_card(&deep_state(&[]), &kw("alpha.deep.parser")).unwrap();
    assert_eq!(get(&card.class, "id"), Some(&kw("alpha.deep.parser")));
    let mu = get(&card.class, "crap")
        .and_then(|c| get(c, "mu"))
        .and_then(num);
    assert_eq!(mu, Some(3.0));
    assert_eq!(card.relations.len(), 1);
    assert_eq!(card.relations[0].other, kw("beta.parser"));
    assert!(card.relations[0].outgoing);
}

#[test]
fn a_focused_view_still_resolves_intermediate_and_leaf_cards() {
    let state = deep_state(&[kw("alpha")]);
    assert!(class_card(&state, &kw("alpha.deep")).is_some());
    assert_eq!(class_card(&state, &kw("alpha")).unwrap().relations.len(), 1);
    assert_eq!(
        class_card(&state, &kw("alpha.deep.parser"))
            .unwrap()
            .relations
            .len(),
        1
    );
}

#[test]
fn a_scene_card_names_both_ends_its_package_and_title() {
    let doc = scene::ir::read_document(
        r#"{:title "Tiny" :packages [{:id :p :label "Domain" :classes [{:id :a :name "A"} {:id :b :name "B"}]}] :edges [{:from :a :to :b :kind :association :label "uses"}]}"#,
    )
    .unwrap();
    let scene = scene::document::compile_view(&doc, &[], &[], &Value::Map(vec![]));
    let id = |name: &str| {
        edn::record::seq(get(&scene, "classes"))
            .iter()
            .find(|c| get(c, "name") == Some(&Value::String(name.into())))
            .and_then(|c| get(c, "id").cloned())
            .unwrap()
    };
    let b = class_card_in(&scene, &id("B")).unwrap();
    assert_eq!(b.relations.len(), 1);
    let rel = &b.relations[0];
    assert!(!rel.outgoing);
    assert_eq!(rel.other, id("A"));
    assert_eq!(rel.other_name, Some(Value::String("A".into())));
    assert_eq!(rel.kind, Some(kw("association")));
    assert_eq!(rel.label, Some(Value::String("uses".into())));
    let package = b.package.unwrap();
    assert_eq!(
        get(&package, "label"),
        Some(&Value::String("Domain".into()))
    );
    assert_eq!(b.title, Some(Value::String("Tiny".into())));
    assert!(class_card_in(&scene, &kw("absent")).is_none());
}

#[test]
fn a_focused_card_names_the_namespace_box_it_sits_in() {
    let card = class_card(&deep_state(&[kw("alpha")]), &kw("alpha.deep")).unwrap();
    let label = card.package.as_ref().and_then(|p| get(p, "label"));
    assert_eq!(label, Some(&Value::String("alpha".into())));
}
