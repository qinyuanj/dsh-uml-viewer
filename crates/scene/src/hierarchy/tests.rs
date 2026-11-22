use crate::hierarchy as H;
use edn::record::{get, local_name, num};
use edn::{Value, parse};

fn kw(s: &str) -> Value {
    Value::keyword(s)
}

fn box_at<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
    for p in seq(get(v, "packages")) {
        for c in seq(get(p, "classes")) {
            if get(c, "id") == Some(&kw(id)) {
                return Some(c);
            }
        }
    }
    None
}

fn edge<'a>(v: &'a Value, a: &str, b: &str) -> Option<&'a Value> {
    seq(get(v, "edges"))
        .iter()
        .find(|e| get(e, "from") == Some(&kw(a)) && get(e, "to") == Some(&kw(b)))
}

fn seq(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Vector(items)) => items,
        _ => &[],
    }
}

fn ids(xs: &[Value]) -> Vec<Value> {
    xs.iter().map(|x| get(x, "id").unwrap().clone()).collect()
}

const GRAPH: &str = r#"{:hierarchical true :prefix "demo" :foreign [:quil :javax.swing] :order [:source :layout :ir] :classes [{:id :ir :ns "demo.ir"} {:id :source :ns "demo.source" :stereotype :interface} {:id :source.lua :ns "demo.source.lua"} {:id :layout :ns "demo.layout"} {:id :quil :name "quil" :foreign true} {:id :javax.swing :name "javax.swing" :foreign true}] :edges [{:from :source.lua :to :source :kind :implements} {:from :layout :to :ir :kind :dependency} {:from :layout :to :quil :kind :dependency} {:from :source.lua :to :javax.swing :kind :dependency}]}"#;

#[test]
fn rolls_directory_metrics_by_function_count_while_files_keep_their_scores() {
    let d = parse(r#"{:hierarchical true :classes [{:id :source :crap {:mu 1 :max 1 :sigma 0} :killed 8 :survived 0 :level 0} {:id :source.lua :crap {:mu 8 :max 9 :sigma 2} :killed 1 :survived 1 :level 2} {:id :layout :crap {:mu 2 :max 3 :sigma 1} :killed 9 :survived 1}] :edges []}"#).unwrap();
    let root = H::view_at(&d, &[]);
    let inner = H::view_at(&d, &[kw("source")]);
    assert_eq!(
        parse("{:mu 4.5 :max 9 :sigma 3.8 :n 2}").unwrap(),
        get(box_at(&root, "source").unwrap(), "crap")
            .unwrap()
            .clone()
    );
    assert_eq!(
        parse("{:mu 2 :max 3 :sigma 1}").unwrap(),
        get(box_at(&root, "layout").unwrap(), "crap")
            .unwrap()
            .clone()
    );
    assert_eq!(
        Some(2.0),
        num(get(box_at(&root, "source").unwrap(), "level").unwrap())
    );
    assert_eq!(
        Some(9.0),
        num(get(box_at(&root, "source").unwrap(), "killed").unwrap())
    );
    assert_eq!(
        Some(1.0),
        num(get(box_at(&root, "source").unwrap(), "survived").unwrap())
    );
    assert_eq!(
        parse("{:mu 1 :max 1 :sigma 0}").unwrap(),
        get(box_at(&inner, "source").unwrap(), "crap")
            .unwrap()
            .clone()
    );
    assert_eq!(
        parse("{:mu 8 :max 9 :sigma 2}").unwrap(),
        get(box_at(&inner, "source.lua").unwrap(), "crap")
            .unwrap()
            .clone()
    );
    let weighted = parse(r#"{:hierarchical true :classes [{:id :engine :ops [{:crap 2} {:crap 2} {:crap 2}]} {:id :engine.layout :ops [{:crap 20}]}] :edges []}"#).unwrap();
    let root = H::view_at(&weighted, &[]);
    assert_eq!(
        parse("{:mu 6.5 :max 20 :sigma 7.8 :n 4}").unwrap(),
        get(box_at(&root, "engine").unwrap(), "crap")
            .unwrap()
            .clone()
    );
}

#[test]
fn counts_unscored_descendants_as_red_without_inheriting_into_files() {
    let d = parse(r#"{:hierarchical true :classes [{:id :source :crap {:mu 1 :max 1 :sigma 0} :killed 8 :survived 0} {:id :source.lua :ops [{:name "a"} {:name "b"}]}] :edges []}"#).unwrap();
    let root = H::view_at(&d, &[]);
    let source = box_at(&root, "source").unwrap();
    assert_eq!(
        parse("{:mu 13.7 :max 20 :sigma 9 :n 3}").unwrap(),
        get(source, "crap").unwrap().clone()
    );
    assert_eq!(Some(2.0), num(get(source, "mut-gap").unwrap()));
    let inner = H::view_at(&d, &[kw("source")]);
    assert_eq!(
        parse("{:mu 1 :max 1 :sigma 0}").unwrap(),
        get(box_at(&inner, "source").unwrap(), "crap")
            .unwrap()
            .clone()
    );
    assert!(get(box_at(&inner, "source.lua").unwrap(), "crap").is_none());
    assert!(get(box_at(&inner, "source.lua").unwrap(), "killed").is_none());
    let one = parse(r#"{:hierarchical true :classes [{:id :source :crap {:mu 1 :max 1 :sigma 0}} {:id :source.lua}] :edges []}"#).unwrap();
    let root = H::view_at(&one, &[]);
    assert_eq!(
        parse("{:mu 10.5 :max 20 :sigma 9.5 :n 2}").unwrap(),
        get(box_at(&root, "source").unwrap(), "crap")
            .unwrap()
            .clone()
    );
}

#[test]
fn uses_namespace_labels_except_rust_files_and_preserves_camel_capitals() {
    let d = parse(r#"{:hierarchical true :classes [{:id :rust :file "src/lib.rs"} {:id :rust.main :file "src/main.rs"} {:id :rust.proto :file "src/proto.RS"} {:id :graph-python :file "src/graph_python.lua"} {:id :tauriFs}] :edges []}"#).unwrap();
    let root = H::view_at(&d, &[]);
    let inner = H::view_at(&d, &[kw("rust")]);
    let name_of = |v: &Value, id: &str| match get(box_at(v, id).unwrap(), "name").unwrap() {
        Value::String(s) => s.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!("Rust", name_of(&root, "rust"));
    assert_eq!("lib.rs", name_of(&inner, "rust"));
    assert_eq!("main.rs", name_of(&inner, "rust.main"));
    assert_eq!("proto.RS", name_of(&inner, "rust.proto"));
    let contents = seq(get(box_at(&root, "rust").unwrap(), "contents"));
    let want = parse(r#"[{:id :rust :name "lib.rs" :drill? false} {:id :rust.main :name "main.rs" :drill? false} {:id :rust.proto :name "proto.RS" :drill? false}]"#).unwrap();
    assert_eq!(want, Value::Vector(contents.to_vec()));
    assert_eq!("GraphPython", name_of(&root, "graph-python"));
    assert_eq!("TauriFs", name_of(&root, "tauriFs"));
}

#[test]
fn routes_off_view_dependencies_into_ports_while_keeping_visible_arrows() {
    let d = parse(r#"{:hierarchical true :classes [{:id :engine} {:id :engine.layout} {:id :engine.route} {:id :engineering} {:id :domain.ir}] :order {"" [:domain :engine] "engine" [:engine.route :engine.layout :engine]} :edges [{:from :engine.layout :to :engine.route :kind :dependency} {:from :engine.layout :to :domain.ir :kind :dependency} {:from :domain.ir :to :engine.route :kind :dependency}]}"#).unwrap();
    let root = H::view_at(&d, &[]);
    let inner = H::view_at(&d, &[kw("engine")]);
    assert_eq!(
        vec![kw("domain"), kw("engine"), kw("engineering")],
        ids(seq(get(&seq(get(&root, "packages"))[0], "classes")))
    );
    assert_eq!(
        vec![kw("engine.route"), kw("engine.layout"), kw("engine")],
        ids(seq(get(&seq(get(&inner, "packages"))[0], "classes")))
    );
    assert!(edge(&root, "engine", "domain").is_some());
    assert!(edge(&inner, "engine.layout", "engine.route").is_some());
    let out_deps = seq(get(box_at(&inner, "engine.layout").unwrap(), "out-deps"));
    assert_eq!(1, out_deps.len());
    assert_eq!(Some(&kw("domain")), get(&out_deps[0], "id"));
    assert_eq!(
        Some(&Value::String("Domain".into())),
        get(&out_deps[0], "name")
    );
    let in_deps = seq(get(box_at(&inner, "engine.route").unwrap(), "in-deps"));
    assert_eq!(1, in_deps.len());
    assert_eq!(Some(&kw("domain")), get(&in_deps[0], "id"));
    assert!(box_at(&inner, "engineering").is_none());
    assert!(edge(&inner, "engine.layout", "domain").is_none());
    let mut d = d;
    geom_put_omit(&mut d);
    let omitted = H::view_at(&d, &[]);
    assert!(box_at(&omitted, "engine").is_none());
    assert!(box_at(&omitted, "engineering").is_some());
}

fn geom_put_omit(d: &mut Value) {
    edn::record::put(d, "omit", parse("[:engine]").unwrap());
}

#[test]
fn remaps_one_level_proposal_groups_and_expands_them() {
    let d = parse(r#"{:hierarchical true :foreign [:quil] :classes [{:id :jvm.cli} {:id :jvm.sketch} {:id :game-api.core} {:id :quil :name "quil" :foreign true}] :edges [{:from :jvm.sketch :to :jvm.cli :kind :dependency} {:from :jvm.sketch :to :quil :kind :dependency}] :proposals [{:id :split :layers [{:id :host :nses [:jvm.cli]} {:id :ui :nses [{:id :quil-swing :label "Quil/Swing" :nses [:jvm.sketch]}]} {:id :game-api :nses [:game-api]}]}]}"#).unwrap();
    let v = H::proposal_view(&d, Some(&kw("split")));
    assert!(edge(&v, "quil-swing", "jvm.cli").is_some());
    assert!(edge(&v, "quil-swing", "quil").is_some());
    assert!(box_at(&v, "game-api.core").is_some());
    assert!(box_at(&v, "game-api").is_none());
    let inner = H::layer_view(&d, Some(&kw("split")), &kw("quil-swing"));
    assert_eq!(
        vec![kw("jvm.sketch")],
        ids(seq(get(&seq(get(&inner, "packages"))[0], "classes")))
    );
    assert!(edge(&inner, "jvm.sketch", "quil").is_some());
    assert!(box_at(&inner, "jvm.cli").is_none());
    let split = H::proposal_view(
        &d,
        Some(&parse("{:id :explicit :layers [{:id :ui :nses [:jvm.sketch]} {:id :host :nses [:jvm.cli]}]}").unwrap()),
    );
    let e = edge(&split, "jvm.sketch", "jvm.cli").unwrap();
    assert_eq!(Some(&Value::Bool(true)), get(e, "violating"));
}

#[test]
fn restamps_violations_from_the_selected_proposal_order() {
    let d = parse(r#"{:hierarchical true :levels [[:ir] [:layout]] :classes [{:id :ir :level 0} {:id :layout :level 1}] :edges [{:from :ir :to :layout :kind :dependency :violating true} {:from :layout :to :ir :kind :dependency}] :proposals [{:id :rev :layers [{:id :engine :nses [:layout]} {:id :kernel :nses [:ir]}]}]}"#).unwrap();
    let actual = H::view_at(&d, &[]);
    let proposed = H::proposal_view(&d, None);
    assert_eq!(
        Some(&Value::Bool(true)),
        get(edge(&actual, "ir", "layout").unwrap(), "violating")
    );
    assert!(get(edge(&actual, "layout", "ir").unwrap(), "violating").is_none());
    assert!(get(edge(&proposed, "ir", "layout").unwrap(), "violating").is_none());
    assert_eq!(
        Some(&Value::Bool(true)),
        get(edge(&proposed, "layout", "ir").unwrap(), "violating")
    );
    assert_eq!(
        Some(1.0),
        num(get(box_at(&proposed, "ir").unwrap(), "level").unwrap())
    );
    assert_eq!(
        Some(0.0),
        num(get(box_at(&proposed, "layout").unwrap(), "level").unwrap())
    );
    let collapsed = H::apply_declutter(&proposed, Some(&kw("arrows")));
    assert_eq!(
        Some(&Value::Bool(true)),
        get(
            edge(&collapsed, "proposal.engine", "proposal.kernel").unwrap(),
            "violating"
        )
    );
}

#[test]
fn keeps_inputs_unchanged_across_navigation_and_declutter() {
    let d = parse(GRAPH).unwrap();
    let before = d.clone();
    let v = H::view_at(&d, &[]);
    let saved = v.clone();
    H::apply_declutter(&v, Some(&kw("classes")));
    H::proposal_view(&d, None);
    H::view_at(&d, &[kw("source")]);
    assert_eq!(before, d);
    assert_eq!(saved, v);
}

#[test]
fn declutter_arrows_keep_leaf_provenance_and_collapsed_quality() {
    let d = parse(r#"{:hierarchical true :foreign [:quil :javax.swing] :proposal [{:id :kernel :label "Kernel" :nses [:ir :source]} {:id :engine :label "Engine" :nses [:layout]}] :classes [{:id :ir :ns "demo.ir" :crap {:mu 3 :max 3 :sigma 0} :killed 2 :survived 0} {:id :source :ns "demo.source" :stereotype :interface} {:id :source.lua :ns "demo.source.lua"} {:id :layout :ns "demo.layout"} {:id :quil :name "quil" :foreign true} {:id :javax.swing :name "javax.swing" :foreign true}] :edges [{:from :source.lua :to :source :kind :implements} {:from :layout :to :ir :kind :dependency} {:from :layout :to :quil :kind :dependency} {:from :source.lua :to :javax.swing :kind :dependency} {:from :ir :to :layout :kind :dependency} {:from :source :to :layout :kind :association}]}"#).unwrap();
    let v = H::apply_declutter(&H::proposal_view(&d, None), Some(&kw("arrows")));
    let e = edge(&v, "proposal.kernel", "proposal.engine").unwrap();
    assert_eq!(2, seq(get(e, "deps")).len());
    assert!(!seq(get(e, "via-ids")).is_empty());
    assert!(get(box_at(&v, "proposal.kernel").unwrap(), "dummy?").is_some());
    let collapsed = H::apply_declutter(&H::proposal_view(&d, None), Some(&kw("classes")));
    assert_eq!(
        1,
        seq(get(&seq(get(&collapsed, "packages"))[0], "classes")).len()
    );
    assert!(get(box_at(&collapsed, "proposal.kernel").unwrap(), "crap").is_some());
    assert_eq!(
        Some(2.0),
        num(get(box_at(&collapsed, "proposal.kernel").unwrap(), "killed").unwrap())
    );
    let full = H::view_at(&d, &[]);
    let triangles = H::apply_declutter(&full, Some(&kw("triangles")));
    assert!(get(&triangles, "hide-edges").is_some());
    assert!(!seq(get(&triangles, "edges")).is_empty());
    for mode in ["elements", "classes", "methods"] {
        let decluttered = H::apply_declutter(&full, Some(&kw(mode)));
        let c = box_at(&decluttered, "source").unwrap();
        assert!(get(c, "hide-members").is_some(), "{mode}");
        assert!(get(c, "contents").is_none(), "{mode}");
        assert!(get(c, "in-deps").is_none(), "{mode}");
        assert!(get(c, "out-deps").is_none(), "{mode}");
    }
    let mut mode: Option<Value> = None;
    for want in ["arrows", "triangles", "elements", "classes", "full"] {
        mode = Some(H::next_declutter(mode.as_ref()));
        assert_eq!(
            want,
            local_name(mode.as_ref().unwrap())
                .unwrap_or_default()
                .to_owned()
        );
    }
}

#[test]
fn preserves_named_proposals_nested_groups_leftovers_omission_and_drill_views() {
    let d = parse(r#"{:hierarchical true :foreign [:quil] :classes [{:id :jvm.cli} {:id :jvm.sketch} {:id :ir} {:id :spare} {:id :quil :name "quil" :foreign true}] :edges [{:from :jvm.sketch :to :jvm.cli :kind :dependency} {:from :jvm.sketch :to :quil :kind :dependency}] :proposals [{:id :split :name "split" :omit [:ir] :layers [{:id :jvm :label "JVM" :nses [:jvm.cli {:id :outer :label "Outer" :nses [{:id :quil-swing :label "Quil/Swing" :nses [:jvm.sketch]}]}]}]}]}"#).unwrap();
    let v = H::proposal_view(&d, Some(&kw("split")));
    let pkgs = seq(get(&v, "packages"));
    assert_eq!(
        vec![kw("proposal.unassigned"), kw("proposal.jvm")],
        ids(pkgs)
    );
    assert_eq!(Some(&Value::Bool(true)), get(&v, "proposal"));
    assert_eq!(
        Some(&Value::String(crate::policy::PROPOSAL_NOTICE.into())),
        get(&v, "title")
    );
    assert_eq!(vec![kw("spare")], ids(seq(get(&pkgs[0], "classes"))));
    assert_eq!(
        vec![kw("jvm.cli"), kw("outer")],
        ids(seq(get(&pkgs[1], "classes")))
    );
    let outer_contents = seq(get(box_at(&v, "outer").unwrap(), "contents"));
    assert_eq!(
        Some(&Value::String("Quil/Swing".into())),
        get(&outer_contents[0], "name")
    );
    assert!(edge(&v, "outer", "jvm.cli").is_none());
    assert!(box_at(&v, "ir").is_none());
    let group = H::find_nested_group(&d, Some(&kw("split")), &kw("quil-swing")).unwrap();
    assert_eq!(
        Some(&Value::String("Quil/Swing".into())),
        get(&group, "label")
    );
    let drilled = H::layer_view(&d, Some(&kw("split")), &kw("outer"));
    assert_eq!(
        vec![kw("quil-swing")],
        ids(seq(get(&seq(get(&drilled, "packages"))[0], "classes")))
    );
    let direct = H::layer_view(&d, Some(&kw("split")), &kw("proposal.jvm"));
    assert_eq!(Some(&Value::String("JVM".into())), get(&direct, "title"));
    assert!(H::proposal_package_id(Some(&kw("proposal.jvm"))));
    assert!(!H::proposal_package_id(Some(&kw("jvm"))));
    assert_eq!(vec![kw("split")], ids(&H::named_proposals(&d)));
    assert_eq!(1, crate::policy::proposal_layers(&d).len());
}

#[test]
fn uses_actual_namespace_children_and_drills_with_self_modules_and_foreign_ovals() {
    let d = parse(GRAPH).unwrap();
    let root = H::view_at(&d, &[]);
    let inner = H::view_at(&d, &[kw("source")]);
    assert_eq!(
        vec![kw("source"), kw("layout"), kw("ir")],
        ids(seq(get(&seq(get(&root, "packages"))[0], "classes")))
    );
    assert_eq!(
        vec![kw("source"), kw("source.lua")],
        ids(seq(get(box_at(&root, "source").unwrap(), "contents")))
    );
    assert_eq!(
        Some(&Value::Bool(true)),
        get(box_at(&root, "source").unwrap(), "drill?")
    );
    assert!(get(box_at(&root, "source").unwrap(), "hide-members").is_some());
    assert_eq!(
        Some(&Value::Bool(false)),
        get(
            &seq(get(box_at(&root, "source").unwrap(), "contents"))[0],
            "drill?"
        )
    );
    assert!(edge(&root, "layout", "ir").is_some());
    assert!(edge(&root, "layout", "quil").is_some());
    assert!(edge(&root, "source", "javax.swing").is_some());
    assert!(edge(&root, "source", "source").is_none());
    assert!(get(box_at(&root, "layout").unwrap(), "out-deps").is_none());
    assert!(edge(&inner, "source.lua", "source").is_some());
    assert!(edge(&inner, "source.lua", "javax.swing").is_some());
    assert_eq!(
        Some(&kw("interface")),
        get(box_at(&inner, "source").unwrap(), "stereotype")
    );
    assert_eq!(vec![kw("javax.swing")], ids(seq(get(&inner, "foreign"))));
    assert!(box_at(&inner, "layout").is_none());
    assert_eq!(Some(&Value::String("source".into())), get(&inner, "title"));
}

const FALSY: [&str; 2] = ["false", "nil"];
const TRUTHY: [&str; 2] = [":yes", "1"];

fn encoded(v: &Value) -> String {
    edn::encode(v).unwrap()
}

#[test]
fn dummy_false_is_not_taken_as_the_package_endpoint_when_hiding_classes() {
    let hidden = |flag: &str| {
        let view = edn::parse(&format!(
            r#"{{:packages [{{:id :p :classes [{{:id :a {flag}}} {{:id :b}}]}} {{:id :q :classes [{{:id :c}}]}}]
                :edges []}}"#
        ))
        .unwrap();
        let out = H::hide_classes(&view);
        let ids: Vec<String> = edn::record::seq(get(&out, "packages"))
            .iter()
            .flat_map(|p| edn::record::seq(get(p, "classes")).to_vec())
            .map(|c| encoded(get(&c, "id").unwrap()))
            .collect();
        ids.join(" ")
    };
    let omitted = hidden("");
    let dummy = hidden(":dummy? true");
    assert_ne!(omitted, dummy, "fixture must tell a dummy apart");
    for v in FALSY {
        assert_eq!(omitted, hidden(&format!(":dummy? {v}")), "{v}");
    }
    for v in TRUTHY {
        assert_eq!(dummy, hidden(&format!(":dummy? {v}")), "{v}");
    }
}

#[test]
fn proposal_group_false_keeps_the_namespace_label() {
    let name = |flag: &str| {
        let doc = edn::parse(&format!(
            r#"{{:hierarchical true :classes [{{:id :a.x :name "Custom" {flag}}} {{:id :b}}] :edges []
                :proposals [{{:id :p :layers [{{:id :l1 :nses [:a.x]}} {{:id :l2 :nses [:b]}}]}}]}}"#
        ))
        .unwrap();
        let v = H::proposal_view(&doc, None);
        let names: Vec<String> = edn::record::seq(get(&v, "packages"))
            .iter()
            .flat_map(|p| edn::record::seq(get(p, "classes")).to_vec())
            .map(|c| encoded(get(&c, "name").unwrap_or(&Value::Nil)))
            .collect();
        names.join(" ")
    };
    let omitted = name("");
    let group = name(":proposal-group? true");
    assert_ne!(omitted, group, "fixture must tell a group apart");
    for v in FALSY {
        assert_eq!(omitted, name(&format!(":proposal-group? {v}")), "{v}");
    }
    for v in TRUTHY {
        assert_eq!(group, name(&format!(":proposal-group? {v}")), "{v}");
    }
}

#[test]
fn violating_false_does_not_turn_a_collapsed_arrow_into_a_violation() {
    let collapsed = |flag: &str| {
        let view = edn::parse(&format!(
            r#"{{:packages [{{:id :p :classes [{{:id :a}} {{:id :b}}]}}]
                :edges [{{:from :a :to :b :kind :association {flag}}}]}}"#
        ))
        .unwrap();
        let out = H::collapse_arrows(&view);
        let e = edn::record::seq(get(&out, "edges"))[0].clone();
        (
            encoded(get(&e, "kind").unwrap()),
            get(&e, "violating").cloned(),
        )
    };
    let omitted = collapsed("");
    assert_eq!((":association".to_string(), None), omitted);
    for v in FALSY {
        assert_eq!(omitted, collapsed(&format!(":violating {v}")), "{v}");
    }
    let violating = collapsed(":violating true");
    for v in TRUTHY {
        assert_eq!(violating, collapsed(&format!(":violating {v}")), "{v}");
    }
}
