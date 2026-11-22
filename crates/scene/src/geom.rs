use edn::Value;
use edn::record::{get, num, numval};

pub fn vec2(v: &Value) -> Option<[f64; 2]> {
    if let Value::Vector(items) = v
        && items.len() == 2
    {
        return Some([num(&items[0])?, num(&items[1])?]);
    }
    None
}

pub fn point(p: [f64; 2]) -> Value {
    Value::Vector(vec![numval(p[0]), numval(p[1])])
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn from_value(v: &Value) -> Option<Rect> {
        Some(Rect {
            x: num(get(v, "x")?)?,
            y: num(get(v, "y")?)?,
            w: num(get(v, "w")?)?,
            h: num(get(v, "h")?)?,
        })
    }

    pub fn to_value(self) -> Value {
        edn::record::record(vec![
            ("x", numval(self.x)),
            ("y", numval(self.y)),
            ("w", numval(self.w)),
            ("h", numval(self.h)),
        ])
    }

    pub fn right(self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(self) -> f64 {
        self.y + self.h
    }

    pub fn cx(self) -> f64 {
        self.x + self.w / 2.0
    }

    pub fn cy(self) -> f64 {
        self.y + self.h / 2.0
    }

    pub fn translate(self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    pub fn inflate(self, pad: f64) -> Rect {
        Rect::new(
            self.x - pad,
            self.y - pad,
            self.w + 2.0 * pad,
            self.h + 2.0 * pad,
        )
    }

    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }

    pub fn overlaps(self, other: Rect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Left,
    Right,
    Top,
    Bottom,
}

pub fn face(r: Rect, p: [f64; 2]) -> Face {
    let ds = [
        (p[0] - r.x).abs(),
        (p[0] - r.right()).abs(),
        (p[1] - r.y).abs(),
        (p[1] - r.bottom()).abs(),
    ];
    let mut bi = 0;
    for (i, d) in ds.iter().enumerate().skip(1) {
        if *d < ds[bi] {
            bi = i;
        }
    }
    [Face::Left, Face::Right, Face::Top, Face::Bottom][bi]
}

pub fn union(rects: &[Option<Rect>]) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for r in rects.iter().flatten() {
        out = Some(match out {
            None => *r,
            Some(b) => Rect {
                x: b.x.min(r.x),
                y: b.y.min(r.y),
                w: b.right().max(r.right()) - b.x.min(r.x),
                h: b.bottom().max(r.bottom()) - b.y.min(r.y),
            },
        });
    }
    out
}

pub fn point_seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let l = dx * dx + dy * dy;
    let mut t = 0.0;
    if l >= 1e-12 {
        t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l).clamp(0.0, 1.0);
    }
    let x = p[0] - a[0] - t * dx;
    let y = p[1] - a[1] - t * dy;
    (x * x + y * y).sqrt()
}

pub fn near_polyline(p: [f64; 2], ps: &[[f64; 2]], pad: f64) -> bool {
    ps.windows(2).any(|s| point_seg_dist(p, s[0], s[1]) <= pad)
}

pub fn near_triangle(p: [f64; 2], ps: &[[f64; 2]; 3], pad: f64) -> bool {
    let sign =
        |a: [f64; 2], b: [f64; 2]| (p[0] - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (p[1] - b[1]);
    let a = sign(ps[0], ps[1]);
    let b = sign(ps[1], ps[2]);
    let c = sign(ps[2], ps[0]);
    !((a < 0.0 || b < 0.0 || c < 0.0) && (a > 0.0 || b > 0.0 || c > 0.0))
        || near_polyline(p, &[*ps.first().unwrap(), ps[1], ps[2], ps[0]], pad)
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    point_seg_dist(a, b, b) < 0.51
}

fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
}

#[cfg(test)]
pub fn gap_segment(a: [f64; 2], b: [f64; 2], rects: &[Rect], pad: f64) -> Vec<[[f64; 2]; 2]> {
    let padded: Vec<Rect> = rects.iter().map(|r| r.inflate(pad)).collect();
    let mut out: Vec<[[f64; 2]; 2]> = Vec::new();
    clip_segment(a, b, &padded, &mut Scratch::default(), &mut |p, q| {
        out.push([p, q])
    });
    out
}

#[derive(Default)]
struct Scratch {
    removed: Vec<[f64; 2]>,
    merged: Vec<[f64; 2]>,
}

fn corners(ps: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    let (mut lo, mut hi) = (ps[0], ps[0]);
    for p in &ps[1..] {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    (lo, hi)
}

/// The box `lo..hi` lies strictly on one side of `r`. A segment inside such
/// a box gets no interval from the slab test in `clip_segment`: subtraction
/// and division round monotonically, so its entry parameter stays ≥ 1 or its
/// exit parameter ≤ 0. Skipping the rect therefore leaves the pieces
/// bit-for-bit unchanged.
fn apart(lo: [f64; 2], hi: [f64; 2], r: Rect) -> bool {
    hi[0] < r.x || lo[0] > r.right() || hi[1] < r.y || lo[1] > r.bottom()
}

fn clip_segment(
    a: [f64; 2],
    b: [f64; 2],
    padded: &[Rect],
    scratch: &mut Scratch,
    keep: &mut impl FnMut([f64; 2], [f64; 2]),
) {
    if near(a, b) {
        return;
    }
    let removed = &mut scratch.removed;
    removed.clear();
    for &r in padded {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let (mut lo, mut hi): (f64, f64) = (0.0, 1.0);
        let mut valid = true;
        for (p, q) in [
            (-dx, a[0] - r.x),
            (dx, r.right() - a[0]),
            (-dy, a[1] - r.y),
            (dy, r.bottom() - a[1]),
        ] {
            if p.abs() < 1e-12 {
                if q < 0.0 {
                    valid = false;
                    break;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    if t > hi {
                        valid = false;
                        break;
                    }
                    lo = lo.max(t);
                } else {
                    if t < lo {
                        valid = false;
                        break;
                    }
                    hi = hi.min(t);
                }
            }
        }
        if valid && lo < hi {
            removed.push([lo, hi]);
        }
    }
    removed.sort_by(|x, y| x[0].partial_cmp(&y[0]).unwrap());
    let merged = &mut scratch.merged;
    merged.clear();
    for &iv in removed.iter() {
        if let Some(last) = merged.last_mut()
            && iv[0] <= last[1] + 1e-9
        {
            last[1] = last[1].max(iv[1]);
            continue;
        }
        merged.push(iv);
    }
    let mut piece = |lo: f64, hi: f64| {
        let (p, q) = (lerp(a, b, lo), lerp(a, b, hi));
        if !near(p, q) {
            keep(p, q);
        }
    };
    let mut u = 0.0;
    for iv in merged.iter() {
        if u < iv[0] - 1e-9 {
            piece(u, iv[0]);
        }
        u = u.max(iv[1]);
    }
    if u < 0.999999 {
        piece(u, 1.0);
    }
}

const BLOCK: usize = 16;

pub fn gap_polyline(ps: &[[f64; 2]], rects: &[Rect], pad: f64) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    if ps.len() < 2 {
        return out;
    }
    if rects.is_empty() {
        return vec![ps.to_vec()];
    }
    let padded: Vec<Rect> = rects.iter().map(|r| r.inflate(pad)).collect();
    let (mut nearby, mut touching): (Vec<Rect>, Vec<Rect>) = (Vec::new(), Vec::new());
    let mut scratch = Scratch::default();
    let mut join = |p: [f64; 2], q: [f64; 2]| match out.last_mut() {
        Some(last) if near(*last.last().unwrap(), p) => last.push(q),
        _ => out.push(vec![p, q]),
    };
    for first in (0..ps.len() - 1).step_by(BLOCK) {
        let block = &ps[first..=(first + BLOCK).min(ps.len() - 1)];
        let (lo, hi) = corners(block);
        nearby.clear();
        nearby.extend(padded.iter().filter(|r| !apart(lo, hi, **r)));
        for seg in block.windows(2) {
            let (lo, hi) = corners(seg);
            touching.clear();
            touching.extend(nearby.iter().filter(|r| !apart(lo, hi, **r)));
            clip_segment(seg[0], seg[1], &touching, &mut scratch, &mut join);
        }
    }
    out
}

#[cfg(test)]
mod tests;
