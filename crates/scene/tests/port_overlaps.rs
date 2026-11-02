//! Port pills and foreign ovals stay off the class boxes in every view of
//! the self-hosted diagram, default and wide (#191), and each pill hangs on
//! its own side of its box so the link's head shows (#189).

use crate::common;

use edn::Value;
use edn::record::{flag, get, seq};
use scene::Rect;

fn rect(v: &Value) -> Option<Rect> {
    get(v, "rect").and_then(Rect::from_value)
}

fn id_rect(v: &Value) -> Option<(&Value, Rect)> {
    Some((get(v, "id")?, rect(v)?))
}

#[test]
fn no_view_of_the_self_hosted_diagram_draws_a_port_or_oval_on_a_box() {
    let mut drawn = 0;
    for (focus, scene) in common::self_hosted_views_both() {
        let classes = seq(get(&scene, "classes"));
        let boxes: Vec<(&Value, Rect)> = classes
            .iter()
            .filter(|c| !flag(c, "dummy?") && !flag(c, "foreign"))
            .filter_map(id_rect)
            .collect();
        let mut ports_and_ovals: Vec<(&Value, Rect)> = classes
            .iter()
            .filter(|c| flag(c, "foreign"))
            .filter_map(id_rect)
            .collect();
        for c in classes {
            let r = rect(c).unwrap();
            for p in seq(get(c, "in-ports")).iter().filter_map(id_rect) {
                assert!(p.1.bottom() < r.y, "{focus:?} in-port {p:?} below {r:?}");
                ports_and_ovals.push(p);
            }
            for p in seq(get(c, "out-ports")).iter().filter_map(id_rect) {
                assert!(p.1.y > r.bottom(), "{focus:?} out-port {p:?} above {r:?}");
                ports_and_ovals.push(p);
            }
        }
        // (port or oval, box it overlaps)
        let on_a_box: Vec<_> = ports_and_ovals
            .iter()
            .flat_map(|(p, pr)| {
                boxes
                    .iter()
                    .filter(|(_, br)| pr.overlaps(*br))
                    .map(move |(b, _)| (*p, *b))
            })
            .collect();
        drawn += ports_and_ovals.len();
        assert!(on_a_box.is_empty(), "{focus:?} on a box: {on_a_box:?}");
    }
    // An isolated view needs no ports; the sweep must still exercise them.
    assert!(drawn > 0, "the sweep draws ports or ovals");
}
