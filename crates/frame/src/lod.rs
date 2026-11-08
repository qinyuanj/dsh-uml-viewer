//! Three-tier zoom LOD, viewport culling and geometry decimation (the #131
//! egui prototype's rules; ADR 0008 "即时模式加视口剔除、三档 LOD 已经够用").
//!
//! - `Blocks` (zoom < 0.15): filled silhouettes and package titles only.
//! - `Frames` (zoom < 0.4): frames, titles, C/M dots, edges decimated at 8px.
//! - `Full`: the complete class-box detail.

use scene::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Blocks = 0,
    Frames = 1,
    Full = 2,
}

pub const FRAMES_ZOOM: f64 = 0.15;
pub const FULL_ZOOM: f64 = 0.4;
/// Mid-tier edge decimation budget, in screen points.
pub const DECIMATE_PX: f64 = 8.0;
/// Smallest canvas text size drawn, in points: below it a label is a smear
/// of glyph quads (#143).
pub const MIN_TEXT_PX: f32 = 5.0;

pub fn tier(zoom: f64) -> Tier {
    if zoom < FRAMES_ZOOM {
        Tier::Blocks
    } else if zoom < FULL_ZOOM {
        Tier::Frames
    } else {
        Tier::Full
    }
}

/// Does a world-space rect intersect the world-space viewport?
pub fn visible(rect: Rect, viewport: Rect) -> bool {
    rect.overlaps(viewport)
}

/// Drop interior points closer than `min_dist` to the last kept point; the
/// first and last points always survive so edge ends stay attached.
pub fn decimate(points: &[[f64; 2]], min_dist: f64) -> Vec<[f64; 2]> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() / 2 + 1);
    out.push(points[0]);
    let mut last = points[0];
    for &p in &points[1..points.len() - 1] {
        let dx = p[0] - last[0];
        let dy = p[1] - last[1];
        if dx * dx + dy * dy >= min_dist * min_dist {
            out.push(p);
            last = p;
        }
    }
    out.push(points[points.len() - 1]);
    out
}

/// `decimate` plus a chord test: an interior point also goes when it lies
/// within `tolerance` of the line from the last kept point to its successor
/// (straight routed runs carry most of the points). Ends always survive.
pub fn simplify(points: &[[f64; 2]], min_dist: f64, tolerance: f64) -> Vec<[f64; 2]> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() / 4 + 2);
    out.push(points[0]);
    for i in 1..points.len() - 1 {
        let last = out[out.len() - 1];
        let (p, next) = (points[i], points[i + 1]);
        let (px, py) = (p[0] - last[0], p[1] - last[1]);
        if px * px + py * py < min_dist * min_dist {
            continue;
        }
        let (dx, dy) = (next[0] - last[0], next[1] - last[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 0.0 && (px * dy - py * dx).abs() / len < tolerance {
            continue;
        }
        out.push(p);
    }
    out.push(points[points.len() - 1]);
    out
}

/// Quantize a scaled font size to whole points (minimum 1) so egui's glyph
/// cache is shared across near-identical zooms.
pub fn font_px(size: f64, scale: f64) -> f32 {
    ((size * scale).round() as f32).max(1.0)
}
