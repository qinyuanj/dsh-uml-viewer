use crate::geom::{self, Rect};
use edn::{Value, parse};

fn rects(text: &str) -> Vec<Rect> {
    let Value::Vector(items) = parse(text).unwrap() else {
        panic!("rects must be a vector")
    };
    items.iter().map(|r| Rect::from_value(r).unwrap()).collect()
}

fn points(text: &str) -> Vec<[f64; 2]> {
    let Value::Vector(items) = parse(text).unwrap() else {
        panic!("points must be a vector")
    };
    items.iter().map(|p| geom::vec2(p).unwrap()).collect()
}

fn show(strokes: &[Vec<[f64; 2]>]) -> Value {
    Value::Vector(
        strokes
            .iter()
            .map(|s| Value::Vector(s.iter().map(|p| geom::point(*p)).collect()))
            .collect(),
    )
}

#[test]
fn gap_polyline_clips_merged_obstacles_with_jvm_padding() {
    let ps = points("[[0 0] [100 0]]");
    let rs = rects("[{:x 40 :y -10 :w 20 :h 20} {:x 55 :y -10 :w 15 :h 20}]");
    let got = show(&geom::gap_polyline(&ps, &rs, 5.0));
    let want = parse("[[[0 0] [35 0]] [[75 0] [100 0]]]").unwrap();
    assert_eq!(want, got);
}

#[test]
fn degenerate_segment_gaps_to_nothing() {
    assert!(geom::gap_segment([0.0, 0.0], [0.5, 0.0], &[], 0.0).is_empty());
}

#[test]
fn rect_algebra_matches_the_lua_helpers() {
    let r = Rect::from_value(&parse("{:x 10 :y 20 :w 30 :h 40}").unwrap()).unwrap();
    assert_eq!(40.0, r.right());
    assert_eq!(60.0, r.bottom());
    assert_eq!(25.0, r.cx());
    assert_eq!(40.0, r.cy());
    assert!(r.contains(10.0, 60.0));
    assert!(!r.contains(41.0, 20.0));
    assert_eq!(Rect::new(5.0, 15.0, 40.0, 50.0), r.inflate(5.0));
    let other = Rect::new(0.0, 0.0, 20.0, 25.0);
    assert!(r.overlaps(other));
    assert_eq!(
        Rect::new(0.0, 0.0, 40.0, 60.0),
        geom::union(&[Some(r), Some(other)]).unwrap()
    );
    assert_eq!(None, geom::union(&[]));
}

#[test]
fn clipped_strokes_never_hit_the_hidden_gap() {
    let ps: Vec<[f64; 2]> = vec![[0.0, 0.0], [100.0, 0.0]];
    let strokes = geom::gap_polyline(&ps, &[Rect::new(40.0, -10.0, 20.0, 20.0)], 5.0);
    assert_eq!(2, strokes.len());
    assert!(
        !geom::near_polyline([50.0, 0.0], &ps, 1.0)
            || !strokes
                .iter()
                .any(|s| geom::near_polyline([50.0, 0.0], s, 1.0))
    );
    assert!(
        strokes
            .iter()
            .any(|s| geom::near_polyline([20.0, 0.0], s, 1.0))
    );
}

fn gap_polyline_unfiltered(ps: &[[f64; 2]], rects: &[Rect], pad: f64) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    for seg in ps.windows(2) {
        for piece in geom::gap_segment(seg[0], seg[1], rects, pad) {
            match out.last_mut() {
                Some(last)
                    if geom::point_seg_dist(*last.last().unwrap(), piece[0], piece[0]) < 0.51 =>
                {
                    last.push(piece[1])
                }
                _ => out.push(vec![piece[0], piece[1]]),
            }
        }
    }
    out
}

#[test]
fn the_rect_filter_keeps_strokes_bit_for_bit() {
    let ps: Vec<[f64; 2]> = (0..400)
        .map(|i| {
            let t = i as f64 * 0.37;
            [
                t * 31.0 + (t * 0.7).sin() * 90.0,
                t * 53.0 + (t * 1.3).cos() * 40.0,
            ]
        })
        .chain([
            [5000.0, 6995.0],
            [5000.0, 7100.0],
            [5100.0, 7100.0],
            [6000.0, 0.0],
            [6000.0, 300.0],
            [6300.0, 300.0],
            [6300.0, 600.0],
            [6600.0, 600.0],
        ])
        .collect();
    let mut rs: Vec<Rect> = (0..30)
        .flat_map(|i| {
            (0..30).map(move |j| Rect::new(i as f64 * 180.0, j as f64 * 260.0, 120.0, 70.0))
        })
        .collect();
    rs.push(Rect::new(5005.0, 7000.0, 40.0, 40.0));
    rs.push(Rect::new(4900.0, 7105.0, 40.0, 40.0));
    rs.push(Rect::new(5900.0, 100.0, 95.0, 50.0));
    rs.push(Rect::new(6100.0, 305.0, 50.0, 50.0));
    rs.push(Rect::new(6400.0, 520.0, 50.0, 75.0));
    let got = geom::gap_polyline(&ps, &rs, 5.0);
    let want = gap_polyline_unfiltered(&ps, &rs, 5.0);
    assert!(got.len() > 10, "the edge is cut many times");
    let bits = |s: &[Vec<[f64; 2]>]| -> Vec<Vec<[u64; 2]>> {
        s.iter()
            .map(|r| r.iter().map(|p| [p[0].to_bits(), p[1].to_bits()]).collect())
            .collect()
    };
    assert_eq!(bits(&want), bits(&got));
}
