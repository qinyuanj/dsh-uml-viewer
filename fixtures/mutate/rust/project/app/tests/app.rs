#[test]
fn app() {
    assert_eq!(app::twice(3), 6);
    assert_eq!(app::more::half(8), 4);
    assert_eq!(extdep::id(1), 1);
    assert!(util::below(9) && !util::below(10));
}
