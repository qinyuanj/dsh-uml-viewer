use edn::Value;
use edn::record::{get, seq};
use scene::Snapshot;
use scene::{Band, GRADE_WORST, metric_rating, rating};

const IR: &str = r#"{:title "Rated" :packages [{:id :p :label "P" :classes [
    {:id :good :name "Good" :ns "demo.good"}
    {:id :fair :name "Fair" :ns "demo.fair"}
    {:id :bare :name "Bare" :ns "demo.bare"}]}]}"#;

const CRAP: &str = r#"{:entries [
    {:namespace "demo.good" :name "go" :complexity 1 :coverage 100 :crap 2}
    {:namespace "demo.fair" :name "go" :complexity 4 :coverage 50 :crap 10}]}"#;

fn scene() -> Value {
    let doc = scene::ir::read_document(IR).unwrap();
    let snapshots = [
        Snapshot::new(".metrics/crap.edn", CRAP, Value::Integer(1)),
        Snapshot::new(
            ".metrics/mutate/demo/good.edn",
            r#"{:namespace "demo.good" :forms [{:id "defn/go" :killed 10 :survived 0}]}"#,
            Value::Integer(1),
        ),
        Snapshot::new(
            ".metrics/mutate/demo/fair.edn",
            r#"{:namespace "demo.fair" :forms [{:id "defn/go" :killed 17 :survived 3}]}"#,
            Value::Integer(1),
        ),
    ];
    scene::document::compile_view(&doc, &snapshots, &[], &Value::Map(vec![]))
}

fn class(scene: &Value, name: &str) -> Value {
    seq(get(scene, "classes"))
        .iter()
        .find(|c| get(c, "name") == Some(&Value::String(name.into())))
        .cloned()
        .unwrap()
}

#[test]
fn a_fully_measured_class_rates_green_at_the_best_grade() {
    let r = rating(&class(&scene(), "Good"));
    assert_eq!(
        (r.crap, r.mutation, r.band()),
        (Band::Green, Band::Green, Band::Green)
    );
    assert_eq!((r.crap_grade, r.mutation_grade), (10.0, 10.0));
    assert_eq!(r.grade(), Some(10.0));
    assert_eq!(r.class_grade(), Some(10.0));
    assert!(!r.missing);
}

#[test]
fn a_mid_class_rates_yellow_on_the_averaged_ramps() {
    let r = rating(&class(&scene(), "Fair"));
    assert_eq!(
        (r.crap, r.mutation, r.band()),
        (Band::Yellow, Band::Yellow, Band::Yellow)
    );
    assert_eq!(r.crap_grade, 7.75);
    assert!((r.mutation_grade - 3.25).abs() < 1e-9);
    assert!((r.grade().unwrap() - 5.5).abs() < 1e-9);
    assert!(!r.missing);
}

#[test]
fn an_unmeasured_class_is_red_missing_and_worst() {
    let r = rating(&class(&scene(), "Bare"));
    assert_eq!(
        (r.crap, r.mutation, r.band()),
        (Band::Red, Band::Red, Band::Red)
    );
    assert!(r.missing);
    assert_eq!(r.class_grade(), Some(GRADE_WORST));
    assert_eq!(Band::Red.name(), "red");
}

#[test]
fn the_worst_band_of_the_two_metrics_wins() {
    let r = rating(&edn::parse("{:crap {:mu 2} :killed 17 :survived 3}").unwrap());
    assert_eq!(
        (r.crap, r.mutation, r.band()),
        (Band::Green, Band::Yellow, Band::Yellow)
    );
    let r = rating(&edn::parse("{:crap {:mu 30} :killed 17 :survived 3}").unwrap());
    assert_eq!(r.band(), Band::Red);
    assert!(!r.missing);
    assert!(rating(&edn::parse("{:crap {:mu 2}}").unwrap()).missing);
}

#[test]
fn a_card_row_rates_by_its_kill_ratio_over_killed_survived_and_uncovered() {
    let r = metric_rating(Some(10.0), Some(3.0), Some(1.0), Some(0.0));
    assert_eq!(r.crap_grade, 7.75);
    assert_eq!(r.kill_ratio, Some(0.75));
    assert_eq!(r.mutation_grade, 1.0);
    assert_eq!(r.grade, Some(4.375));
    let none = metric_rating(None, None, None, None);
    assert_eq!((none.crap_grade, none.kill_ratio), (1.0, None));
    assert_eq!((none.mutation_grade, none.grade), (1.0, Some(1.0)));
    let uncovered = metric_rating(None, Some(9.0), None, Some(1.0));
    assert_eq!(uncovered.kill_ratio, Some(0.9));
}
