//! The diagram's legend (#50, ADR 0015): what the diagram's shapes, quality
//! colors and relation colors mean, as the rows the panel's legend draws.
//!
//! The rows are typeset here, in the resident's language (`strings::Text`'s
//! `legend`), for the same reason the class card's rows are (#44): the words
//! are the resident's text, so the panel never words them itself. A row's
//! swatch names a semantic [`Role`], the same vocabulary the diagram is drawn
//! with, so the panel resolves a legend color and a diagram color through one
//! theme and they cannot drift apart (`packages/client/src/theme.ts`).
//!
//! The legend describes the drawing; it never changes it. It is not chrome:
//! nothing here is painted on the canvas, and the panel decides where the rows
//! go (a popover from 「更多」, or resident from 900px of panel width).

use serde_json::{Value as Json, json};

use scene::GRADE_WORST;

use crate::strings::Lang;
use crate::theme::Role;

/// A box's outline: the two shapes the diagram draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A class or component box.
    Rect,
    /// A foreign package's ellipse.
    Oval,
}

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Shape::Rect => "rect",
            Shape::Oval => "oval",
        }
    }
}

/// The small sample drawn beside a legend row: the same roles and shapes the
/// diagram uses, so a swatch is a miniature of what the row explains.
#[derive(Debug, Clone, PartialEq)]
pub enum Sample {
    /// A class or component box: its fill and its frame.
    Box {
        fill: Role,
        stroke: Role,
        shape: Shape,
        dimmed: bool,
    },
    /// A relation line, in its interactive state's color.
    Line { stroke: Role, dimmed: bool },
    /// A dot or marker in one color (无数据).
    Dot { fill: Role },
    /// The continuous 质量色 ramp: its sample stops, worst to best.
    Ramp { stops: Vec<Role> },
}

impl Sample {
    pub fn kind(&self) -> &'static str {
        match self {
            Sample::Box { .. } => "box",
            Sample::Line { .. } => "line",
            Sample::Dot { .. } => "dot",
            Sample::Ramp { .. } => "ramp",
        }
    }
}

/// One explained thing: a stable identity, the resident's label and a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: &'static str,
    pub label: &'static str,
    pub sample: Sample,
}

/// A titled group of rows; the panel draws the title above them.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    pub rows: Vec<Row>,
}

/// The whole legend, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub struct Legend {
    pub sections: Vec<Section>,
}

/// The legend of the diagram, worded in `lang`.
///
/// The grades and colors are chosen to show the range, not an element of any
/// particular view: the legend explains the drawing, so it is the same legend
/// for every frame.
pub fn legend(lang: Lang) -> Legend {
    let text = &lang.text().legend;
    let row = |id, label, sample| Row { id, label, sample };
    Legend {
        sections: vec![
            Section {
                id: "shapes",
                title: text.shapes,
                rows: vec![
                    row(
                        "class-box",
                        text.class_box,
                        Sample::Box {
                            fill: Role::QualityFill(Some(3.0)),
                            stroke: Role::QualityStroke(Some(3.0)),
                            shape: Shape::Rect,
                            dimmed: false,
                        },
                    ),
                    row(
                        "component-box",
                        text.component_box,
                        Sample::Box {
                            fill: Role::PackageFill(Some(7.0)),
                            stroke: Role::PackageStroke(Some(7.0)),
                            shape: Shape::Rect,
                            dimmed: false,
                        },
                    ),
                    row(
                        "foreign-package",
                        text.foreign_package,
                        Sample::Box {
                            // A foreign package has no measured quality, so
                            // commands.rs draws its oval at the worst grade —
                            // the same red as a class with no data.
                            fill: Role::QualityFill(Some(GRADE_WORST)),
                            stroke: Role::QualityStroke(Some(GRADE_WORST)),
                            shape: Shape::Oval,
                            dimmed: false,
                        },
                    ),
                ],
            },
            Section {
                id: "quality",
                title: text.quality,
                rows: vec![
                    row(
                        "quality-ramp",
                        text.quality_ramp,
                        Sample::Ramp {
                            stops: vec![
                                Role::QualityFill(Some(0.0)),
                                Role::QualityFill(Some(5.0)),
                                Role::QualityFill(Some(10.0)),
                            ],
                        },
                    ),
                    row("no-data", text.no_data, Sample::Dot { fill: Role::NoData }),
                    row(
                        "selected",
                        text.selected,
                        Sample::Box {
                            fill: Role::QualityFill(Some(3.0)),
                            stroke: Role::Selected,
                            shape: Shape::Rect,
                            dimmed: false,
                        },
                    ),
                ],
            },
            Section {
                id: "relations",
                title: text.relations,
                rows: vec![
                    row(
                        "relation",
                        text.relation,
                        Sample::Line {
                            stroke: Role::Relation,
                            dimmed: false,
                        },
                    ),
                    row(
                        "violation",
                        text.violation,
                        Sample::Line {
                            stroke: Role::Violation,
                            dimmed: false,
                        },
                    ),
                    row(
                        "relation-hover",
                        text.relation_hover,
                        Sample::Line {
                            stroke: Role::RelationHover,
                            dimmed: false,
                        },
                    ),
                    row(
                        "dependency-chain",
                        text.dependency_chain,
                        Sample::Line {
                            stroke: Role::DependencyChain,
                            dimmed: false,
                        },
                    ),
                    row(
                        "dimmed",
                        text.dimmed,
                        Sample::Line {
                            stroke: Role::Relation,
                            dimmed: true,
                        },
                    ),
                ],
            },
        ],
    }
}

fn sample_json(sample: &Sample) -> Json {
    let color = crate::frame::role_json;
    match sample {
        Sample::Box {
            fill,
            stroke,
            shape,
            dimmed,
        } => json!({
            "kind": sample.kind(),
            "shape": shape.name(),
            "fill": color(*fill),
            "stroke": color(*stroke),
            "dimmed": dimmed,
        }),
        Sample::Line { stroke, dimmed } => json!({
            "kind": sample.kind(),
            "stroke": color(*stroke),
            "dimmed": dimmed,
        }),
        Sample::Dot { fill } => json!({
            "kind": sample.kind(),
            "fill": color(*fill),
        }),
        Sample::Ramp { stops } => json!({
            "kind": sample.kind(),
            "stops": stops.iter().map(|stop| color(*stop)).collect::<Vec<_>>(),
        }),
    }
}

/// The legend as the frame's `legend` field: sections of rows, each sample
/// naming its semantic roles.
pub fn json(legend: &Legend) -> Json {
    json!({
        "sections": legend
            .sections
            .iter()
            .map(|section| json!({
                "id": section.id,
                "title": section.title,
                "rows": section
                    .rows
                    .iter()
                    .map(|row| json!({
                        "id": row.id,
                        "label": row.label,
                        "sample": sample_json(&row.sample),
                    }))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}
