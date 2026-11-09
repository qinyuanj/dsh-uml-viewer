//! Inspector sidebar model (#141). Expected values are those of
//! `love/spec/inspector_spec.lua` and `display_spec.lua` (view units ==
//! egui points).

use edn::Value;
use edn::record::get;
use scene::document::{self, ViewState};
use viewer::inspector::{self, Choice};
use viewer::project_files;

fn load(text: &str) -> ViewState {
    document::new_state(
        scene::ir::read_document(text).unwrap(),
        None,
        None,
        None,
        &[],
        None,
    )
}

const PLAN: &str = r#"{:hierarchical true :prefix "demo" :classes [{:id :a :ns "demo.a"}] :proposals [{:id :plan :name "中文方案" :layers []}]}"#;

#[test]
fn lists_the_source_diagram_then_declared_proposals_with_the_active_one() {
    let state = load(PLAN);
    let entries = inspector::entries(&state);
    assert_eq!(entries.len(), 2);
    assert_eq!(
        (entries[0].label.as_str(), entries[0].active),
        ("源码图", true)
    );
    assert_eq!(entries[0].id, None);
    assert_eq!(
        (entries[1].label.as_str(), entries[1].active),
        ("中文方案", false)
    );
    let titled = load(r#"{:title "中文 Inspector" :hierarchical true :classes []}"#);
    assert_eq!(inspector::entries(&titled)[0].label, "中文 Inspector");
    assert_eq!(
        inspector::entries(&load(r#"{:hierarchical true :classes []}"#)).len(),
        1
    );
}

#[test]
fn choosing_switches_the_view_and_names_the_discussion_context() {
    let state = load(PLAN);
    let Choice {
        state: proposal,
        context,
    } = inspector::choose(&state, Some(&Value::keyword("plan")));
    assert_eq!(get(&proposal.extra, "proposal"), Some(&Value::Bool(true)));
    assert_eq!(
        context,
        edn::parse(r#"{:context :proposal :proposal-id :plan :name "中文方案"}"#).unwrap()
    );
    assert!(inspector::entries(&proposal)[1].active);
    let Choice {
        state: real,
        context,
    } = inspector::choose(&proposal, None);
    assert_eq!(get(&real.extra, "proposal"), Some(&Value::Bool(false)));
    assert_eq!(context, edn::parse("{:context :real}").unwrap());
}

#[test]
fn explains_the_empty_inspector_until_something_is_selected() {
    let mut state = load(PLAN);
    let text = inspector::empty_explanation(&state).unwrap();
    // The help says what a click does: a summary, then a card or a level.
    assert!(text.contains("单击类或命名空间查看摘要"), "{text}");
    assert!(text.contains("双击类打开详情卡片") && text.contains("双击命名空间进入下一层"));
    // The first list entry carries the diagram title, so no fixed name.
    assert!(!text.contains(viewer::strings::REAL_DIAGRAM), "{text}");
    state.selected = Some(edn::parse("{:kind :class :id :a}").unwrap());
    assert_eq!(inspector::empty_explanation(&state), None);
}

#[test]
fn names_the_current_declutter_mode_on_its_button() {
    let mut state = load(PLAN);
    for expect in [
        "精简：无",
        "精简：箭头",
        "移除箭头",
        "精简：元素",
        "精简：类",
        "精简：无",
    ] {
        assert_eq!(inspector::declutter_label(&state), expect);
        state = scene::events::cycle_declutter(&state);
    }
}

#[test]
fn sidebar_width_defaults_to_280_and_keeps_a_usable_canvas() {
    assert_eq!(inspector::clamp_width(None, 1500.0), 280.0);
    assert_eq!(inspector::clamp_width(Some(500.0), 1500.0), 500.0);
    assert_eq!(inspector::clamp_width(Some(40.0), 1500.0), 200.0);
    assert_eq!(inspector::clamp_width(Some(5000.0), 1500.0), 1260.0);
    assert_eq!(inspector::clamp_width(Some(900.0), 300.0), 200.0);
    assert_eq!(inspector::canvas_width(1500.0, 280.0), 1220.0);
    assert_eq!(inspector::canvas_width(100.0, 200.0), 0.0);
}

#[test]
fn the_left_edge_grip_drags_the_width() {
    // Window 1500, width 280: the edge is at x = 1220, grip ±4.
    assert!(inspector::on_edge(1220.0, 1500.0, 280.0));
    assert!(inspector::on_edge(1216.0, 1500.0, 280.0));
    assert!(!inspector::on_edge(1210.0, 1500.0, 280.0));
    assert_eq!(inspector::drag_width(1160.0, 1500.0), 340.0);
    assert_eq!(inspector::drag_width(1490.0, 1500.0), 200.0);
}

#[test]
fn card_wheel_scrolls_in_48_point_steps_clamped_to_the_content() {
    let mut state = load(PLAN);
    state.selected = Some(edn::parse("{:kind :class :id :a}").unwrap());
    let scrolled = inspector::scroll(&state, 1.0, 0.0);
    assert_eq!(scrolled.detail_scroll, 48.0);
    assert_eq!(inspector::scroll(&scrolled, -2.0, 0.0).detail_scroll, 0.0);
    assert_eq!(inspector::scroll(&state, 99.0, 0.0).detail_scroll, 72.0);
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("uml141-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn proposal_write_back_keeps_other_proposals_and_policy_content() {
    let root = temp_dir("policy");
    let ir = root.join("diagram.edn");
    let policy = root.join("diagram.policy.edn");
    std::fs::write(
        &ir,
        r#"{:title "T" :hierarchical true :classes [{:id :a}] :proposals [{:id :plan :name "中文方案" :layers []} {:id :keep :name "保留" :layers [{:name "L" :members [:a]}]}]}"#,
    )
    .unwrap();
    std::fs::write(
        &policy,
        r#"{:lang :rust :src "crates" :omit [:x] :proposals [{:id :plan :name "中文方案" :layers []} {:id :keep :name "保留" :layers [{:name "L" :members [:a]}]}]}"#,
    )
    .unwrap();
    let mut state = load(&std::fs::read_to_string(&ir).unwrap());
    state.path = Some(ir.to_string_lossy().into_owned());
    let renamed = scene::events::rename_proposal(
        &state,
        Some(&Value::keyword("plan")),
        "中文方案输入",
        Some(&project_files::Files),
    );
    // Proposals are written in their normalized form (as the viewer does):
    // added `:omit []` and layer ids are not losses.
    let written = edn::parse(&std::fs::read_to_string(&policy).unwrap()).unwrap();
    assert_eq!(get(&written, "lang"), Some(&Value::keyword("rust")));
    assert_eq!(get(&written, "src"), Some(&Value::String("crates".into())));
    assert_eq!(get(&written, "omit"), Some(&edn::parse("[:x]").unwrap()));
    let Some(Value::Vector(proposals)) = get(&written, "proposals") else {
        panic!("proposals lost")
    };
    let names: Vec<_> = proposals.iter().map(|p| get(p, "name").cloned()).collect();
    assert_eq!(
        names,
        [
            Some(Value::String("中文方案输入".into())),
            Some(Value::String("保留".into()))
        ]
    );
    let Some(Value::Vector(layers)) = get(&proposals[1], "layers") else {
        panic!("layers lost")
    };
    assert_eq!(get(&layers[0], "name"), Some(&Value::String("L".into())));
    assert_eq!(
        get(&layers[0], "members"),
        Some(&edn::parse("[:a]").unwrap())
    );
    assert_eq!(inspector::entries(&renamed)[1].label, "中文方案输入");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_diagram_list_is_recomputed_only_when_the_revision_changes() {
    // The sidebar asks for this every frame. Rebuilding it walks and clones
    // the whole document (measured ~10 ms on 2000 classes), so the GUI keeps
    // it until the state revision or a policy write moves.
    let mut doc = "{:hierarchical true :classes [".to_string();
    for n in 0..3000 {
        if n > 0 {
            doc.push(' ');
        }
        doc.push_str(&format!(
            "{{:id :c{n} :name \"C{n}\" :ops [{{:name \"go\"}}]}}"
        ));
    }
    doc.push_str("] :proposals [{:id :plan :name \"方案\" :layers []}]}");
    let mut state = load(&doc);
    let mut gui = viewer::gui::Gui::default();
    let (first, rebuilt) = gui.entries_for(&state);
    assert!(rebuilt);
    assert_eq!(first.len(), 2);
    assert_eq!(first[1].label, "方案");
    let (again, rebuilt) = gui.entries_for(&state);
    assert_eq!(again, first);
    assert!(!rebuilt, "a repeat call must reuse the cache");
    state.revision += 1;
    let (_, rebuilt) = gui.entries_for(&state);
    assert!(rebuilt, "a new revision must rebuild");
}
