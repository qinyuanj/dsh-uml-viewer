use edn::{Value, parse};
use scene::hit::{self, dep_indicators};

fn close(expected: &Value, actual: &Value, path: &str) {
    match expected {
        Value::Integer(_) | Value::Float(_) => {
            let a = edn::record::num(expected).unwrap();
            let b = edn::record::num(actual).unwrap_or(f64::NAN);
            assert!(b.is_finite(), "{path}: actual not finite");
            assert!((a - b).abs() <= 1e-6, "{path}: expected {a} actual {b}");
        }
        Value::Map(entries) => {
            let Value::Map(other) = actual else {
                panic!("{path}: expected map, got {actual:?}")
            };
            assert_eq!(entries.len(), other.len(), "{path}");
            for (k, v) in entries {
                close(v, actual.get(k).unwrap(), &format!("{path}{k:?}"));
            }
        }
        Value::Vector(items) => {
            let Value::Vector(other) = actual else {
                panic!("{path}: expected vector")
            };
            assert_eq!(items.len(), other.len(), "{path}");
            for (i, v) in items.iter().enumerate() {
                close(v, &other[i], &format!("{path}[{i}]"));
            }
        }
        _ => assert_eq!(expected, actual, "{path}"),
    }
}

#[test]
fn selects_ports_before_classes_and_the_last_edge_with_leaf_dependencies() {
    let s = parse(
        "{:classes [{:id :a :rect {:x 0 :y 0 :w 40 :h 20} :in-ports [{:id :p :rect {:x 0 :y 0 :w 20 :h 18}}]}] :packages [] :edges [{:from :a :to :b :points [[0 50] [100 50]]} {:from :x :to :y :points [[0 50] [100 50]] :strokes []}]}",
    )
    .unwrap();
    close(
        &parse("{:kind :port :id :p :parent :a :dir :in}").unwrap(),
        &hit::at(&s, 10.0, 10.0).unwrap(),
        "port",
    );
    close(
        &parse("{:kind :edge :from :x :to :y :deps [{:from :x :to :y :violating false}]}").unwrap(),
        &hit::at(&s, 50.0, 50.0).unwrap(),
        "edge",
    );
}

#[test]
fn collapsed_leaf_indicators_sit_above_classes_and_dummies_are_skipped() {
    let mut s = parse(
        "{:classes [{:id :child :rect {:x 10 :y 10 :w 40 :h 20}} {:id :container :dummy? true :rect {:x 0 :y 0 :w 80 :h 80}} {:id :other :rect {:x 0 :y 120 :w 40 :h 20}}] :packages [] :edges [{:from :container :to :other :deps [{:from :child :to :leaf :violating true}]}]}",
    )
    .unwrap();
    let indicators = dep_indicators(&s);
    edn::record::put(&mut s, "dep-indicators", indicators);
    let h = hit::at(&s, 30.0, 38.0).unwrap();
    assert_eq!(
        "dep",
        edn::record::local_name(edn::record::get(&h, "kind").unwrap())
            .unwrap_or_default()
            .to_owned()
    );
    assert_eq!(Some(&Value::keyword("child")), edn::record::get(&h, "id"));
    assert_eq!(Some(&Value::Bool(true)), edn::record::get(&h, "violating"));
    assert!(hit::at(&s, 75.0, 75.0).is_none());
    assert!(hit::class_by_id(&s, &Value::keyword("child")).is_some());
    assert!(hit::class_by_id(&s, &Value::keyword("missing")).is_none());
}

#[test]
fn selects_child_rows_outgoing_ports_class_boundaries_and_package_frames() {
    let s = parse(
        "{:classes [{:id :a :drill? true :rect {:x 10 :y 10 :w 100 :h 100} :lines [{:kind :text :text \"A\"} {:kind :rule} {:kind :child :id :leaf :drill? true}] :out-ports [{:id :p :rect {:x 20 :y 130 :w 40 :h 18}}]}] :packages [{:id :pkg :rect {:x 0 :y 0 :w 200 :h 200}}] :edges []}",
    )
    .unwrap();
    close(
        &parse("{:kind :child :id :leaf :parent :a :drill? true}").unwrap(),
        &hit::at(&s, 20.0, 56.0).unwrap(),
        "child",
    );
    close(
        &parse("{:kind :class :id :a :drill? true}").unwrap(),
        &hit::at(&s, 110.0, 110.0).unwrap(),
        "class",
    );
    close(
        &parse("{:kind :port :id :p :parent :a :dir :out}").unwrap(),
        &hit::at(&s, 30.0, 140.0).unwrap(),
        "port",
    );
    close(
        &parse("{:kind :package :id :pkg}").unwrap(),
        &hit::at(&s, 1.0, 1.0).unwrap(),
        "package",
    );
    assert!(hit::package_by_id(&s, &Value::keyword("pkg")).is_some());
}
