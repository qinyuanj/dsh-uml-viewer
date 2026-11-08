//! Class card model (#141). Expected values are those of
//! `love/spec/detail_spec.lua` and `detail_view_spec.lua`.

use crate::common;

use common::{composite, contrast};
use edn::Value;
use edn::record::{get, num};
use viewer::detail::{self, Hover, Model, Row};
use viewer::theme;

const TINY: &str = r#"{:title "Tiny" :packages [{:id :p :label "Domain" :classes [{:id :a :name "A" :ns "demo.a" :coverage 0.9 :crap 1.2 :ops [{:name "go" :args ["x"] :returns "void" :coverage 0.75 :cc 2 :crap 1.8 :killed 3 :survived 1 :uncovered 2 :sites 6} {:name "hide" :private true :crap 4.0} {:name "bare"} {:name "low" :coverage 0.3 :killed 4 :survived 0 :uncovered 0 :sites 4}]} {:id :b :name "B"}]}] :edges [{:from :a :to :b :kind :dependency}]}"#;

/// The compiled view of a package document: its one section prefixes the
/// ids with `d0-`.
fn scene_of(text: &str) -> Value {
    let doc = scene::ir::read_document(text).unwrap();
    scene::document::compile_view(&doc, &[], &[], &Value::Map(vec![]))
}

fn kw(s: &str) -> Value {
    Value::keyword(s)
}

fn tiny_rows() -> (Model, Vec<Row>) {
    let model = detail::model(&scene_of(TINY), &kw("d0-a")).unwrap();
    let rows = detail::rows(&model);
    (model, rows)
}

fn class_model(class: &str) -> Model {
    Model::of(edn::parse(class).unwrap())
}

fn texts(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|r| r.text.as_str()).collect()
}

fn op<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.op_name.as_deref() == Some(name))
        .unwrap()
}

#[test]
fn reserves_a_20px_title_row_followed_by_18px_rows() {
    let rows = detail::rows(&class_model(
        r#"{:name "中文类名" :level 2 :stereotype :bean}"#,
    ));
    assert_eq!((rows[0].y, rows[0].h), (0.0, 20.0));
    assert_eq!((rows[1].y, rows[1].h), (20.0, 18.0));
    assert_eq!(rows[2].y, 38.0);
}

#[test]
fn lays_out_six_metric_columns_and_two_group_spans() {
    let cols = detail::column_layout(None);
    let labels: Vec<&str> = cols.iter().map(|c| c.label).collect();
    assert_eq!(
        labels,
        ["Crap", "CC", "Cov", "killed", "survived", "uncovered"]
    );
    assert_eq!(cols[0].left, 252.0);
    assert_eq!(cols[5].right, 624.0);
    let groups = detail::group_layout(None);
    assert_eq!((groups[0].left, groups[0].right), (252.0, 404.0));
    assert_eq!((groups[1].left, groups[1].right), (414.0, 624.0));
    assert_eq!(
        (groups[0].label, groups[1].label),
        ("--crap--", "--mutation--")
    );
}

#[test]
fn preserves_identity_mutation_and_source_label_rules() {
    assert_eq!(detail::content_h(&[]), 32.0);
    let solo = detail::rows(&class_model(r#"{:name "Solo" :package :orphan}"#));
    assert_eq!(texts(&solo), ["Solo", "包 orphan"]);
    for (file, ns, shown, drill) in [
        ("src/lib.rs", "demo.rust", "lib.rs", false),
        ("src/PROTO.RS", "demo.rust", "PROTO.RS", false),
        ("src/lib.rs", "demo.rust", "demo.rust", true),
        ("src/model.ts", "demo.model", "demo.model", false),
    ] {
        let mut m = class_model(&format!(r#"{{:name "Module" :drill? {drill}}}"#));
        m.file = Some(file.into());
        m.ns = Some(ns.into());
        let rows = detail::rows(&m);
        assert_eq!(rows.iter().find(|r| r.module).unwrap().text, shown);
    }
    let mut m = class_model(
        r#"{:name "D" :level 2 :stereotype :bean :crap {:mu 14.2 :max 134.6 :sigma 34.9} :cc 65 :killed 5 :survived 1 :uncovered 0 :sites 0 :fields [{:text "n"}] :ops [{:name "go" :text "go()" :killed 0 :survived 0 :uncovered 0 :sites 4}]}"#,
    );
    m.rels = vec![detail::Rel {
        id: kw("b"),
        name: "B".into(),
        label: Some("db".into()),
        phrase: "依赖".into(),
    }];
    let rows = detail::rows(&m);
    for t in ["层级 2", "«bean»", "n", "依赖  B  «db»"] {
        assert!(texts(&rows).contains(&t), "{t}");
    }
    let crap = rows.iter().find(|r| r.kind == "crap").unwrap();
    assert_eq!(crap.text, "Crap μ 14.2 max 134.6 σ 34.9");
    let cls = rows
        .iter()
        .find(|r| r.cells.crap_s.as_deref() == Some("14.2μ"))
        .unwrap();
    assert_eq!(cls.cells.cc_s, None);
    assert_eq!(cls.cells.mut_note, None);
    assert_eq!(cls.cells.killed_s.as_deref(), Some("5"));
    let go = op(&rows, "go");
    assert_eq!(go.cells.killed_s.as_deref(), Some("0"));
    assert_eq!(detail::content_h(&rows), rows.last().unwrap().y + 34.0);
    assert_eq!(detail::rel_at(&rows, 16.0), None);
    assert_eq!(detail::member_at(&rows, go.y + go.h), None);
    assert!(!detail::module_at(&rows, 16.0));
    let only = detail::rows(&class_model(r#"{:name "Only uncovered" :uncovered 4}"#));
    assert!(only.iter().all(|r| r.kind != "stats"));
    for metric in ["coverage", "cc", "killed", "survived"] {
        let zero = detail::rows(&class_model(&format!(r#"{{:name "Zero" :{metric} 0}}"#)));
        assert!(zero.iter().any(|r| r.kind == "stats"), "{metric}");
    }
}

#[test]
fn describes_every_relationship_kind_in_both_directions() {
    for (kind, out, inc) in [
        ("inheritance", "继承", "被继承"),
        ("implements", "实现", "被实现"),
        ("association", "关联", "被关联"),
        ("dependency", "依赖", "被依赖"),
        ("aggregation", "聚合", "被聚合"),
        ("composition", "组合", "被组合"),
        ("other", "指向", "来自"),
    ] {
        let s = edn::parse(&format!(
            r#"{{:classes [{{:id :a :name "A"}} {{:id :b :name "B"}}] :edges [{{:from :a :to :b :kind :{kind}}}]}}"#
        ))
        .unwrap();
        assert_eq!(detail::model(&s, &kw("a")).unwrap().rels[0].phrase, out);
        assert_eq!(detail::model(&s, &kw("b")).unwrap().rels[0].phrase, inc);
    }
}

#[test]
fn retains_aggregate_and_leaf_relationships_across_root_and_focus() {
    let doc = scene::ir::read_document(
        "{:hierarchical true :classes [{:id :alpha.deep.parser} {:id :beta.parser}] :edges [{:from :alpha.deep.parser :to :beta.parser :kind :dependency}]}",
    )
    .unwrap();
    let mut state = scene::document::new_state(doc.clone(), None, None, None, &[], None);
    let root = detail::for_state(&state, &kw("alpha")).unwrap();
    assert_eq!(root.rels.len(), 1);
    assert_eq!(root.rels[0].id, kw("beta"));
    let leaf = detail::for_state(&state, &kw("alpha.deep.parser")).unwrap();
    assert_eq!(leaf.rels.len(), 1);
    assert_eq!(leaf.rels[0].id, kw("beta.parser"));
    state.focus = vec![kw("alpha")];
    state.scene = scene::document::compile_view(&doc, &[], &state.focus, &Value::Map(vec![]));
    assert!(detail::for_state(&state, &kw("alpha.deep")).is_some());
    assert_eq!(
        detail::for_state(&state, &kw("alpha")).unwrap().rels.len(),
        1
    );
    assert_eq!(
        detail::for_state(&state, &kw("alpha.deep.parser"))
            .unwrap()
            .rels
            .len(),
        1
    );
}

#[test]
fn clamps_card_scrolling_to_content_and_viewport() {
    let (_, rows) = tiny_rows();
    let content = detail::content_h(&rows);
    assert_eq!(detail::max_scroll(&rows, content), 0.0);
    assert_eq!(detail::max_scroll(&rows, 0.0), content);
    assert_eq!(detail::scroll(&rows, 100.0, 0.0, -1.0), 0.0);
    assert_eq!(detail::scroll(&rows, 100.0, 0.0, 1.0), detail::SCROLL_STEP);
    assert_eq!(detail::scroll(&rows, 100.0, 48.0, 1.0), 96.0);
    assert_eq!(detail::scroll(&rows, 100.0, 0.0, 9999.0), content - 100.0);
    assert_eq!(detail::scroll(&rows, 100.0, content - 100.0, -9999.0), 0.0);
}

#[test]
fn highlights_only_the_hovered_operation_relationship_or_module_row() {
    let (_, rows) = tiny_rows();
    let hl = |h: Hover| detail::highlight(&rows, Some(&h)).map(|r| r.y);
    assert_eq!(hl(Hover::Op("go".into())), Some(op(&rows, "go").y));
    assert_eq!(hl(Hover::Op("A".into())), None);
    assert_eq!(hl(Hover::Op("absent".into())), None);
    assert_eq!(detail::highlight(&rows, None).map(|r| r.y), None);
    assert_eq!(
        hl(Hover::Module),
        Some(rows.iter().find(|r| r.module).unwrap().y)
    );
    let rel = rows.iter().find(|r| r.kind == "rel").unwrap();
    assert_eq!(hl(Hover::Rel(rel.id.clone().unwrap())), Some(rel.y));

    let marker = detail::highlight_marker(&rows, Some(&Hover::Op("go".into()))).unwrap();
    assert_eq!(255, theme::rgba(marker.bar).a);
    assert!(theme::rgba(marker.wash).a < 128);
    let background = composite(theme::POPUP_BG, viewer::color::color(marker.wash));
    assert!(contrast(theme::INK, background) >= 4.5);
    assert!(contrast(theme::rgb(marker.bar), theme::POPUP_BG) >= 3.0);
    assert_eq!(detail::highlight_marker(&rows, None), None);
}

#[test]
fn shows_class_and_member_quality_source_identity_and_relationships() {
    let (model, rows) = tiny_rows();
    assert_eq!(model.ns.as_deref(), Some("demo.a"));
    assert_eq!(rows[0].kind, "name");
    assert_eq!(rows[0].text, "A");
    let cls = rows
        .iter()
        .find(|r| r.cells.crap_s.as_deref() == Some("1.2μ"))
        .unwrap();
    assert_eq!(cls.cells.cov_s.as_deref(), Some("90%"));
    assert_eq!(cls.cells.cc_s, None);
    assert_eq!(
        cls.cells.killed_s.as_deref(),
        Some("7"),
        "class sums its ops' killed"
    );
    let go = op(&rows, "go");
    assert_eq!(go.text, "+ go(x) : void");
    assert_eq!(go.cells.cov_s.as_deref(), Some("75%"));
    assert_eq!(go.cells.cc_s.as_deref(), Some("2"));
    assert_eq!(go.cells.crap_s.as_deref(), Some("1.8"));
    assert_eq!(detail::member_at(&rows, go.y + 1.0).as_deref(), Some("go"));
    let hide = op(&rows, "hide");
    assert_eq!(hide.text, "- hide");
    assert!(hide.private);
    assert_eq!(
        hide.cells.mut_note.as_deref(),
        Some("---无 mutation 位点---")
    );
    let rel = rows.iter().find(|r| r.kind == "rel").unwrap();
    assert_eq!(rel.text, "依赖  B");
    assert_eq!(detail::rel_at(&rows, rel.y + 1.0), Some(kw("d0-b")));
    let module = rows.iter().find(|r| r.module).unwrap();
    assert!(detail::module_at(&rows, module.y));
    assert!(!detail::module_at(&rows, go.y));
    let ident = detail::member_ident(&model, Some("go"));
    assert_eq!(
        (ident.ns.as_deref(), ident.name.as_deref()),
        (Some("demo.a"), Some("go"))
    );
    assert_eq!(detail::member_ident(&model, Some("")).name, None);
    assert!(detail::model(&scene_of(TINY), &kw("absent")).is_none());
}

fn view(width: Option<f64>) -> Vec<detail::Item> {
    let (_, rows) = tiny_rows();
    detail::view(&rows, width)
}

fn item<'a>(items: &'a [detail::Item], name: &str) -> &'a detail::Item {
    items
        .iter()
        .find(|it| it.op_name.as_deref() == Some(name))
        .unwrap()
}

fn cell<'a>(it: &'a detail::Item, id: &str) -> Option<&'a detail::Cell> {
    it.cells.iter().find(|c| c.id == id)
}

#[test]
fn view_keeps_title_and_rows_at_fixed_right_aligned_columns() {
    let items = view(None);
    assert_eq!((items[0].h, items[0].title_font), (20.0, true));
    let go = item(&items, "go");
    assert_eq!((go.h, go.title_font), (18.0, false));
    for col in detail::column_layout(None) {
        if let Some(c) = cell(go, col.id) {
            assert_eq!((c.right, c.center), (col.right, false));
        }
    }
    let header = items.iter().find(|it| it.kind == "col-header").unwrap();
    assert_eq!(header.text.as_deref(), Some("成员"));
    assert_eq!(header.cells.len(), 6);
    assert!(
        header
            .cells
            .iter()
            .all(|c| theme::rgb(c.color) == theme::GOLD && c.tip.is_some())
    );
}

#[test]
fn view_uses_available_width_with_a_scrollable_minimum() {
    for (width, right) in [
        (300.0, 560.0),
        (576.0, 576.0),
        (624.0, 624.0),
        (900.0, 900.0),
    ] {
        let items = view(Some(width));
        let go = item(&items, "go");
        assert_eq!(cell(go, "uncovered").unwrap().right, right);
        assert!(go.clip_right.unwrap() >= 130.0);
    }
}

#[test]
fn view_colors_cells_with_the_semantic_rules() {
    let items = view(None);
    let go = item(&items, "go");
    assert_eq!(
        theme::rgb(cell(go, "crap").unwrap().color),
        theme::stroke(Some(
            scene::metric_rating(Some(1.8), None, None, None).crap_grade,
        ))
    );
    assert_eq!(theme::rgb(cell(go, "cc").unwrap().color), theme::MUTED);
    assert_eq!(theme::rgb(cell(go, "cov").unwrap().color), theme::GOLD);
    assert_eq!(theme::rgb(cell(go, "killed").unwrap().color), theme::WHITE);
    assert_eq!(
        theme::rgb(cell(go, "survived").unwrap().color),
        theme::VIOLATION
    );
    assert_eq!(
        theme::rgb(cell(go, "uncovered").unwrap().color),
        theme::VIOLATION
    );
    let low = item(&items, "low");
    assert_eq!(theme::rgb(cell(low, "cov").unwrap().color), theme::WARN);
    assert_eq!(
        theme::rgb(cell(low, "survived").unwrap().color),
        theme::GOOD
    );
    assert_eq!(
        theme::rgb(items.iter().find(|it| it.kind == "crap").unwrap().color),
        theme::GOLD
    );
    assert_eq!(theme::rgb(item(&items, "hide").color), theme::MUTED);
    assert_eq!(theme::rgb(go.color), theme::INK);
    let note = cell(item(&items, "hide"), "mut-note").unwrap();
    let groups = detail::group_layout(None);
    assert_eq!(
        (note.left, note.right, note.center),
        (groups[1].left, groups[1].right, true)
    );
    assert!(cell(item(&items, "hide"), "killed").is_none());
}

#[test]
fn view_stripes_stats_rows_draws_bars_swatches_and_marks_clickable_rows() {
    let items = view(None);
    let stats: Vec<_> = items.iter().filter(|it| it.kind == "stats").collect();
    for (i, it) in stats.iter().enumerate() {
        assert_eq!(it.stripe, (i + 1) % 2 == 0);
    }
    let go = item(&items, "go");
    let bar = |id: &str| go.bars.iter().find(|b| b.id == id).unwrap();
    assert_eq!(
        (bar("cov").fraction, theme::rgb(bar("cov").color)),
        (0.75, theme::GOLD)
    );
    assert_eq!(bar("kill").fraction, 0.5);
    assert!(item(&items, "bare").bars.is_empty());
    assert_eq!(
        item(&items, "bare").swatch.map(theme::rgb),
        Some(theme::stroke(Some(1.0)))
    );
    assert_eq!(items[0].swatch, None);
    assert!(items.iter().find(|it| it.module).unwrap().clickable);
    assert!(go.clickable);
    assert!(items.iter().find(|it| it.kind == "rel").unwrap().clickable);
    assert!(!items[0].clickable);
}

#[test]
fn heads_the_card_with_one_meta_line() {
    let mut m = class_model(r#"{:name "app.rs" :level 2}"#);
    m.package = Some(edn::parse(r#"{:id :viewer :label "viewer"}"#).unwrap());
    let rows = detail::rows(&m);
    assert_eq!(texts(&rows), ["app.rs", "层级 2 · 包 viewer"]);
}

#[test]
fn a_namespace_box_reads_as_the_namespace_not_its_root_module() {
    // A crate root module shares its namespace's id. The root view draws
    // the namespace box, so the inspector names that box and its totals.
    let doc = scene::ir::read_document(
        "{:hierarchical true :classes [{:id :alpha :name \"lib.rs\" :ns \"alpha\"} {:id :alpha.deep :name \"deep.rs\" :ns \"alpha.deep\"}] :edges []}",
    )
    .unwrap();
    let state = scene::document::new_state(doc, None, None, None, &[], None);
    let model = detail::for_state(&state, &kw("alpha")).unwrap();
    assert_eq!(
        get(&model.class, "name"),
        Some(&Value::String("Alpha".into()))
    );
    let n = get(&model.class, "crap")
        .and_then(|c| get(c, "n"))
        .and_then(num);
    assert_eq!(n, Some(2.0), "totals over both modules");
}
