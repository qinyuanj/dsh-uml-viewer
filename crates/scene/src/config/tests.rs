use crate::config::{self, crap_band, mutation_band};
use edn::record::local_name;
use edn::{Value, parse};

fn band(v: &Value) -> &str {
    match v {
        Value::Keyword(n) => n.as_str(),
        _ => panic!("band must be a keyword"),
    }
}

#[test]
fn crap_band_boundaries_and_missing_is_red() {
    assert_eq!("red", band(&crap_band(None)));
    for (score, want) in [
        (0.0, "green"),
        (8.0, "green"),
        (8.1, "yellow"),
        (12.0, "yellow"),
        (12.1, "red"),
        (20.0, "red"),
        (100.0, "red"),
    ] {
        assert_eq!(want, band(&crap_band(Some(score))), "{score}");
    }
}

#[test]
fn mutation_band_boundaries_and_missing_is_red() {
    assert_eq!("red", band(&mutation_band(None)));
    for (score, want) in [
        (0.0, "red"),
        (0.8, "red"),
        (0.81, "yellow"),
        (0.9, "yellow"),
        (0.91, "green"),
        (1.0, "green"),
    ] {
        assert_eq!(want, band(&mutation_band(Some(score))), "{score}");
    }
}

#[test]
fn grades_ramp_between_fixed_thresholds() {
    for (score, want) in [
        (0.0, 10.0),
        (8.0, 10.0),
        (10.0, 7.75),
        (12.0, 5.5),
        (16.0, 3.25),
        (20.0, 1.0),
        (100.0, 1.0),
    ] {
        assert_eq!(want, config::crap_grade(Some(score)), "{score}");
    }
    for (score, want) in [(0.0, 1.0), (0.8, 1.0), (0.9, 5.5), (1.0, 10.0)] {
        assert_eq!(want, config::mutation_grade(Some(score)), "{score}");
    }
    assert!((config::mutation_grade(Some(0.95)) - 7.75).abs() < 1e-12);
    assert_eq!(1.0, config::crap_grade(None));
    assert_eq!(1.0, config::mutation_grade(None));
}

#[test]
fn risk_combined_grade_and_ratio() {
    assert_eq!(
        Some(6.0),
        config::crap_risk(Some(&parse("{:mu 6}").unwrap()))
    );
    assert_eq!(
        Some(12.0),
        config::crap_risk(Some(&parse("{:mu 6 :sigma 6}").unwrap()))
    );
    assert_eq!(None, config::crap_risk(Some(&parse("{}").unwrap())));
    assert_eq!(Some(5.5), config::combined_grade(Some(10.0), Some(1.0)));
    assert_eq!(Some(10.0), config::combined_grade(Some(10.0), None));
    assert_eq!(Some(1.0), config::combined_grade(None, Some(1.0)));
    assert_eq!(None, config::combined_grade(None, None));
    for (text, want) in [
        ("{:killed 4 :survived 0}", Some(1.0)),
        ("{:killed 4}", Some(1.0)),
        ("{:survived 3}", Some(0.0)),
        ("{:killed 8 :survived 2}", Some(0.8)),
        ("{:killed 9 :survived 1 :mut-gap 2}", Some(0.75)),
        ("{:killed 0 :survived 0}", None),
        ("{}", None),
    ] {
        assert_eq!(
            want,
            config::mutation_ratio(&parse(text).unwrap()),
            "{text}"
        );
    }
}

fn pool_crap(text: &str, rolled: bool) -> Value {
    let Value::Vector(classes) = parse(text).unwrap() else {
        panic!()
    };
    config::pool_crap(&classes, rolled).unwrap()
}

#[test]
fn pool_crap_counts_absent_metrics_as_red_gaps() {
    let cases: [(&str, &str, bool); 5] = [
        (
            "[{:ops [{:crap 2} {:crap 2} {:crap 2}]} {:ops [{:crap 20}]}]",
            "{:mu 6.5 :max 20 :sigma 7.8 :n 4}",
            false,
        ),
        (
            "[{:crap {:mu 2 :sigma 0 :max 2 :n 3}} {:crap {:mu 20 :sigma 0 :max 20 :n 1}}]",
            "{:mu 6.5 :max 20 :sigma 7.8 :n 4}",
            false,
        ),
        (
            "[{:crap {:mu 2 :sigma 0 :max 2 :n 10}} {:ops [{:name \"a\"} {:name \"b\"}]}]",
            "{:mu 5 :max 20 :sigma 6.7 :n 12}",
            false,
        ),
        (
            "[{:crap {:mu 2 :sigma 0 :max 2 :n 1}} {}]",
            "{:mu 11 :max 20 :sigma 9 :n 2}",
            false,
        ),
        (
            "[{:crap {:mu 2 :sigma 0 :max 2 :n 10} :ops [{:crap 20}]}]",
            "{:mu 2 :max 2 :sigma 0 :n 10}",
            true,
        ),
    ];
    for (input, want, rolled) in cases {
        assert_eq!(parse(want).unwrap(), pool_crap(input, rolled), "{input}");
    }
    assert_eq!(
        None,
        config::pool_crap(
            &match parse("[{:foreign true} {:shape :oval}]").unwrap() {
                Value::Vector(v) => v,
                _ => panic!(),
            },
            false
        )
    );
}

#[test]
fn pool_mutants_sums_counts_and_gaps() {
    let cases: [(&str, &str); 3] = [
        (
            "[{:killed 9 :survived 1} {:killed 1 :survived 1}]",
            "{:killed 10 :survived 2}",
        ),
        (
            "[{:killed 9 :survived 1} {:ops [{} {}]}]",
            "{:killed 9 :survived 1 :mut-gap 2}",
        ),
        (
            "[{:killed 9 :survived 1 :mut-gap 1} {:ops [{} {}]}]",
            "{:killed 9 :survived 1 :mut-gap 3}",
        ),
    ];
    for (input, want) in cases {
        let Value::Vector(classes) = parse(input).unwrap() else {
            panic!()
        };
        assert_eq!(
            parse(want).unwrap(),
            config::pool_mutants(&classes).unwrap(),
            "{input}"
        );
    }
    let Value::Vector(foreign) = parse("[{:foreign true} {:shape :oval}]").unwrap() else {
        panic!()
    };
    assert_eq!(None, config::pool_mutants(&foreign));
}

#[test]
fn summarize_scores_mixes_numbers_and_summaries() {
    let Value::Vector(scores) = parse("[2 {:mu 4} {}]").unwrap() else {
        panic!()
    };
    assert_eq!(
        parse("{:mu 3 :max 4 :sigma 1 :n 2}").unwrap(),
        config::summarize_scores(&scores).unwrap()
    );
    assert_eq!(None, config::summarize_scores(&[]));
}

#[test]
fn names_rendered_for_bands_stay_keywords() {
    assert_eq!(
        "red",
        local_name(&crap_band(None)).unwrap_or_default().to_owned()
    );
    assert_eq!(
        "green",
        local_name(&mutation_band(Some(1.0)))
            .unwrap_or_default()
            .to_owned()
    );
}
