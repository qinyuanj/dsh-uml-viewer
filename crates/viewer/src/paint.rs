//! egui painter for the pure command model: LOD selection, viewport culling,
//! the zoom transform and the canvas chrome. Strokes are fixed screen points;
//! only world geometry scales with zoom (the LOVE adapter's contract).

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align, Color32, FontFamily, FontId, Galley, Pos2, Rect as ERect, Stroke, StrokeKind};
use scene::Rect;
use scene::document::ViewState;

use crate::color::color;
use crate::commands::{self, Command, EdgeCommand};
use crate::render_cache::RenderCache;
use crate::theme::Role;
use crate::{lod, strings, theme};

pub fn canvas_bg() -> Color32 {
    color(Role::Canvas)
}

/// World -> screen (points): `screen = origin + world * scale`.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub origin: Pos2,
    pub scale: f32,
}

impl Transform {
    pub fn new(rect: ERect, state: &ViewState) -> Transform {
        let scale = (if state.zoom > 0.0 { state.zoom } else { 1.0 }) as f32;
        Transform {
            origin: Pos2::new(
                rect.min.x - state.cam_x as f32 * scale,
                rect.min.y - state.cam_y as f32 * scale,
            ),
            scale,
        }
    }

    pub fn pos(&self, p: [f64; 2]) -> Pos2 {
        Pos2::new(
            self.origin.x + p[0] as f32 * self.scale,
            self.origin.y + p[1] as f32 * self.scale,
        )
    }

    pub fn rect(&self, r: Rect) -> ERect {
        ERect::from_min_max(self.pos([r.x, r.y]), self.pos([r.right(), r.bottom()]))
    }

    /// The world rect currently visible inside `screen`.
    pub fn world_viewport(&self, screen: ERect) -> Rect {
        let scale = self.scale.max(1e-6);
        Rect::new(
            ((screen.min.x - self.origin.x) / scale) as f64,
            ((screen.min.y - self.origin.y) / scale) as f64,
            (screen.width() / scale) as f64,
            (screen.height() / scale) as f64,
        )
    }

    /// Screen point back to world coordinates (event input).
    pub fn world(&self, p: Pos2) -> (f64, f64) {
        let scale = self.scale.max(1e-6);
        (
            ((p.x - self.origin.x) / scale) as f64,
            ((p.y - self.origin.y) / scale) as f64,
        )
    }
}

fn ink(role: Role, dimmed: bool) -> Color32 {
    crate::color::ink(role, dimmed)
}

fn stroke_of(color: Color32, width: f32) -> Stroke {
    Stroke::new(width, color)
}

/// Measure a string at a font size through the live egui fonts.
pub fn measure(ctx: &egui::Context, size: f32, text: &str) -> f64 {
    ctx.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), FontId::monospace(size), Color32::WHITE)
            .size()
            .x as f64
    })
}

/// Canvas text layouts kept across frames (#143). Font sizes are whole
/// pixels, so zooming revisits a bounded set of (label, size) pairs; egui's
/// own galley cache keeps a layout for one frame only, so every size step
/// used to lay out every visible label again.
#[derive(Default)]
pub struct TextCache {
    galleys: HashMap<(String, u32, bool, bool, Color32), Arc<Galley>>,
    widths: HashMap<(String, u32), f32>,
    /// Pixels per point and atlas fill ratio at the last frame: a changed
    /// scale or a cleared atlas (fill dropped) invalidates every galley.
    epoch: Option<(f32, f32)>,
    layouts: u64,
}

/// Entries kept before the cache starts over (labels × sizes × styles).
const TEXT_CACHE_LIMIT: usize = 50_000;

impl TextCache {
    /// Call once per frame before drawing.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        let ppp = ctx.pixels_per_point();
        let fill = ctx.fonts(|fonts| fonts.font_atlas_fill_ratio());
        let stale = self.epoch.is_some_and(|(p, f)| p != ppp || fill < f);
        if stale || self.galleys.len() + self.widths.len() > TEXT_CACHE_LIMIT {
            self.galleys.clear();
            self.widths.clear();
        }
        self.epoch = Some((ppp, fill));
    }

    /// How many layouts were computed (cache misses).
    pub fn layouts(&self) -> u64 {
        self.layouts
    }

    /// The monospace advance of `text` at `size` (see [`measure`]).
    pub fn width(&mut self, ctx: &egui::Context, text: &str, size: f32) -> f32 {
        let key = (text.to_owned(), size.to_bits());
        if let Some(w) = self.widths.get(&key) {
            return *w;
        }
        self.layouts += 1;
        let w = measure(ctx, size, text) as f32;
        self.widths.insert(key, w);
        w
    }

    /// The laid-out label; centred galleys are centred on x = 0.
    pub fn galley(
        &mut self,
        ctx: &egui::Context,
        text: &str,
        size: f32,
        color: Color32,
        italic: bool,
        center: bool,
    ) -> Arc<Galley> {
        let key = (text.to_owned(), size.to_bits(), italic, center, color);
        if let Some(g) = self.galleys.get(&key) {
            return g.clone();
        }
        self.layouts += 1;
        let mut job = egui::text::LayoutJob::default();
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: FontId::new(size, FontFamily::Monospace),
                color,
                italics: italic,
                ..Default::default()
            },
        );
        job.halign = if center { Align::Center } else { Align::LEFT };
        let galley = ctx.fonts_mut(|fonts| fonts.layout_job(job));
        self.galleys.insert(key, galley.clone());
        galley
    }
}

/// `commands::fit` on whole pixels: the largest integer size not above
/// `size` whose text, bold offset and italic shear fit `budget`. Spec #134
/// requires integer font sizes; fractional fits miss egui's galley cache and
/// rasterize a new glyph size every zoom frame.
pub fn fit_px(
    ctx: &egui::Context,
    text: &mut TextCache,
    size: f32,
    label: &str,
    budget: f32,
    bold: bool,
    italic: bool,
) -> f32 {
    let mut width = |s: f32| {
        text.width(ctx, label, s)
            + if bold { s * 0.05 } else { 0.0 }
            + if italic { s * 0.2 } else { 0.0 }
    };
    let nominal = size.floor().max(1.0);
    let natural = width(nominal);
    if natural <= budget || natural <= 0.0 {
        return nominal;
    }
    let mut px = (nominal * budget.max(0.0) / natural)
        .floor()
        .min(nominal - 1.0);
    while px >= 1.0 && width(px) > budget {
        px -= 1.0;
    }
    px
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    ctx: &egui::Context,
    text: &mut TextCache,
    painter: &egui::Painter,
    x: f32,
    y: f32,
    label: &str,
    size: f32,
    color: Color32,
    italic: bool,
    bold: bool,
    center: bool,
    max_w: Option<f32>,
) {
    if label.is_empty() || size < 1.0 {
        return;
    }
    let size = max_w.map_or(size, |budget| {
        fit_px(ctx, text, size, label, budget, bold, italic)
    });
    // Below legibility a label is a smear of glyph quads (#143).
    if size < MIN_TEXT_PX {
        return;
    }
    // No wrap width: the fit keeps the label inside its budget, and centred
    // rows centre on x = 0 either way; a per-zoom wrap width would defeat
    // the cache.
    let galley = text.galley(ctx, label, size, color, italic, center);
    // Bold is a second pass with the adapter's 5% offset.
    for pass in 0..if bold { 2 } else { 1 } {
        let offset = if pass == 0 { 0.0 } else { size * 0.05 };
        painter.galley(Pos2::new(x + offset, y), galley.clone(), color);
    }
}

fn dashed_rect(painter: &egui::Painter, rect: ERect, stroke: Stroke) {
    let dash = theme::widths::DASH;
    let gap = theme::widths::DASH_GAP;
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    for pair in corners.windows(2) {
        for shape in egui::Shape::dashed_line(pair, stroke, dash, gap) {
            painter.add(shape);
        }
    }
}

fn ellipse(painter: &egui::Painter, rect: ERect, fill: Color32, stroke: Stroke) {
    let center = rect.center();
    let (rx, ry) = (rect.width() / 2.0, rect.height() / 2.0);
    let points: Vec<Pos2> = (0..32)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / 32.0;
            Pos2::new(center.x + rx * a.cos(), center.y + ry * a.sin())
        })
        .collect();
    painter.add(egui::Shape::convex_polygon(points, fill, stroke));
}

fn paint_head(
    painter: &egui::Painter,
    transform: &Transform,
    head: &str,
    tip: [f64; 2],
    behind: [f64; 2],
    stroke: Stroke,
    fill: Color32,
) {
    let p = transform.pos(tip);
    let b = transform.pos(behind);
    let (mut dx, mut dy) = (p.x - b.x, p.y - b.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        dx = 1.0;
        dy = 0.0;
    } else {
        dx /= len;
        dy /= len;
    }
    let (sin, cos) = (std::f32::consts::PI / 9.0).sin_cos();
    let l = 16.0 * transform.scale;
    let a = Pos2::new(
        p.x - l * (dx * cos - dy * sin),
        p.y - l * (dy * cos + dx * sin),
    );
    let c = Pos2::new(
        p.x - l * (dx * cos + dy * sin),
        p.y - l * (dy * cos - dx * sin),
    );
    match head {
        "triangle" => {
            painter.add(egui::Shape::convex_polygon(vec![p, a, c], fill, stroke));
        }
        "diamond" | "diamond-fill" => {
            let back = Pos2::new(p.x - l * dx, p.y - l * dy);
            painter.add(egui::Shape::convex_polygon(
                vec![p, a, back, c],
                fill,
                stroke,
            ));
        }
        _ => {
            painter.line_segment([p, a], stroke);
            painter.line_segment([p, c], stroke);
        }
    }
}

fn add_quad(mesh: &mut egui::Mesh, corners: [Pos2; 4], color: Color32) {
    let i = mesh.vertices.len() as u32;
    for v in corners {
        mesh.colored_vertex(v, color);
    }
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
}

fn add_rect(mesh: &mut egui::Mesh, r: ERect, color: Color32) {
    add_quad(
        mesh,
        [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
        ],
        color,
    );
}

/// A filled rect with an inside stroke, square-cornered.
fn add_boxed(mesh: &mut egui::Mesh, r: ERect, fill: Color32, line: Stroke) {
    add_rect(mesh, r, fill);
    let w = line.width.min(r.width() / 2.0).min(r.height() / 2.0);
    let c = line.color;
    add_rect(
        mesh,
        ERect::from_min_max(r.min, Pos2::new(r.max.x, r.min.y + w)),
        c,
    );
    add_rect(
        mesh,
        ERect::from_min_max(Pos2::new(r.min.x, r.max.y - w), r.max),
        c,
    );
    add_rect(
        mesh,
        ERect::from_min_max(
            Pos2::new(r.min.x, r.min.y + w),
            Pos2::new(r.min.x + w, r.max.y - w),
        ),
        c,
    );
    add_rect(
        mesh,
        ERect::from_min_max(
            Pos2::new(r.max.x - w, r.min.y + w),
            Pos2::new(r.max.x, r.max.y - w),
        ),
        c,
    );
}

fn add_disc(mesh: &mut egui::Mesh, center: Pos2, radius: f32, color: Color32) {
    if radius <= 0.0 {
        return;
    }
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(center, color);
    for k in 0..8 {
        let a = std::f32::consts::TAU * k as f32 / 8.0;
        mesh.colored_vertex(center + radius * egui::vec2(a.cos(), a.sin()), color);
    }
    for k in 0..8 {
        mesh.add_triangle(i, i + 1 + k, i + 1 + (k + 1) % 8);
    }
}

/// Below this zoom Full-tier strokes also take the unfeathered mesh: a
/// 2000-class view at zoom 0.4 strokes ~300k points (#143).
const MESH_EDGE_SCALE: f32 = 0.6;

/// Corner radius on screen; under 2 px it is invisible and square corners
/// skip egui's arc tessellation.
fn corner(radius: f32, scale: f32) -> f32 {
    let r = radius * scale;
    if r < 2.0 { 0.0 } else { r }
}

/// A mid-tier class name drawn above the overview mesh: position, label,
/// size, colour, [italic, bold, centred], width budget.
type DeferredName<'a> = (Pos2, &'a str, f32, Color32, [bool; 3], f32);

/// Smallest canvas text size drawn, in points.
const MIN_TEXT_PX: f32 = lod::MIN_TEXT_PX;

/// Screen-space chrome text size, in points.
const CHROME_PX: f32 = theme::fonts::CHROME;

/// Append a polyline as butt-ended quads (one per segment) to `mesh`.
fn add_segments(
    mesh: &mut egui::Mesh,
    points: impl Iterator<Item = Pos2>,
    width: f32,
    color: Color32,
) {
    let half = width / 2.0;
    let mut last: Option<Pos2> = None;
    for p in points {
        if let Some(a) = last {
            let d = p - a;
            let len = d.length();
            if len > 0.0 {
                let n = egui::vec2(-d.y, d.x) * (half / len);
                add_quad(mesh, [a + n, a - n, p - n, p + n], color);
            }
        }
        last = Some(p);
    }
}

fn point_visible(p: [f64; 2], viewport: Rect) -> bool {
    Rect::new(p[0], p[1], 0.0, 0.0).overlaps(viewport)
}

fn paint_edges(
    painter: &egui::Painter,
    transform: &Transform,
    viewport: Rect,
    cache: &RenderCache,
    tier: lod::Tier,
) {
    // Mid-tier strokes: one unfeathered quad mesh. Feathered tessellation of
    // the ~56k decimated points of a 500-class overview cost ~15 ms a frame
    // (#143); at overview zoom the 1.6 pt lines need no anti-aliasing.
    // Blocks (smallest zoom) draw no edges (spec #134 渲染).
    if tier == lod::Tier::Blocks {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for (command, polyline) in cache.edges() {
        if command.tier() > tier {
            continue;
        }
        match command {
            EdgeCommand::Stroke {
                color,
                width,
                dimmed,
                ..
            } if tier == lod::Tier::Frames || transform.scale < MESH_EDGE_SCALE => {
                let Some(line) = polyline else {
                    continue;
                };
                if line.bounds.overlaps(viewport) {
                    let color = ink(*color, *dimmed);
                    let screen = line.points.iter().map(|p| transform.pos(*p));
                    add_segments(&mut mesh, screen, *width, color);
                }
            }
            EdgeCommand::Stroke {
                color,
                width,
                dimmed,
                ..
            } => {
                let Some(line) = polyline else {
                    continue;
                };
                if !line.bounds.overlaps(viewport) {
                    continue;
                }
                let screen: Vec<Pos2> = line.points.iter().map(|p| transform.pos(*p)).collect();
                painter.add(egui::Shape::line(
                    screen,
                    stroke_of(ink(*color, *dimmed), *width),
                ));
            }
            EdgeCommand::Head {
                head,
                tip,
                behind,
                color,
                fill,
                width,
                dimmed,
                ..
            } => {
                if !point_visible(*tip, viewport) && !point_visible(*behind, viewport) {
                    continue;
                }
                paint_head(
                    painter,
                    transform,
                    head,
                    *tip,
                    *behind,
                    stroke_of(ink(*color, *dimmed), *width),
                    ink(*fill, *dimmed),
                );
            }
            EdgeCommand::Indicator {
                points,
                color,
                dimmed,
                ..
            } => {
                let fill = ink(*color, *dimmed);
                let screen: Vec<Pos2> = points.iter().map(|p| transform.pos(*p)).collect();
                painter.add(egui::Shape::convex_polygon(
                    screen,
                    fill,
                    stroke_of(fill, 1.0),
                ));
            }
        }
    }
    if !mesh.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// Paint one frame of the canvas: background, geometry and chrome.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    ctx: &egui::Context,
    painter: &egui::Painter,
    state: &ViewState,
    screen: ERect,
    hint: Option<&str>,
    status: Option<&str>,
    performance: Option<String>,
    show_performance: bool,
    cache: &mut RenderCache,
    text: &mut TextCache,
) {
    text.begin_frame(ctx);
    painter.rect_filled(screen, 0.0, canvas_bg());
    let transform = Transform::new(screen, state);
    let tier = lod::tier(if state.zoom > 0.0 { state.zoom } else { 1.0 });
    let viewport = transform.world_viewport(screen);
    let clipped = painter.with_clip_rect(screen);

    // Geometry is rebuilt only on scene/selection/hover/tier change.
    cache.frame(state, tier);
    // Mid-tier overview: class boxes and C/M dots go into one unfeathered
    // mesh and names are drawn above it. Feathered tessellation of 500 boxes
    // and 1000 dots alone cost ~10 ms a frame (#143); at overview zoom the
    // sub-pixel corner radii and anti-aliasing are invisible.
    let overview = tier != lod::Tier::Full;
    let mut boxes = egui::Mesh::default();
    let mut names: Vec<DeferredName> = Vec::new();
    for (bounds, command) in cache.commands() {
        if command.tier() > tier || bounds.is_some_and(|b| !lod::visible(b, viewport)) {
            continue;
        }
        match command {
            Command::Package {
                rect,
                text: label,
                fill,
                stroke,
                stroke_width,
                ink: title,
                missing,
                dimmed,
                max_w,
                ..
            } => {
                let r = transform.rect(*rect);
                if r.width() < 1.0 || r.height() < 1.0 {
                    continue;
                }
                let corner = corner(theme::radius::PACKAGE, transform.scale);
                painter.rect_filled(r, corner, ink(*fill, *dimmed));
                let line = stroke_of(ink(*stroke, *dimmed), *stroke_width);
                if *missing {
                    dashed_rect(&clipped, r, line);
                } else {
                    painter.rect_stroke(r, corner, line, StrokeKind::Inside);
                }
                let budget = (*max_w * transform.scale as f64) as f32;
                draw_text(
                    ctx,
                    text,
                    &clipped,
                    r.min.x + commands::PAD as f32 * transform.scale,
                    r.min.y + 8.0 * transform.scale,
                    label,
                    lod::font_px(theme::fonts::CANVAS as f64 * transform.scale as f64, 1.0),
                    ink(*title, *dimmed),
                    true,
                    false,
                    false,
                    Some(budget),
                );
            }
            Command::Class {
                rect,
                shape,
                fill,
                stroke,
                stroke_width,
                missing,
                dimmed,
                ..
            } => {
                let r = transform.rect(*rect);
                if r.width() < 1.0 || r.height() < 1.0 {
                    continue;
                }
                let corner = corner(theme::radius::CLASS, transform.scale);
                let line_color = ink(*stroke, *dimmed);
                let width = *stroke_width;
                if overview && shape != "oval" && !*missing {
                    add_boxed(
                        &mut boxes,
                        r,
                        ink(*fill, *dimmed),
                        stroke_of(line_color, width),
                    );
                } else if shape == "oval" {
                    ellipse(
                        &clipped,
                        r,
                        ink(*fill, *dimmed),
                        stroke_of(line_color, width),
                    );
                } else {
                    painter.rect_filled(r, corner, ink(*fill, *dimmed));
                    if *missing {
                        dashed_rect(&clipped, r, stroke_of(line_color, width));
                    } else {
                        painter.rect_stroke(
                            r,
                            corner,
                            stroke_of(line_color, width),
                            StrokeKind::Inside,
                        );
                    }
                }
            }
            Command::Quality {
                x,
                y,
                radius,
                fill,
                stroke,
                ink: letter,
                text: label,
                dimmed,
                ..
            } => {
                let center = transform.pos([*x, *y]);
                let r = *radius as f32 * transform.scale;
                if overview {
                    let half = theme::widths::DOT / 2.0;
                    add_disc(&mut boxes, center, r + half, ink(*stroke, *dimmed));
                    add_disc(&mut boxes, center, r - half, ink(*fill, *dimmed));
                    continue;
                }
                painter.circle_filled(center, r, ink(*fill, *dimmed));
                painter.circle_stroke(
                    center,
                    r,
                    stroke_of(ink(*stroke, *dimmed), theme::widths::DOT),
                );
                if tier == lod::Tier::Full {
                    // Centred on the dot by the laid-out row height, which
                    // scales with the zoom.
                    let px = lod::font_px(9.0 * transform.scale as f64, 1.0);
                    let color = ink(*letter, *dimmed);
                    let row = text.galley(ctx, label, px, color, false, true).size().y;
                    draw_text(
                        ctx,
                        text,
                        &clipped,
                        center.x,
                        center.y - row / 2.0,
                        label,
                        px,
                        color,
                        false,
                        false,
                        true,
                        None,
                    );
                }
            }
            Command::Marker {
                x,
                y,
                text: label,
                size,
                color,
                dimmed,
                ..
            } => {
                let p = transform.pos([*x, *y]);
                draw_text(
                    ctx,
                    text,
                    &clipped,
                    p.x,
                    p.y,
                    label,
                    lod::font_px(*size * transform.scale as f64, 1.0),
                    ink(*color, *dimmed),
                    false,
                    false,
                    false,
                    None,
                );
            }
            Command::Level {
                x,
                y,
                text: label,
                size,
                color,
                dimmed,
                ..
            } => {
                let p = transform.pos([*x, *y]);
                draw_text(
                    ctx,
                    text,
                    &clipped,
                    p.x,
                    p.y,
                    label,
                    lod::font_px(*size * transform.scale as f64, 1.0),
                    ink(*color, *dimmed),
                    false,
                    false,
                    false,
                    None,
                );
            }
            Command::Band {
                x,
                w,
                y,
                color,
                dimmed,
                ..
            } => {
                let a = transform.pos([*x, *y]);
                let b = transform.pos([x + w, *y]);
                painter.line_segment([a, b], stroke_of(ink(*color, *dimmed), theme::widths::BAND));
            }
            Command::Rule {
                x,
                y,
                w,
                color,
                dimmed,
                ..
            } => {
                let a = transform.pos([x + 6.0, *y]);
                let b = transform.pos([x + w - 6.0, *y]);
                painter.line_segment([a, b], stroke_of(ink(*color, *dimmed), theme::widths::RULE));
            }
            Command::Text {
                italic,
                bold,
                center,
                x,
                y,
                text: label,
                size,
                top,
                h,
                max_w,
                ink: text_ink,
                dimmed,
                ..
            } => {
                // A slotted line draws at its slot height: text_slots scales
                // a crowded run down together, and the nominal size would
                // overflow onto the title band and the next line.
                let px = lod::font_px(h.unwrap_or(*size) * transform.scale as f64, 1.0);
                // Text below legibility is skipped (the #131 prototype drew
                // no text under 4 px).
                if px < MIN_TEXT_PX {
                    continue;
                }
                let top_px = transform.pos([*x, top.unwrap_or(*y)]).y;
                let slot_px = h.unwrap_or(*size) as f32 * transform.scale;
                let base_y = top_px + ((slot_px - px) / 2.0).max(0.0);
                let cx = if *center {
                    transform.pos([x + max_w / 2.0, 0.0]).x
                } else {
                    transform.pos([*x, 0.0]).x
                };
                let color = ink(*text_ink, *dimmed);
                let budget = (*max_w * transform.scale as f64) as f32;
                if overview {
                    names.push((
                        Pos2::new(cx, base_y),
                        label,
                        px,
                        color,
                        [*italic, *bold, *center],
                        budget,
                    ));
                    continue;
                }
                draw_text(
                    ctx,
                    text,
                    &clipped,
                    cx,
                    base_y,
                    label,
                    px,
                    color,
                    *italic,
                    *bold,
                    *center,
                    Some(budget),
                );
            }
            Command::NoData {
                right,
                y,
                text: label,
                size,
                color,
                dimmed,
                ..
            } => {
                let px = lod::font_px(*size * transform.scale as f64, 1.0);
                let p = transform.pos([*right, *y]);
                let width = text.width(ctx, label, px);
                draw_text(
                    ctx,
                    text,
                    &clipped,
                    p.x - width,
                    p.y,
                    label,
                    px,
                    ink(*color, *dimmed),
                    false,
                    false,
                    false,
                    None,
                );
            }
            Command::ChildWash {
                rect,
                marker,
                dimmed,
                ..
            } => {
                let r = transform.rect(*rect);
                painter.rect_filled(
                    r,
                    corner(theme::radius::ROW_WASH, transform.scale),
                    ink(marker.wash, *dimmed),
                );
                painter.rect_filled(
                    ERect::from_min_size(
                        r.min,
                        egui::Vec2::new(marker.bar_width * transform.scale, r.height()),
                    ),
                    0.0,
                    ink(marker.bar, *dimmed),
                );
            }
            Command::PortLink {
                behind,
                tip,
                color,
                dimmed,
                ..
            } => {
                let color = ink(*color, *dimmed);
                let a = transform.pos(*behind);
                let b = transform.pos(*tip);
                painter.line_segment([a, b], stroke_of(color, theme::widths::PORT_LINK));
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.0 {
                    let (dx, dy) = (dx / len, dy / len);
                    let (si, co) = (std::f32::consts::PI / 9.0).sin_cos();
                    for sign in [-1.0f32, 1.0] {
                        let end = Pos2::new(
                            b.x - 10.0 * (dx * co - sign * dy * si),
                            b.y - 10.0 * (dy * co + sign * dx * si),
                        );
                        painter.line_segment([b, end], stroke_of(color, theme::widths::PORT_LINK));
                    }
                }
            }
            Command::Port {
                rect,
                text: label,
                fill,
                stroke,
                stroke_width,
                ink: label_ink,
                dimmed,
                ..
            } => {
                let r = transform.rect(*rect);
                let corner = corner(theme::radius::PORT, transform.scale);
                painter.rect_filled(r, corner, ink(*fill, *dimmed));
                painter.rect_stroke(
                    r,
                    corner,
                    stroke_of(ink(*stroke, *dimmed), *stroke_width),
                    StrokeKind::Inside,
                );
                draw_text(
                    ctx,
                    text,
                    &clipped,
                    r.center().x,
                    r.center().y - 5.0 * transform.scale,
                    label,
                    lod::font_px(theme::fonts::PORT as f64 * transform.scale as f64, 1.0),
                    ink(*label_ink, *dimmed),
                    false,
                    false,
                    true,
                    Some(r.width() - 6.0 * transform.scale),
                );
            }
            _ => {}
        }
    }

    if !boxes.is_empty() {
        clipped.add(egui::Shape::mesh(boxes));
    }
    for (at, label, px, color, [italic, bold, center], budget) in names {
        draw_text(
            ctx,
            text,
            &clipped,
            at.x,
            at.y,
            label,
            px,
            color,
            italic,
            bold,
            center,
            Some(budget),
        );
    }
    paint_edges(&clipped, &transform, viewport, cache, tier);

    // Screen-space chrome: banner, return scope, popup (view units = points).
    let chrome = frame::frame::chrome(
        state,
        screen.width() as f64,
        screen.height() as f64,
        hint,
        status,
    );
    for command in chrome {
        match command {
            Command::Title {
                text: label,
                x,
                y,
                color: role,
            }
            | Command::ProposalBanner {
                text: label,
                x,
                y,
                color: role,
            }
            | Command::Back {
                text: label,
                x,
                y,
                color: role,
            } => {
                let color = color(role);
                let at = screen.min + egui::vec2(x as f32, y as f32);
                // A plate keeps the scope legible over the diagram beneath.
                let size = text
                    .galley(ctx, &label, CHROME_PX, color, false, false)
                    .size();
                painter.rect_filled(
                    egui::Rect::from_min_size(at, size).expand2(egui::vec2(6.0, 2.0)),
                    3.0,
                    crate::color::color(Role::ChromePlate),
                );
                draw_text(
                    ctx, text, painter, at.x, at.y, &label, CHROME_PX, color, false, false, false,
                    None,
                )
            }
            Command::Status {
                text: label,
                x,
                y,
                color: role,
            } => draw_text(
                ctx,
                text,
                painter,
                screen.min.x + x as f32,
                screen.min.y + y as f32,
                &label,
                CHROME_PX,
                color(role),
                false,
                false,
                false,
                None,
            ),
            Command::Hint {
                text: label,
                color: role,
            } => draw_text(
                ctx,
                text,
                painter,
                screen.center().x,
                screen.center().y,
                &label,
                CHROME_PX,
                color(role),
                false,
                false,
                true,
                None,
            ),
            Command::Popup {
                x,
                y,
                w,
                h,
                fill,
                border,
                rows,
            } => {
                let box_rect = ERect::from_min_size(
                    Pos2::new(screen.min.x + x as f32, screen.min.y + y as f32),
                    egui::vec2(w as f32, h as f32),
                );
                painter.rect_filled(box_rect, 4.0, color(fill));
                painter.rect_stroke(
                    box_rect,
                    4.0,
                    stroke_of(color(border), 1.0),
                    StrokeKind::Inside,
                );
                for (i, row) in rows.iter().enumerate() {
                    draw_text(
                        ctx,
                        text,
                        painter,
                        box_rect.min.x + 8.0,
                        box_rect.min.y + 8.0 + i as f32 * 20.0,
                        &row.text,
                        CHROME_PX,
                        color(row.color),
                        false,
                        false,
                        false,
                        None,
                    );
                }
            }
            _ => {}
        }
    }
    if show_performance && let Some(line) = performance {
        draw_text(
            ctx,
            text,
            painter,
            screen.min.x + 12.0,
            screen.min.y + 48.0,
            &line,
            CHROME_PX,
            color(Role::Muted),
            false,
            false,
            false,
            None,
        );
    }
}

/// Lay out the probe upright and slanted through the live font atlas and
/// compare the tessellated glyph vertices: identical topology, identical
/// baselines, shifted x. This is the egui equivalent of the retired
/// cimgui vertex-slant assertion.
pub fn italic_glyph_probe(ctx: &egui::Context) -> Result<(), String> {
    italic_glyph_probe_text(ctx, "Abstract")
}

pub fn italic_glyph_probe_text(ctx: &egui::Context, probe: &str) -> Result<(), String> {
    if probe.trim().is_empty() {
        return Err("italic probe needs a non-empty text".into());
    }
    let layout = |italics: bool| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            probe,
            0.0,
            egui::TextFormat {
                font_id: FontId::monospace(18.0),
                color: Color32::WHITE,
                italics,
                ..Default::default()
            },
        );
        ctx.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let plain = layout(false);
    let slanted = layout(true);
    let vertices = |galley: &egui::Galley| -> Vec<Pos2> {
        galley
            .rows
            .iter()
            .flat_map(|row| row.row.visuals.mesh.vertices.iter().map(|v| v.pos))
            .collect()
    };
    let before = vertices(&plain);
    let after = vertices(&slanted);
    if before.len() != after.len() {
        return Err("italic must keep the same glyph topology".into());
    }
    if before.is_empty() {
        return Err("italic probe text produced no glyph vertices".into());
    }
    let mut slanted_x = false;
    for (a, b) in before.iter().zip(after.iter()) {
        if (a.y - b.y).abs() > 0.01 {
            return Err("italic must preserve glyph baselines".into());
        }
        if (a.x - b.x).abs() > 0.1 {
            slanted_x = true;
        }
    }
    if !slanted_x {
        return Err("italic glyphs must slant".into());
    }
    Ok(())
}

/// The chrome hint for an unreadable diagram path.
pub fn missing_hint(path: &str) -> String {
    strings::missing_file(path)
}

/// Every canvas text command fits its vertical and horizontal slot under
/// this window's fonts (the driver's `:assert-text-fits`).
pub fn assert_text_fits(ctx: &egui::Context, state: &ViewState) -> Result<(), String> {
    let measure = |s: f64, t: &str| measure(ctx, s as f32, t);
    let mut count = 0;
    let commands = commands::commands(&state.scene, state.selected.as_ref(), state.hover.as_ref());
    for command in &commands {
        let (text, size, h, max_w, bold, italic) = match command {
            Command::Text {
                text,
                size,
                h,
                max_w,
                bold,
                italic,
                ..
            } => (text, *size, h.unwrap_or(*size), *max_w, *bold, *italic),
            Command::Package {
                text, size, max_w, ..
            } => (text, *size, *size, *max_w, false, true),
            _ => continue,
        };
        if text.is_empty() {
            continue;
        }
        let fitted = commands::fit(measure, size.min(h), text, max_w, bold, italic);
        if fitted > 18.0 || fitted > h + 1e-6 {
            return Err(format!("text exceeds vertical slot: {text}"));
        }
        let width = measure(fitted, text)
            + if bold { fitted * 0.05 } else { 0.0 }
            + if italic { fitted * 0.2 } else { 0.0 };
        if width > max_w + 1e-4 {
            return Err(format!(
                "text exceeds horizontal slot: {text} (size {size} -> {fitted}, width {width} > {max_w})"
            ));
        }
        count += 1;
    }
    if count == 0 {
        return Err("empty capture scene".into());
    }
    Ok(())
}
