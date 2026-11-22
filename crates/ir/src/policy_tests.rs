use edn::Value;

use crate::policy;
use edn::record::get;

fn parse(text: &str) -> Value {
    edn::parse(text).unwrap()
}

fn assert_same(expected: &str, actual: Option<&Value>) {
    let actual = actual.unwrap_or_else(|| panic!("missing value; expected {expected}"));
    assert_eq!(
        parse(expected),
        *actual,
        "actual: {}",
        edn::encode(actual).unwrap()
    );
}

#[test]
fn a_bare_policy_builds_a_hierarchy_without_inventing_proposals() {
    let doc = policy::apply_policy(
        &parse("{}"),
        &parse(
            r#"{:classes [{:id :calc :name "Calc" :ns "calc" :lang :lua :file "src/calc/init.lua"}] :edges []}"#,
        ),
    );
    assert_same(
        r#"{:title "UML" :hierarchical true :prefix "uml-viewer" :order [] :levels [] :edge-kinds {} :omit-edges [] :omit [] :classes [{:id :calc :name "Calc" :ns "calc" :lang :lua :file "src/calc/init.lua"}] :edges []}"#,
        Some(&doc),
    );
}

#[test]
fn foreign_namespaces_collapse_and_edges_filter_and_bundle() {
    let doc = policy::apply_policy(
        &parse("{:foreign [:socket :socket.http]}"),
        &parse(
            r#"{:classes [{:id :calc :name "Calc"} {:id :socket.http.client :foreign true} {:id :socket.udp :foreign true} {:id :unknown :foreign true}]
      :edges [{:from :calc :to :socket.http.client :kind :dependency} {:from :calc :to :socket.http.client :kind :association :note "first"} {:from :calc :to :socket.http.client :kind :association :note "last"} {:from :calc :to :socket.udp :kind :dependency} {:from :calc :to :unknown :kind :dependency} {:from :calc :to :missing :kind :dependency} {:from :calc :to :calc :kind :dependency}]}"#,
        ),
    );
    assert_same(
        r#"[{:id :calc :name "Calc"} {:id :socket :name "socket" :foreign true} {:id :socket.http :name "socket.http" :foreign true}]"#,
        get(&doc, "classes"),
    );
    assert_same(
        r#"[{:from :calc :to :socket.http :kind :association :note "last"} {:from :calc :to :socket :kind :dependency}]"#,
        get(&doc, "edges"),
    );
}

#[test]
fn named_proposals_keep_nested_groups_and_longest_prefix_ranks() {
    let doc = policy::apply_policy(
        &parse(
            r#"{:levels [[:calc] [:calc.io]] :proposals [{:id :plan :name "计算" :omit ["calc.old"] :layers [{:label "计算" :nses [:calc {:label "IO" :nses [calc.io]}]}]}] :omit [:calc.old] :edge-kinds {[:calc :calc.io] :association}}"#,
        ),
        &parse(
            r#"{:classes [{:id :calc :name "Calc"} {:id :calc.io.read :name "Read"}]
      :edges [{:from :calc :to :calc.io.read :kind :dependency} {:from :calc.io.read :to :calc :kind :dependency :violating true}]}"#,
        ),
    );
    assert_same("[[:calc] [:calc.io]]", get(&doc, "levels"));
    assert_same(
        r#"[{:id :calc :name "Calc" :level 0} {:id :calc.io.read :name "Read" :level 1}]"#,
        get(&doc, "classes"),
    );
    assert_same(
        r#"[{:from :calc :to :calc.io.read :kind :dependency :violating true} {:from :calc.io.read :to :calc :kind :dependency}]"#,
        get(&doc, "edges"),
    );
    assert_same(
        r#"[{:id :plan :name "计算" :omit [:calc.old] :notice "PROPOSAL — not instantiated in code" :layers [{:id :计算 :label "计算" :nses [:calc {:id :IO :label "IO" :nses [:calc.io]}]}]}]"#,
        get(&doc, "proposals"),
    );
    assert_same("{[:calc :calc.io] :association}", get(&doc, "edge-kinds"));
}

#[test]
fn legacy_package_diagrams_add_unassigned_stubs_and_foreign_ovals() {
    let doc = policy::apply_policy(
        &parse(
            r#"{:title "Calc" :foreign [:socket] :packages [{:id :calc :label "Calc" :nses [:calc]}] :diagrams [{:title "All" :view :overview :hide-members true} {:title "Calc" :package :calc :edge-kinds {[:calc :util] :association}}]}"#,
        ),
        &parse(
            r#"{:classes [{:id :calc :name "Calc" :ns "demo.calc" :lang :lua :file "src/calc.lua" :stereotype :interface} {:id :util :name "Util"} {:id :orphan :name "Orphan"} {:id :socket.http :foreign true}]
      :edges [{:from :calc :to :util :kind :dependency} {:from :calc :to :socket.http :kind :dependency} {:from :util :to :orphan :kind :dependency}]}"#,
        ),
    );
    let diagrams = match get(&doc, "diagrams") {
        Some(Value::Vector(items)) => items.clone(),
        _ => panic!("missing diagrams"),
    };
    assert_same(
        r#"{:title "All" :direction :tb :packages [{:id :calc :label "Calc" :classes [{:id :calc :name "Calc" :ns "demo.calc" :lang :lua :file "src/calc.lua" :stereotype :interface :hide-members true}]} {:id :unassigned :label "Unassigned" :classes [{:id :orphan :name "Orphan" :hide-members true} {:id :util :name "Util" :hide-members true}]}] :foreign [{:id :socket :name "socket" :shape :oval}] :edges [{:from :calc :to :util :kind :dependency} {:from :calc :to :socket :kind :dependency} {:from :util :to :orphan :kind :dependency}]}"#,
        diagrams.first(),
    );
    assert_same(
        r#"{:title "Calc" :direction :lr :packages [{:id :calc :label "Calc" :classes [{:id :calc :name "Calc" :ns "demo.calc" :lang :lua :file "src/calc.lua" :stereotype :interface} {:id :util :name "Util" :hide-members true}]}] :foreign [{:id :socket :name "socket" :shape :oval}] :edges [{:from :calc :to :util :kind :association} {:from :calc :to :socket :kind :dependency}]}"#,
        diagrams.get(1),
    );
}

#[test]
fn legacy_nested_proposal_layers_rank_the_hierarchy() {
    let doc = policy::apply_policy(
        &parse(
            r#"{:hierarchical false :order [calc] :omit [calc.old] :omit-edges [[:calc :io]] :proposal [{:nses [:calc {:label "IO" :nses [:io]}]} {:id :host :nses [:host]}]}"#,
        ),
        &parse(
            r#"{:classes [{:id :calc :name "Calc"} {:id :io :name "Io"} {:id :host :name "Host"}] :edges [{:from :calc :to :host :kind :dependency} {:from :host :to :calc :kind :association}]}"#,
        ),
    );
    assert_same("[[:calc :io] [:host]]", get(&doc, "levels"));
    assert_same(
        r#"[{:from :calc :to :host :kind :dependency :violating true} {:from :host :to :calc :kind :association}]"#,
        get(&doc, "edges"),
    );
    assert_same("[:calc.old]", get(&doc, "omit"));
    assert_same("[[:calc :io]]", get(&doc, "omit-edges"));
    assert_same("[:calc]", get(&doc, "order"));
    let first_layer_id = get(&doc, "proposal")
        .and_then(|p| get(p, "layers"))
        .and_then(|layers| match layers {
            Value::Vector(items) => items.first(),
            _ => None,
        })
        .and_then(|layer| get(layer, "id"));
    assert_same(":layer-0", first_layer_id);
}

#[test]
fn edge_kind_overrides_drop_stale_violations() {
    let edges = policy::apply_edge_kinds(
        &[
            parse(r#"{:from :calc :to :io :kind :dependency :violating true}"#),
            parse(r#"{:from :io :to :calc :kind :dependency}"#),
        ],
        &parse("{[:io :calc] :association}"),
        &[parse(r#"["calc" "io"]"#)],
    );
    assert_same(
        "[{:from :io :to :calc :kind :association}]",
        Some(&Value::Vector(edges)),
    );
}

#[test]
fn explicit_empty_values_differ_from_absent_ones() {
    assert_same(
        r#"{:title "UML" :diagrams []}"#,
        Some(&policy::apply_policy(
            &parse("{:title nil :packages [] :diagrams []}"),
            &parse("{:classes [] :edges []}"),
        )),
    );
    let doc = policy::apply_policy(
        &parse("{:title false :prefix nil :proposals [] :proposal [{:nses [:calc]}]}"),
        &parse("{:classes [{:id :calc}] :edges []}"),
    );
    assert_eq!(None, get(&doc, "proposals"));
    assert_same(r#""UML""#, get(&doc, "title"));
    assert_same(r#""uml-viewer""#, get(&doc, "prefix"));
    assert_same("[[:calc]]", get(&doc, "levels"));
}

#[test]
fn bundled_violations_survive_until_a_stronger_kind_replaces_them() {
    let first = parse(
        r#"[{:from :calc :to :io :kind :dependency :violating true} {:from :calc :to :io :kind :dependency :note "later"}]"#,
    );
    assert_same(
        r#"[{:from :calc :to :io :kind :dependency :violating true :note "later"}]"#,
        Some(&policy::merge_edges(first.as_vector())),
    );
    let second = parse(
        r#"[{:from :calc :to :io :kind :implements :violating true} {:from :calc :to :io :kind :inheritance}]"#,
    );
    assert_same(
        "[{:from :calc :to :io :kind :inheritance}]",
        Some(&policy::merge_edges(second.as_vector())),
    );
}

trait AsVector {
    fn as_vector(&self) -> &[Value];
}

impl AsVector for Value {
    fn as_vector(&self) -> &[Value] {
        match self {
            Value::Vector(items) => items,
            _ => panic!("expected vector"),
        }
    }
}

#[test]
fn clojure_group_by_order_holds_past_the_array_map_boundary() {
    // Independently captured with Clojure 1.12.0 group-by (policy_ir_spec).
    let mut classes = vec![parse("{:id :目标/终点}")];
    let mut edges = Vec::new();
    let names = ["团队/计算", "a", "b", "c", "d", "e", "f", "g"];
    let add = |n: &str, classes: &mut Vec<Value>, edges: &mut Vec<Value>| {
        classes.push(parse(&format!("{{:id :{n}}}")));
        edges.push(parse(&format!(
            "{{:from :{n} :to :目标/终点 :kind :dependency}}"
        )));
    };
    // Eight distinct pairs stay in an array map; the ninth promotes to a HAMT.
    for name in names {
        add(name, &mut classes, &mut edges);
    }
    edges.push(parse(
        r#"{:from :团队/计算 :to :目标/终点 :kind :dependency :note "last" :violating true}"#,
    ));
    let pairs = |doc: &Value| -> Value {
        Value::Vector(
            edn::record::seq(get(doc, "edges"))
                .iter()
                .map(|e| {
                    Value::Vector(vec![
                        get(e, "from").unwrap().clone(),
                        get(e, "to").unwrap().clone(),
                    ])
                })
                .collect(),
        )
    };
    let graph = Value::Map(vec![
        (Value::keyword("classes"), Value::Vector(classes.clone())),
        (Value::keyword("edges"), Value::Vector(edges.clone())),
    ]);
    let doc = policy::apply_policy(&parse("{}"), &graph);
    assert_same(
        "[[:团队/计算 :目标/终点] [:a :目标/终点] [:b :目标/终点] [:c :目标/终点] [:d :目标/终点] [:e :目标/终点] [:f :目标/终点] [:g :目标/终点]]",
        Some(&pairs(&doc)),
    );
    add("h", &mut classes, &mut edges);
    let graph = Value::Map(vec![
        (Value::keyword("classes"), Value::Vector(classes)),
        (Value::keyword("edges"), Value::Vector(edges)),
    ]);
    let doc = policy::apply_policy(&parse("{}"), &graph);
    assert_same(
        "[[:b :目标/终点] [:d :目标/终点] [:c :目标/终点] [:f :目标/终点] [:h :目标/终点] [:a :目标/终点] [:团队/计算 :目标/终点] [:g :目标/终点] [:e :目标/终点]]",
        Some(&pairs(&doc)),
    );
}

#[test]
fn a_timestamp_named_proposal_is_kept_but_never_invented() {
    let doc = policy::apply_policy(
        &parse(r#"{:proposals [{:layers [] :name "2026-10-22 14:11:13"}]}"#),
        &parse("{:classes [] :edges []}"),
    );
    let name = get(&doc, "proposals")
        .and_then(|p| match p {
            Value::Vector(items) => items.first(),
            _ => None,
        })
        .and_then(|p| get(p, "name"));
    assert_same(r#""2026-10-22 14:11:13""#, name);
    assert_eq!(None, get(&doc, "proposal"));
}

#[test]
fn an_explicit_nil_proposal_has_no_layers() {
    assert!(policy::proposal_layers(&parse("{:proposal nil}")).is_empty());
}
