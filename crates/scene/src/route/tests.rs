use crate::geom::{self, Rect};
use crate::route::route;
use edn::parse;

fn points_of(scene: &edn::Value, edge: usize) -> Vec<[f64; 2]> {
    let edges = match edn::record::get(scene, "edges").unwrap() {
        edn::Value::Vector(v) => v,
        _ => panic!(),
    };
    let ps = match edn::record::get(&edges[edge], "points").unwrap() {
        edn::Value::Vector(v) => v,
        _ => panic!(),
    };
    ps.iter().map(|p| geom::vec2(p).unwrap()).collect()
}

fn close_points(want: &str, got: &[[f64; 2]]) {
    let edn::Value::Vector(items) = parse(want).unwrap() else {
        panic!()
    };
    let want: Vec<[f64; 2]> = items.iter().map(|p| geom::vec2(p).unwrap()).collect();
    assert_eq!(want.len(), got.len(), "point count: want {want:?}");
    for (w, g) in want.iter().zip(got) {
        assert!(
            (w[0] - g[0]).abs() <= 1e-6 && (w[1] - g[1]).abs() <= 1e-6,
            "want {want:?} got {got:?}"
        );
    }
}

#[test]
fn drops_unlaid_endpoints_and_preserves_metadata_and_head_kinds() {
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :missing} {:from :a :to :b :kind :implements :dashed? true}]} :classes [{:id :a :rect {:x 0 :y 0 :w 40 :h 20}} {:id :b :rect {:x 0 :y 80 :w 40 :h 20}} {:id :missing}]}",
    )
    .unwrap();
    route(&mut s);
    let edges = match edn::record::get(&s, "edges").unwrap() {
        edn::Value::Vector(v) => v,
        _ => panic!(),
    };
    assert_eq!(1, edges.len());
    assert_eq!(
        Some(&edn::Value::keyword("triangle")),
        edn::record::get(&edges[0], "head")
    );
    assert_eq!(
        Some(&edn::Value::Bool(true)),
        edn::record::get(&edges[0], "dashed?")
    );
}

#[test]
fn uses_the_last_equally_smooth_clear_detour_and_first_blocked_fallback() {
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :b}]} :classes [{:id :a :rect {:x 0 :y 0 :w 40 :h 20}} {:id :b :rect {:x 80 :y 0 :w 40 :h 20}} {:id :wall :rect {:x 50 :y 0 :w 20 :h 20}}]}",
    )
    .unwrap();
    route(&mut s);
    close_points("[[20 20] [20 40] [100 40] [100 20]]", &points_of(&s, 0));
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :b}]} :classes [{:id :a :rect {:x 0 :y 0 :w 40 :h 20}} {:id :b :rect {:x 80 :y 0 :w 40 :h 20}} {:id :wall :rect {:x -100 :y -100 :w 400 :h 400}}]}",
    )
    .unwrap();
    route(&mut s);
    close_points("[[40 10] [80 10]]", &points_of(&s, 0));
}

#[test]
fn same_rank_edges_route_below_the_rank_box() {
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :b :kind :dependency}]} :classes [{:id :a :rect {:x 0 :y 0 :w 40 :h 20} :rank 0} {:id :b :rect {:x 80 :y 0 :w 40 :h 20} :rank 0}]}",
    )
    .unwrap();
    route(&mut s);
    let pts = points_of(&s, 0);
    assert!(pts.len() >= 2);
    for p in &pts {
        assert!(p[1] >= -1e-9, "stays at or below the boxes: {pts:?}");
    }
}

#[test]
fn face_picks_the_nearest_rect_side() {
    let r = Rect::new(0.0, 0.0, 400.0, 100.0);
    assert_eq!(
        crate::route::Face::Left,
        crate::route::face(r, [-50.0, 50.0])
    );
    assert_eq!(
        crate::route::Face::Right,
        crate::route::face(r, [450.0, 50.0])
    );
    assert_eq!(
        crate::route::Face::Top,
        crate::route::face(r, [200.0, -30.0])
    );
    assert_eq!(
        crate::route::Face::Bottom,
        crate::route::face(r, [200.0, 150.0])
    );
}

fn crosses(p: [f64; 2], q: [f64; 2], r: Rect) -> bool {
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (pp, qq) in [
        (-dx, p[0] - r.x),
        (dx, r.right() - p[0]),
        (-dy, p[1] - r.y),
        (dy, r.bottom() - p[1]),
    ] {
        if pp == 0.0 {
            if qq <= 0.0 {
                return false;
            }
        } else if pp < 0.0 {
            t0 = t0.max(qq / pp);
        } else {
            t1 = t1.min(qq / pp);
        }
        if t0 >= t1 {
            return false;
        }
    }
    true
}

#[test]
fn a_long_edge_passes_intermediate_ranks_beside_their_boxes() {
    let d = parse(
        "{:packages [{:id :p :classes [{:id :a :name \"a\" :level 3} {:id :b :name \"b\" :level 2} {:id :c :name \"c\" :level 1} {:id :d :name \"d\" :level 0}]}] :edges [{:from :a :to :b} {:from :b :to :c} {:from :c :to :d} {:from :a :to :d}]}",
    )
    .unwrap();
    let mut s = crate::layout::layout(&d);
    route(&mut s);
    assert!(
        edn::record::get(&s, "lanes").is_none(),
        "lanes stay internal"
    );
    let classes = edn::record::seq(edn::record::get(&s, "classes"));
    let rect = |id: &str| {
        let c = classes
            .iter()
            .find(|c| edn::record::get(c, "id") == Some(&edn::Value::keyword(id)))
            .unwrap();
        Rect::from_value(edn::record::get(c, "rect").unwrap()).unwrap()
    };
    let pts = points_of(&s, 3);
    for id in ["b", "c"] {
        for w in pts.windows(2) {
            assert!(
                !crosses(w[0], w[1], rect(id)),
                "a -> d crosses {id}: {pts:?}"
            );
        }
    }
}

fn rect_of(scene: &edn::Value, id: &str) -> Rect {
    let classes = edn::record::seq(edn::record::get(scene, "classes"));
    let c = classes
        .iter()
        .find(|c| edn::record::get(c, "id") == Some(&edn::Value::keyword(id)))
        .unwrap();
    Rect::from_value(edn::record::get(c, "rect").unwrap()).unwrap()
}

fn assert_clear(scene: &edn::Value, pts: &[[f64; 2]], ids: &[&str]) {
    for id in ids {
        for w in pts.windows(2) {
            assert!(
                !crosses(w[0], w[1], rect_of(scene, id)),
                "crosses {id}: {pts:?}"
            );
        }
    }
}

#[test]
fn a_lane_stub_clears_a_taller_box_in_its_start_row() {
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :b}]} :lanes [{:edge 0 :points [[200 230] [200 290]]}] :classes [{:id :a :rank 2 :rect {:x 0 :y 0 :w 40 :h 20}} {:id :t :rank 2 :rect {:x 60 :y 0 :w 40 :h 100}} {:id :m :rank 1 :rect {:x 0 :y 240 :w 140 :h 40}} {:id :b :rank 0 :rect {:x 180 :y 400 :w 40 :h 20}}]}",
    )
    .unwrap();
    route(&mut s);
    let pts = points_of(&s, 0);
    assert!(pts.contains(&[200.0, 230.0]), "keeps its lane: {pts:?}");
    assert_clear(&s, &pts, &["t", "m"]);
}

#[test]
fn an_unranked_oval_is_not_part_of_a_rank_row() {
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :b}]} :lanes [{:edge 0 :points [[160 152] [160 254]]}] :classes [{:id :a :rank 2 :rect {:x 0 :y 0 :w 120 :h 42}} {:id :c :rank 1 :rect {:x 0 :y 182 :w 120 :h 42}} {:id :b :rank 0 :rect {:x 0 :y 364 :w 120 :h 42}} {:id :o :oval? true :rect {:x 300 :y 0 :w 80 :h 30}}]}",
    )
    .unwrap();
    route(&mut s);
    let pts = points_of(&s, 0);
    assert!(pts.contains(&[160.0, 152.0]), "keeps its lane: {pts:?}");
    assert_clear(&s, &pts, &["c"]);
}

#[test]
fn an_edge_bent_across_a_taller_row_neighbour_leaves_the_row_first() {
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :b}]} :classes [{:id :t :rank 1 :rect {:x 0 :y 0 :w 200 :h 558}} {:id :a :rank 1 :rect {:x 252 :y 0 :w 136 :h 108}} {:id :b :rank 0 :rect {:x 12 :y 756 :w 120 :h 162}}]}",
    )
    .unwrap();
    route(&mut s);
    let pts = points_of(&s, 0);
    assert!(pts[1][1] > 558.0, "drops below the row first: {pts:?}");
    assert_clear(&s, &pts, &["t"]);
}

#[test]
fn spreading_the_attachments_does_not_bend_an_edge_into_a_box() {
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :g} {:from :d :to :b} {:from :a :to :d} {:from :c :to :b} {:from :a :to :b}]} :classes [{:id :d :rank 5 :rect {:x 740 :y 84 :w 160 :h 252}} {:id :b :rank 5 :rect {:x 940 :y 84 :w 128 :h 126}} {:id :c :rank 5 :rect {:x 1108 :y 84 :w 128 :h 90}} {:id :a :rank 5 :rect {:x 1276 :y 124 :w 120 :h 72}} {:id :g :rank 3 :rect {:x 292 :y 1434 :w 136 :h 450}} {:id :s :rank 3 :rect {:x 468 :y 1434 :w 120 :h 72}} {:id :o :rank 2 :rect {:x 52 :y 2082 :w 80 :h 44}}]}",
    )
    .unwrap();
    route(&mut s);
    let pts = points_of(&s, 4);
    assert_clear(&s, &pts, &["c"]);
}

fn fell_back(scene: &edn::Value, edge: usize) -> bool {
    edn::record::flag(
        &edn::record::seq(edn::record::get(scene, "edges"))[edge],
        "lane-fallback",
    )
}

#[test]
fn a_lane_through_a_box_is_dropped_and_flagged() {
    let mut s = parse(
        "{:diagram {:edges [{:from :a :to :b}]} :lanes [{:edge 0 :points [[60 230] [60 290]]}] :classes [{:id :a :rank 2 :rect {:x 0 :y 0 :w 40 :h 20}} {:id :m :rank 1 :rect {:x 0 :y 240 :w 140 :h 40}} {:id :b :rank 0 :rect {:x 180 :y 400 :w 40 :h 20}}]}",
    )
    .unwrap();
    route(&mut s);
    assert!(!points_of(&s, 0).contains(&[60.0, 230.0]));
    assert!(fell_back(&s, 0));
    let mut kept = parse(
        "{:diagram {:edges [{:from :a :to :b}]} :lanes [{:edge 0 :points [[160 230] [160 290]]}] :classes [{:id :a :rank 2 :rect {:x 0 :y 0 :w 40 :h 20}} {:id :m :rank 1 :rect {:x 0 :y 240 :w 140 :h 40}} {:id :b :rank 0 :rect {:x 180 :y 400 :w 40 :h 20}}]}",
    )
    .unwrap();
    route(&mut kept);
    assert!(points_of(&kept, 0).contains(&[160.0, 230.0]));
    assert!(!fell_back(&kept, 0));
}
