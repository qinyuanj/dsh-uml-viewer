//! Text width and class box lines — the layout_text_spec.lua oracle plus the
//! member-line rules exercised by viewer_spec.

use crate::layout::{CHAR_W, class_box_size, class_lines, text_w};
use edn::parse;
use edn::record::{get, local_name};

#[test]
fn jvm_width_for_ascii_two_byte_and_astral_text() {
    assert_eq!(8.0, CHAR_W);
    assert_eq!(0.0, text_w(None));
    assert_eq!(40.0, text_w(Some("Hello")));
    assert_eq!(24.0, text_w(Some("«α»")));
    // U+1F600 is one UTF-16 surrogate pair on the JVM: two units.
    assert_eq!(16.0, text_w(Some("\u{1F600}")));
}

#[test]
fn full_width_cjk_counts_as_two_half_width_characters() {
    assert_eq!(32.0, text_w(Some("中文")));
    assert_eq!(16.0 + 8.0 * 3.0 + 16.0, text_w(Some("类Box提")));
    assert_eq!(16.0, text_w(Some("，"))); // U+FF0C full-width comma
    assert_eq!(16.0, text_w(Some("。"))); // U+3002 CJK punctuation
    assert_eq!(16.0, text_w(Some("한"))); // Hangul syllable
    assert_eq!(8.0, text_w(Some("—"))); // U+2014 stays half width
    assert_eq!(8.0, text_w(Some("ｱ"))); // U+FF71 half-width katakana
}

#[test]
fn chinese_names_widen_class_boxes() {
    let (w, _, _) = class_box_size(&parse("{:name \"中文类名中文类名中文类名\"}").unwrap());
    assert_eq!(24.0 + 12.0 * 16.0, w);
}

#[test]
fn member_lines_show_stereotype_public_members_and_rules() {
    let c = parse(
        "{:id :a :name \"a\" :stereotype :interface :fields [{:text \"field\"}] :ops [{:text \"visible\"} {:text \"secret\" :private true}]}",
    )
    .unwrap();
    let lines = class_lines(&c);
    let texts: Vec<String> = lines
        .iter()
        .filter_map(|l| match get(l, "text") {
            Some(edn::Value::String(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(vec!["«interface»", "a", "field", "visible"], texts);
    let rules = lines
        .iter()
        .filter(|l| local_name(get(l, "kind").unwrap()).unwrap_or_default() == "rule")
        .count();
    assert_eq!(2, rules);
    let (_, h, _) = class_box_size(&c);
    // 24 base + name/stereo 2*18 + field/op 2*18 + rules 2*12.
    assert_eq!(24.0 + 4.0 * 18.0 + 2.0 * 12.0, h);
}

#[test]
fn hidden_members_and_children_replace_member_lines() {
    let c = parse("{:name \"a\" :contents [{:id :a.b :name \"B\" :drill? true}]}").unwrap();
    let lines = class_lines(&c);
    let kinds: Vec<String> = lines
        .iter()
        .map(|l| {
            local_name(get(l, "kind").unwrap())
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(vec!["name", "rule", "child"], kinds);
    let child = &lines[2];
    assert_eq!(Some(&edn::Value::keyword("a.b")), get(child, "id"),);
    let hidden = parse("{:name \"a\" :hide-members true :fields [{:text \"f\"}]}").unwrap();
    assert_eq!(1, class_lines(&hidden).len());
}

#[test]
fn box_width_grows_with_the_widest_line() {
    // 24 + 10 half-width cells is below the 120 floor.
    let (w, _, _) = class_box_size(&parse("{:name \"abcdefghij\"}").unwrap());
    assert_eq!(120.0, w);
    let (w, _, _) = class_box_size(&parse("{:name \"abcdefghijklmnopqrstuvw\"}").unwrap());
    assert_eq!(24.0 + 23.0 * 8.0, w);
}

fn row_of(scene: &edn::Value, id: &str) -> f64 {
    let c = edn::record::seq(get(scene, "classes"))
        .iter()
        .find(|c| get(c, "id") == Some(&edn::Value::keyword(id)))
        .unwrap();
    crate::geom::Rect::from_value(get(c, "rect").unwrap())
        .unwrap()
        .y
}

#[test]
fn one_level_stacks_dependents_above_their_dependencies() {
    // a -> b -> c in one level; d and e depend on each other, then on c.
    let d = parse(
        "{:packages [{:id :p :classes [{:id :a :name \"a\" :level 1} {:id :b :name \"b\" :level 1} {:id :c :name \"c\" :level 1} {:id :d :name \"d\" :level 1} {:id :e :name \"e\" :level 1} {:id :z :name \"z\" :level 0}]}] :edges [{:from :a :to :b} {:from :b :to :c} {:from :d :to :e} {:from :e :to :d} {:from :e :to :c} {:from :c :to :z}]}",
    )
    .unwrap();
    let s = crate::layout::layout(&d);
    let (a, b, c, z) = (
        row_of(&s, "a"),
        row_of(&s, "b"),
        row_of(&s, "c"),
        row_of(&s, "z"),
    );
    assert!(a < b && b < c && c < z, "a {a} b {b} c {c} z {z}");
    assert_eq!(row_of(&s, "d"), row_of(&s, "e"), "a cycle shares a row");
    assert_eq!(b, row_of(&s, "d"), "d and e sit one row above c");
}

#[test]
fn a_level_without_inner_dependencies_keeps_one_row() {
    let d = parse(
        "{:packages [{:id :p :classes [{:id :a :name \"a\" :level 1} {:id :b :name \"b\" :level 1} {:id :z :name \"z\" :level 0}]}] :edges [{:from :a :to :z} {:from :b :to :z}]}",
    )
    .unwrap();
    let s = crate::layout::layout(&d);
    assert_eq!(row_of(&s, "a"), row_of(&s, "b"));
}

#[test]
fn a_deep_chain_in_one_level_shares_its_top_row() {
    // a -> b -> c -> d -> e in one level: rows stop at three, so a and b
    // join c in the top row.
    let d = parse(
        "{:packages [{:id :p :classes [{:id :a :name \"a\" :level 0} {:id :b :name \"b\" :level 0} {:id :c :name \"c\" :level 0} {:id :d :name \"d\" :level 0} {:id :e :name \"e\" :level 0}]}] :edges [{:from :a :to :b} {:from :b :to :c} {:from :c :to :d} {:from :d :to :e}]}",
    )
    .unwrap();
    let s = crate::layout::layout(&d);
    let ys: Vec<f64> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|id| row_of(&s, id))
        .collect();
    assert_eq!(ys[0], ys[1], "{ys:?}");
    assert_eq!(ys[1], ys[2], "{ys:?}");
    assert!(ys[2] < ys[3] && ys[3] < ys[4], "{ys:?}");
}

#[test]
fn a_levels_package_puts_foreign_ovals_in_a_rank_below_its_lowest_level() {
    let d = parse(
        "{:packages [{:id :p :classes [{:id :a :name \"a\" :level 1} {:id :b :name \"b\" :level 0}]}] :foreign [{:id :std :name \"std\" :foreign true}] :edges [{:from :a :to :b} {:from :a :to :std}]}",
    )
    .unwrap();
    let s = crate::layout::layout(&d);
    let classes = edn::record::seq(get(&s, "classes"));
    let std = classes
        .iter()
        .find(|c| get(c, "id") == Some(&edn::Value::keyword("std")))
        .unwrap();
    assert!(get(std, "rank").is_some(), "the oval is ranked");
    assert!(
        row_of(&s, "std") > row_of(&s, "b"),
        "below the lowest level"
    );
    let lanes = edn::record::seq(get(&s, "lanes"));
    assert_eq!(1, lanes.len(), "a -> std passes b's rank in a lane");
}

fn rect_of(scene: &edn::Value, id: &str) -> crate::geom::Rect {
    let c = edn::record::seq(get(scene, "classes"))
        .iter()
        .find(|c| get(c, "id") == Some(&edn::Value::keyword(id)))
        .unwrap();
    crate::geom::Rect::from_value(get(c, "rect").unwrap()).unwrap()
}

#[test]
fn levels_are_further_apart_than_the_sub_rows_of_one_level() {
    // a -> b -> c: a and b share level 1 (two sub-rows), c is level 0.
    let d = parse(
        "{:packages [{:id :p :classes [{:id :a :name \"a\" :level 1} {:id :b :name \"b\" :level 1} {:id :c :name \"c\" :level 0}]}] :edges [{:from :a :to :b} {:from :b :to :c}]}",
    )
    .unwrap();
    let s = crate::layout::layout(&d);
    let (a, b, c) = (rect_of(&s, "a"), rect_of(&s, "b"), rect_of(&s, "c"));
    assert_eq!(140.0, b.y - a.bottom(), "sub-rows");
    assert_eq!(220.0, c.y - b.bottom(), "levels");
}

#[test]
fn edgeless_classes_of_a_levels_package_sit_in_a_grid_below() {
    // a -> b; c..g have no edges: four to a grid row, below b.
    let d = parse(
        "{:packages [{:id :p :classes [{:id :a :name \"a\" :level 1} {:id :b :name \"b\" :level 0} {:id :c :name \"c\" :level 1} {:id :d :name \"d\" :level 1} {:id :e :name \"e\" :level 0} {:id :f :name \"f\" :level 1} {:id :g :name \"g\" :level 0}]}] :edges [{:from :a :to :b}]}",
    )
    .unwrap();
    let s = crate::layout::layout(&d);
    let first = row_of(&s, "c");
    assert!(first > row_of(&s, "b"), "below the levels");
    for id in ["d", "e", "f"] {
        assert_eq!(first, row_of(&s, id), "{id} shares the first grid row");
    }
    assert!(row_of(&s, "g") > first, "the fifth starts a new row");
}

/// Rows of (id, width, lane?) and the neighbour lists for `nearby`.
fn nearby_of(
    rows: &[&[(&str, f64, bool)]],
    links: &[(&str, &str)],
) -> std::collections::HashMap<String, f64> {
    let rows: Vec<Vec<super::Slot>> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|(id, w, l)| (id.to_string(), *w, *l))
                .collect()
        })
        .collect();
    let mut nbr: std::collections::HashMap<String, Vec<String>> = rows
        .iter()
        .flatten()
        .map(|n| (n.0.clone(), Vec::new()))
        .collect();
    for (a, b) in links {
        nbr.get_mut(*a).unwrap().push(b.to_string());
        nbr.get_mut(*b).unwrap().push(a.to_string());
    }
    let gap = |a: bool, b: bool| match (a, b) {
        (true, true) => 12.0,
        (true, false) | (false, true) => 20.0,
        _ => 40.0,
    };
    super::nearby(&rows, &nbr, &gap)
}

#[test]
fn nearby_moves_a_box_toward_its_neighbour_but_not_the_widest_row() {
    // The top row packs to 380 wide; b slides under the far right box c.
    let x = nearby_of(
        &[
            &[
                ("a", 100.0, false),
                ("x", 100.0, false),
                ("c", 100.0, false),
            ],
            &[("b", 100.0, false)],
        ],
        &[("b", "c")],
    );
    assert_eq!(
        (0.0, 140.0, 280.0),
        (x["a"], x["x"], x["c"]),
        "widest row stays"
    );
    assert_eq!(280.0, x["b"], "b sits under c");
}

#[test]
fn nearby_never_moves_a_row_past_the_widest_packed_row() {
    // The top row packs to 340 wide. b (300 wide) wants its centre under
    // c's at 240, i.e. x = 90, but may only shift 340 - 300 = 40.
    let x = nearby_of(
        &[
            &[("a", 100.0, false), ("c", 200.0, false)],
            &[("b", 300.0, false)],
        ],
        &[("b", "c")],
    );
    assert_eq!((0.0, 140.0), (x["a"], x["c"]));
    assert_eq!(40.0, x["b"]);
}

#[test]
fn nearby_keeps_lanes_in_order_between_their_boxes() {
    // Both lanes lead toward z far left of the gap between a and c: they
    // stop at a's right edge plus 20 and keep their 12px spacing.
    let x = nearby_of(
        &[
            &[("z", 40.0, false)],
            &[
                ("a", 100.0, false),
                ("l1", 0.0, true),
                ("l2", 0.0, true),
                ("c", 100.0, false),
            ],
        ],
        &[("l1", "z"), ("l2", "z")],
    );
    assert_eq!(120.0, x["l1"]);
    assert_eq!(132.0, x["l2"]);
    assert!(x["l2"] + 20.0 <= x["c"], "lanes stay left of c");
}

/// Level-2 classes of [`wide_diagram`]: one rank far taller than wide.
const LEAVES: usize = 30;

/// A `:levels` package in the wide layout (#191): [`LEAVES`] level-2
/// classes on one hub, a level-0 base, an edgeless class, a foreign oval, and a long
/// edge from the first leaf to the base.
fn wide_diagram(aspect: f64) -> edn::Value {
    let leaves: String = (1..=LEAVES)
        .map(|i| format!("{{:id :a{i} :name \"leaf{i}\" :level 2}}"))
        .collect();
    let edges: String = (1..=LEAVES)
        .map(|i| format!("{{:from :a{i} :to :hub}}"))
        .collect();
    parse(&format!(
        "{{:direction :lr :wide {aspect} :packages [{{:id :p :classes [{leaves} {{:id :hub :name \"hub\" :level 1 :in-deps [{{:id :x.y :name \"y\"}}] :out-deps [{{:id :x.z :name \"z\"}}]}} {{:id :base :name \"base\" :level 0}} {{:id :lone :name \"lone\" :level 1}}]}}] :foreign [{{:id :std :name \"std\" :foreign true}}] :edges [{edges} {{:from :hub :to :base}} {{:from :a1 :to :base}} {{:from :base :to :std}}]}}"
    ))
    .unwrap()
}

fn class_of<'a>(scene: &'a edn::Value, id: &str) -> &'a edn::Value {
    edn::record::seq(get(scene, "classes"))
        .iter()
        .find(|c| get(c, "id") == Some(&edn::Value::keyword(id)))
        .unwrap()
}

/// Distinct left edges of the leaves: one per column they fill.
fn leaf_columns(scene: &edn::Value) -> Vec<f64> {
    let mut xs: Vec<f64> = (1..=LEAVES)
        .map(|i| rect_of(scene, &format!("a{i}")).x)
        .collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup();
    xs
}

/// The shelf each box sits on: the wide layout stacks its columns into
/// shelves separated by a gutter, and one shelf's boxes all cross its height
/// (#89).
fn shelf_bands(scene: &edn::Value) -> std::collections::HashMap<String, usize> {
    let key = |v: &edn::Value| edn::encode(v).unwrap();
    let mut boxes: Vec<(f64, f64, String)> = edn::record::seq(get(scene, "classes"))
        .iter()
        .filter(|c| !edn::record::flag(c, "dummy?"))
        .filter_map(|c| {
            let r = crate::geom::Rect::from_value(get(c, "rect")?)?;
            Some((r.y, r.bottom(), key(get(c, "id")?)))
        })
        .collect();
    boxes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut out = std::collections::HashMap::new();
    let (mut band, mut bottom) = (0, f64::NEG_INFINITY);
    for (y, b, id) in boxes {
        if y > bottom {
            band += 1;
            bottom = b;
        } else {
            bottom = bottom.max(b);
        }
        out.insert(id, band);
    }
    out
}

/// Whether the two ends of `e` sit on different shelves.
fn wraps(scene: &edn::Value, e: &edn::Value) -> bool {
    let bands = shelf_bands(scene);
    let key = |v: &edn::Value| edn::encode(v).unwrap();
    match (get(e, "from"), get(e, "to")) {
        (Some(f), Some(t)) => bands.get(&key(f)) != bands.get(&key(t)),
        _ => false,
    }
}

#[test]
fn a_wide_layout_folds_a_tall_rank_into_consecutive_columns() {
    let s = crate::compose::compile_diagram(&wide_diagram(crate::document::WIDE_ASPECT));
    assert!(leaf_columns(&s).len() >= 2, "{:?}", leaf_columns(&s));
    let mut ranks: Vec<i64> = edn::record::seq(get(&s, "classes"))
        .iter()
        .map(|c| edn::record::num(get(c, "rank").unwrap()).unwrap() as i64)
        .collect();
    ranks.sort();
    ranks.dedup();
    assert_eq!((0..ranks.len() as i64).collect::<Vec<_>>(), ranks);
    let edges = edn::record::seq(get(&s, "edges"));
    assert_eq!(LEAVES + 3, edges.len());
    for e in edges {
        assert!(!edn::record::flag(e, "lane-fallback"), "{e:?}");
        // A lane is reserved through the columns of one shelf; an edge that
        // wraps between shelves keeps none and may cross (#89).
        if wraps(&s, e) {
            continue;
        }
        let ps: Vec<[f64; 2]> = edn::record::seq(get(e, "points"))
            .iter()
            .filter_map(crate::geom::vec2)
            .collect();
        for c in edn::record::seq(get(&s, "classes")) {
            let id = get(c, "id");
            if id == get(e, "from") || id == get(e, "to") {
                continue;
            }
            let r = crate::geom::Rect::from_value(get(c, "rect").unwrap()).unwrap();
            let crossed = ps.windows(2).any(|w| {
                w[0][0].min(w[1][0]) < r.right()
                    && w[0][0].max(w[1][0]) > r.x
                    && w[0][1].min(w[1][1]) < r.bottom()
                    && w[0][1].max(w[1][1]) > r.y
            });
            assert!(
                !crossed,
                "{:?} -> {:?} across {id:?}",
                get(e, "from"),
                get(e, "to")
            );
        }
    }
    assert_eq!(
        s,
        crate::compose::compile_diagram(&wide_diagram(crate::document::WIDE_ASPECT))
    );
}

#[test]
fn the_target_aspect_decides_the_fold() {
    let wide = crate::compose::compile_diagram(&wide_diagram(crate::document::WIDE_ASPECT));
    let tall = crate::compose::compile_diagram(&wide_diagram(0.3));
    // The shelf search answers the target aspect with both knobs: how many
    // boxes a column stacks and where the strip wraps (#89). A tall canvas
    // therefore comes out narrower than a wide one.
    let shape = |s: &edn::Value| {
        let size = get(s, "size").unwrap();
        let n = |k: &str| edn::record::num(get(size, k).unwrap()).unwrap();
        n("w") / n("h")
    };
    assert!(
        shape(&tall) < shape(&wide),
        "a tall canvas keeps a taller diagram: {:.3} vs {:.3}",
        shape(&tall),
        shape(&wide)
    );
    assert!(leaf_columns(&wide).len() > 1);
}

#[test]
fn a_wide_layout_runs_its_levels_left_to_right_then_the_oval_and_the_grid() {
    let s = crate::compose::compile_diagram(&wide_diagram(crate::document::WIDE_ASPECT));
    // Reading order: the strip wraps into shelves (#89), so the rank a column
    // carries is the order - left to right, then the shelf below.
    let rank = |id: &str| edn::record::num(get(class_of(&s, id), "rank").unwrap()).unwrap();
    let first_leaf = (1..=LEAVES)
        .map(|i| rank(&format!("a{i}")))
        .fold(f64::INFINITY, f64::min);
    let (hub, base, std, lone) = (rank("hub"), rank("base"), rank("std"), rank("lone"));
    assert!(first_leaf > hub, "level 2 before level 1");
    assert!(hub > base, "level 1 before level 0");
    assert!(base > std, "the oval after level 0");
    assert!(std > lone, "the edgeless grid last");
    let hub = class_of(&s, "hub");
    let r = crate::geom::Rect::from_value(get(hub, "rect").unwrap()).unwrap();
    for (k, above) in [("in-ports", true), ("out-ports", false)] {
        let ports = edn::record::seq(get(hub, k));
        assert_eq!(1, ports.len());
        let p = crate::geom::Rect::from_value(get(&ports[0], "rect").unwrap()).unwrap();
        assert_eq!(above, p.bottom() < r.y, "{k} above the box");
        assert_eq!(!above, p.y > r.bottom(), "{k} below the box");
    }
}
