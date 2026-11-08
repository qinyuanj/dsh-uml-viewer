#[test]
fn below() {
    assert!(util::below(9));
    assert!(!util::below(10));
}
