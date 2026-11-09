//! compile_view, reload/retention and proposal write-back — the
//! proposal_spec.lua and viewer_spec.lua oracle. Expected file contents are
//! checked structurally against literals from the spec.

use edn::record::get;
use edn::{Value, parse};
use scene::Snapshot;
use scene::document::{self, Boundary, MemoryBoundary, ViewState};

fn kw(s: &str) -> Value {
    Value::keyword(s)
}

fn seq<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    match get(v, key) {
        Some(Value::Vector(items)) => items,
        _ => &[],
    }
}

fn proposal_named<'a>(doc: &'a Value, name_want: &str) -> Option<&'a Value> {
    seq(doc, "proposals")
        .iter()
        .find(|p| matches!(get(p, "name"), Some(Value::String(s)) if s == name_want))
}

fn load(text: &str) -> ViewState {
    document::new_state(
        scene::ir::read_document(text).unwrap(),
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    )
}

#[test]
fn reload_retains_the_last_valid_scene_on_invalid_edits() {
    let first = r#"{:packages [{:label "P" :classes [{:id :a :ops ["public" {:text "secret" :private true}]}]}]}"#;
    let state = load(first);
    let (unchanged, changed, _) = document::reload(&state, first, state.mtime.clone(), &[], None);
    assert!(!changed);
    assert_eq!(state.scene, unchanged.scene);
    let changed_text = first.replace("public", "long public method");
    let (next, reloaded, _) = document::reload(
        &state,
        &changed_text,
        Some(Value::String("two".into())),
        &[],
        None,
    );
    assert!(reloaded);
    assert_eq!(2, next.revision);
    let (retained, accepted, err) =
        document::reload(&next, "{", Some(Value::String("three".into())), &[], None);
    assert!(!accepted);
    assert_eq!(next.scene, retained.scene);
    assert!(err.is_some());
}

#[test]
fn reload_validates_retained_focus_and_card_targets() {
    let first = r#"{:hierarchical true :classes [{:id :alpha.deep.parser} {:id :alpha.other} {:id :beta.parser}] :edges []}"#;
    let doc = scene::ir::read_document(first).unwrap();
    let mut state = document::new_state(
        doc,
        Some(Value::String("one".into())),
        Some(&[kw("alpha"), kw("deep")]),
        None,
        &[],
        None,
    );
    state.cam_x = 71.0;
    state.cam_y = -23.0;
    state.zoom = 1.7;
    state.selected = Some(parse("{:kind :class :id :alpha.deep.parser}").unwrap());
    state.detail_id = Some(kw("alpha.deep.parser"));
    state.discussion_context = Some(parse("{:context :proposal :proposal-id :discussed}").unwrap());
    let (same, changed, _) = document::reload(
        &state,
        first,
        Some(Value::String("one".into())),
        &[],
        Some(Value::String("metrics-two".into())),
    );
    assert!(changed);
    assert_eq!(state.focus, same.focus);
    assert_eq!(state.selected, same.selected);
    assert_eq!(state.detail_id, same.detail_id);
    let reduced = first.replace("{:id :alpha.deep.parser} ", "");
    let (ancestor, reloaded, _) = document::reload(
        &same,
        &reduced,
        Some(Value::String("two".into())),
        &[],
        Some(Value::String("metrics-three".into())),
    );
    assert!(reloaded);
    assert_eq!(vec![kw("alpha")], ancestor.focus);
    assert!(ancestor.selected.is_none());
    assert!(ancestor.detail_id.is_none());
    let reduced2 = reduced.replace("{:id :alpha.other} ", "");
    let (root, _, _) = document::reload(
        &ancestor,
        &reduced2,
        Some(Value::String("three".into())),
        &[],
        Some(Value::String("metrics-four".into())),
    );
    assert_eq!(0, root.focus.len());
    assert_eq!(1, seq(&root.scene, "classes").len());
    for s in [&same, &ancestor, &root] {
        assert_eq!((71.0, -23.0, 1.7), (s.cam_x, s.cam_y, s.zoom));
        assert_eq!(state.discussion_context, s.discussion_context);
    }
}

#[test]
fn adds_renames_and_deletes_a_proposal_without_touching_the_ir_path() {
    let doc = parse(r#"{:hierarchical true :title "T" :classes [] :edges [] :order [] :proposals [{:id :old :name "Old" :layers []}]}"#).unwrap();
    let state = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    let boundary = MemoryBoundary::default();
    boundary
        .files
        .lock()
        .unwrap()
        .insert("probe".into(), "x".into());
    let added = scene::events::add_proposal(&state, 1_700_000_000, "Fixed", Some(&boundary));
    let id = get(&added.extra, "proposal-id").cloned().unwrap();
    assert_eq!(Value::keyword("p-1700000000"), id);
    assert_eq!(Some(&Value::Bool(true)), get(&added.extra, "proposal"));
    assert_eq!(2, seq(added.document.as_ref().unwrap(), "proposals").len());
    let records = seq(added.document.as_ref().unwrap(), "proposals");
    assert_eq!(Some(&id), get(&records[1], "id"));
    assert_eq!(
        Some(&Value::String("Fixed".into())),
        get(&records[1], "name")
    );
    // The same epoch again takes the next de-duplication suffix.
    let again = scene::events::add_proposal(&added, 1_700_000_000, "Again", None);
    let records = seq(again.document.as_ref().unwrap(), "proposals");
    assert_eq!(3, records.len());
    assert_eq!(
        Some(&Value::keyword("p-1700000000-1")),
        get(&records[2], "id")
    );
    assert_eq!(
        Some(&Value::String("Again".into())),
        get(&records[2], "name")
    );
    let third = scene::events::add_proposal(&again, 1_700_000_000, "Third", None);
    assert_eq!(
        Some(&Value::keyword("p-1700000000-2")),
        get(&seq(third.document.as_ref().unwrap(), "proposals")[3], "id")
    );
    let renamed = scene::events::rename_proposal(&added, Some(&id), "Named", None);
    assert!(proposal_named(renamed.document.as_ref().unwrap(), "Named").is_some());
    assert_eq!(
        2,
        seq(renamed.document.as_ref().unwrap(), "proposals").len()
    );
    let gone = scene::events::delete_proposal(&renamed, Some(&id), None);
    assert!(get(&gone.extra, "proposal-id").is_none());
    assert_eq!(Some(&Value::Bool(false)), get(&gone.extra, "proposal"));
    assert_eq!(1, seq(gone.document.as_ref().unwrap(), "proposals").len());
    assert_eq!(
        Some(&Value::String("Old".into())),
        get(
            &seq(gone.document.as_ref().unwrap(), "proposals")[0],
            "name"
        )
    );
    // Blank names and missing ids are no-ops.
    assert_eq!(
        renamed.document,
        scene::events::rename_proposal(&renamed, Some(&id), "   ", None).document
    );
    assert_eq!(
        renamed.document,
        scene::events::delete_proposal(&renamed, None, None).document
    );
}

#[test]
fn persists_add_rename_delete_through_the_sibling_policy_boundary() {
    let b = MemoryBoundary::default();
    let edn = "/tmp/diagram.edn";
    let policy = "/tmp/diagram.policy.edn";
    b.write(
        policy,
        r#"{:title "Keep" :proposals [{:id :old :name "Old" :layers []}]}"#,
    );
    let doc = parse(r#"{:hierarchical true :title "T" :classes [] :edges [] :order [] :proposals [{:id :old :name "Old" :layers []}]}"#).unwrap();
    let mut state = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    state.path = Some(edn.into());
    let added = scene::events::add_proposal(&state, 1_700_000_000, "Fixed", Some(&b));
    assert!(b.exists(edn));
    assert!(b.read(edn).unwrap().starts_with(";; Generated"));
    let ir = parse(&b.read(edn).unwrap()).unwrap();
    assert_eq!(2, seq(&ir, "proposals").len());
    let pol = parse(&b.read(policy).unwrap()).unwrap();
    assert_eq!(2, seq(&pol, "proposals").len());
    assert_eq!(Some(&Value::String("Keep".into())), get(&pol, "title"));
    let id = get(&added.extra, "proposal-id").cloned().unwrap();
    let renamed = scene::events::rename_proposal(&added, Some(&id), "Named", Some(&b));
    assert!(proposal_named(&parse(&b.read(policy).unwrap()).unwrap(), "Named").is_some());
    let _ = scene::events::delete_proposal(&renamed, Some(&id), Some(&b));
    assert_eq!(
        1,
        seq(&parse(&b.read(policy).unwrap()).unwrap(), "proposals").len()
    );
    assert_eq!(
        1,
        seq(&parse(&b.read(edn).unwrap()).unwrap(), "proposals").len()
    );
    assert!(!b.exists("/tmp/other.policy.edn"));
}

#[test]
fn write_proposals_preserves_omissions_and_authored_grouping_state() {
    let b = MemoryBoundary::default();
    let edn = "/tmp/diagram.edn";
    let policy = "/tmp/diagram.policy.edn";
    let proposal = r#"{:id :kept :name "Named" :layers [{:id :group :nses [:app] :layout [:c :a]}] :omit [:app.hidden :app.other] :layout {:order [:b :a]} :notice "derived" :proposal true}"#;
    let original = parse(&format!(
        r#"{{:title "Keep" :lang :lua :omit [:global.hidden] :custom {{:order [:z :a]}} :proposals [{proposal}]}}"#
    ))
    .unwrap();
    b.write(policy, &edn::encode(&original).unwrap());
    let doc = parse(&format!(
        r#"{{:hierarchical true :title "T" :classes [] :edges [] :order [] :proposals [{proposal}]}}"#
    ))
    .unwrap();
    let mut state = document::new_state(
        doc,
        Some(Value::String("one".into())),
        None,
        None,
        &[],
        None,
    );
    state.path = Some(edn.into());
    let state = scene::events::add_proposal(&state, 1_700_000_000, "Fixed", Some(&b));
    let persisted = parse(&b.read(policy).unwrap()).unwrap();
    for record in [
        seq(&persisted, "proposals")[0].clone(),
        seq(state.document.as_ref().unwrap(), "proposals")[0].clone(),
    ] {
        let record = &record;
        assert_eq!(
            parse("[:app.hidden :app.other]").unwrap(),
            *get(record, "omit").unwrap()
        );
        assert_eq!(
            parse("{:order [:b :a]}").unwrap(),
            *get(record, "layout").unwrap()
        );
        let layers = seq(record, "layers");
        assert_eq!(
            parse("[:c :a]").unwrap(),
            *get(&layers[0], "layout").unwrap()
        );
        assert!(get(record, "notice").is_none());
        assert!(get(record, "proposal").is_none());
    }
    for key in ["title", "lang", "omit", "custom"] {
        assert_eq!(get(&original, key), get(&persisted, key), "{key}");
    }
    let added = get(&state.extra, "proposal-id").cloned().unwrap();
    let state = scene::events::rename_proposal(&state, Some(&kw("kept")), "Renamed", Some(&b));
    assert_eq!(
        Some(&Value::String("Renamed".into())),
        get(
            &seq(state.document.as_ref().unwrap(), "proposals")[0],
            "name"
        )
    );
    let state = scene::events::delete_proposal(&state, Some(&added), Some(&b));
    assert_eq!(1, seq(state.document.as_ref().unwrap(), "proposals").len());
}

#[test]
fn writes_proposals_to_the_ir_and_an_explicit_policy_file() {
    let b = MemoryBoundary::default();
    let edn = "/tmp/diagram.edn";
    let policy = "/tmp/diagram.policy.edn";
    let explicit = "/tmp/explicit.policy.edn";
    let doc = parse(r#"{:title "T" :proposal {:name "old" :layers []} :proposals [{:id :one :name "One" :layers []} {:id :two :name "Two" :layers [{:id :kernel :label "Kernel" :nses [:domain]}]}]}"#).unwrap();
    b.write(policy, r#"{:title "Keep" :proposal {:name "old"}}"#);
    b.write(explicit, r#"{:title "Keep" :proposal {:name "old"}}"#);
    let out = document::write_proposals(Some(edn), &doc, &b);
    let written = parse(&b.read(edn).unwrap()).unwrap();
    let pol = parse(&b.read(policy).unwrap()).unwrap();
    assert_eq!(2, seq(&out, "proposals").len());
    assert_eq!(Some(&kw("one")), get(&seq(&out, "proposals")[0], "id"));
    assert_eq!(
        Some(&Value::String("Two".into())),
        get(&seq(&out, "proposals")[1], "name")
    );
    assert_eq!(get(&out, "proposals"), get(&written, "proposals"));
    assert_eq!(get(&out, "proposals"), get(&pol, "proposals"));
    assert!(get(&pol, "proposal").is_none());
    assert_eq!(Some(&Value::String("Keep".into())), get(&pol, "title"));
    assert!(get(&out, "proposal").is_some());
    let explicit_doc = parse(&format!(
        r#"{{:title "T" :policy-file "{explicit}" :proposals [{{:id :one :name "One" :layers []}}]}}"#
    ))
    .unwrap();
    let by_file = document::write_proposals(None, &explicit_doc, &b);
    let pol2 = parse(&b.read(explicit).unwrap()).unwrap();
    assert_eq!(get(&by_file, "proposals"), get(&pol2, "proposals"));
    assert!(get(&pol2, "proposal").is_none());
    let only = "/tmp/only.edn";
    let skipped = document::write_proposals(
        Some(only),
        &parse(r#"{:title "T" :policy-file "/tmp/nope.policy.edn" :proposals [{:id :one :name "One" :layers []}]}"#).unwrap(),
        &b,
    );
    assert_eq!(1, seq(&skipped, "proposals").len());
    assert!(b.exists(only));
    assert!(!b.exists("/tmp/nope.policy.edn"));
}

#[test]
fn sibling_policy_discovery_requires_an_existing_file() {
    let files = ["/d/diagram.policy.edn"];
    let exists = |p: &str| files.contains(&p);
    assert!(document::policy_path_for("/d/notes.txt", &exists).is_none());
    assert!(document::policy_path_for("/d/missing.edn", &exists).is_none());
    assert_eq!(
        Some("/d/diagram.policy.edn".to_string()),
        document::policy_path_for("/d/diagram.edn", &exists)
    );
}

#[test]
fn compiles_explicit_proposal_options_with_focus_priority() {
    let doc = parse(r#"{:hierarchical true :classes [{:id :source.lua} {:id :other}] :edges [] :proposals [{:id :split :name "Split" :layers [{:id :core :label "Core" :nses [:source.lua]}]}]}"#).unwrap();
    let p = document::compile_view(&doc, &[], &[], &parse("{:proposal-id :split}").unwrap());
    assert!(get(get(&p, "diagram").unwrap(), "proposal").is_some());
    let n = document::compile_view(
        &doc,
        &[],
        &[kw("source")],
        &parse("{:proposal true :proposal-id :split}").unwrap(),
    );
    assert!(get(get(&n, "diagram").unwrap(), "proposal").is_none());
    assert_eq!(
        Some(&Value::String("source".into())),
        get(get(&n, "diagram").unwrap(), "title")
    );
}

#[test]
fn metric_only_changes_reload_from_the_base_ir_and_retain_focus() {
    let fixture = r#"{:packages [{:label "P" :classes [{:id :calc :ns "calc.util" :ops [{:name "M.clamp" :text "authored signature"}]}]}]}"#;
    let doc = scene::ir::read_document(fixture).unwrap();
    let files = vec![Snapshot::new(
        ".metrics/crap.edn",
        r#"{:entries [{:namespace "calc.util" :name "f" :complexity 1 :crap 1}]}"#,
        Value::String("1".into()),
    )];
    let state = document::new_state(
        doc,
        Some(Value::String("ir".into())),
        None,
        None,
        &files,
        Some(Value::String("metrics-1".into())),
    );
    assert_eq!(
        Some(1.0),
        edn::record::num(
            get(get(&seq(&state.scene, "classes")[0], "crap").unwrap(), "mu").unwrap()
        )
    );
    let files2 = vec![Snapshot::new(
        ".metrics/crap.edn",
        r#"{:entries [{:namespace "calc.util" :name "f" :complexity 9 :crap 9}]}"#,
        Value::String("2".into()),
    )];
    let (next, changed, _) = document::reload(
        &state,
        fixture,
        Some(Value::String("ir".into())),
        &files2,
        Some(Value::String("metrics-2".into())),
    );
    assert!(changed);
    assert_eq!(2, next.revision);
    assert_eq!(state.focus, next.focus);
    assert_eq!(
        Some(9.0),
        edn::record::num(get(get(&seq(&next.scene, "classes")[0], "crap").unwrap(), "mu").unwrap())
    );
}

#[test]
fn displays_a_new_diagram_with_a_fit_and_keeps_the_view_on_the_same_one() {
    struct B;
    impl document::MailBoundary for B {
        fn resolve(&self, p: &str, _root: Option<&str>) -> String {
            p.into()
        }
        fn canonical(&self, p: &str) -> String {
            p.into()
        }
        fn load(&self, p: &str) -> Option<ViewState> {
            let text = r#"{:hierarchical true :title "Tree" :classes [{:id :source} {:id :source.lua} {:id :other}] :edges []}"#;
            let mut s = document::new_state(
                scene::ir::read_document(text).unwrap(),
                Some(Value::String("m1".into())),
                None,
                None,
                &[],
                None,
            );
            s.path = Some(p.into());
            Some(s)
        }
    }
    let display = |s: &ViewState, path: &str, id: i64| {
        document::apply_mail(
            s,
            &parse(&format!("{{:id {id} :op :display :path \"{path}\"}}")).unwrap(),
            &B,
        )
    };
    let mut current = document::MailBoundary::load(&B, "a.edn").unwrap();
    current.project_root = Some(".".into());
    current.zoom = 2.5;
    current.cam_x = 33.0;
    current.cam_y = 44.0;
    current.selected = Some(parse("{:kind :class :id :source}").unwrap());
    let same = display(&current, "a.edn", 1);
    assert!(!same.fit_pending);
    assert_eq!((2.5, 33.0, 44.0), (same.zoom, same.cam_x, same.cam_y));
    assert_eq!(
        Some(&kw("source")),
        get(same.selected.as_ref().unwrap(), "id")
    );
    let other = display(&current, "b.edn", 2);
    assert!(other.fit_pending);
    let mut waiting = document::waiting_state(Some("a.edn".into()), 0);
    waiting.project_root = Some(".".into());
    assert!(
        display(&waiting, "a.edn", 1).fit_pending,
        "first display after a fresh launch is a new load"
    );
    let (reloaded, _, _) = document::reload(
        &same,
        r#"{:hierarchical true :title "Tree" :classes [{:id :source} {:id :source.lua} {:id :other}] :edges []}"#,
        Some(Value::String("m2".into())),
        &[],
        Some(Value::String("stamp".into())),
    );
    assert!(!reloaded.fit_pending);
    assert_eq!(2.5, reloaded.zoom);
}

#[test]
fn context_commands_update_discussion_without_recompiling() {
    let state = load(r#"{:hierarchical true :classes [{:id :a}] :edges []}"#);
    let ctx = document::apply_mail(
        &state,
        &parse("{:id 3 :op :context :context :proposal :proposal-id :p :name \"N\"}").unwrap(),
        &NoIo,
    );
    assert_eq!(
        Some(&parse("{:context :proposal :proposal-id :p :name \"N\"}").unwrap()),
        ctx.discussion_context.as_ref()
    );
    assert_eq!(3, ctx.mail_seen);
    assert_eq!(state.scene, ctx.scene);
    assert_eq!(state.revision, ctx.revision);
    // An unknown op only advances the cursor.
    let other = document::apply_mail(&state, &parse("{:id 5 :op :omit}").unwrap(), &NoIo);
    assert_eq!(5, other.mail_seen);
    assert_eq!(state.scene, other.scene);
}

struct NoIo;

impl document::MailBoundary for NoIo {
    fn resolve(&self, p: &str, _root: Option<&str>) -> String {
        p.into()
    }
    fn canonical(&self, p: &str) -> String {
        p.into()
    }
    fn load(&self, _p: &str) -> Option<ViewState> {
        None
    }
}

#[allow(dead_code)]
fn boundary_is_object_safe(_b: &dyn Boundary) {}

#[test]
fn lists_named_proposals_in_policy_order_then_the_legacy_singular() {
    let doc = scene::ir::read_document(
        r#"{:hierarchical true :classes [] :proposals [{:id :split :name "Split" :layers []} {:id :merge :name "Merge" :layers []}]}"#,
    )
    .unwrap();
    let ids: Vec<Value> = scene::proposals(&doc)
        .iter()
        .filter_map(|p| get(p, "id").cloned())
        .collect();
    assert_eq!(vec![kw("split"), kw("merge")], ids);
    let legacy = parse(r#"{:proposal {:name "Only" :layers []}}"#).unwrap();
    let only = scene::proposals(&legacy);
    assert_eq!(1, only.len());
    assert_eq!(Some(&Value::String("Only".into())), get(&only[0], "name"));
    assert!(scene::proposals(&parse("{:classes []}").unwrap()).is_empty());
}

#[test]
fn a_drilled_package_frame_names_only_its_own_segment() {
    // The chrome title and the back link carry the whole path; the frame
    // repeats only the last segment, so a deep path fits the frame title.
    let doc = scene::ir::read_document(
        "{:hierarchical true :prefix \"x\" \
          :classes [{:id :a.b.c :name \"C\" :ns \"x.a.b.c\"} {:id :a.b.d :name \"D\" :ns \"x.a.b.d\"}] \
          :edges []}",
    )
    .unwrap();
    let focus = [kw("a"), kw("b")];
    let view = document::compile_view(&doc, &[], &focus, &Value::Map(vec![]));
    let diagram = get(&view, "diagram").unwrap();
    assert_eq!(Some(&Value::String("a.b".into())), get(diagram, "title"));
    let packages = match get(&view, "packages") {
        Some(Value::Vector(ps)) => ps.clone(),
        other => panic!("{other:?}"),
    };
    let labels: Vec<_> = packages
        .iter()
        .map(|p| get(p, "title").or_else(|| get(p, "label")).cloned())
        .collect();
    assert_eq!(vec![Some(Value::String("b".into()))], labels);
}
