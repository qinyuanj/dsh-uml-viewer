use crate::common;

#[test]
fn no_view_of_the_self_hosted_diagram_drops_a_lane() {
    let mut routed = 0;
    for (focus, scene) in common::self_hosted_views_both() {
        let edges = edn::record::seq(edn::record::get(&scene, "edges"));
        let dropped: Vec<_> = edges
            .iter()
            .filter(|e| edn::record::flag(e, "lane-fallback"))
            .map(|e| (edn::record::get(e, "from"), edn::record::get(e, "to")))
            .collect();
        routed += edges.len();
        assert!(dropped.is_empty(), "{focus:?} dropped lanes: {dropped:?}");
    }
    assert!(routed > 0, "the sweep routes edges");
}
