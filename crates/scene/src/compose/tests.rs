use crate::compose::{compile_diagram, compile_document};
use crate::geom::{self, Rect};
use edn::{Value, parse};

fn seq<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    match edn::record::get(v, key) {
        Some(Value::Vector(items)) => items,
        _ => &[],
    }
}

#[test]
fn stacks_flat_diagrams_with_qualified_endpoints_and_prepared_strokes() {
    let s = compile_document(&parse(
        "{:title \"D\" :diagrams [{:packages [{:id :p :label \"P\" :classes [{:id :a} {:id :b}]}] :edges [{:from :a :to :b}]} {:packages [{:id :p :label \"P\" :classes [{:id :a} {:id :b}]}] :edges [{:from :a :to :b}]}]}",
    ).unwrap());
    assert_eq!(2, seq(&s, "sections").len());
    let edges = seq(&s, "edges");
    assert_eq!(2, edges.len());
    for (i, e) in edges.iter().enumerate() {
        let want_from = Value::keyword(&format!("d{i}-a"));
        let want_to = Value::keyword(&format!("d{i}-b"));
        assert_eq!(Some(&want_from), edn::record::get(e, "from"));
        assert_eq!(Some(&want_to), edn::record::get(e, "to"));
        assert!(!seq(e, "strokes").is_empty());
    }
    let tip_y = |e: &Value| geom::vec2(edn::record::get(e, "tip").unwrap()).unwrap()[1];
    assert!(tip_y(&edges[1]) > tip_y(&edges[0]));
    let h = edn::record::num(edn::record::get(edn::record::get(&s, "size").unwrap(), "h").unwrap())
        .unwrap();
    assert!(h > tip_y(&edges[1]));
}

#[test]
fn hide_edges_replaces_strokes_with_incoming_and_outgoing_indicators() {
    let s = compile_diagram(&parse(
        "{:hide-edges true :packages [{:id :p :label \"P\" :classes [{:id :a :name \"A\"} {:id :b :name \"B\"}]}] :edges [{:from :a :to :b :kind :dependency :violating true}]}",
    ).unwrap());
    let indicators = seq(&s, "dep-indicators");
    assert_eq!(2, indicators.len());
    for ind in indicators {
        assert_eq!(Some(&Value::Bool(true)), edn::record::get(ind, "violating"));
    }
}

#[test]
fn hide_edges_is_a_lua_truthy_flag() {
    let compiled = |hide: &str| {
        compile_diagram(&parse(&format!(
            "{{{hide} :packages [{{:id :p :label \"P\" :classes [{{:id :a :name \"A\"}} {{:id :b :name \"B\"}}]}}] :edges [{{:from :a :to :b :kind :dependency}}]}}"
        )).unwrap())
    };
    for hide in ["", ":hide-edges false", ":hide-edges nil"] {
        let s = compiled(hide);
        assert_eq!(None, edn::record::get(&s, "dep-indicators"), "{hide:?}");
    }
    for hide in [":hide-edges true", ":hide-edges :yes", ":hide-edges 1"] {
        assert_eq!(2, seq(&compiled(hide), "dep-indicators").len(), "{hide:?}");
    }
}

#[test]
fn prepare_scene_clips_strokes_around_a_wall() {
    let s = compile_diagram(&parse(
        "{:packages [{:id :p :label \"P\" :classes [{:id :a} {:id :wall} {:id :b}]}] :edges [{:from :a :to :b :kind :dependency}]}",
    ).unwrap());
    let edges = seq(&s, "edges");
    assert_eq!(1, edges.len());
    let strokes = seq(&edges[0], "strokes");
    assert!(!strokes.is_empty());
    let wall = seq(&s, "classes")
        .iter()
        .find(|c| edn::record::get(c, "id") == Some(&Value::keyword("wall")))
        .unwrap();
    let wr = Rect::from_value(edn::record::get(wall, "rect").unwrap())
        .unwrap()
        .inflate(4.9);
    for stroke in strokes {
        let pts: Vec<[f64; 2]> = match stroke {
            Value::Vector(items) => items.iter().map(|p| geom::vec2(p).unwrap()).collect(),
            _ => panic!(),
        };
        for w in pts.windows(2) {
            let mid = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
            assert!(
                !wr.contains(mid[0], mid[1]),
                "stroke enters the wall: {mid:?}"
            );
        }
    }
}

#[test]
fn single_diagram_document_compiles_through_the_same_path() {
    let s = compile_document(
        &parse("{:title \"Solo\" :packages [{:id :p :label \"P\" :classes [{:id :a}]}] :edges []}")
            .unwrap(),
    );
    assert_eq!(1, seq(&s, "sections").len());
    assert_eq!(1, seq(&s, "classes").len());
    assert_eq!(
        Some(&Value::keyword("d0-a")),
        edn::record::get(&seq(&s, "classes")[0], "id")
    );
}

const FALSY: [&str; 2] = ["false", "nil"];
const TRUTHY: [&str; 2] = [":yes", "1"];

fn encoded(v: &Value) -> String {
    edn::encode(v).unwrap()
}

fn drawn(scene: &Value) -> String {
    let get = edn::record::get;
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
fn dummy_false_lays_out_and_routes_like_an_ordinary_class() {
    let compiled = |flag: &str| {
        compile_diagram(&edn::parse(&format!(
            r#"{{:packages [{{:id :p :label "P" :classes [{{:id :a :name "A" {flag}}} {{:id :wall :name "W"}} {{:id :b :name "B"}}]}}]
                :edges [{{:from :a :to :b :kind :dependency}} {{:from :b :to :a :kind :dependency}}]}}"#
        )).unwrap())
    };
    let omitted = drawn(&compiled(""));
    let dummy = drawn(&compiled(":dummy? true"));
    assert_ne!(omitted, dummy, "fixture must tell a dummy apart");
    for v in FALSY {
        assert_eq!(omitted, drawn(&compiled(&format!(":dummy? {v}"))), "{v}");
    }
    for v in TRUTHY {
        assert_eq!(dummy, drawn(&compiled(&format!(":dummy? {v}"))), "{v}");
    }
}

#[test]
fn ports_move_with_their_class_when_a_section_is_stacked() {
    let s = compile_document(&parse(
        "{:packages [{:id :p :label \"P\" :classes [{:id :a :name \"A\" :in-deps [{:id :x :name \"X\"}] :out-deps [{:id :y :name \"Y\"}]}]}] :edges []}",
    ).unwrap());
    let a = &seq(&s, "classes")[0];
    let r = Rect::from_value(edn::record::get(a, "rect").unwrap()).unwrap();
    let port =
        |k: &str| Rect::from_value(edn::record::get(&seq(a, k)[0], "rect").unwrap()).unwrap();
    let (in_port, out_port) = (port("in-ports"), port("out-ports"));
    assert_eq!(r.y - 40.0, in_port.y);
    assert_eq!(r.bottom() + 40.0, out_port.y);
    assert_eq!(r.cx(), in_port.cx());
    assert_eq!(r.cx(), out_port.cx());
}
