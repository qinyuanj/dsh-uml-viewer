// Integration tests live in a `tests` directory, which the walk skips.

#[test]
fn integration_classifies() {
    assert_eq!(crap_demo::classify(0), "zero");
}
