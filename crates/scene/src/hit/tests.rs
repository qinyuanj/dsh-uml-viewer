use crate::hit;
use edn::{Value, parse};

#[test]
fn connected_edges_are_those_of_the_box_not_of_its_collapsed_leaves() {
    let s = parse(
        "{:classes [{:id :child :rect {:x 10 :y 10 :w 40 :h 20}} {:id :container :dummy? true :rect {:x 0 :y 0 :w 80 :h 80}} {:id :other :rect {:x 0 :y 120 :w 40 :h 20}}] :packages [] :edges [{:from :container :to :other :deps [{:from :child :to :leaf :violating true}]}]}",
    )
    .unwrap();
    assert_eq!(
        1,
        hit::connected_edges(&s, &Value::keyword("container")).len()
    );
    assert_eq!(0, hit::connected_edges(&s, &Value::keyword("child")).len());
}
