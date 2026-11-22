use edn::Value;
use edn::record::get;

fn compiled(text: &str) -> Value {
    let doc = scene::ir::read_document(text).unwrap();
    scene::document::compile_view(&doc, &[], &[], &Value::Map(vec![]))
}

fn class(scene: &Value, id: &str) -> Value {
    edn::record::seq(get(scene, "classes"))
        .iter()
        .find(|c| {
            get(c, "id")
                .map(|v| edn::record::local_name(v).unwrap_or_default().to_owned())
                .as_deref()
                == Some(id)
        })
        .cloned()
        .unwrap_or_else(|| panic!("class {id}"))
}

fn rect(c: &Value) -> String {
    edn::encode(get(c, "rect").unwrap()).unwrap()
}

#[test]
fn foreign_false_on_a_package_class_keeps_the_class_box() {
    let scene = compiled(
        r#"{:packages [{:id :p :label "P" :classes [{:id :a :name "A" :foreign false
             :ops [{:name "run"}]} {:id :b :name "B"}]}]
            :edges [{:from :b :to :a :kind :dependency}]}"#,
    );
    let a = class(&scene, "d0-a");
    assert_eq!(None, get(&a, "shape"), "Lua: shape nil, not :oval");
    assert_eq!("{:h 72 :w 120 :x 108 :y 346}", rect(&a));
}

#[test]
fn foreign_false_on_a_hierarchical_class_keeps_it_a_module() {
    let scene = compiled(
        r#"{:hierarchical true :prefix "x" :order [a b]
            :classes [{:id :a :name "A" :ns "x.a" :foreign false :ops [{:name "run" :text "run"}]}
                      {:id :b :name "B" :ns "x.b"}]
            :edges [{:from :b :to :a :kind :dependency}]}"#,
    );
    let a = class(&scene, "a");
    assert_eq!(
        None,
        get(&a, "shape"),
        "Lua: a module box, not an oval stub"
    );
    assert_eq!("{:h 72 :w 120 :x 68 :y 282}", rect(&a));
}

#[test]
fn private_false_on_an_op_keeps_the_member_line() {
    let scene = compiled(
        r#"{:hierarchical true :prefix "x" :order [a]
            :classes [{:id :a :name "A" :ns "x.a"
                       :ops [{:name "run" :text "run" :private false} {:name "stop" :text "stop"}]}]
            :edges []}"#,
    );
    assert_eq!("{:h 90 :w 120 :x 68 :y 100}", rect(&class(&scene, "a")));
}

const FALSY: [&str; 2] = ["false", "nil"];
const TRUTHY: [&str; 2] = [":yes", "1"];

fn encoded(v: &Value) -> String {
    edn::encode(v).unwrap()
}

fn drawn(scene: &Value) -> String {
    let rects: Vec<Value> = edn::record::seq(get(scene, "classes"))
        .iter()
        .map(|c| get(c, "rect").cloned().unwrap_or(Value::Nil))
        .collect();
    format!(
        "{} {} {}",
        encoded(&Value::Vector(rects)),
        encoded(get(scene, "packages").unwrap_or(&Value::Nil)),
        encoded(get(scene, "edges").unwrap_or(&Value::Nil)),
    )
}

#[test]
fn dummy_false_is_hit_before_a_dummy_roll_up() {
    let hit = |flag: &str| {
        let scene = edn::parse(&format!(
            r#"{{:classes [{{:id :a :rect {{:x 0 :y 0 :w 100 :h 50}} {flag}}}
                          {{:id :roll :dummy? true :rect {{:x 0 :y 0 :w 100 :h 50}}}}]}}"#
        ))
        .unwrap();
        let h = scene::hit::at(&scene, 50.0, 40.0).unwrap();
        encoded(get(&h, "id").unwrap())
    };
    assert_eq!(":a", hit(""));
    for v in FALSY {
        assert_eq!(":a", hit(&format!(":dummy? {v}")), "{v}");
    }
}

#[test]
fn violating_is_lua_truthy_on_hit_deps_and_indicators() {
    for (v, want) in [
        ("false", false),
        ("nil", false),
        ("true", true),
        (":yes", true),
        ("1", true),
    ] {
        let e = edn::parse(&format!("{{:from :a :to :b :violating {v}}}")).unwrap();
        let deps = scene::hit::deps_of(&e).unwrap();
        let dep = &edn::record::seq(Some(&deps))[0];
        assert_eq!(
            Some(&Value::Bool(want)),
            get(dep, "violating"),
            "deps_of {v}"
        );
        let scene = edn::parse(&format!(
            r#"{{:classes [{{:id :a :rect {{:x 0 :y 0 :w 100 :h 50}}}}]
                :edges [{{:from :a :to :b :deps [{{:from :a :to :b :violating {v}}}]}}]}}"#
        ))
        .unwrap();
        let inds = scene::hit::dep_indicators(&scene);
        let ind = &edn::record::seq(Some(&inds))[0];
        assert_eq!(
            Some(&Value::Bool(want)),
            get(ind, "violating"),
            "indicator {v}"
        );
    }
}

#[test]
fn drill_is_lua_truthy_on_class_hits_and_layer_ids() {
    for (v, want) in [
        ("false", false),
        ("nil", false),
        ("true", true),
        (":yes", true),
        ("1", true),
    ] {
        let scene = edn::parse(&format!(
            r#"{{:classes [{{:id :a :drill? {v} :rect {{:x 0 :y 0 :w 100 :h 50}}}}]}}"#
        ))
        .unwrap();
        let h = scene::hit::at(&scene, 50.0, 4.0).unwrap();
        assert_eq!(Some(&Value::Bool(want)), get(&h, "drill?"), "hit {v}");
        let raw = edn::parse(&format!("{{:kind :class :id :a :drill? {v}}}")).unwrap();
        assert_eq!(
            want,
            scene::events::layer_id(Some(&raw)).is_some(),
            "layer_id {v}"
        );
    }
}

#[test]
fn hierarchical_is_lua_truthy_on_read_and_compile() {
    let doc = |v: &str| {
        format!(
            r#"{{:hierarchical {v} :prefix "x" :order [a b]
                :classes [{{:id :a :name "A" :ns "x.a"}} {{:id :b :name "B" :ns "x.b"}}]
                :edges [{{:from :b :to :a :kind :dependency}}]}}"#
        )
    };
    let want = drawn(&compiled(&doc("true")));
    for v in TRUTHY {
        assert_eq!(want, drawn(&compiled(&doc(v))), "{v}");
    }
}

#[test]
fn proposal_extra_is_lua_truthy_when_compiling_a_view() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [{:id :a} {:id :b}] :edges []
            :proposals [{:id :p :layers [{:id :l1 :nses [:a]} {:id :l2 :nses [:b]}]}]}"#,
    )
    .unwrap();
    let view = |extra: &str| {
        drawn(&scene::document::compile_view(
            &doc,
            &[],
            &[],
            &edn::parse(extra).unwrap(),
        ))
    };
    let real = view("{}");
    let proposed = view("{:proposal true}");
    assert_ne!(real, proposed, "fixture must tell the proposal apart");
    for v in FALSY {
        assert_eq!(real, view(&format!("{{:proposal {v}}}")), "{v}");
    }
    for v in TRUTHY {
        assert_eq!(proposed, view(&format!("{{:proposal {v}}}")), "{v}");
    }
}
