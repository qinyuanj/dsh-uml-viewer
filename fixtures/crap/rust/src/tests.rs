//! A `tests.rs` file module is test code: nothing here is scored.

use super::*;

fn helper() -> i32 {
    if true {
        1
    } else {
        0
    }
}

#[test]
fn classify_covers_some_branches() {
    assert_eq!(classify(-2), "negative even");
    assert_eq!(classify(5), "positive");
    assert_eq!(helper(), 1);
}

#[test]
fn sum_until_stops_at_the_limit() {
    assert_eq!(sum_until(&[1, 2, 3], 4), 4);
}

#[test]
fn shapes_and_parsing() {
    use crate::shapes::{Circle, Describe, Shape, Wrapper};
    assert_eq!(Shape::Square(2.0).area(), 4.0);
    assert_eq!(Shape::Circle(Circle { radius: 1.0 }).to_string(), "circle");
    assert_eq!((&Circle { radius: 2.0 }).describe(), "big");
    assert_eq!(Wrapper { inner: vec![3] }.first_or(0), 3);
    assert_eq!(parse::parse_pairs("1,2\nx\n3, 4").unwrap(), vec![(1, 2), (3, 4)]);
    assert_eq!(parse::drain(vec![4, 5, 6]), 5);
    assert_eq!(parse::outer(-3), 4);
    assert_eq!(geometry::distance((0.0, 0.0), (3.0, 4.0)), 5.0);
    assert_eq!(geometry::inner::clamp(12, 0, 10), 10);
    assert_eq!(fixture_helper() + test_only_helper(), 8);
}
