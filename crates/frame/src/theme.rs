//! Single viewer theme (port of `love/uml/domain/theme.lua`): the semantic
//! color [`Role`]s every drawn element names, the one dark [`palette`] that
//! resolves them, screen-px line widths and the continuous quality ramps.
//!
//! Drawn output never carries RGB: a frame item names a role (a quality
//! role also carries its 0-10 grade or coverage ratio) and the renderer
//! resolves it. [`rgba`] is the reference dark palette; the panel
//! resolves roles through host theme tokens, keeping the quality and
//! violation hues and adapting only to light and dark mode (ADR 0011).
//! Sizes are in points; the renderer applies the display scale.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// An unmultiplied color; `a` 255 is opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// What a color means, never which color it is. Every color the viewer
/// draws is one of these roles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    // Surfaces and ink.
    /// The canvas behind the diagram; arrowhead interiors.
    Canvas,
    /// Panels: the card and source window background, card stripes.
    Panel,
    /// The inspector sidebar.
    Sidebar,
    /// Hairlines: popup frames, separators, bar tracks.
    Line,
    /// Primary text; filled composition diamonds.
    Ink,
    /// Secondary text: stereotypes, levels, hints, private members.
    Muted,
    /// Gold emphasis: package titles, CRAP lines, the proposal banner, the
    /// return line and card headings.
    Accent,
    /// White glyphs on quality dots and stereotype markers; bright figures.
    Marker,
    /// The outline of the selected box or dependency port.
    Selected,
    /// A good figure (no survived or uncovered mutants).
    Good,
    /// A destructive control under the pointer (the window close button).
    Danger,
    /// Behind screen-space chrome text over the diagram.
    ChromePlate,
    /// The dependency hover popup.
    Popup,
    /// The translucent wash of a marked row (target line, card row, child).
    RowWash,
    /// The opaque rule at a marked row's left edge.
    RowBar,
    // Quality (质量色): the hue follows the grade.
    /// A class box or quality dot fill at a 0-10 grade (`None`: unmeasured).
    QualityFill(Option<f64>),
    /// A class frame, title band, rule or quality dot ring at a grade.
    QualityStroke(Option<f64>),
    /// A package (component) fill at its pooled grade, translucent.
    PackageFill(Option<f64>),
    /// A package frame at its pooled grade, translucent.
    PackageStroke(Option<f64>),
    /// Line coverage ink at a 0-1 ratio: good, gold or warn; unknown muted.
    Coverage(Option<f64>),
    /// The 无数据 marker.
    NoData,
    // Relations.
    /// 普通关系色: an edge neither violating nor highlighted.
    Relation,
    /// 关系悬停高亮: a hovered non-violating edge.
    RelationHover,
    /// 依赖链高亮: chain edges and the frames of the boxes on the chain.
    DependencyChain,
    /// 违规关系色: violating edges, indicators and popup rows.
    Violation,
    /// A violating edge on the chain or under the pointer.
    ViolationHot,
    /// A collapsed dependency indicator without violations.
    Indicator,
    // Dependency ports.
    PortFill,
    PortFillMarked,
    PortOutline,
    PortLink,
    // Inspector buttons.
    ButtonHover,
    ButtonActive,
    // Source window.
    SourceBg,
    SourceInk,
    SourceGutterBg,
    SourceGutter,
    SourceSelection,
    SourceComment,
    SourceString,
    SourceKeyword,
}

impl Role {
    /// The stable kebab-case name the scene frame writes.
    pub fn name(self) -> &'static str {
        match self {
            Role::Canvas => "canvas",
            Role::Panel => "panel",
            Role::Sidebar => "sidebar",
            Role::Line => "line",
            Role::Ink => "ink",
            Role::Muted => "muted",
            Role::Accent => "accent",
            Role::Marker => "marker",
            Role::Selected => "selected",
            Role::Good => "good",
            Role::Danger => "danger",
            Role::ChromePlate => "chrome-plate",
            Role::Popup => "popup",
            Role::RowWash => "row-wash",
            Role::RowBar => "row-bar",
            Role::QualityFill(_) => "quality-fill",
            Role::QualityStroke(_) => "quality-stroke",
            Role::PackageFill(_) => "package-fill",
            Role::PackageStroke(_) => "package-stroke",
            Role::Coverage(_) => "coverage",
            Role::NoData => "no-data",
            Role::Relation => "relation",
            Role::RelationHover => "relation-hover",
            Role::DependencyChain => "dependency-chain",
            Role::Violation => "violation",
            Role::ViolationHot => "violation-hot",
            Role::Indicator => "indicator",
            Role::PortFill => "port-fill",
            Role::PortFillMarked => "port-fill-marked",
            Role::PortOutline => "port-outline",
            Role::PortLink => "port-link",
            Role::ButtonHover => "button-hover",
            Role::ButtonActive => "button-active",
            Role::SourceBg => "source-bg",
            Role::SourceInk => "source-ink",
            Role::SourceGutterBg => "source-gutter-bg",
            Role::SourceGutter => "source-gutter",
            Role::SourceSelection => "source-selection",
            Role::SourceComment => "source-comment",
            Role::SourceString => "source-string",
            Role::SourceKeyword => "source-keyword",
        }
    }

    /// Every role, quality roles at one representative parameter; the
    /// frame schema lists exactly these names.
    pub const ALL: [Role; 41] = [
        Role::Canvas,
        Role::Panel,
        Role::Sidebar,
        Role::Line,
        Role::Ink,
        Role::Muted,
        Role::Accent,
        Role::Marker,
        Role::Selected,
        Role::Good,
        Role::Danger,
        Role::ChromePlate,
        Role::Popup,
        Role::RowWash,
        Role::RowBar,
        Role::QualityFill(None),
        Role::QualityStroke(None),
        Role::PackageFill(None),
        Role::PackageStroke(None),
        Role::Coverage(None),
        Role::NoData,
        Role::Relation,
        Role::RelationHover,
        Role::DependencyChain,
        Role::Violation,
        Role::ViolationHot,
        Role::Indicator,
        Role::PortFill,
        Role::PortFillMarked,
        Role::PortOutline,
        Role::PortLink,
        Role::ButtonHover,
        Role::ButtonActive,
        Role::SourceBg,
        Role::SourceInk,
        Role::SourceGutterBg,
        Role::SourceGutter,
        Role::SourceSelection,
        Role::SourceComment,
        Role::SourceString,
        Role::SourceKeyword,
    ];
}

/// The dark palette: a role's color. Translucent roles carry their alpha.
pub fn rgba(role: Role) -> Rgba {
    let (Rgb(r, g, b), a) = match role {
        Role::Canvas => (CANVAS_BG, 255),
        Role::Panel => (PANEL, 255),
        Role::Sidebar => (SIDEBAR, 255),
        Role::Line => (LINE, 255),
        Role::Ink => (INK, 255),
        Role::Muted => (MUTED, 255),
        Role::Accent => (GOLD, 255),
        Role::Marker | Role::Selected | Role::RelationHover => (WHITE, 255),
        Role::Good => (GOOD, 255),
        Role::Danger => (VIOLATION_HOT, 255),
        Role::ChromePlate => (CANVAS_BG, ALPHA_CHROME_PLATE),
        Role::Popup => (POPUP_BG, ALPHA_POPUP),
        Role::RowWash => (GOLD, ALPHA_ROW_WASH),
        Role::RowBar => (GOLD, 255),
        Role::QualityFill(g) => (fill(g), 255),
        Role::QualityStroke(g) => (stroke(g), 255),
        Role::PackageFill(g) => (fill(g), ALPHA_PACKAGE_FILL),
        Role::PackageStroke(g) => (stroke(g), ALPHA_PACKAGE_STROKE),
        Role::Coverage(p) => (coverage(p), 255),
        Role::NoData => (BAND_RED, 255),
        Role::Relation => (EDGE, 255),
        Role::DependencyChain => (RELATED, 255),
        Role::Violation => (EDGE_VIOLATING, 255),
        Role::ViolationHot => (EDGE_VIOLATING_HOT, 255),
        Role::Indicator => (INDICATOR, 255),
        Role::PortFill => (PORT_FILL, 255),
        Role::PortFillMarked => (PORT_FILL_ON, 255),
        Role::PortOutline | Role::PortLink => (MUTED, 255),
        Role::ButtonHover => (BUTTON_HOVER, 255),
        Role::ButtonActive => (BUTTON_ACTIVE, 255),
        Role::SourceBg => (SOURCE_BG, 255),
        Role::SourceInk => (SOURCE_INK, 255),
        Role::SourceGutterBg => (SOURCE_GUTTER_BG, 255),
        Role::SourceGutter => (SOURCE_GUTTER, 255),
        Role::SourceSelection => (SOURCE_SELECTION, 255),
        Role::SourceComment => (SOURCE_COMMENT, 255),
        Role::SourceString => (SOURCE_STRING, 255),
        Role::SourceKeyword => (SOURCE_KEYWORD, 255),
    };
    Rgba { r, g, b, a }
}

/// The palette color of a role without its alpha.
pub fn rgb(role: Role) -> Rgb {
    let c = rgba(role);
    Rgb(c.r, c.g, c.b)
}

/// The shared target/member/hover row marker: a translucent gold wash and
/// an opaque gold rule at the row's left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowMarker {
    pub wash: Role,
    pub bar: Role,
    pub bar_width: f32,
}

pub fn row_marker() -> RowMarker {
    RowMarker {
        wash: Role::RowWash,
        bar: Role::RowBar,
        bar_width: 3.0,
    }
}

pub mod fonts {
    pub const BODY: f32 = 18.0;
    pub const TITLE: f32 = 18.0;
    /// Canvas class, member and package text: the size the layout's 8px a
    /// char estimate sizes boxes for, so labels draw at one size.
    pub const CANVAS: f32 = 16.0;
    pub const BADGE: f32 = 9.0;
    pub const LEVEL: f32 = 11.0;
    pub const MARKER: f32 = 12.0;
    pub const CODE: f32 = 13.0;
    pub const PORT: f32 = 10.0;
    /// Screen-space chrome text (banner, return line, hints, popup rows).
    pub const CHROME: f32 = 18.0;
}

pub mod radius {
    pub const CLASS: f32 = 4.0;
    pub const PACKAGE: f32 = 8.0;
    pub const PORT: f32 = 3.0;
    pub const ROW_WASH: f32 = 2.0;
}

/// Baseline point widths at DPI scale 1: never multiplied by canvas zoom.
pub mod widths {
    pub const CLASS: f32 = 2.5;
    pub const PACKAGE: f32 = 2.0;
    pub const SELECTED: f32 = 3.5;
    pub const EDGE: f32 = 1.6;
    pub const EDGE_HOT: f32 = 2.8;
    pub const EDGE_VIOLATING: f32 = 2.4;
    pub const EDGE_VIOLATING_HOT: f32 = 3.2;
    pub const BAND: f32 = 2.0;
    pub const RULE: f32 = 1.0;
    pub const DOT: f32 = 1.0;
    pub const DASH: f32 = 6.0;
    pub const DASH_GAP: f32 = 4.0;
    pub const PORT: f32 = 1.0;
    pub const PORT_ON: f32 = 2.2;
    pub const PORT_LINK: f32 = 1.3;
}

pub const CANVAS_BG: Rgb = Rgb(22, 28, 32);
pub const PANEL: Rgb = Rgb(26, 36, 40);
pub const SIDEBAR: Rgb = Rgb(18, 22, 24);
pub const LINE: Rgb = Rgb(42, 61, 54);
pub const INK: Rgb = Rgb(236, 236, 228);
pub const WHITE: Rgb = Rgb(255, 255, 255);
pub const MUTED: Rgb = Rgb(157, 184, 168);
pub const GOLD: Rgb = Rgb(232, 196, 72);
pub const GOOD: Rgb = Rgb(95, 181, 138);
pub const VIOLATION: Rgb = Rgb(196, 42, 36);
pub const VIOLATION_HOT: Rgb = Rgb(255, 64, 48);
pub const WARN: Rgb = Rgb(224, 122, 74);
pub const BAND_RED: Rgb = Rgb(196, 42, 36);
pub const BAND_YELLOW: Rgb = Rgb(232, 196, 72);
pub const BAND_GREEN: Rgb = Rgb(95, 181, 138);
pub const EDGE: Rgb = Rgb(157, 184, 168);
pub const EDGE_VIOLATING: Rgb = Rgb(196, 42, 36);
pub const EDGE_VIOLATING_HOT: Rgb = Rgb(255, 64, 48);
pub const INDICATOR: Rgb = Rgb(157, 184, 168);
/// The dependency chain: a blue no quality grade or plain edge uses.
pub const RELATED: Rgb = Rgb(62, 152, 236);
pub const PORT_FILL: Rgb = Rgb(42, 56, 52);
pub const PORT_FILL_ON: Rgb = Rgb(74, 96, 78);
pub const POPUP_BG: Rgb = Rgb(18, 22, 24);
// Sidebar buttons and the dark source window. Its target line is a GOLD
// wash (ALPHA_ROW_WASH); tokens avoid the red/green quality hues.
pub const BUTTON_HOVER: Rgb = Rgb(54, 76, 66);
pub const BUTTON_ACTIVE: Rgb = Rgb(66, 90, 76);
pub const SOURCE_BG: Rgb = PANEL;
pub const SOURCE_INK: Rgb = INK;
pub const SOURCE_GUTTER_BG: Rgb = SIDEBAR;
pub const SOURCE_GUTTER: Rgb = MUTED;
pub const SOURCE_SELECTION: Rgb = Rgb(38, 54, 70);
pub const SOURCE_COMMENT: Rgb = Rgb(120, 140, 130);
pub const SOURCE_STRING: Rgb = Rgb(230, 160, 110);
pub const SOURCE_KEYWORD: Rgb = Rgb(122, 162, 247);

pub const ALPHA_PACKAGE_FILL: u8 = 80;
pub const ALPHA_PACKAGE_STROKE: u8 = 170;
/// A dimmed element (关系淡化) multiplies every premultiplied channel by
/// this /255; changing alpha alone would brighten an opaque color.
pub const ALPHA_DIMMED: u8 = 70;
pub const ALPHA_POPUP: u8 = 240;
/// Gold wash over the hovered class-card row.
pub const ALPHA_ROW_WASH: u8 = 32;
/// The plate behind chrome text: the canvas at 92% (235/255).
pub const ALPHA_CHROME_PLATE: u8 = 235;

pub fn band_color(band: &str) -> Rgb {
    match band {
        "yellow" => BAND_YELLOW,
        "green" => BAND_GREEN,
        _ => BAND_RED,
    }
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let channel = |x: u8, y: u8| (x as f64 + t * (y as f64 - x as f64) + 0.5).floor() as u8;
    Rgb(channel(a.0, b.0), channel(a.1, b.1), channel(a.2, b.2))
}

fn ramp(g: Option<f64>, none: Rgb, red: Rgb, mid: Rgb, green: Rgb) -> Rgb {
    match g {
        None => none,
        Some(g) if g <= 5.0 => mix(red, mid, g / 5.0),
        Some(g) => mix(mid, green, ((g - 5.0) / 5.0).min(1.0)),
    }
}

/// Continuous 0-10 grade ramps (draw.clj fill-for / stroke-for).
pub fn fill(grade: Option<f64>) -> Rgb {
    ramp(
        grade,
        Rgb(36, 52, 48),
        Rgb(74, 40, 24),
        Rgb(61, 58, 24),
        Rgb(30, 74, 56),
    )
}

pub fn stroke(grade: Option<f64>) -> Rgb {
    ramp(
        grade,
        Rgb(90, 110, 100),
        Rgb(224, 122, 74),
        Rgb(212, 192, 90),
        Rgb(95, 181, 138),
    )
}

/// Line coverage ink: >=80% good, >=50% gold, else warn; unknown muted.
pub fn coverage(p: Option<f64>) -> Rgb {
    match p {
        None => MUTED,
        Some(p) if p >= 0.8 => GOOD,
        Some(p) if p >= 0.5 => GOLD,
        _ => WARN,
    }
}
