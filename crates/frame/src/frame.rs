//! The scene frame (#16): one view, fully drawable, as a versioned JSON
//! document: the typed parts ([`layers`], [`chrome`]) and the
//! [`Frame::to_json`] the viewer panel receives. The schema is
//! `docs/scene-frame.md`; `version` changes only on an incompatible change.
//!
//! Colors are semantic [`Role`]s, never RGB. World items are in scene
//! coordinates (`screen = (world - camera) * zoom`, plus the viewport
//! origin); chrome items are in viewport points.

use edn::Value;
use scene::Rect;
use scene::document::ViewState;
use serde_json::{Map, Value as Json, json};

use crate::commands::{self, Command, EdgeCommand, PopupRow};
use crate::lod::{self, Tier};
use crate::strings::Lang;
use crate::theme::{self, Role, RowMarker};

/// The `schema` tag every frame carries.
pub const SCHEMA: &str = "dsh-uml-viewer/scene-frame";
/// The frame schema version.
pub const VERSION: u64 = 1;

/// The scene-space draw lists of one view: class boxes, packages, badges
/// and ports, then the edges. A pure function of the scene, the selection
/// and the hover.
#[derive(Debug, Clone, PartialEq)]
pub struct Layers {
    pub world: Vec<Command>,
    pub edges: Vec<EdgeCommand>,
}

pub fn layers(
    scene: &Value,
    selected: Option<&Value>,
    hover: Option<&Value>,
    lang: Lang,
) -> Layers {
    Layers {
        world: commands::commands(scene, selected, hover, lang),
        edges: commands::edge_commands(scene, selected, hover),
    }
}

/// Screen-space chrome over the canvas (#46, ADR 0015): the hover popup, and
/// nothing else. The title, the proposal banner, the return line, the status
/// line and the centred hint the resident used to draw live in the panel's own
/// DOM now, so this list stays empty unless the pointer is over something with
/// rows.
pub fn chrome(state: &ViewState, width: f64, height: f64) -> Vec<Command> {
    commands::popup(state, width, height).into_iter().collect()
}

/// One drawable view.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: f64,
    pub height: f64,
    pub camera_x: f64,
    pub camera_y: f64,
    pub zoom: f64,
    pub tier: Tier,
    pub waiting: bool,
    pub layers: Layers,
    pub chrome: Vec<Command>,
}

/// The zoom a view paints at: a non-positive zoom (never fitted) is 1.
pub fn paint_zoom(state: &ViewState) -> f64 {
    if state.zoom > 0.0 { state.zoom } else { 1.0 }
}

/// The frame of `state` in a `width` x `height` viewport (points), with the
/// resident-laid-out text in `lang`.
pub fn build(state: &ViewState, width: f64, height: f64, lang: Lang) -> Frame {
    let zoom = paint_zoom(state);
    Frame {
        width,
        height,
        camera_x: state.cam_x,
        camera_y: state.cam_y,
        zoom,
        tier: lod::tier(zoom),
        waiting: state.waiting,
        layers: if state.waiting {
            Layers {
                world: Vec::new(),
                edges: Vec::new(),
            }
        } else {
            layers(
                &state.scene,
                state.selected.as_ref(),
                state.hover.as_ref(),
                lang,
            )
        },
        chrome: chrome(state, width, height),
    }
}

impl Frame {
    pub fn to_json(&self) -> Json {
        json!({
            "schema": SCHEMA,
            "version": VERSION,
            "viewport": {"width": self.width, "height": self.height},
            "camera": {"x": self.camera_x, "y": self.camera_y, "zoom": self.zoom},
            "lod": tier_name(self.tier),
            "waiting": self.waiting,
            "style": style(),
            "world": self.layers.world.iter().map(world_json).collect::<Vec<_>>(),
            "edges": self.layers.edges.iter().map(edge_json).collect::<Vec<_>>(),
            "chrome": self.chrome.iter().map(chrome_json).collect::<Vec<_>>(),
        })
    }

    /// The frame plus the optional bounded hover metadata (#26): the resident's
    /// frame, whose dependency-tooltip rows and bundled-edge `via-ids` the panel
    /// cannot derive from the strokes themselves. The golden path
    /// ([`Frame::to_json_string`]) deliberately omits it, so the checked-in
    /// fixtures stay byte for byte what they were; a consumer that does not know
    /// the field ignores it (version 1, additive).
    pub fn to_json_with_hover(&self, scene: &Value) -> Json {
        let mut json = self.to_json();
        if let Some(object) = json.as_object_mut() {
            object.insert("hover".into(), crate::hover::metadata(scene));
        }
        json
    }

    /// [`Frame::to_json_with_hover`] as the writer's bytes.
    pub fn to_json_string_with_hover(&self, scene: &Value) -> String {
        let mut text =
            serde_json::to_string_pretty(&self.to_json_with_hover(scene)).expect("frame JSON");
        text.push('\n');
        text
    }

    /// Pretty JSON with sorted keys and a final newline: the golden bytes.
    pub fn to_json_string(&self) -> String {
        let mut text = serde_json::to_string_pretty(&self.to_json()).expect("frame JSON");
        text.push('\n');
        text
    }
}

pub fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Blocks => "blocks",
        Tier::Frames => "frames",
        Tier::Full => "full",
    }
}

/// An f32 style constant at its decimal value (1.6, not 1.600000023841858).
fn f32n(x: f32) -> Json {
    let value: f64 = x.to_string().parse().unwrap_or(x as f64);
    json!(value)
}

fn style() -> Json {
    use theme::{fonts, radius, widths};
    json!({
        "fonts": {
            "canvas": f32n(fonts::CANVAS),
            "level": f32n(fonts::LEVEL),
            "marker": f32n(fonts::MARKER),
            "badge": f32n(fonts::BADGE),
            "port": f32n(fonts::PORT),
            "chrome": f32n(fonts::CHROME),
        },
        "widths": {
            "band": f32n(widths::BAND),
            "rule": f32n(widths::RULE),
            "dot": f32n(widths::DOT),
            "dash": f32n(widths::DASH),
            "dashGap": f32n(widths::DASH_GAP),
            "portLink": f32n(widths::PORT_LINK),
        },
        "radius": {
            "class": f32n(radius::CLASS),
            "package": f32n(radius::PACKAGE),
            "port": f32n(radius::PORT),
            "rowWash": f32n(radius::ROW_WASH),
        },
        "dimAlpha": theme::ALPHA_DIMMED,
        "lodZoom": {"frames": lod::FRAMES_ZOOM, "full": lod::FULL_ZOOM},
        "minTextPx": lod::MIN_TEXT_PX,
        "edgeDecimatePx": lod::DECIMATE_PX,
    })
}

/// A color role: `{"role": name}`, plus `grade` (0-10, null unmeasured) or
/// `ratio` (0-1, null unknown) for the quality roles.
pub fn role_json(role: Role) -> Json {
    let mut map = Map::new();
    map.insert("role".into(), json!(role.name()));
    match role {
        Role::QualityFill(g)
        | Role::QualityStroke(g)
        | Role::PackageFill(g)
        | Role::PackageStroke(g) => {
            map.insert("grade".into(), json!(g));
        }
        Role::Coverage(p) => {
            map.insert("ratio".into(), json!(p));
        }
        _ => {}
    }
    Json::Object(map)
}

/// An element id: its EDN text (`:ir.graph`), which round-trips.
fn id_json(id: &Option<Value>) -> Json {
    match id {
        Some(v) => json!(edn::encode(v).unwrap_or_default()),
        None => Json::Null,
    }
}

fn rect_json(r: &Rect) -> Json {
    json!({"x": r.x, "y": r.y, "w": r.w, "h": r.h})
}

fn point(p: &[f64; 2]) -> Json {
    json!([p[0], p[1]])
}

fn marker_json(m: &RowMarker) -> Json {
    json!({"wash": role_json(m.wash), "bar": role_json(m.bar), "barWidth": f32n(m.bar_width)})
}

fn object(pairs: Vec<(&str, Json)>) -> Json {
    Json::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// A scene-space item: its kind, owner class, dim flag, coarsest LOD tier
/// and culling bounds, then its own fields.
fn world_json(c: &Command) -> Json {
    let mut fields = command_fields(c);
    if let Json::Object(map) = &mut fields {
        map.insert("lod".into(), json!(tier_name(c.tier())));
        map.insert(
            "bounds".into(),
            c.bounds().map_or(Json::Null, |b| rect_json(&b)),
        );
    }
    fields
}

fn chrome_json(c: &Command) -> Json {
    command_fields(c)
}

fn command_fields(c: &Command) -> Json {
    let kind = ("kind", json!(c.kind()));
    match c {
        Command::Package {
            id,
            rect,
            text,
            band,
            size,
            max_w,
            missing,
            missing_count,
            grade,
            fill,
            stroke,
            stroke_width,
            ink,
            selected,
            related,
            dimmed,
            ..
        } => object(vec![
            kind,
            ("id", id_json(id)),
            ("rect", rect_json(rect)),
            ("text", json!(text)),
            ("band", json!(band)),
            ("grade", json!(grade)),
            ("size", json!(size)),
            ("maxW", json!(max_w)),
            ("missing", json!(missing)),
            ("missingCount", json!(missing_count)),
            ("fill", role_json(*fill)),
            ("stroke", role_json(*stroke)),
            ("strokeWidth", f32n(*stroke_width)),
            ("ink", role_json(*ink)),
            ("selected", json!(selected)),
            ("related", json!(related)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::Class {
            rect,
            shape,
            band,
            id,
            missing,
            grade,
            fill,
            stroke,
            stroke_width,
            selected,
            hovered,
            related,
            owner_id,
            dimmed,
            ..
        } => object(vec![
            kind,
            ("id", id_json(id)),
            ("owner", id_json(owner_id)),
            ("rect", rect_json(rect)),
            ("shape", json!(shape)),
            ("band", json!(band)),
            ("grade", json!(grade)),
            ("missing", json!(missing)),
            ("fill", role_json(*fill)),
            ("stroke", role_json(*stroke)),
            ("strokeWidth", f32n(*stroke_width)),
            ("selected", json!(selected)),
            ("hovered", json!(hovered)),
            ("related", json!(related)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::Quality {
            owner,
            id,
            text,
            band,
            grade,
            radius,
            x,
            y,
            fill,
            stroke,
            ink,
            owner_id,
            dimmed,
            ..
        } => object(vec![
            kind,
            ("id", id_json(id)),
            ("owner", id_json(owner_id)),
            ("of", json!(owner)),
            ("text", json!(text)),
            ("band", json!(band)),
            ("grade", json!(grade)),
            ("center", json!([x, y])),
            ("radius", json!(radius)),
            ("fill", role_json(*fill)),
            ("stroke", role_json(*stroke)),
            ("ink", role_json(*ink)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::Marker {
            text,
            x,
            y,
            size,
            color,
            owner_id,
            dimmed,
            ..
        }
        | Command::Level {
            text,
            x,
            y,
            size,
            color,
            owner_id,
            dimmed,
            ..
        } => object(vec![
            kind,
            ("owner", id_json(owner_id)),
            ("text", json!(text)),
            ("at", json!([x, y])),
            ("size", json!(size)),
            ("ink", role_json(*color)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::Band {
            x,
            w,
            y,
            color,
            owner_id,
            dimmed,
            ..
        } => object(vec![
            kind,
            ("owner", id_json(owner_id)),
            ("from", json!([x, y])),
            ("to", json!([x + w, y])),
            ("stroke", role_json(*color)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::Rule {
            x,
            y,
            w,
            color,
            owner_id,
            dimmed,
        } => object(vec![
            kind,
            ("owner", id_json(owner_id)),
            ("from", json!([x + 6.0, y])),
            ("to", json!([x + w - 6.0, y])),
            ("stroke", role_json(*color)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::Text {
            role,
            italic,
            bold,
            center,
            x,
            y,
            text,
            size,
            top,
            h,
            max_w,
            ink,
            owner_id,
            dimmed,
            ..
        } => object(vec![
            kind,
            ("owner", id_json(owner_id)),
            ("line", json!(role)),
            ("text", json!(text)),
            ("x", json!(x)),
            ("top", json!(top.unwrap_or(*y))),
            ("slot", json!(h.unwrap_or(*size))),
            ("size", json!(size)),
            ("maxW", json!(max_w)),
            ("italic", json!(italic)),
            ("bold", json!(bold)),
            ("center", json!(center)),
            ("ink", role_json(*ink)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::NoData {
            text,
            right,
            y,
            size,
            color,
            owner_id,
            dimmed,
            ..
        } => object(vec![
            kind,
            ("owner", id_json(owner_id)),
            ("text", json!(text)),
            ("right", json!(right)),
            ("y", json!(y)),
            ("size", json!(size)),
            ("ink", role_json(*color)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::ChildWash {
            rect,
            marker,
            owner_id,
            dimmed,
        } => object(vec![
            kind,
            ("owner", id_json(owner_id)),
            ("rect", rect_json(rect)),
            ("marker", marker_json(marker)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::PortLink {
            behind,
            tip,
            color,
            owner_id,
            dimmed,
        } => object(vec![
            kind,
            ("owner", id_json(owner_id)),
            ("behind", point(behind)),
            ("tip", point(tip)),
            ("stroke", role_json(*color)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::Port {
            id,
            rect,
            text,
            marked,
            fill,
            stroke,
            stroke_width,
            ink,
            owner_id,
            dimmed,
        } => object(vec![
            kind,
            ("id", id_json(id)),
            ("owner", id_json(owner_id)),
            ("rect", rect_json(rect)),
            ("text", json!(text)),
            ("marked", json!(marked)),
            ("fill", role_json(*fill)),
            ("stroke", role_json(*stroke)),
            ("strokeWidth", f32n(*stroke_width)),
            ("ink", role_json(*ink)),
            ("dimmed", json!(dimmed)),
        ]),
        Command::Popup {
            x,
            y,
            w,
            h,
            fill,
            border,
            rows,
        } => object(vec![
            kind,
            ("rect", json!({"x": x, "y": y, "w": w, "h": h})),
            ("fill", role_json(*fill)),
            ("border", role_json(*border)),
            (
                "rows",
                Json::Array(rows.iter().map(popup_row_json).collect()),
            ),
        ]),
    }
}

fn popup_row_json(row: &PopupRow) -> Json {
    json!({"text": row.text, "violating": row.violating, "ink": role_json(row.color)})
}

fn edge_json(e: &EdgeCommand) -> Json {
    let lod = ("lod", json!(tier_name(e.tier())));
    match e {
        EdgeCommand::Stroke {
            from,
            to,
            points,
            color,
            width,
            hot,
            warm,
            dimmed,
        } => object(vec![
            ("kind", json!("stroke")),
            lod,
            ("from", id_json(from)),
            ("to", id_json(to)),
            ("points", Json::Array(points.iter().map(point).collect())),
            ("stroke", role_json(*color)),
            ("width", f32n(*width)),
            ("hot", json!(hot)),
            ("warm", json!(warm)),
            ("dimmed", json!(dimmed)),
        ]),
        EdgeCommand::Head {
            from,
            to,
            head,
            tip,
            behind,
            color,
            fill,
            width,
            hot,
            warm,
            dimmed,
        } => object(vec![
            ("kind", json!("head")),
            lod,
            ("from", id_json(from)),
            ("to", id_json(to)),
            ("head", json!(head)),
            ("tip", point(tip)),
            ("behind", point(behind)),
            ("stroke", role_json(*color)),
            ("fill", role_json(*fill)),
            ("width", f32n(*width)),
            ("hot", json!(hot)),
            ("warm", json!(warm)),
            ("dimmed", json!(dimmed)),
        ]),
        EdgeCommand::Indicator {
            id,
            points,
            violating,
            color,
            dimmed,
        } => object(vec![
            ("kind", json!("indicator")),
            lod,
            ("id", id_json(id)),
            ("points", Json::Array(points.iter().map(point).collect())),
            ("violating", json!(violating)),
            ("fill", role_json(*color)),
            ("dimmed", json!(dimmed)),
        ]),
    }
}
