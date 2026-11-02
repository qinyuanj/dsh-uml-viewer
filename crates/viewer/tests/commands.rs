//! Class-box and edge command parity with the LOVE adapters
//! (`love/spec/class_boxes_spec.lua`, `love/spec/edges_spec.lua`): the
//! pure scene -> command layer, independent of any renderer.

use crate::common;

use common::{composite, contrast};
use edn::Value;
use scene::document::compile_view;
use viewer::commands::{self, Command, EdgeCommand};

fn scene(classes: &str) -> Value {
    let text = format!("{{:packages [{{:label \"P\" :classes {classes}}}]}}");
    let doc = scene::ir::read_document(&text).unwrap();
    compile_view(&doc, &[], &[], &Value::Map(vec![]))
}

/// The root view of two modules `a` and `b` joined by `edges`, compiled
/// with `extra`; a hierarchical view keeps the authored ids.
fn two_modules(edges: &str, extra: &str) -> Value {
    let text = format!(
        "{{:hierarchical true :prefix \"x\" :order [a b] \
          :classes [{{:id :a :name \"A\" :ns \"x.a\"}} {{:id :b :name \"B\" :ns \"x.b\"}}] \
          :edges {edges}}}"
    );
    let doc = scene::ir::read_document(&text).unwrap();
    compile_view(&doc, &[], &[], &edn::parse(extra).unwrap())
}

fn commands_of(s: &Value) -> Vec<Command> {
    commands::commands(s, None, None)
}

fn attention_scene() -> Value {
    edn::parse(
        r#"{
          :packages []
          :classes [
            {:id :a :name "A" :rect {:x 0 :y 0 :w 100 :h 60} :lines [{:kind :name :text "A"}]}
            {:id :b :name "B" :rect {:x 120 :y 0 :w 100 :h 60} :lines [{:kind :name :text "B"}]}
            {:id :c :name "C" :rect {:x 240 :y 0 :w 100 :h 60} :lines [{:kind :name :text "C"}]}
            {:id :e :name "E" :rect {:x 360 :y 0 :w 100 :h 60} :lines [{:kind :name :text "E"} {:kind :child :id :run :text "run"}]}
          ]
          :edges [
            {:from :a :to :b :kind :dependency :strokes [[[0 0] [1 1]]]}
            {:from :b :to :c :kind :dependency :strokes [[[1 1] [2 2]]]}
            {:from :c :to :a :kind :dependency :violating true :strokes [[[2 2] [0 0]]]}
            {:from :e :to :a :kind :dependency :strokes [[[3 3] [0 0]]]}
            {:from :a :to :e :kind :implements :strokes [[[0 0] [3 3]]]}
          ]
        }"#,
    )
    .unwrap()
}

fn collapsed_scene(violating: bool) -> Value {
    let mut doc = scene::ir::read_document(
        r#"{:hierarchical true
            :classes [{:id :a.one :name "One"} {:id :a.two :name "Two"} {:id :b.leaf :name "B"}]
            :edges [{:from :a.one :to :b.leaf :kind :association}
                    {:from :a.two :to :b.leaf :kind :dependency}]}"#,
    )
    .unwrap();
    if violating {
        let mut edges = edn::record::seq(doc.get(&Value::keyword("edges"))).to_vec();
        edges[1].assoc(Value::keyword("violating"), Value::Bool(true));
        doc.assoc(Value::keyword("edges"), Value::Vector(edges));
    }
    compile_view(&doc, &[], &[], &edn::parse("{:declutter :arrows}").unwrap())
}

fn class_command<'a>(commands: &'a [Command], wanted: &str) -> &'a Command {
    commands
        .iter()
        .find(|c| match c {
            Command::Class { id: Some(id), .. } => edn::record::local_name(id) == Some(wanted),
            _ => false,
        })
        .unwrap_or_else(|| panic!("missing class {wanted}"))
}

fn class_style(command: &Command) -> (viewer::theme::Rgb, viewer::theme::Rgb, bool, bool) {
    let Command::Class {
        fill,
        stroke,
        selected,
        dimmed,
        ..
    } = command
    else {
        unreachable!()
    };
    (*fill, *stroke, *selected, *dimmed)
}

fn edge_style(commands: &[EdgeCommand], from: &str, to: &str) -> (viewer::theme::Rgb, f32, bool) {
    commands
        .iter()
        .find_map(|c| match c {
            EdgeCommand::Stroke {
                from: Some(actual_from),
                to: Some(actual_to),
                color,
                width,
                dimmed,
                ..
            } if edn::record::local_name(actual_from) == Some(from)
                && edn::record::local_name(actual_to) == Some(to) =>
            {
                Some((*color, *width, *dimmed))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing edge {from} -> {to}"))
}

fn of_kind<'a>(commands: &'a [Command], kind: &str) -> Vec<&'a Command> {
    commands.iter().filter(|c| c.kind() == kind).collect()
}

#[test]
fn selected_class_illuminates_the_whole_outgoing_dependency_chain() {
    let s = attention_scene();
    let plain = commands_of(&s);
    let selected = edn::parse("{:kind :class :id :a}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    let (plain_a_fill, ..) = class_style(class_command(&plain, "a"));
    let (a_fill, a_stroke, a_selected, a_dimmed) = class_style(class_command(&commands, "a"));
    let (b_fill, b_stroke, b_selected, b_dimmed) = class_style(class_command(&commands, "b"));
    let (c_fill, c_stroke, c_selected, c_dimmed) = class_style(class_command(&commands, "c"));
    let (e_fill, e_stroke, e_selected, e_dimmed) = class_style(class_command(&commands, "e"));

    assert!(a_selected && !a_dimmed);
    assert_eq!(viewer::theme::WHITE, a_stroke);
    assert_eq!(plain_a_fill, a_fill, "selection preserves quality fill");
    for (fill, stroke, selected, dimmed) in [
        (b_fill, b_stroke, b_selected, b_dimmed),
        (c_fill, c_stroke, c_selected, c_dimmed),
    ] {
        assert!(!selected && !dimmed);
        assert_eq!(plain_a_fill, fill, "chain preserves quality fill");
        assert!(contrast(viewer::theme::WHITE, stroke) >= 3.0, "{stroke:?}");
    }
    assert_eq!(b_stroke, c_stroke);
    let Command::Class {
        stroke_width: a_width,
        ..
    } = class_command(&commands, "a")
    else {
        unreachable!()
    };
    let Command::Class {
        stroke_width: b_width,
        ..
    } = class_command(&commands, "b")
    else {
        unreachable!()
    };
    assert_eq!(viewer::theme::widths::SELECTED, *a_width);
    assert_eq!(viewer::theme::widths::CLASS, *b_width);
    assert!(!e_selected && e_dimmed);
    assert_ne!(b_stroke, e_stroke, "chain and outside strokes differ");

    let edges = commands::edge_commands(&s, Some(&selected), None);
    let (ab, ab_width, ab_dimmed) = edge_style(&edges, "a", "b");
    let (bc, _, bc_dimmed) = edge_style(&edges, "b", "c");
    let (ca, _, ca_dimmed) = edge_style(&edges, "c", "a");
    assert!(!ab_dimmed && !bc_dimmed && !ca_dimmed);
    assert_eq!(ab, bc);
    assert_eq!(viewer::theme::widths::EDGE, ab_width);
    assert!(contrast(viewer::theme::WHITE, ab) >= 3.0);
    assert!(
        contrast(
            ab,
            composite(
                viewer::theme::CANVAS_BG,
                viewer::theme::fade(e_stroke.color32())
            )
        ) >= 3.0
    );
    assert_eq!(viewer::theme::EDGE_VIOLATING_HOT, ca);
    assert!(
        edge_style(&edges, "e", "a").2,
        "incoming edge stays outside"
    );
    assert!(
        edge_style(&edges, "a", "e").2,
        "implementation stays outside"
    );

    let outside_name = commands.iter().find(|c| match c {
        Command::Text { role, text, .. } => role == "name" && text == "E",
        _ => false,
    });
    let Some(Command::Text { dimmed, .. }) = outside_name else {
        panic!("missing outside class name")
    };
    assert!(!dimmed, "outside class names stay light");
    let outside_fill = composite(
        viewer::theme::CANVAS_BG,
        viewer::theme::fade(e_fill.color32()),
    );
    assert!(contrast(commands::text_color("name"), outside_fill) >= 4.5);
}

#[test]
fn every_outgoing_branch_is_lit_and_a_leaf_has_no_chain_edges() {
    let s = edn::parse(
        r#"{:packages []
            :classes [{:id :a :rect {:x 0 :y 0 :w 80 :h 40}}
                      {:id :b :rect {:x 90 :y 0 :w 80 :h 40}}
                      {:id :c :rect {:x 180 :y 0 :w 80 :h 40}}
                      {:id :d :rect {:x 270 :y 0 :w 80 :h 40}}
                      {:id :e :rect {:x 360 :y 0 :w 80 :h 40}}]
            :edges [{:from :a :to :b :kind :dependency :strokes [[[0 0] [1 0]]]}
                    {:from :a :to :c :kind :dependency :strokes [[[0 0] [2 0]]]}
                    {:from :b :to :d :kind :dependency :strokes [[[1 0] [3 0]]]}
                    {:from :c :to :d :kind :dependency :strokes [[[2 0] [3 0]]]}]}"#,
    )
    .unwrap();
    let selected = edn::parse("{:kind :class :id :a}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    for id in ["b", "c", "d"] {
        let (_, stroke, selected, dimmed) = class_style(class_command(&commands, id));
        assert!(!selected && !dimmed, "branch {id}");
        assert!(contrast(viewer::theme::WHITE, stroke) >= 3.0);
    }
    assert!(class_style(class_command(&commands, "e")).3);

    let selected = edn::parse("{:kind :class :id :d}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    for id in ["a", "b", "c", "e"] {
        assert!(
            class_style(class_command(&commands, id)).3,
            "outside leaf {id}"
        );
    }
    assert!(
        commands::edge_commands(&s, Some(&selected), None)
            .iter()
            .all(|e| matches!(e, EdgeCommand::Stroke { dimmed: true, .. }))
    );
}

#[test]
fn non_dependency_relationships_never_join_the_chain() {
    for kind in [
        "implements",
        "association",
        "inheritance",
        "aggregation",
        "composition",
    ] {
        let s = edn::parse(&format!(
            "{{:packages [] :classes [{{:id :a :rect {{:x 0 :y 0 :w 80 :h 40}}}} {{:id :b :rect {{:x 90 :y 0 :w 80 :h 40}}}}] :edges [{{:from :a :to :b :kind :{kind} :strokes [[[0 0] [1 0]]]}}]}}"
        ))
        .unwrap();
        let selected = edn::parse("{:kind :class :id :a}").unwrap();
        let commands = commands::commands(&s, Some(&selected), None);
        assert!(class_style(class_command(&commands, "b")).3, "{kind}");
        assert!(
            edge_style(
                &commands::edge_commands(&s, Some(&selected), None),
                "a",
                "b"
            )
            .2,
            "{kind}"
        );
    }
}

#[test]
fn outside_violations_stay_red_and_dim() {
    let s = edn::parse(
        r#"{:packages []
            :classes [{:id :a :rect {:x 0 :y 0 :w 80 :h 40}}
                      {:id :b :rect {:x 90 :y 0 :w 80 :h 40}}]
            :edges [{:from :b :to :a :kind :dependency :violating true
                     :strokes [[[1 0] [0 0]]]}]}"#,
    )
    .unwrap();
    let selected = edn::parse("{:kind :class :id :a}").unwrap();
    let (color, _, dimmed) = edge_style(
        &commands::edge_commands(&s, Some(&selected), None),
        "b",
        "a",
    );
    assert!(dimmed);
    assert_eq!(viewer::theme::EDGE_VIOLATING, color);
    assert_ne!(viewer::theme::WHITE, color);
}

#[test]
fn a_violating_dependency_inside_a_folded_bundle_stays_red() {
    let collapsed = collapsed_scene(true);
    let selected = edn::parse("{:kind :class :id :a.two}").unwrap();
    let (color, _, dimmed) = edge_style(
        &commands::edge_commands(&collapsed, Some(&selected), None),
        "a",
        "b",
    );
    assert!(!dimmed);
    assert_eq!(viewer::theme::EDGE_VIOLATING_HOT, color);
    assert_ne!(viewer::theme::WHITE, color);
}

#[test]
fn foreign_ovals_use_the_same_selected_and_chain_frames() {
    let s = edn::parse(
        r#"{:packages []
            :classes [{:id :a :shape :oval :rect {:x 0 :y 0 :w 80 :h 40}}
                      {:id :b :shape :oval :rect {:x 90 :y 0 :w 80 :h 40}}]
            :edges [{:from :a :to :b :kind :dependency :strokes [[[0 0] [1 0]]]}]}"#,
    )
    .unwrap();
    let selected = edn::parse("{:kind :class :id :a}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    for (id, selected, dimmed) in [("a", true, false), ("b", false, false)] {
        let command = class_command(&commands, id);
        let Command::Class {
            shape,
            stroke,
            selected: actual_selected,
            dimmed: actual_dimmed,
            ..
        } = command
        else {
            unreachable!()
        };
        assert_eq!("oval", shape);
        assert_eq!((selected, dimmed), (*actual_selected, *actual_dimmed));
        if selected {
            assert_eq!(viewer::theme::WHITE, *stroke);
        } else {
            assert!(contrast(viewer::theme::WHITE, *stroke) >= 3.0);
        }
    }
}

#[test]
fn hover_whitens_edges_without_changing_frames_or_violation_red() {
    let s = attention_scene();
    let selected = edn::parse("{:kind :class :id :a}").unwrap();
    let hovered_e = edn::parse("{:kind :class :id :e}").unwrap();
    let before = commands::commands(&s, Some(&selected), None);
    let during = commands::commands(&s, Some(&selected), Some(&hovered_e));
    assert_eq!(
        class_style(class_command(&before, "e")),
        class_style(class_command(&during, "e")),
        "hover does not modify a class frame"
    );

    let edges = commands::edge_commands(&s, Some(&selected), Some(&hovered_e));
    for (from, to) in [("e", "a"), ("a", "e")] {
        let (color, width, dimmed) = edge_style(&edges, from, to);
        assert_eq!(viewer::theme::WHITE, color);
        assert_eq!(viewer::theme::widths::EDGE, width);
        assert!(!dimmed, "hovered-class edges do not dim");
    }
    let chain_before = edge_style(
        &commands::edge_commands(&s, Some(&selected), None),
        "a",
        "b",
    );
    assert_eq!(chain_before, edge_style(&edges, "a", "b"));

    let hovered_ab = edn::parse("{:kind :edge :from :a :to :b}").unwrap();
    let hovered_edges = commands::edge_commands(&s, Some(&selected), Some(&hovered_ab));
    assert_eq!(viewer::theme::WHITE, edge_style(&hovered_edges, "a", "b").0);
    assert!(!edge_style(&hovered_edges, "a", "b").2);
    assert_eq!(chain_before, edge_style(&edges, "a", "b"));

    let hovered_ca = edn::parse("{:kind :edge :from :c :to :a}").unwrap();
    let red = commands::edge_commands(&s, Some(&selected), Some(&hovered_ca));
    assert_eq!(
        viewer::theme::EDGE_VIOLATING_HOT,
        edge_style(&red, "c", "a").0
    );
    assert_ne!(viewer::theme::WHITE, edge_style(&red, "c", "a").0);

    let unselected = commands::edge_commands(&s, None, Some(&hovered_e));
    assert_eq!(viewer::theme::WHITE, edge_style(&unselected, "e", "a").0);
    assert!(commands_of(&s).iter().all(|c| match c {
        Command::Class { dimmed, .. } => !dimmed,
        _ => true,
    }));
}

#[test]
fn collapsed_dependency_bundles_light_only_their_visible_endpoints() {
    let collapsed = collapsed_scene(false);
    assert_eq!(
        1,
        edn::record::seq(collapsed.get(&Value::keyword("edges"))).len()
    );
    let selected = edn::parse("{:kind :class :id :a.two}").unwrap();
    let edges = commands::edge_commands(&collapsed, Some(&selected), None);
    let (color, _, dimmed) = edge_style(&edges, "a", "b");
    assert!(
        !dimmed,
        "a bundled dependency keeps its visible edge on-chain"
    );
    assert!(contrast(viewer::theme::WHITE, color) >= 3.0);

    let commands = commands::commands(&collapsed, Some(&selected), None);
    let visible: Vec<&Command> = commands
        .iter()
        .filter(|c| {
            matches!(
                c,
                Command::Class {
                    related: true,
                    dimmed: false,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        2,
        visible.len(),
        "both visible class endpoints join the chain"
    );
    assert!(
        commands.iter().all(|c| match c {
            Command::Class { id, .. } =>
                id.as_ref().and_then(edn::record::local_name) != Some("a.one")
                    && id.as_ref().and_then(edn::record::local_name) != Some("a.two")
                    && id.as_ref().and_then(edn::record::local_name) != Some("b.leaf"),
            _ => true,
        }),
        "highlighting does not add off-screen classes"
    );
}

#[test]
fn selecting_a_visible_folded_class_follows_its_bundled_leaf_dependencies() {
    let collapsed = collapsed_scene(false);
    let selected = edn::parse("{:kind :class :id :a}").unwrap();
    let edges = commands::edge_commands(&collapsed, Some(&selected), None);
    assert!(
        !edge_style(&edges, "a", "b").2,
        "the visible aggregate's outgoing dependency stays on-chain"
    );
    let commands = commands::commands(&collapsed, Some(&selected), None);
    let (_, a_stroke, a_selected, a_dimmed) = class_style(class_command(&commands, "a"));
    let (_, b_stroke, b_selected, b_dimmed) = class_style(class_command(&commands, "b"));
    assert!(a_selected && !a_dimmed);
    assert_eq!(viewer::theme::WHITE, a_stroke);
    assert!(!b_selected && !b_dimmed);
    assert!(contrast(viewer::theme::WHITE, b_stroke) >= 3.0);
}

#[test]
fn chain_stays_three_to_one_above_dimmed_yellow_quality() {
    let mut s = attention_scene();
    let mut classes = edn::record::seq(s.get(&Value::keyword("classes"))).to_vec();
    classes[3].assoc(
        Value::keyword("crap"),
        edn::parse("{:mu 13.777777777777778}").unwrap(),
    );
    classes[3].assoc(Value::keyword("killed"), Value::Integer(9));
    classes[3].assoc(Value::keyword("survived"), Value::Integer(1));
    assert_eq!(Some(5.0), scene::rating(&classes[3]).class_grade());
    s.assoc(Value::keyword("classes"), Value::Vector(classes));

    let selected = edn::parse("{:kind :class :id :a}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    let chain = class_style(class_command(&commands, "b")).1;
    let outside = class_style(class_command(&commands, "e")).1;
    let outside = composite(
        viewer::theme::CANVAS_BG,
        viewer::theme::fade(outside.color32()),
    );
    assert!(contrast(chain, outside) >= 3.0, "{chain:?} vs {outside:?}");
}

#[test]
fn folded_package_endpoints_use_the_chain_frame() {
    let s = edn::parse(
        r#"{:packages [{:id :p :label "P" :rect {:x 0 :y 0 :w 100 :h 80}}
                       {:id :q :label "Q" :rect {:x 120 :y 0 :w 100 :h 80}}]
            :classes []
            :edges [{:from :p :to :q :kind :association
                     :deps [{:from :a.one :to :b.leaf :kind :association}
                            {:from :a.two :to :b.leaf :kind :dependency}]
                     :strokes [[[0 0] [1 0]]]}]}"#,
    )
    .unwrap();
    let selected = edn::parse("{:kind :class :id :a.two}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    let frames: Vec<(viewer::theme::Rgb, bool, bool)> = commands
        .iter()
        .filter_map(|c| match c {
            Command::Package {
                stroke,
                related,
                dimmed,
                ..
            } => Some((*stroke, *related, *dimmed)),
            _ => None,
        })
        .collect();
    assert_eq!(2, frames.len());
    for (stroke, related, dimmed) in frames {
        assert!(related && !dimmed);
        assert!(contrast(viewer::theme::WHITE, stroke) >= 3.0);
    }
}

#[test]
fn selected_member_uses_the_shared_readable_row_marker() {
    let s = attention_scene();
    let selected = edn::parse("{:kind :child :id :run :parent :e}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    let washes: Vec<&Command> = of_kind(&commands, "child-wash");
    assert_eq!(1, washes.len());
    let Command::ChildWash { marker, .. } = washes[0] else {
        unreachable!()
    };
    let fill = class_style(class_command(&commands, "e")).0;
    let background = composite(fill, marker.wash);
    assert!(contrast(commands::text_color("child"), background) >= 4.5);
    assert!(
        contrast(
            viewer::theme::Rgb(marker.bar.r(), marker.bar.g(), marker.bar.b()),
            fill
        ) >= 3.0
    );
    assert!(marker.wash.a() < 128);

    let hovered = commands::commands(&s, None, Some(&selected));
    assert!(of_kind(&hovered, "child-wash").is_empty());
}

#[test]
fn member_and_port_selection_focus_their_owning_class() {
    let mut s = attention_scene();
    let mut classes = edn::record::seq(s.get(&Value::keyword("classes"))).to_vec();
    classes[0].assoc(
        Value::keyword("out-ports"),
        edn::parse("[{:id :io :name \"IO\" :rect {:x 40 :y 55 :w 20 :h 12}}]").unwrap(),
    );
    s.assoc(Value::keyword("classes"), Value::Vector(classes));

    for selected in [
        edn::parse("{:kind :child :id :run :parent :a}").unwrap(),
        edn::parse("{:kind :port :id :io :parent :a}").unwrap(),
    ] {
        let commands = commands::commands(&s, Some(&selected), None);
        let (_, a_stroke, a_selected, _) = class_style(class_command(&commands, "a"));
        let (_, b_stroke, b_selected, b_dimmed) = class_style(class_command(&commands, "b"));
        assert!(a_selected);
        assert_eq!(viewer::theme::WHITE, a_stroke);
        assert!(!b_selected && !b_dimmed);
        assert!(contrast(viewer::theme::WHITE, b_stroke) >= 3.0);
    }

    let selected = edn::parse("{:kind :port :id :io :parent :a}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    let port = commands
        .iter()
        .find(|c| matches!(c, Command::Port { .. }))
        .unwrap();
    let Command::Port {
        marked,
        stroke,
        stroke_width,
        ..
    } = port
    else {
        unreachable!()
    };
    assert!(marked);
    assert_eq!(viewer::theme::WHITE, *stroke);
    assert_eq!(viewer::theme::widths::PORT_ON, *stroke_width);
}

#[test]
fn selecting_a_package_marks_only_that_package() {
    let mut s = attention_scene();
    s.assoc(
        Value::keyword("packages"),
        edn::parse("[{:id :p :label \"P\" :rect {:x 0 :y 0 :w 500 :h 100}}]").unwrap(),
    );
    let selected = edn::parse("{:kind :package :id :p}").unwrap();
    let commands = commands::commands(&s, Some(&selected), None);
    let plain = commands_of(&s);
    let package = commands
        .iter()
        .find(|c| matches!(c, Command::Package { .. }))
        .unwrap();
    let Command::Package {
        stroke,
        selected,
        stroke_width,
        fill,
        ..
    } = package
    else {
        unreachable!()
    };
    assert!(selected);
    assert_eq!(viewer::theme::WHITE, *stroke);
    assert_eq!(viewer::theme::widths::SELECTED, *stroke_width);
    let Command::Package {
        fill: plain_fill, ..
    } = plain
        .iter()
        .find(|c| matches!(c, Command::Package { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(*plain_fill, *fill);
    for id in ["a", "b", "c", "e"] {
        assert_eq!(
            class_style(class_command(&plain, id)),
            class_style(class_command(&commands, id)),
            "package selection creates no class chain or dimming"
        );
    }
}

#[test]
fn separate_c_and_m_dots_keep_either_absent_metric_red() {
    let s = scene(
        "[{:id :a :crap {:mu 2} :killed 9 :survived 1} {:id :b :killed 1 :survived 0} {:id :c :crap {:mu 2}} {:id :d}]",
    );
    let commands = commands_of(&s);
    let mut statuses = Vec::new();
    for command in of_kind(&commands, "quality") {
        let Command::Quality {
            owner,
            rect,
            text,
            band,
            radius,
            x,
            y,
            ..
        } = command
        else {
            unreachable!()
        };
        if *owner != "class" {
            continue;
        }
        statuses.push((*text, *band));
        // Stacked inside the right padding, clear of the text rows.
        assert!(x - radius >= rect.right() - commands::PAD && x + radius <= rect.right());
        assert!(y - radius >= rect.y && y + radius <= rect.bottom());
    }
    assert_eq!(
        vec![
            ("C", "green"),
            ("M", "yellow"),
            ("C", "red"),
            ("M", "green"),
            ("C", "green"),
            ("M", "red"),
            ("C", "red"),
            ("M", "red"),
        ],
        statuses
    );
}

#[test]
fn white_markers_and_level_zero_stay_inside_their_bounds() {
    let s = scene("[{:id :a :stereotype :abstract :level 0} {:id :b :stereotype :interface}]");
    let commands = commands_of(&s);
    let markers = of_kind(&commands, "marker");
    assert_eq!(2, markers.len());
    for (marker, text) in markers.iter().zip(["α", "I"]) {
        let Command::Marker {
            rect,
            text: t,
            x,
            y,
            color,
            ..
        } = marker
        else {
            unreachable!()
        };
        assert_eq!(text, t);
        assert_eq!(egui::Color32::WHITE, *color);
        assert!(*x >= rect.x && x + 8.0 < rect.right() && *y >= rect.y);
    }
    let levels = of_kind(&commands, "level");
    let Command::Level { rect, text, x, .. } = levels[0] else {
        unreachable!()
    };
    assert_eq!("0", text);
    assert_eq!(rect.x + 4.0, *x);
}

#[test]
fn italic_distinguishes_classifier_and_grouping_names() {
    // drill?/contents reach the scene through hierarchy views, not the flat
    // IR normalizer; like the busted spec, put them on the compiled classes.
    let mut s = scene(
        "[{:id :a :stereotype :interface} {:id :b :stereotype :abstract} {:id :c} {:id :d} {:id :e :stereotype :class :fields [\"field\"]}]",
    );
    if let Some(Value::Vector(classes)) = s.get(&Value::keyword("classes")).cloned() {
        let mut classes = classes;
        classes[2].assoc(Value::keyword("drill?"), Value::Bool(true));
        classes[3].assoc(
            Value::keyword("contents"),
            edn::parse("[{:id :kid :name \"Kid\"}]").unwrap(),
        );
        s.assoc(Value::keyword("classes"), Value::Vector(classes));
    }
    let commands = commands_of(&s);
    let mut styles = Vec::new();
    for command in of_kind(&commands, "text") {
        let Command::Text {
            role,
            italic,
            bold,
            text,
            size,
            ..
        } = command
        else {
            unreachable!()
        };
        if role == "name" {
            styles.push(*italic);
            assert_eq!(16.0, *size);
            assert!(bold);
        } else if text == "field" {
            assert!(!italic && !bold);
            assert_eq!(16.0, *size);
        }
    }
    assert_eq!(vec![true, true, true, true, false], styles);
}

#[test]
fn pooled_package_metrics_paint_separate_dots() {
    let s = scene(
        "[{:id :a :crap {:mu 2 :sigma 0 :n 3} :killed 9 :survived 1 :ops [\"a\" \"b\" \"c\"]} {:id :b :crap {:mu 20 :sigma 0 :n 1} :killed 1 :survived 1}]",
    );
    let commands = commands_of(&s);
    let packages = of_kind(&commands, "package");
    let Command::Package { band, text, .. } = packages[0] else {
        unreachable!()
    };
    assert_eq!("red", *band);
    assert_eq!("P", text);
    let statuses: Vec<(&str, &str)> = of_kind(&commands, "quality")
        .into_iter()
        .filter_map(|c| match c {
            Command::Quality {
                owner: "package",
                text,
                band,
                ..
            } => Some((*text, *band)),
            _ => None,
        })
        .collect();
    assert_eq!(vec![("C", "red"), ("M", "yellow")], statuses);

    let absent = scene("[{:id :a :crap 2 :killed 1 :survived 0} {:id :b :ops [\"x\" \"y\"]}]");
    let commands = commands_of(&absent);
    let Command::Package { band, .. } = of_kind(&commands, "package")[0] else {
        unreachable!()
    };
    assert_eq!("red", *band);
}

#[test]
fn unmeasured_classes_get_the_worst_grade_and_a_no_data_marker() {
    let s = scene("[{:id :a}]");
    let commands = commands_of(&s);
    let Command::Class { missing, grade, .. } = of_kind(&commands, "class")[0] else {
        unreachable!()
    };
    assert!(missing);
    assert_eq!(Some(scene::GRADE_WORST), *grade);
    let no_data = of_kind(&commands, "no-data");
    assert_eq!(1, no_data.len());
    let Command::NoData {
        text, right, rect, ..
    } = no_data[0]
    else {
        unreachable!()
    };
    assert_eq!(viewer::strings::NO_DATA, text);
    assert!(*right <= rect.right());
}

#[test]
fn a_name_band_runs_under_the_name_slot_in_the_box_stroke() {
    let s = scene("[{:id :a :name \"Plain\" :crap {:mu 2} :killed 9 :survived 1}]");
    let commands = commands_of(&s);
    let bands = of_kind(&commands, "band");
    assert_eq!(1, bands.len());
    let Command::Band {
        rect, x, w, color, ..
    } = bands[0]
    else {
        unreachable!()
    };
    assert_eq!(rect.x + 6.0, *x);
    assert_eq!(rect.w - 12.0, *w);
    let g = scene::metric_rating(Some(2.0), Some(9.0), Some(1.0), None).grade;
    assert_eq!(viewer::theme::stroke(g), *color);
}

#[test]
fn edges_choose_heads_and_violation_widths() {
    let text = "{:packages [{:id :p :label \"P\" :classes [{:id :a :name \"A\"} {:id :b :name \"B\"}]}] \
                :edges [{:from :a :to :b :kind :association} {:from :a :to :b :kind :dependency} \
                        {:from :a :to :b :kind :aggregation} {:from :a :to :b :kind :composition} \
                        {:from :a :to :b :kind :inheritance} {:from :a :to :b :kind :implements :violating true}]}";
    let s = compile_view(
        &scene::ir::read_document(text).unwrap(),
        &[],
        &[],
        &Value::Map(vec![]),
    );
    let heads: Vec<String> = commands::edge_commands(&s, None, None)
        .into_iter()
        .filter_map(|c| match c {
            EdgeCommand::Head { head, .. } => Some(head),
            _ => None,
        })
        .collect();
    assert_eq!(
        vec![
            "open",
            "open",
            "diamond",
            "diamond-fill",
            "triangle",
            "triangle"
        ],
        heads
    );
    // A hovered non-violating edge is white without a width boost; a
    // violating one keeps its hot violation width and red family.
    let hovered = edn::parse("{:kind :edge :from :d0-a :to :d0-b}").unwrap();
    let hot: Vec<(f32, viewer::theme::Rgb)> = commands::edge_commands(&s, None, Some(&hovered))
        .into_iter()
        .filter_map(|c| match c {
            EdgeCommand::Head { width, color, .. } => Some((width, color)),
            _ => None,
        })
        .collect();
    assert_eq!(6, hot.len());
    assert_eq!((viewer::theme::widths::EDGE, viewer::theme::WHITE), hot[0]);
    assert_eq!(
        (
            viewer::theme::widths::EDGE_VIOLATING_HOT,
            viewer::theme::EDGE_VIOLATING_HOT
        ),
        *hot.last().unwrap()
    );
}

#[test]
fn hidden_edges_become_violation_aware_indicators() {
    let s = two_modules(
        "[{:from :a :to :b :kind :dependency :violating true}]",
        "{:declutter :triangles}",
    );
    let commands = commands::edge_commands(&s, None, None);
    assert_eq!(2, commands.len());
    for command in &commands {
        let EdgeCommand::Indicator {
            violating, points, ..
        } = command
        else {
            panic!("expected an indicator, got {command:?}")
        };
        assert!(violating);
        assert_eq!(3, points.len());
    }
}

#[test]
fn hide_edges_draws_indicators_and_its_absence_draws_strokes() {
    // adapters/edges.lua: `if g(g(scene,'diagram'),'hide-edges') then` draw
    // indicators, else strokes. Through the scene interface the flag comes
    // from the triangles declutter; the full view leaves it unset.
    let commands_for = |declutter: &str| {
        let s = two_modules(
            "[{:from :a :to :b :kind :dependency}]",
            &format!("{{:declutter {declutter}}}"),
        );
        commands::edge_commands(&s, None, None)
    };
    let hidden = commands_for(":triangles");
    assert_eq!(2, hidden.len());
    assert!(
        hidden
            .iter()
            .all(|c| matches!(c, EdgeCommand::Indicator { .. })),
        "{hidden:?}"
    );
    let shown = commands_for(":full");
    assert!(
        shown
            .iter()
            .any(|c| matches!(c, EdgeCommand::Stroke { .. })),
        "{shown:?}"
    );
    assert!(
        !shown
            .iter()
            .any(|c| matches!(c, EdgeCommand::Indicator { .. })),
        "{shown:?}"
    );
}

#[test]
fn indicator_hover_offers_the_from_to_dependency_rows() {
    // The chrome popup lists `from -> to` sorted with violations first.
    let s = edn::parse("{:classes [] :packages [] :edges [] :dep-indicators []}").unwrap();
    let hover = edn::parse(
        "{:kind :dep :id :a :deps [{:from :a :to :b :violating false} {:from :c :to :d :violating true}]}",
    )
    .unwrap();
    let mut state = scene::document::ViewState {
        scene: s,
        hover: Some(hover),
        pointer: Some((100.0, 100.0)),
        ..Default::default()
    };
    state.focus = vec![];
    let chrome = commands::chrome(&state, 800.0, 600.0);
    let popup = chrome
        .iter()
        .find_map(|c| match c {
            Command::Popup { rows, .. } => Some(rows),
            _ => None,
        })
        .expect("a dep hover with deps shows the popup");
    let rows: Vec<(String, bool)> = popup
        .iter()
        .map(|r| (r.text.clone(), r.violating))
        .collect();
    assert_eq!(
        vec![("c -> d".to_string(), true), ("a -> b".to_string(), false)],
        rows
    );
}

#[test]
fn waiting_state_chrome_shows_only_the_status_line() {
    let mut state = scene::document::waiting_state(Some("diagram.edn".into()), 0);
    let chrome = commands::chrome(&state, 800.0, 600.0);
    assert!(
        chrome
            .iter()
            .all(|c| matches!(c, Command::Status { .. } | Command::Hint { .. }))
    );
    state.hover = Some(edn::parse("{:kind :dep :deps [{:from :a :to :b}]}").unwrap());
    state.pointer = Some((1.0, 1.0));
    let chrome = commands::chrome(&state, 800.0, 600.0);
    assert!(!chrome.iter().any(|c| matches!(c, Command::Popup { .. })));
}

#[test]
fn the_fit_rule_never_grows_past_the_nominal_size() {
    // Monospace estimate like the busted fake font: text_w/16*size.
    let measure = |size: f64, text: &str| scene::text_w(Some(text)) / 16.0 * size;
    assert_eq!(18.0, commands::fit(measure, 18.0, "短", 200.0, true, false));
    let shrunk = commands::fit(
        measure,
        18.0,
        "AVeryLongClassNameThatFillsTheBox",
        100.0,
        true,
        false,
    );
    assert!(shrunk < 18.0);
    // The shrunk text plus its bold offset fits the budget.
    let natural = measure(shrunk, "AVeryLongClassNameThatFillsTheBox") + shrunk * 0.05;
    assert!(natural <= 100.0 + 1e-9);
}

#[test]
fn the_fit_rule_holds_for_a_measure_that_snaps_to_the_pixel_grid() {
    // Real fonts round advances, so width is not linear in size; the fitted
    // size must still measure within the budget (else egui wraps the label).
    let measure = |size: f64, text: &str| (text.chars().count() as f64 * size * 0.61).ceil();
    let text = "Detail_view";
    let fitted = commands::fit(measure, 18.0, text, 76.0, false, true);
    assert!(fitted < 18.0);
    assert!(
        measure(fitted, text) + fitted * 0.2 <= 76.0 + 1e-9,
        "fitted {fitted} still measures {}",
        measure(fitted, text) + fitted * 0.2
    );
}

#[test]
fn the_canvas_chrome_has_no_declutter_control() {
    // #141 moved Declutter into the inspector sidebar (its labels are
    // checked by the inspector tests); the canvas keeps only banner,
    // return scope and popups.
    let doc =
        scene::ir::read_document("{:hierarchical true :classes [{:id :a.b} {:id :a.c}] :edges []}")
            .unwrap();
    let state = scene::document::new_state(doc, None, None, None, &[], None);
    let kinds: Vec<&str> = commands::chrome(&state, 1500.0, 920.0)
        .iter()
        .map(|c| c.kind())
        .collect();
    assert!(!kinds.contains(&"declutter"), "{kinds:?}");
}

#[test]
fn the_glyph_probe_covers_the_whole_ui_string_table() {
    let probe = viewer::strings::glyph_text();
    for text in [
        "搜索类名或命名空间",
        "没有匹配的类",
        "精简：箭头",
        "源码图",
        "«»αμσ←→—…",
        "重新生成 IR",
    ] {
        assert!(probe.contains(text), "glyph probe lacks {text}");
    }
}

// #154: edges.lua and class_boxes.lua read these as `if g(x,'k')` (or
// `not not`), so an authored `false`/`nil` draws like omission and `:yes`/`1`
// like `true`. The omitted and `true` outputs are the reference.

const FALSY: [&str; 2] = ["false", "nil"];
const TRUTHY: [&str; 2] = [":yes", "1"];

#[test]
fn edge_and_indicator_violating_is_a_lua_truthy_flag() {
    // edges.lua:32 `local violating=g(e,'violating')`; edges.lua:24/53 the
    // indicator's `violating` picks the red band.
    let stroke = |flag: &str| {
        let s = edn::parse(&format!(
            "{{:classes [] :edges [{{:from :a :to :b :strokes [[[0 0] [10 10]]] {flag}}}]}}"
        ))
        .unwrap();
        commands::edge_commands(&s, None, None)
    };
    let indicator = |flag: &str| {
        let s = edn::parse(&format!(
            "{{:diagram {{:hide-edges true}} :classes [] \
             :dep-indicators [{{:id :a :triangle [[0 0] [6 0] [3 5]] {flag}}}]}}"
        ))
        .unwrap();
        commands::edge_commands(&s, None, None)
    };
    type Draw<'a> = &'a dyn Fn(&str) -> Vec<EdgeCommand>;
    for draw in [&stroke as Draw, &indicator] {
        let omitted = draw("");
        let violating = draw(":violating true");
        assert_ne!(omitted, violating, "fixture must tell a violation apart");
        for v in FALSY {
            assert_eq!(omitted, draw(&format!(":violating {v}")), "{v}");
        }
        for v in TRUTHY {
            assert_eq!(violating, draw(&format!(":violating {v}")), "{v}");
        }
    }
}

#[test]
fn dependency_popup_rows_read_violating_as_a_lua_truthy_flag() {
    // class_boxes.lua:179 `violating=not not G.get(d,'violating')`.
    for (v, want) in [
        ("false", false),
        ("nil", false),
        ("true", true),
        (":yes", true),
        ("1", true),
    ] {
        let hover = format!("{{:kind :dep :id :a :deps [{{:from :a :to :b :violating {v}}}]}}");
        let state = scene::document::ViewState {
            scene: edn::parse("{:classes [] :packages [] :edges [] :dep-indicators []}").unwrap(),
            hover: Some(edn::parse(&hover).unwrap()),
            pointer: Some((100.0, 100.0)),
            ..Default::default()
        };
        let rows: Vec<bool> = commands::chrome(&state, 800.0, 600.0)
            .iter()
            .find_map(|c| match c {
                Command::Popup { rows, .. } => Some(rows.iter().map(|r| r.violating).collect()),
                _ => None,
            })
            .expect("popup");
        assert_eq!(vec![want], rows, "{v}");
    }
}

#[test]
fn proposal_banner_reads_the_diagram_proposal_as_a_lua_truthy_flag() {
    // class_boxes.lua:162 `if G.get(diagram,'proposal') then` banner.
    let banner = |flag: &str| {
        let state = scene::document::ViewState {
            scene: edn::parse(&format!(
                "{{:diagram {{:title \"T\" {flag}}} :classes [] :packages [] :edges []}}"
            ))
            .unwrap(),
            ..Default::default()
        };
        commands::chrome(&state, 800.0, 600.0)
            .iter()
            .any(|c| matches!(c, Command::ProposalBanner { .. }))
    };
    assert!(!banner(""));
    assert!(banner(":proposal true"));
    for v in FALSY {
        assert!(!banner(&format!(":proposal {v}")), "{v}");
    }
    for v in TRUTHY {
        assert!(banner(&format!(":proposal {v}")), "{v}");
    }
}

/// Every class name with the size it draws at under the layout's monospace
/// estimate (Sarasa's half-width Latin, 8px a char at 16px).
fn fitted_names(s: &Value) -> Vec<(String, f64)> {
    commands_of(s)
        .iter()
        .filter_map(|command| match command {
            Command::Text {
                role,
                text,
                size,
                max_w,
                bold,
                italic,
                ..
            } if role == "name" => Some((
                text.clone(),
                commands::fit(
                    |s, t| commands::estimate_w(t, s),
                    *size,
                    text,
                    *max_w,
                    *bold,
                    *italic,
                ),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_name_the_layout_sized_its_box_for_draws_at_the_canvas_size() {
    // The C/M dots and the α/I marker must not take the name row's width:
    // the layout sizes a box for its widest line and nothing else.
    let s = scene(
        "[{:id :a :name \"RenderCacheInvalidation\" :crap {:mu 2}} \
          {:id :b :name \"AbstractSceneProjection\" :stereotype :abstract} \
          {:id :c :name \"ItalicContainerName\" :contents [{:id :k :name \"K\"}]}]",
    );
    let doc = scene::ir::read_document(
        "{:hierarchical true :prefix \"x\" :order [a] \
          :classes [{:id :a :name \"A\" :ns \"x.a\"} {:id :eframe :name \"eframe\" :foreign true}] \
          :edges [{:from :a :to :eframe}]}",
    )
    .unwrap();
    let ovals = compile_view(&doc, &[], &[], &Value::Map(vec![]));
    let names = [fitted_names(&s), fitted_names(&ovals)].concat();
    assert!(names.iter().any(|(n, _)| n == "eframe"), "{names:?}");
    for (name, size) in names {
        assert_eq!(16.0, size, "{name}");
    }
}

#[test]
fn a_package_title_as_wide_as_its_slot_draws_at_the_canvas_size() {
    // A title whose characters fill the frame beside the C/M dots: its
    // italic shear overhangs into the dots' reserve, as a class name's does
    // into its padding.
    // se-fish's component_scripts frame: 184 wide, 17 characters.
    let s = edn::parse(
        "{:packages [{:id :p :label \"component_scripts\" :rect {:x 0 :y 0 :w 184 :h 100}}] \
          :classes [] :edges []}",
    )
    .unwrap();
    let mut packages = 0;
    for command in commands_of(&s) {
        if let Command::Package {
            rect, size, max_w, ..
        } = command
        {
            let title = "x".repeat(((rect.w - commands::PACKAGE_TITLE_RESERVE) / 8.0) as usize);
            let measure = |s: f64, t: &str| commands::estimate_w(t, s);
            assert_eq!(
                16.0,
                commands::fit(measure, size, &title, max_w, false, true)
            );
            packages += 1;
        }
    }
    assert_eq!(1, packages);
}

#[test]
fn a_focused_view_names_its_scope_once() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :title "T" :classes [{:id :a.one :name "One"} {:id :a.two :name "Two"}] :edges []}"#,
    )
    .unwrap();
    let state =
        scene::document::new_state(doc, None, Some(&[Value::keyword("a")]), None, &[], None);
    let kinds: Vec<&str> = commands::chrome(&state, 800.0, 600.0)
        .iter()
        .map(Command::kind)
        .collect();
    assert_eq!(kinds, ["back"]);
}

#[test]
fn a_foreign_oval_keeps_its_red_frame_without_a_no_data_label() {
    let s = edn::parse(
        r#"{:packages [] :classes [{:id :std :shape :oval :rect {:x 0 :y 0 :w 80 :h 40}}] :edges []}"#,
    )
    .unwrap();
    let commands = commands_of(&s);
    let Command::Class { missing, .. } = class_command(&commands, "std") else {
        unreachable!()
    };
    assert!(missing);
    assert!(of_kind(&commands, "no-data").is_empty());
}

fn hover_class_at(zoom: f64) -> Vec<Command> {
    let s = edn::parse(
        r#"{:packages [] :edges [] :classes [{:id :vs :name "viewer_session.rs" :rect {:x 0 :y 0 :w 40 :h 40}}]}"#,
    )
    .unwrap();
    let state = scene::document::ViewState {
        scene: s,
        zoom,
        hover: Some(edn::parse("{:kind :class :id :vs}").unwrap()),
        pointer: Some((100.0, 100.0)),
        ..Default::default()
    };
    commands::chrome(&state, 800.0, 600.0)
}

#[test]
fn an_overview_hover_names_the_class_its_box_is_too_small_to_show() {
    let popup = hover_class_at(0.3)
        .into_iter()
        .find_map(|c| match c {
            Command::Popup { rows, .. } => Some(rows),
            _ => None,
        })
        .expect("overview hover shows the class name");
    let texts: Vec<&str> = popup.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, ["viewer_session.rs"]);
    assert!(
        !hover_class_at(1.0)
            .iter()
            .any(|c| matches!(c, Command::Popup { .. })),
        "full detail draws the name in the box"
    );
}
