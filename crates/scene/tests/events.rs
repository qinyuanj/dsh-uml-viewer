//! Navigation and proposal transitions — the events_spec,
//! canvas_navigation_spec and search_spec oracle.

use edn::record::{get, local_name};
use edn::{Value, parse};
use scene::document::{self, ViewState};
use scene::events::{self, Dims};

fn kw(s: &str) -> Value {
    Value::keyword(s)
}

fn tree() -> ViewState {
    document::new_state(
        scene::ir::read_document(
            r#"{:hierarchical true :title "Tree" :classes [{:id :source} {:id :source.lua} {:id :source.lua.parser} {:id :other}] :edges []}"#,
        )
        .unwrap(),
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    )
}

fn ids(s: &ViewState) -> Vec<String> {
    match get(&s.scene, "classes") {
        Some(Value::Vector(cs)) => cs
            .iter()
            .map(|c| {
                local_name(get(c, "id").unwrap())
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect(),
        _ => vec![],
    }
}

fn dims() -> Dims {
    Dims {
        window_w: 1500.0,
        window_h: 900.0,
        view_w: Some(1220.0),
        ..Dims::default()
    }
}

fn with(extra: impl FnOnce(&mut Dims)) -> Dims {
    let mut d = dims();
    extra(&mut d);
    d
}

#[test]
fn enters_nested_namespaces_and_returns_with_back_escape_and_hotspot() {
    let mut s = tree();
    s.cam_x = 10.0;
    s.cam_y = 20.0;
    s.zoom = 2.0;
    s.selected = Some(kw("old"));
    s.discussion_context = Some(parse("{:context :proposal :name \"Independent\"}").unwrap());
    let a = events::drill(&s, &kw("source"));
    let b = events::drill(&a, &kw("source.lua"));
    assert_eq!(vec!["source.lua", "source.lua.parser"], ids(&b));
    assert_eq!(vec![kw("source"), kw("lua")], b.focus);
    assert_eq!((0.0, 0.0, 2.0), (b.cam_x, b.cam_y, b.zoom));
    assert!(b.selected.is_none());
    assert_eq!(s.discussion_context, b.discussion_context);
    assert_eq!(
        a.focus,
        events::on_key(&b, Some(&Value::String("escape".into())), &dims()).focus
    );
    assert_eq!(Vec::<Value>::new(), events::on_press(&a, 20.0, 20.0).focus);
    assert_eq!(s.focus, events::back(&s).focus);
    assert_eq!(10.0, s.cam_x);
}

#[test]
fn source_tree_drill_ignores_same_named_nested_proposal_groups() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [{:id :client} {:id :client.packages} {:id :client.packages.inner} {:id :client.packages.inner.leaf}] :edges [] :proposals [{:id :split :name "Split" :layers [{:id :core :label "Core" :nses [{:id :client.packages :label "Packages" :nses [{:id :client.packages.inner :label "Inner" :nses [:client.packages.inner.leaf]}]}]}]}]}"#,
    )
    .unwrap();
    let s = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    let client = events::drill(&s, &kw("client"));
    let packages = events::drill(&client, &kw("client.packages"));
    assert_eq!(vec![kw("client"), kw("packages")], packages.focus);
    assert!(get(&packages.extra, "open-layer").is_none());

    let inner = events::drill(&packages, &kw("client.packages.inner"));
    assert_eq!(vec![kw("client"), kw("packages"), kw("inner")], inner.focus);
    assert!(get(&inner.extra, "open-layer").is_none());
    assert_eq!(packages.focus, events::back(&inner).focus);
    assert_eq!(client.focus, events::back(&packages).focus);
}

#[test]
fn source_tree_drill_ignores_all_same_named_proposal_groups() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [{:id :client} {:id :client.ScreenHandlers} {:id :client.packages} {:id :server} {:id :server.Mgr} {:id :server.Data} {:id :server.packages} {:id :common} {:id :common.cfg} {:id :common.packages}] :edges [] :proposals [{:id :split :name "Split" :layers [{:id :core :label "Core" :nses [{:id :client.ScreenHandlers :nses []} {:id :server.Mgr :nses []} {:id :server.Data :nses []} {:id :common.cfg :nses []} {:id :client.packages :nses []} {:id :common.packages :nses []} {:id :server.packages :nses []}]}]}]}"#,
    )
    .unwrap();
    let source = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    for id in [
        "client.ScreenHandlers",
        "server.Mgr",
        "server.Data",
        "common.cfg",
        "client.packages",
        "common.packages",
        "server.packages",
    ] {
        let mut state = source.clone();
        let mut path = Vec::new();
        let mut full = String::new();
        for segment in id.split('.') {
            if !full.is_empty() {
                full.push('.');
            }
            full.push_str(segment);
            path.push(kw(segment));
            state = events::drill(&state, &kw(&full));
        }
        assert_eq!(path, state.focus, "{id}");
        assert!(get(&state.extra, "open-layer").is_none(), "{id}");
    }
}

#[test]
fn pans_within_scene_bounds_including_negative_origins() {
    let mut s = tree();
    s.scene.assoc(
        Value::keyword("size"),
        parse("{:w 1400 :h 4000 :min-x -400 :min-y -50}").unwrap(),
    );
    let a = events::on_scroll(&s, 100.0, &with(|d| d.horizontal = true));
    assert_eq!(180.0, a.cam_x);
    let b = events::on_scroll(&a, -100.0, &with(|d| d.horizontal = true));
    assert_eq!(-400.0, b.cam_x);
    assert_eq!(
        96.0,
        events::on_key(&s, Some(&Value::String("down".into())), &dims()).cam_y
    );
    assert_eq!(-50.0, events::on_scroll(&s, -100.0, &dims()).cam_y);
    assert_eq!(0.0, events::on_scroll(&s, 0.0, &dims()).cam_y);
    // The wheel count is a multiplier (one notch = 48/zoom).
    assert_eq!(96.0, events::on_scroll(&s, 2.0, &dims()).cam_y);
}

#[test]
fn zooms_around_the_canvas_centre_and_normalizes_keys() {
    let s = tree();
    let control = with(|d| d.control = true);
    let a = events::on_key(&s, Some(&Value::String("+".into())), &control);
    assert!((a.zoom - 1.1).abs() < 1e-12);
    let (x, y) = events::world_xy(&a, 610.0, 450.0);
    assert!((x - 610.0).abs() < 1e-9 && (y - 450.0).abs() < 1e-9);
    let b = events::on_key(
        &a,
        Some(&Value::String("unknown".into())),
        &with(|d| {
            d.control = true;
            d.key_code = Some(45);
        }),
    );
    assert!((b.zoom - 1.0).abs() < 1e-12);
    assert_eq!(0.0, b.cam_x);
    assert!(
        (events::on_key(&a, Some(&Value::String("0".into())), &control).zoom - 1.0).abs() < 1e-12
    );
    assert_eq!(
        s.zoom,
        events::on_key(&s, Some(&Value::String("+".into())), &Dims::default()).zoom
    );
    assert_eq!(
        Some("out"),
        events::zoom_dir(
            Some("unknown"),
            &with(|d| d.raw_key = Some("\u{1f}".into()))
        )
    );
    let mut z = a;
    for _ in 0..100 {
        z = events::on_key(&z, Some(&Value::String("=".into())), &control);
    }
    assert_eq!(10.0, z.zoom);
    for _ in 0..100 {
        z = events::on_key(&z, Some(&Value::String("minus".into())), &control);
    }
    assert_eq!(0.1, z.zoom);
    let mut s2 = tree();
    s2.zoom = 2.0;
    s2.cam_x = 10.0;
    s2.cam_y = 20.0;
    assert_eq!((15.0, 30.0), events::world_xy(&s2, 10.0, 20.0));
}

#[test]
fn hits_hovers_and_selects_in_world_coordinates_without_touching_the_card() {
    let mut s = tree();
    s.zoom = 2.0;
    s.cam_x = 10.0;
    s.cam_y = 20.0;
    s.detail_id = Some(kw("kept"));
    let c = scene::hit::class_by_id(&s.scene, &kw("source"))
        .unwrap()
        .clone();
    let r = scene::Rect::from_value(get(&c, "rect").unwrap()).unwrap();
    let (x, y) = ((r.x + 5.0 - 10.0) * 2.0, (r.y + 5.0 - 20.0) * 2.0);
    let moved = events::on_move(&s, x, y);
    let pressed = events::on_press(&moved, x, y);
    assert_eq!(get(&c, "id"), get(pressed.selected.as_ref().unwrap(), "id"));
    assert_eq!(moved.hover, pressed.selected);
    assert_eq!(Some((x, y)), moved.pointer);
    assert!(
        events::on_press(&pressed, -10000.0, -10000.0)
            .selected
            .is_none()
    );
    assert_eq!(
        s.detail_id,
        events::on_key(&pressed, Some(&Value::String("escape".into())), &dims()).detail_id
    );
    assert_eq!(
        get(&c, "id"),
        events::layer_id(pressed.selected.as_ref()).as_ref()
    );
    assert!(events::layer_id(Some(&parse("{:kind :class :id :leaf}").unwrap())).is_none());
    assert_eq!(
        Some(kw("source")),
        events::layer_id(Some(
            &parse("{:kind :child :id :source.lua :parent :source}").unwrap()
        ))
    );
    assert_eq!(
        s.scene,
        events::on_key(&s, Some(&Value::String("x".into())), &dims()).scene
    );
    let mut w = tree();
    w.waiting = true;
    w.metrics_stamp = Some(Value::String("old".into()));
    let reload = events::on_key(&w, Some(&Value::String("r".into())), &dims());
    assert!(reload.mtime.is_none());
    assert!(reload.metrics_stamp.is_none());
    assert!(!reload.waiting);
}

#[test]
fn opens_existing_proposal_packages_and_nested_groups_then_returns() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [{:id :source.lua} {:id :other}] :edges [] :proposals [{:id :split :name "Split" :layers [{:id :core :label "Core" :nses [{:id :parser :label "Parser" :nses [:source.lua]}]}]}]}"#,
    )
    .unwrap();
    let mut s = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    s.discussion_context = Some(parse("{:context :real}").unwrap());
    let p = events::show_proposal(&s, Some(&kw("split")));
    assert_eq!(Some(&Value::Bool(true)), get(&p.extra, "proposal"));
    let layer = events::drill(&p, &kw("proposal.core"));
    assert_eq!(Some(&kw("proposal.core")), get(&layer.extra, "open-layer"));
    let group = events::drill(&p, &kw("parser"));
    assert_eq!(vec!["source.lua"], ids(&group));
    assert_eq!(0, group.focus.len());
    assert!(
        get(
            &events::on_key(&group, Some(&Value::String("esc".into())), &dims()).extra,
            "open-layer"
        )
        .is_none()
    );
    assert!(get(&events::on_press(&layer, 20.0, 20.0).extra, "open-layer").is_none());
    let real = events::show_proposal(&group, None);
    assert!(get(&real.extra, "proposal-id").is_none());
    assert_eq!(Some(&Value::Bool(false)), get(&real.extra, "proposal"));
    assert_eq!(s.document, real.document);
    assert_eq!(s.discussion_context, real.discussion_context);
    assert_eq!(
        s.document,
        events::show_proposal(&s, Some(&kw("absent"))).document
    );
    assert_eq!(
        "arrows",
        local_name(get(&events::cycle_declutter(&s).extra, "declutter").unwrap())
            .unwrap_or_default()
            .to_owned()
    );
    let mut c = s.clone();
    for _ in 0..5 {
        c = events::cycle_declutter(&c);
    }
    assert_eq!(
        "full",
        local_name(get(&c.extra, "declutter").unwrap())
            .unwrap_or_default()
            .to_owned()
    );
}

#[test]
fn legacy_default_proposal_opens_nested_groups_without_a_proposal_id() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [{:id :source.lua}] :edges [] :proposals [{:id :split :name "Split" :layers [{:id :core :label "Core" :nses [{:id :parser :label "Parser" :nses [:source.lua]}]}]}]}"#,
    )
    .unwrap();
    let mut s = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    s.extra.assoc(Value::keyword("proposal"), Value::Bool(true));
    assert!(get(&s.extra, "proposal-id").is_none());

    let group = events::drill(&s, &kw("parser"));
    assert_eq!(Some(&kw("parser")), get(&group.extra, "open-layer"));
    assert!(group.focus.is_empty());
    assert_eq!(vec!["source.lua"], ids(&group));
    assert!(get(&events::back(&group).extra, "open-layer").is_none());
}

#[test]
fn retains_navigation_and_discussion_state_across_a_valid_reload() {
    let mut s = events::drill(&tree(), &kw("source"));
    s = events::on_key(
        &s,
        Some(&Value::String("+".into())),
        &with(|d| d.control = true),
    );
    s.discussion_context = Some(parse("{:context :proposal}").unwrap());
    s.selected = Some(kw("source.lua"));
    s.detail_id = Some(kw("source.lua"));
    let text = r#"{:hierarchical true :title "Tree" :classes [{:id :source} {:id :source.lua} {:id :source.lua.parser} {:id :other}] :edges []}"#;
    let (n, changed, _) = document::reload(
        &s,
        text,
        Some(Value::String("two".into())),
        &[],
        Some(Value::String("metrics-two".into())),
    );
    assert!(changed);
    assert_eq!(s.cam_x, n.cam_x);
    assert_eq!(s.zoom, n.zoom);
    assert_eq!(s.discussion_context, n.discussion_context);
    assert_eq!(s.selected, n.selected);
    assert_eq!(s.detail_id, n.detail_id);
    assert_eq!(s.focus, n.focus);
    assert_eq!(ids(&s), ids(&n));
}

#[test]
fn fits_after_navigation_once_the_canvas_is_known_never_on_rebuilds() {
    let mut s = tree();
    s.zoom = 4.0;
    s.cam_x = 50.0;
    let entered = events::drill(&s, &kw("source"));
    assert!(entered.fit_pending);
    let settled = events::settle(&entered, &dims());
    assert!(!settled.fit_pending);
    let size = get(&entered.scene, "size").unwrap();
    let w = edn::record::num(get(size, "w").unwrap()).unwrap();
    let h = edn::record::num(get(size, "h").unwrap()).unwrap();
    let x0 = edn::record::num(get(size, "min-x").unwrap()).unwrap();
    let y0 = edn::record::num(get(size, "min-y").unwrap()).unwrap();
    let want = (1220.0 / (w - x0)).min(900.0 / (h - y0)).min(1.0);
    assert!((settled.zoom - want).abs() < 1e-12);
    assert!(events::back(&settled).fit_pending);
    assert!(events::show_proposal(&settled, None).fit_pending);
    let mut rebuilt = settled.clone();
    rebuilt.zoom = 2.0;
    rebuilt.cam_x = 11.0;
    rebuilt.selected = Some(parse("{:kind :class :id :source.lua}").unwrap());
    let cleared = events::cycle_declutter(&rebuilt);
    assert!(!cleared.fit_pending);
    assert_eq!(2.0, cleared.zoom);
    assert_eq!(11.0, cleared.cam_x);
    assert_eq!(rebuilt.selected, cleared.selected);
}

#[test]
fn zooms_with_control_wheel_around_the_cursor_keeping_the_world_point() {
    let mut s = tree();
    s.cam_x = 30.0;
    s.cam_y = 40.0;
    s.zoom = 1.0;
    let (wx, wy) = events::world_xy(&s, 400.0, 300.0);
    let a = events::on_wheel(
        &s,
        0.0,
        1.0,
        &with(|d| {
            d.control = true;
            d.pointer = Some((400.0, 300.0));
        }),
    );
    assert!((a.zoom - 1.1).abs() < 1e-12);
    let (ax, ay) = events::world_xy(&a, 400.0, 300.0);
    assert!((wx - ax).abs() < 1e-9 && (wy - ay).abs() < 1e-9);
    let b = events::on_wheel(
        &a,
        0.0,
        -1.0,
        &with(|d| {
            d.control = true;
            d.pointer = Some((400.0, 300.0));
        }),
    );
    assert!((b.zoom - 1.0).abs() < 1e-9);
    assert!((b.cam_x - 30.0).abs() < 1e-9);
    assert!((b.cam_y - 40.0).abs() < 1e-9);
    let mut z = a;
    for _ in 0..100 {
        z = events::on_wheel(
            &z,
            0.0,
            1.0,
            &with(|d| {
                d.control = true;
                d.pointer = Some((10.0, 10.0));
            }),
        );
    }
    assert_eq!(10.0, z.zoom);
    for _ in 0..100 {
        z = events::on_wheel(
            &z,
            0.0,
            -1.0,
            &with(|d| {
                d.control = true;
                d.pointer = Some((10.0, 10.0));
            }),
        );
    }
    assert_eq!(0.1, z.zoom);
}

#[test]
fn pans_with_the_plain_wheel_and_sideways_with_shift_or_a_horizontal_wheel() {
    let mut s = tree();
    s.scene
        .assoc(Value::keyword("size"), parse("{:w 4000 :h 4000}").unwrap());
    let down = events::on_wheel(&s, 0.0, -1.0, &dims());
    assert_eq!(48.0, down.cam_y);
    assert_eq!(0.0, down.cam_x);
    let side = events::on_wheel(&s, 0.0, -1.0, &with(|d| d.shift = true));
    assert_eq!(48.0, side.cam_x);
    assert_eq!(0.0, side.cam_y);
    let wheel = events::on_wheel(&s, -2.0, 0.0, &dims());
    assert_eq!(96.0, wheel.cam_x);
    s.zoom = 2.0;
    assert_eq!(24.0, events::on_wheel(&s, 0.0, -1.0, &dims()).cam_y);
}

#[test]
fn anchors_on_the_last_canvas_pointer_and_forgets_it_on_leave() {
    let s = events::on_move(&tree(), 200.0, 100.0);
    let (wx, wy) = events::world_xy(&s, 200.0, 100.0);
    let a = events::on_wheel(&s, 0.0, 1.0, &with(|d| d.control = true));
    let (x, y) = events::world_xy(&a, 200.0, 100.0);
    assert!((wx - x).abs() < 1e-9 && (wy - y).abs() < 1e-9);
    let left = events::on_leave(&events::on_move(&tree(), 80.0, 110.0));
    assert!(left.pointer.is_none());
    assert!(left.hover.is_none());
    let (cx, cy) = events::world_xy(&left, 610.0, 450.0);
    let b = events::on_wheel(&left, 0.0, 1.0, &with(|d| d.control = true));
    let (x, y) = events::world_xy(&b, 610.0, 450.0);
    assert!((cx - x).abs() < 1e-9 && (cy - y).abs() < 1e-9);
}

#[test]
fn fits_the_whole_diagram_left_of_the_sidebar_with_control_zero() {
    let mut s = tree();
    s.zoom = 3.0;
    s.cam_x = 999.0;
    s.cam_y = -5.0;
    s.scene.assoc(
        Value::keyword("size"),
        parse("{:w 2440 :h 900 :min-x 0 :min-y 0}").unwrap(),
    );
    let f = events::on_key(
        &s,
        Some(&Value::String("0".into())),
        &with(|d| d.control = true),
    );
    assert!((f.zoom - 0.5).abs() < 1e-12);
    // The centre sits mid-way below the chrome inset: 36 + 864/2.
    let (mx, my) = events::world_xy(&f, 610.0, 468.0);
    assert!((mx - 1220.0).abs() < 1e-9 && (my - 450.0).abs() < 1e-9);
    s.scene.assoc(
        Value::keyword("size"),
        parse("{:w 300 :h 200 :min-x -100 :min-y 0}").unwrap(),
    );
    let small = events::fit(&s, &dims());
    assert!((small.zoom - 1.0).abs() < 1e-12);
    let (sx, sy) = events::world_xy(&small, 610.0, 468.0);
    assert!((sx - 100.0).abs() < 1e-9 && (sy - 100.0).abs() < 1e-9);
}

#[test]
fn pans_by_dragging_blank_canvas_and_clears_selection_on_a_blank_click() {
    let mut s = tree();
    s.zoom = 2.0;
    s.cam_x = 10.0;
    s.cam_y = 20.0;
    s.selected = Some(parse("{:kind :class :id :source}").unwrap());
    let a = events::on_left_press(&s, 1000.0, 800.0);
    assert!(a.drag.is_some());
    let b = events::on_drag(&a, 1001.0, 801.0);
    assert_eq!(10.0, b.cam_x);
    let c = events::on_drag(&b, 960.0, 700.0);
    assert_eq!(30.0, c.cam_x);
    assert_eq!(70.0, c.cam_y);
    let d = events::on_release(&c, 960.0, 700.0);
    assert!(d.drag.is_none());
    assert_eq!(Some(&kw("source")), get(d.selected.as_ref().unwrap(), "id"));
    assert_eq!(30.0, d.cam_x);
    let click = events::on_release(&events::on_left_press(&s, 1000.0, 800.0), 1000.0, 800.0);
    assert!(click.selected.is_none());
    assert_eq!(10.0, click.cam_x);
}

#[test]
fn a_drag_starting_on_a_class_does_nothing_but_the_press_selects() {
    let s = tree();
    let c = scene::hit::class_by_id(&s.scene, &kw("source"))
        .unwrap()
        .clone();
    let r = scene::Rect::from_value(get(&c, "rect").unwrap()).unwrap();
    let z = events::zoom_of(&s);
    let (x, y) = ((r.x + 5.0 - s.cam_x) * z, (r.y + 5.0 - s.cam_y) * z);
    let a = events::on_left_press(&s, x, y);
    assert!(a.drag.is_none());
    assert_eq!(Some(&kw("source")), get(a.selected.as_ref().unwrap(), "id"));
    let b = events::on_release(
        &events::on_drag(&a, x + 200.0, y + 200.0),
        x + 200.0,
        y + 200.0,
    );
    assert_eq!(0.0, b.cam_x);
    assert_eq!(0.0, b.cam_y);
    assert_eq!(Some(&kw("source")), get(b.selected.as_ref().unwrap(), "id"));
}

#[test]
fn a_right_press_selects_what_it_hits_without_arming_a_drag() {
    let mut s = tree();
    s.selected = Some(parse("{:kind :class :id :other}").unwrap());
    let c = scene::hit::class_by_id(&s.scene, &kw("source"))
        .unwrap()
        .clone();
    let r = scene::Rect::from_value(get(&c, "rect").unwrap()).unwrap();
    let z = events::zoom_of(&s);
    let (x, y) = ((r.x + 5.0 - s.cam_x) * z, (r.y + 5.0 - s.cam_y) * z);
    let a = events::on_secondary_press(&s, x, y);
    assert!(a.drag.is_none());
    assert_eq!(Some(&kw("source")), get(a.selected.as_ref().unwrap(), "id"));
    assert!(events::element_menu(a.selected.as_ref()));
    let blank = events::on_secondary_press(&s, 1000.0, 800.0);
    assert!(blank.drag.is_none());
    assert!(blank.selected.is_none());
    assert!(!events::element_menu(blank.selected.as_ref()));
}

#[test]
fn numeric_and_absent_keys_are_ignored_and_keyword_aliases_work() {
    let s = tree();
    assert_eq!(None, events::zoom_dir(Some("45"), &Dims::default()));
    assert_eq!(None, events::zoom_dir(None, &Dims::default()));
    assert_eq!(s.scene, events::on_key(&s, None, &Dims::default()).scene);
    assert_eq!(
        Some("out"),
        events::zoom_dir(Some("minus"), &Dims::default())
    );
    assert_eq!(Some("in"), events::zoom_dir(Some("+"), &Dims::default()));
    assert_eq!(Some("reset"), events::zoom_dir(Some("0"), &Dims::default()));
}

#[test]
fn search_matches_chinese_names_and_full_namespaces_case_insensitively() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [{:id :alpha.deep.parser :name "解析器" :ns "love.alpha.deep.parser"} {:id :beta.parser :name "解析器" :ns "love.beta.parser"}] :edges [{:from :alpha.deep.parser :to :beta.parser :kind :dependency}]}"#,
    )
    .unwrap();
    let results = scene::search::search(&doc, "解析");
    assert_eq!(2, results.len());
    assert_eq!("love.alpha.deep.parser", results[0].ns);
    assert_eq!("love.beta.parser", results[1].ns);
    assert_eq!(1, scene::search::search(&doc, "LOVE.ALPHA.DEEP").len());
    assert_eq!(0, scene::search::search(&doc, "").len());
    assert_eq!(0, scene::search::search(&doc, "   ").len());
}

#[test]
fn locate_enters_the_namespace_selects_opens_the_card_and_centres() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [{:id :alpha.deep.parser :name "解析器" :ns "love.alpha.deep.parser"} {:id :beta.parser :name "解析器" :ns "love.beta.parser"}] :edges [{:from :alpha.deep.parser :to :beta.parser :kind :dependency}]}"#,
    )
    .unwrap();
    let mut s = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    s.discussion_context = Some(parse("{:context :proposal :name \"Keep\"}").unwrap());
    let a = events::locate(
        &s,
        &kw("alpha.deep.parser"),
        &Dims {
            view_w: Some(800.0),
            window_h: 600.0,
            ..Dims::default()
        },
    );
    assert_eq!(vec![kw("alpha"), kw("deep")], a.focus);
    assert_eq!(
        Some(&kw("alpha.deep.parser")),
        get(a.selected.as_ref().unwrap(), "id")
    );
    assert_eq!(Some(kw("alpha.deep.parser")), a.detail_id);
    assert_eq!(s.discussion_context, a.discussion_context);
    assert_eq!(s.document, a.document);
    let c = scene::hit::class_by_id(&a.scene, a.detail_id.as_ref().unwrap())
        .unwrap()
        .clone();
    let r = scene::Rect::from_value(get(&c, "rect").unwrap()).unwrap();
    let (x, y) = events::world_xy(&a, 400.0, 300.0);
    assert_eq!(r.cx(), x);
    assert_eq!(r.cy(), y);
    assert_eq!(a.scene, events::locate(&a, &kw("gone"), &dims()).scene);
}

#[test]
fn search_results_refresh_on_document_replacement_and_stale_targets_are_rejected() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [{:id :alpha.deep.parser :name "解析器" :ns "love.alpha.deep.parser"}] :edges []}"#,
    )
    .unwrap();
    let s = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    let id = scene::search::search(s.document.as_ref().unwrap(), "alpha")[0]
        .id
        .clone();
    let (n, _, _) = document::reload(
        &s,
        r#"{:hierarchical true :classes [{:id :replacement :name "替代"}] :edges []}"#,
        Some(Value::String("two".into())),
        &[],
        None,
    );
    assert_eq!(
        0,
        scene::search::search(n.document.as_ref().unwrap(), "alpha").len()
    );
    assert_eq!(n.scene, events::locate(&n, &id, &dims()).scene);
    assert_eq!(
        1,
        scene::search::search(n.document.as_ref().unwrap(), "替代").len()
    );
}

#[test]
fn fit_keeps_the_scene_below_the_chrome_line() {
    let mut s = tree();
    s.scene.assoc(
        Value::keyword("size"),
        parse("{:w 2440 :h 1800 :min-x 0 :min-y 0}").unwrap(),
    );
    let f = events::fit(&s, &dims());
    // The scene top sits at or below the screen inset the scope line uses.
    let (_, top) = events::world_xy(&f, 0.0, events::CHROME_INSET);
    assert!(top <= 1e-9, "scene top drawn above the chrome inset: {top}");
    let (_, bottom) = events::world_xy(&f, 0.0, 900.0);
    assert!(bottom >= 1800.0 - 1e-9, "scene bottom cut off: {bottom}");
}
