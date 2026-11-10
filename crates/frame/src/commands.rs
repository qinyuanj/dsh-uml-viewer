//! Pure drawing model (port of `love/uml/adapters/class_boxes.lua` and
//! `edges.lua`): scene EDN plus the selection/hover hits in, renderer
//! commands out; `frame` turns them into its typed draw lists. Colors are
//! semantic theme roles; widths stay in points and scale with DPI at paint
//! time, never with zoom.

use edn::Value;
use edn::record::{flag, get, local_name, seq};
use scene::document::ViewState;
use scene::highlight::Highlight;
use scene::{Rect, vec2};

use crate::lod::{self, Tier};
use crate::strings::{self, Lang};
use crate::theme::{self, Role, RowMarker, fonts};

// Text keeps the layout's 12px side padding; package titles also clear the
// C/M dots. A class stacks its C/M dots in the right padding and puts its
// α/I marker in the left one, so its text rows keep the full width the
// layout sized the box for.
pub const PAD: f64 = 12.0;
pub const PACKAGE_TITLE_RESERVE: f64 = 48.0;
/// An unmeasured class reserves a bottom strip for its 无数据 marker.
pub const NO_DATA_STRIP: f64 = 12.0;

/// One popup row of the dependency hover list.
#[derive(Debug, Clone, PartialEq)]
pub struct PopupRow {
    pub text: String,
    pub violating: bool,
    pub color: Role,
}

/// Renderer commands; every class-owned command carries its owner id and the
/// dim flag of the relationship highlighting.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Package {
        id: Option<Value>,
        rect: Rect,
        text: String,
        band: &'static str,
        red: bool,
        size: f64,
        max_w: f64,
        missing: bool,
        missing_count: i64,
        grade: Option<f64>,
        fill: Role,
        stroke: Role,
        stroke_width: f32,
        /// The title ink.
        ink: Role,
        selected: bool,
        related: bool,
        dimmed: bool,
    },
    Class {
        rect: Rect,
        shape: String,
        red: bool,
        band: &'static str,
        id: Option<Value>,
        missing: bool,
        grade: Option<f64>,
        fill: Role,
        stroke: Role,
        stroke_width: f32,
        selected: bool,
        hovered: bool,
        related: bool,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    Quality {
        owner: &'static str,
        id: Option<Value>,
        rect: Rect,
        text: &'static str,
        band: &'static str,
        grade: Option<f64>,
        radius: f64,
        x: f64,
        y: f64,
        fill: Role,
        stroke: Role,
        /// The C/M letter, drawn at the Full tier only.
        ink: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    Marker {
        rect: Rect,
        text: String,
        x: f64,
        y: f64,
        size: f64,
        color: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    Level {
        rect: Rect,
        text: String,
        x: f64,
        y: f64,
        size: f64,
        color: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    /// The 2px title band under the name slot, in the box stroke color.
    Band {
        rect: Rect,
        x: f64,
        w: f64,
        y: f64,
        color: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    Rule {
        x: f64,
        y: f64,
        w: f64,
        color: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    Text {
        role: String,
        italic: bool,
        bold: bool,
        center: bool,
        box_w: f64,
        x: f64,
        y: f64,
        text: String,
        size: f64,
        top: Option<f64>,
        h: Option<f64>,
        max_w: f64,
        ink: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    NoData {
        rect: Rect,
        text: String,
        right: f64,
        y: f64,
        size: f64,
        color: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    ChildWash {
        rect: Rect,
        marker: RowMarker,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    PortLink {
        behind: [f64; 2],
        tip: [f64; 2],
        color: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    Port {
        id: Option<Value>,
        rect: Rect,
        text: String,
        marked: bool,
        fill: Role,
        stroke: Role,
        stroke_width: f32,
        ink: Role,
        owner_id: Option<Value>,
        dimmed: bool,
    },
    // Screen-space chrome (view coordinates, scaled by DPI at paint time).
    Title {
        text: String,
        x: f64,
        y: f64,
        color: Role,
    },
    ProposalBanner {
        text: String,
        x: f64,
        y: f64,
        color: Role,
    },
    Back {
        text: String,
        x: f64,
        y: f64,
        color: Role,
    },
    Status {
        text: String,
        x: f64,
        y: f64,
        color: Role,
    },
    /// Empty-canvas / missing-file hint, centered on the canvas.
    Hint { text: String, color: Role },
    Popup {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        fill: Role,
        border: Role,
        rows: Vec<PopupRow>,
    },
}

impl Command {
    pub fn kind(&self) -> &'static str {
        match self {
            Command::Package { .. } => "package",
            Command::Class { .. } => "class",
            Command::Quality { .. } => "quality",
            Command::Marker { .. } => "marker",
            Command::Level { .. } => "level",
            Command::Band { .. } => "band",
            Command::Rule { .. } => "rule",
            Command::Text { .. } => "text",
            Command::NoData { .. } => "no-data",
            Command::ChildWash { .. } => "child-wash",
            Command::PortLink { .. } => "port-link",
            Command::Port { .. } => "port",
            Command::Title { .. } => "title",
            Command::ProposalBanner { .. } => "proposal-banner",
            Command::Back { .. } => "back",
            Command::Status { .. } => "status",
            Command::Hint { .. } => "hint",
            Command::Popup { .. } => "popup",
        }
    }

    /// The coarsest LOD tier that draws this command (`lod`): silhouettes
    /// and package titles at every tier, C/M dots and class names from
    /// Frames, the remaining class detail at Full only. A quality dot's
    /// letter is drawn at Full only.
    pub fn tier(&self) -> Tier {
        match self {
            Command::Package { .. } | Command::Class { .. } => Tier::Blocks,
            Command::Quality { .. } => Tier::Frames,
            Command::Text { role, .. } if role == "name" => Tier::Frames,
            Command::Marker { .. }
            | Command::Level { .. }
            | Command::Band { .. }
            | Command::Rule { .. }
            | Command::Text { .. }
            | Command::NoData { .. }
            | Command::ChildWash { .. }
            | Command::PortLink { .. }
            | Command::Port { .. } => Tier::Full,
            _ => Tier::Blocks,
        }
    }

    /// A rough world-space extent for viewport culling; chrome returns None.
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            Command::Package { rect, .. }
            | Command::Class { rect, .. }
            | Command::Quality { rect, .. }
            | Command::Marker { rect, .. }
            | Command::Level { rect, .. }
            | Command::Band { rect, .. }
            | Command::NoData { rect, .. }
            | Command::ChildWash { rect, .. }
            | Command::Port { rect, .. } => Some(*rect),
            Command::Rule { x, y, w, .. } => Some(Rect::new(*x, *y - 1.0, *w, 2.0)),
            Command::Text {
                x, y, box_w, size, ..
            } => Some(Rect::new(*x, *y, *box_w, *size + 4.0)),
            Command::PortLink { behind, tip, .. } => {
                let x = behind[0].min(tip[0]);
                let y = behind[1].min(tip[1]);
                Some(Rect::new(
                    x,
                    y,
                    (behind[0] - tip[0]).abs().max(1.0),
                    (behind[1] - tip[1]).abs().max(1.0),
                ))
            }
            _ => None,
        }
    }
}

/// Edge commands (port of `edges.lua`): strokes, oriented heads and the
/// collapsed-leaf dependency indicators.
#[derive(Debug, Clone, PartialEq)]
pub enum EdgeCommand {
    Stroke {
        from: Option<Value>,
        to: Option<Value>,
        points: Vec<[f64; 2]>,
        color: Role,
        width: f32,
        hot: bool,
        warm: bool,
        dimmed: bool,
    },
    Head {
        from: Option<Value>,
        to: Option<Value>,
        head: String,
        tip: [f64; 2],
        behind: [f64; 2],
        color: Role,
        /// Opaque interior of a closed head (canvas, or ink for a filled diamond).
        /// Never dim this fill: it masks the shaft; `dimmed` affects only the outline.
        fill: Role,
        width: f32,
        hot: bool,
        warm: bool,
        dimmed: bool,
    },
    Indicator {
        id: Option<Value>,
        points: [[f64; 2]; 3],
        violating: bool,
        color: Role,
        dimmed: bool,
    },
}

impl EdgeCommand {
    /// The coarsest LOD tier that draws this command: no edges at Blocks,
    /// strokes and indicators from Frames, arrowheads at Full only.
    pub fn tier(&self) -> Tier {
        match self {
            EdgeCommand::Stroke { .. } | EdgeCommand::Indicator { .. } => Tier::Frames,
            EdgeCommand::Head { .. } => Tier::Full,
        }
    }
}

fn quality(commands: &mut Vec<Command>, c: &Value, owner: &'static str) {
    let Some(r) = Rect::from_value(get(c, "rect").unwrap_or(&Value::Nil)) else {
        return;
    };
    let rating = scene::rating(c);
    for (i, (b, g)) in [
        (rating.crap, rating.crap_grade),
        (rating.mutation, rating.mutation_grade),
    ]
    .into_iter()
    .enumerate()
    {
        commands.push(Command::Quality {
            owner,
            id: get(c, "id").cloned(),
            rect: r,
            text: if i == 0 { "C" } else { "M" },
            band: b.name(),
            grade: Some(g),
            radius: if owner == "class" { 5.0 } else { 6.0 },
            x: if owner == "class" {
                r.right() - 6.0
            } else {
                r.right() - 24.0 + i as f64 * 14.0
            },
            y: if owner == "class" {
                r.y + 9.0 + i as f64 * 12.0
            } else {
                r.y + 10.0
            },
            fill: Role::QualityFill(Some(g)),
            stroke: Role::QualityStroke(Some(g)),
            ink: Role::Marker,
            owner_id: None,
            dimmed: false,
        });
    }
}

/// Vertical slots for a class's text lines (port of `text_slots`): the
/// layout keeps its golden 18px line pitch; nominal sizes when they fit,
/// scaled down together when they do not, with even gaps.
pub fn text_slots(c: &Value, r: Rect, unmeasured: bool) -> Vec<Command> {
    let (x, ry, w, h) = (r.x, r.y, r.w, r.h);
    let mut out = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let mut y = ry + 16.0;
    let mut run_top = ry + if get(c, "level").is_some() { 14.0 } else { 4.0 };
    let stereo = local_name(get(c, "stereotype").unwrap_or(&Value::Nil))
        .unwrap_or_default()
        .to_owned();
    let flush = |out: &mut Vec<Command>, run: &mut Vec<usize>, run_top: f64, run_bottom: f64| {
        let total: f64 = run
            .iter()
            .map(|&i| match &out[i] {
                Command::Text { size, .. } => *size,
                _ => 0.0,
            })
            .sum();
        if !run.is_empty() {
            let avail = run_bottom - run_top;
            let k = (avail / total).min(1.0);
            let gap = (avail - total * k) / (run.len() as f64 + 1.0);
            let mut top = run_top + gap;
            for &i in run.iter() {
                if let Command::Text {
                    size,
                    top: t,
                    h: hh,
                    max_w,
                    x: tx,
                    bold,
                    italic,
                    ..
                } = &mut out[i]
                {
                    *t = Some(top);
                    *hh = Some(*size * k);
                    // The bold offset and italic shear overhang into the
                    // side padding, so a name as wide as the layout's
                    // estimate still draws at full size.
                    let overhang =
                        *size * (if *bold { 0.05 } else { 0.0 } + if *italic { 0.2 } else { 0.0 });
                    *max_w = w - 2.0 * PAD + overhang;
                    *tx = x + PAD - overhang / 2.0;
                    top += *size * k + gap;
                }
            }
        }
        run.clear();
    };
    for line in seq(get(c, "lines")) {
        let kind = local_name(get(line, "kind").unwrap_or(&Value::Nil))
            .unwrap_or_default()
            .to_owned();
        if kind == "rule" {
            flush(&mut out, &mut run, run_top, y - 5.0);
            out.push(Command::Rule {
                x,
                y: y - 4.0,
                w,
                color: Role::QualityStroke(None),
                owner_id: None,
                dimmed: false,
            });
            run_top = y - 3.0;
            y += 12.0;
        } else {
            let contents_len = match get(c, "contents") {
                Some(Value::Vector(v)) => v.len(),
                _ => 0,
            };
            let italic = kind == "name"
                && ((!stereo.is_empty() && stereo != "class")
                    || edn::record::is_true(c, "drill?")
                    || contents_len > 0);
            out.push(Command::Text {
                role: kind.clone(),
                italic,
                bold: kind == "name",
                center: kind == "name",
                box_w: w,
                x: x + PAD,
                y,
                text: match get(line, "text") {
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                },
                size: fonts::CANVAS as f64,
                top: None,
                h: None,
                max_w: w - 2.0 * PAD,
                ink: text_color(&kind),
                owner_id: None,
                dimmed: false,
            });
            run.push(out.len() - 1);
            y += 18.0;
        }
    }
    let bottom = ry + h - 4.0 - if unmeasured { NO_DATA_STRIP } else { 0.0 };
    flush(&mut out, &mut run, run_top, bottom);
    out
}

fn marked(view_hit: Option<&Value>, kind: &str, id: Option<&Value>) -> bool {
    view_hit.is_some_and(|hit| {
        local_name(get(hit, "kind").unwrap_or(&Value::Nil)).unwrap_or_default() == kind
            && get(hit, "id") == id
    })
}

/// The class boxes, packages, badges and ports of one scene.
/// The scene-space draw list; `lang` picks the 无数据 marker's text (#44).
pub fn commands(
    scene: &Value,
    selected: Option<&Value>,
    hover: Option<&Value>,
    lang: Lang,
) -> Vec<Command> {
    let h = Highlight::new(scene, selected, hover);
    let hover_id = h.hover.clone();
    let mut out = Vec::new();
    for p in seq(get(scene, "packages")) {
        let rating = scene::rating(p);
        let band = rating.band().name();
        let Some(pr) = Rect::from_value(get(p, "rect").unwrap_or(&Value::Nil)) else {
            continue;
        };
        // Pooled package CRAP substitutes mu=20 for unmeasured members, so
        // missing is counted from the member classes themselves.
        let mut count = 0i64;
        for c in seq(get(scene, "classes")) {
            if get(c, "package") == get(p, "id") && scene::rating(c).missing {
                count += 1;
            }
        }
        let g = rating.grade();
        let id = get(p, "id");
        let is_selected = marked(selected, "package", id);
        let is_related = id.is_some_and(|id| h.related(id));
        let dimmed = id.is_some_and(|id| h.dimmed(id));
        out.push(Command::Package {
            id: id.cloned(),
            rect: pr,
            text: match get(p, "title").or_else(|| get(p, "label")) {
                Some(Value::String(s)) => s.clone(),
                Some(v) => local_name(v).unwrap_or_default().to_owned(),
                None => String::new(),
            },
            band,
            red: band == "red",
            size: fonts::CANVAS as f64,
            // The italic shear overhangs into the dots' reserve.
            max_w: pr.w - PACKAGE_TITLE_RESERVE + fonts::CANVAS as f64 * 0.2,
            missing: count > 0,
            missing_count: count,
            grade: g,
            fill: Role::PackageFill(g),
            stroke: if is_selected {
                Role::Selected
            } else if is_related {
                Role::DependencyChain
            } else {
                Role::PackageStroke(g)
            },
            ink: Role::Accent,
            selected: is_selected,
            related: is_related,
            dimmed,
            stroke_width: if is_selected {
                theme::widths::SELECTED
            } else {
                theme::widths::PACKAGE
            },
        });
        quality(&mut out, p, "package");
    }
    for c in seq(get(scene, "classes")) {
        let Some(r) = Rect::from_value(get(c, "rect").unwrap_or(&Value::Nil)) else {
            continue;
        };
        let rating = scene::rating(c);
        let band = rating.band().name();
        let id = get(c, "id").cloned();
        let unmeasured = rating.missing;
        let g = rating.class_grade();
        let dimmed = id.as_ref().is_some_and(|id| h.dimmed(id));
        let is_selected = h.focus.is_some() && h.focus == id;
        let is_related = id.as_ref().is_some_and(|id| h.related(id));
        let tag = |command: &mut Command| match command {
            Command::Class {
                owner_id,
                dimmed: d,
                ..
            }
            | Command::Quality {
                owner_id,
                dimmed: d,
                ..
            }
            | Command::Marker {
                owner_id,
                dimmed: d,
                ..
            }
            | Command::Level {
                owner_id,
                dimmed: d,
                ..
            }
            | Command::Band {
                owner_id,
                dimmed: d,
                ..
            }
            | Command::Rule {
                owner_id,
                dimmed: d,
                ..
            } => {
                *owner_id = id.clone();
                *d = dimmed;
            }
            Command::Text {
                role,
                owner_id,
                dimmed: d,
                ..
            } => {
                *owner_id = id.clone();
                *d = dimmed && role != "name";
            }
            Command::NoData {
                owner_id,
                dimmed: d,
                ..
            }
            | Command::ChildWash {
                owner_id,
                dimmed: d,
                ..
            }
            | Command::PortLink {
                owner_id,
                dimmed: d,
                ..
            }
            | Command::Port {
                owner_id,
                dimmed: d,
                ..
            } => {
                *owner_id = id.clone();
                *d = dimmed;
            }
            _ => {}
        };
        let shape = local_name(get(c, "shape").unwrap_or(&Value::Nil))
            .unwrap_or_default()
            .to_owned();
        // A foreign oval is never measured: its red frame says enough.
        let no_data_label = unmeasured && shape != "oval";
        let mut class_commands = Vec::new();
        class_commands.push(Command::Class {
            rect: r,
            shape,
            red: band == "red",
            band,
            id: id.clone(),
            missing: unmeasured,
            grade: g,
            fill: Role::QualityFill(g),
            stroke: if is_selected {
                Role::Selected
            } else if is_related {
                Role::DependencyChain
            } else {
                Role::QualityStroke(g)
            },
            selected: is_selected,
            hovered: hover_id.is_some() && hover_id == id,
            related: is_related,
            stroke_width: if is_selected {
                theme::widths::SELECTED
            } else {
                theme::widths::CLASS
            },
            owner_id: None,
            dimmed: false,
        });
        quality(&mut class_commands, c, "class");
        let stereo = local_name(get(c, "stereotype").unwrap_or(&Value::Nil))
            .unwrap_or_default()
            .to_owned();
        let level = get(c, "level").map(|level| match level {
            Value::Integer(n) => n.to_string(),
            Value::Float(x) => format!("{x}"),
            _ => local_name(level).unwrap_or_default().to_owned(),
        });
        let marker = stereo == "abstract" || stereo == "interface";
        if marker {
            class_commands.push(Command::Marker {
                rect: r,
                text: if stereo == "abstract" {
                    "α".into()
                } else {
                    "I".into()
                },
                // Top-left, after the level number when there is one.
                x: r.x
                    + 4.0
                    + level
                        .as_deref()
                        .map_or(0.0, |t| estimate_w(t, fonts::LEVEL as f64) + 3.0),
                y: r.y + 3.0,
                size: fonts::MARKER as f64,
                color: Role::Marker,
                owner_id: None,
                dimmed: false,
            });
        }
        if let Some(level) = level {
            class_commands.push(Command::Level {
                rect: r,
                text: level,
                x: r.x + 4.0,
                y: r.y + 3.0,
                size: fonts::LEVEL as f64,
                color: Role::Muted,
                owner_id: None,
                dimmed: false,
            });
        }
        let mut slots = text_slots(c, r, unmeasured);
        // The 2px title band runs under the name slot in the box stroke color.
        for s in &slots {
            if let Command::Text {
                role,
                top: Some(top),
                h: Some(sh),
                ..
            } = s
                && role == "name"
            {
                class_commands.push(Command::Band {
                    rect: r,
                    x: r.x + 6.0,
                    w: r.w - 12.0,
                    y: (r.bottom() - 2.0).min(top + sh + 1.0),
                    color: Role::QualityStroke(g),
                    owner_id: None,
                    dimmed: false,
                });
                break;
            }
        }
        let lines = seq(get(c, "lines"));
        for (i, mut line) in slots.drain(..).enumerate() {
            if let Command::Text {
                role,
                top,
                h: sh,
                y,
                ..
            } = &line
                && role == "child"
            {
                let source_id = lines.get(i).and_then(|l| get(l, "id"));
                if marked(selected, "child", source_id) {
                    class_commands.push(Command::ChildWash {
                        rect: Rect::new(
                            r.x + 5.0,
                            top.unwrap_or(*y) - 1.0,
                            r.w - 10.0,
                            sh.unwrap_or(18.0),
                        ),
                        marker: theme::row_marker(),
                        owner_id: None,
                        dimmed: false,
                    });
                }
            }
            if let Command::Rule { color, .. } = &mut line {
                *color = Role::QualityStroke(g);
            }
            class_commands.push(line);
        }
        for dir in ["in", "out"] {
            for p in seq(get(c, &format!("{dir}-ports"))) {
                let Some(pr) = Rect::from_value(get(p, "rect").unwrap_or(&Value::Nil)) else {
                    continue;
                };
                let px = pr.cx();
                let bx = (r.x + 6.0).max(px.min(r.right() - 6.0));
                let behind = if dir == "in" {
                    [px, pr.bottom()]
                } else {
                    [bx, r.bottom()]
                };
                let tip = if dir == "in" { [bx, r.y] } else { [px, pr.y] };
                class_commands.push(Command::PortLink {
                    behind,
                    tip,
                    color: Role::PortLink,
                    owner_id: None,
                    dimmed: false,
                });
                let is_marked = marked(selected, "port", get(p, "id"));
                class_commands.push(Command::Port {
                    id: get(p, "id").cloned(),
                    rect: pr,
                    text: match get(p, "name") {
                        Some(Value::String(s)) => s.clone(),
                        _ => local_name(get(p, "id").unwrap_or(&Value::Nil))
                            .unwrap_or_default()
                            .to_owned(),
                    },
                    marked: is_marked,
                    fill: if is_marked {
                        Role::PortFillMarked
                    } else {
                        Role::PortFill
                    },
                    stroke: if is_marked {
                        Role::Selected
                    } else {
                        Role::PortOutline
                    },
                    ink: if is_marked { Role::Ink } else { Role::Muted },
                    stroke_width: if is_marked {
                        theme::widths::PORT_ON
                    } else {
                        theme::widths::PORT
                    },
                    owner_id: None,
                    dimmed: false,
                });
            }
        }
        if no_data_label {
            class_commands.push(Command::NoData {
                rect: r,
                text: lang.text().no_data.into(),
                right: r.right() - PAD,
                y: r.bottom() - NO_DATA_STRIP - 2.0,
                size: fonts::LEVEL as f64,
                color: Role::NoData,
                owner_id: None,
                dimmed: false,
            });
        }
        for command in &mut class_commands {
            tag(command);
        }
        out.extend(class_commands);
    }
    out
}

fn edge_violating(e: &Value) -> bool {
    flag(e, "violating") || seq(get(e, "deps")).iter().any(|dep| flag(dep, "violating"))
}

#[derive(Clone, Copy)]
enum Attention {
    Outside,
    Chain,
    Hovered,
}

/// Violation semantics win: attention never turns a violating edge white or
/// gold. Other edges use white for hover and blue for the chain, at the
/// same width so color alone distinguishes those states.
fn edge_style(violating: bool, attention: Attention) -> (Role, f32) {
    use theme::widths as W;
    match (violating, attention) {
        (true, Attention::Hovered) => (Role::ViolationHot, W::EDGE_VIOLATING_HOT),
        (true, Attention::Chain) => (Role::ViolationHot, W::EDGE_VIOLATING),
        (true, Attention::Outside) => (Role::Violation, W::EDGE_VIOLATING),
        (false, Attention::Hovered) => (Role::RelationHover, W::EDGE),
        (false, Attention::Chain) => (Role::DependencyChain, W::EDGE),
        (false, Attention::Outside) => (Role::Relation, W::EDGE),
    }
}

fn points_of(v: Option<&Value>) -> Vec<[f64; 2]> {
    seq(v).iter().filter_map(vec2).collect()
}

/// The edge strokes, arrowheads and collapsed dependency indicators.
pub fn edge_commands(
    scene: &Value,
    selected: Option<&Value>,
    hover: Option<&Value>,
) -> Vec<EdgeCommand> {
    let h = Highlight::new(scene, selected, hover);
    let mut out = Vec::new();
    let hide = get(scene, "diagram").is_some_and(|d| flag(d, "hide-edges"));
    if hide {
        for ind in seq(get(scene, "dep-indicators")) {
            let points = points_of(get(ind, "triangle"));
            if points.len() != 3 {
                continue;
            }
            let violating = flag(ind, "violating");
            out.push(EdgeCommand::Indicator {
                id: get(ind, "id").cloned(),
                points: [points[0], points[1], points[2]],
                violating,
                color: if violating {
                    Role::Violation
                } else {
                    Role::Indicator
                },
                dimmed: get(ind, "id").is_some_and(|id| h.dimmed(id)),
            });
        }
        return out;
    }
    for e in seq(get(scene, "edges")) {
        let chain = h.on_chain(e);
        let hot = h.hot(e, hover);
        let warm = !hot && h.warm(e);
        let attention = if hot || warm {
            Attention::Hovered
        } else if chain {
            Attention::Chain
        } else {
            Attention::Outside
        };
        let dimmed = h.focus.is_some() && matches!(attention, Attention::Outside);
        let violating = edge_violating(e);
        let (color, width) = edge_style(violating, attention);
        let from = get(e, "from").cloned();
        let to = get(e, "to").cloned();
        for ps in seq(get(e, "strokes")) {
            out.push(EdgeCommand::Stroke {
                from: from.clone(),
                to: to.clone(),
                points: points_of(Some(ps)),
                color,
                width,
                hot,
                warm,
                dimmed,
            });
        }
        if let (Some(head), Some(tip), Some(behind)) = (
            get(e, "head"),
            vec2(get(e, "tip").unwrap_or(&Value::Nil)),
            vec2(get(e, "behind").unwrap_or(&Value::Nil)),
        ) {
            let head = local_name(head).unwrap_or_default().to_owned();
            out.push(EdgeCommand::Head {
                from,
                to,
                fill: if head == "diamond-fill" {
                    Role::Ink
                } else {
                    Role::Canvas
                },
                head,
                tip,
                behind,
                color,
                width,
                hot,
                warm,
                dimmed,
            });
        }
    }
    out
}

/// Natural advance fallback: the layout's 8px/char monospace estimate at
/// 16px, scaled. The real measure comes from the renderer at paint time.
pub fn estimate_w(text: &str, size: f64) -> f64 {
    scene::text_w(Some(text)) / 16.0 * size
}

/// The layout's 8px/char estimate fixes box widths (JVM goldens); a bold or
/// italic name may be wider, so the drawn size shrinks until the text, bold offset
/// and italic shear fit inside `max_w`. Never grows past the nominal size.
pub fn fit(
    measure: impl Fn(f64, &str) -> f64,
    size: f64,
    text: &str,
    max_w: f64,
    bold: bool,
    italic: bool,
) -> f64 {
    let width = |s: f64| {
        measure(s, text) + if bold { s * 0.05 } else { 0.0 } + if italic { s * 0.2 } else { 0.0 }
    };
    let natural = width(size);
    if natural <= max_w || natural <= 0.0 {
        return size;
    }
    let budget = max_w.max(0.0);
    let mut fitted = size * budget / natural;
    // Real glyph advances snap to the pixel grid, so width is not linear in
    // size: shrink until the measured text is inside the budget (a wider
    // label would overrun its slot).
    for _ in 0..8 {
        let w = width(fitted);
        if w <= budget || w <= 0.0 {
            break;
        }
        fitted *= budget / w * 0.999;
    }
    fitted
}

/// Text ink by role: crap lines gold, stereotypes muted, everything else ink.
pub fn text_color(role: &str) -> Role {
    match role {
        "crap" => Role::Accent,
        "stereo" => Role::Muted,
        _ => Role::Ink,
    }
}

/// A centered canvas hint command (waiting for :display, missing file).
pub fn hint(text: &str) -> Command {
    Command::Hint {
        text: text.into(),
        color: Role::Muted,
    }
}

/// Screen-space canvas chrome (port of `class_boxes.chrome`): the waiting
/// hint, the diagram banner, return scope and the leaf-dependency popup
/// (Declutter lives in the inspector, #141).
pub fn chrome(state: &ViewState, width: f64, height: f64) -> Vec<Command> {
    let mut out = Vec::new();
    if state.waiting {
        out.push(hint(strings::WAITING_DISPLAY));
        return out;
    }
    let diagram = get(&state.scene, "diagram")
        .cloned()
        .unwrap_or(Value::Map(vec![]));
    let title = get(&diagram, "title");
    // A scoped view names its scope on the return line, not twice.
    let scoped = !state.focus.is_empty() || get(&state.extra, "open-layer").is_some();
    let banner = flag(&diagram, "proposal");
    if banner {
        let mut proposal_name: Option<String> = None;
        if let Some(doc) = &state.document {
            for p in &scene::proposals(doc) {
                if get(p, "id") == get(&state.extra, "proposal-id") {
                    proposal_name = Some(match get(p, "name") {
                        Some(Value::String(s)) => s.clone(),
                        _ => local_name(get(p, "id").unwrap_or(&Value::Nil))
                            .unwrap_or_default()
                            .to_owned(),
                    });
                }
            }
        }
        out.push(Command::ProposalBanner {
            text: format!(
                "{}{}",
                strings::PROPOSAL_BANNER,
                proposal_name.map(|n| format!(" · {n}")).unwrap_or_default()
            ),
            x: 12.0,
            y: 8.0,
            color: Role::Accent,
        });
    } else if let Some(Value::String(title)) = title.filter(|_| !scoped) {
        out.push(Command::Title {
            text: title.clone(),
            x: 12.0,
            y: 8.0,
            color: Role::Muted,
        });
    }
    if scoped {
        let parts: Vec<String> = state
            .focus
            .iter()
            .map(|v| local_name(v).unwrap_or_default().to_owned())
            .collect();
        let scope = if !parts.is_empty() {
            parts.join(".")
        } else {
            title
                .map(|t| match t {
                    Value::String(s) => s.clone(),
                    other => local_name(other).unwrap_or_default().to_owned(),
                })
                .unwrap_or_else(|| {
                    local_name(get(&state.extra, "open-layer").unwrap_or(&Value::Nil))
                        .unwrap_or_default()
                        .to_owned()
                })
        };
        out.push(Command::Back {
            text: format!("{} · {scope}", strings::BACK),
            x: 12.0,
            y: if banner { 28.0 } else { 8.0 },
            color: Role::Accent,
        });
    }
    let rows = hover_rows(state);
    if let (false, Some((mx, my))) = (rows.is_empty(), state.pointer) {
        let mut tw = 40.0f64;
        for row in &rows {
            tw = tw.max(estimate_w(&row.text, fonts::BODY as f64));
        }
        let w = (tw + 16.0).min((16.0f64).max(width - 16.0));
        let h = 16.0 + rows.len() as f64 * (fonts::BODY as f64 + 2.0);
        let x = (8.0f64).max(mx.min(width - w - 8.0));
        let mut y = my + 16.0;
        if y + h > height {
            y = (8.0f64).max(my - h - 8.0);
        }
        out.push(Command::Popup {
            x,
            y,
            w,
            h,
            fill: Role::Popup,
            border: Role::Line,
            rows,
        });
    }
    out
}

/// The hover popup rows: an edge's dependencies, or in the overview tiers
/// the name of the hovered class, whose box is too narrow to show it.
fn hover_rows(state: &ViewState) -> Vec<PopupRow> {
    let hover_kind = state
        .hover
        .as_ref()
        .map(|hv| {
            local_name(get(hv, "kind").unwrap_or(&Value::Nil))
                .unwrap_or_default()
                .to_owned()
        })
        .unwrap_or_default();
    let deps = state.hover.as_ref().and_then(|hv| get(hv, "deps"));
    if (hover_kind == "edge" || hover_kind == "dep")
        && let Some(Value::Vector(deps)) = deps
        && !deps.is_empty()
    {
        dependency_rows(deps)
    } else if hover_kind == "class" && lod::tier(state.zoom) != lod::Tier::Full {
        let id = state.hover.as_ref().and_then(|hv| get(hv, "id"));
        seq(get(&state.scene, "classes"))
            .iter()
            .find(|c| get(c, "id") == id)
            .and_then(|c| get(c, "name"))
            .and_then(|n| match n {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .map(|text| {
                vec![PopupRow {
                    text,
                    violating: false,
                    color: Role::Ink,
                }]
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// `from -> to` rows, violations first: the dependency hover tooltip, which the
/// panel also renders from the frame's optional `hover` metadata (#26).
pub(crate) fn dependency_rows(deps: &[Value]) -> Vec<PopupRow> {
    let mut rows: Vec<PopupRow> = deps
        .iter()
        .map(|d| {
            let from = local_name(get(d, "from").unwrap_or(&Value::Nil))
                .unwrap_or_default()
                .to_owned();
            let to = local_name(get(d, "to").unwrap_or(&Value::Nil))
                .unwrap_or_default()
                .to_owned();
            let violating = flag(d, "violating");
            PopupRow {
                text: format!("{from} -> {to}"),
                violating,
                color: if violating {
                    Role::Violation
                } else {
                    Role::Ink
                },
            }
        })
        .collect();
    rows.sort_by(|a, b| b.violating.cmp(&a.violating).then(a.text.cmp(&b.text)));
    rows
}
