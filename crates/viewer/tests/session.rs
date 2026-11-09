//! Snapshot compatibility for view/session restore. LOVE_SESSION preserves
//! the historical LOVE 11.5 snapshot fixture; resident persistence retains
//! its camera, chrome, selection, discussion context and cursor fields.

use edn::Value;
use edn::record::get;
use scene::document::{self, ViewState};
use viewer::session::{self, Chrome};

const LOVE_SESSION: &str = r#"{:window-w 1100 :window-h 700 :inspector-width 340 :path "/tmp/uml142 love 项目/diagram.edn" :discussion-context {:context :proposal :proposal-id :plan :name "中文方案"} :mail-seen 2 :focus [] :cam-x 33 :cam-y 21 :zoom 1.7 :selected {:kind :class :id :a} :detail-id :a}"#;

const DIAGRAM: &str = r#"{:title "LOVE 会话" :hierarchical true :prefix "demo" :classes [{:id :a :name "中文类" :ns "demo.a" :ops [{:name "work" :text "work()"}]} {:id :b :name "Beta" :ns "demo.b" :fields ["count"]}] :edges [{:from :a :to :b :kind :dependency}] :proposals [{:id :plan :name "中文方案" :layers []}]}"#;

fn loaded() -> ViewState {
    let doc = scene::ir::read_document(DIAGRAM).unwrap();
    let mut state = document::new_state(doc, None, None, None, &[], None);
    state.fit_pending = true;
    state
}

fn love() -> Value {
    edn::parse(LOVE_SESSION).unwrap()
}

#[test]
fn a_love_written_session_restores_view_selection_card_context_and_cursor() {
    let restored = session::restore(&loaded(), &love());
    assert_eq!(1.7, restored.zoom);
    assert_eq!((33.0, 21.0), (restored.cam_x, restored.cam_y));
    assert!(!restored.fit_pending, "a restored view is not re-fitted");
    assert_eq!(
        Some(&Value::keyword("a")),
        restored.selected.as_ref().and_then(|s| get(s, "id"))
    );
    assert_eq!(Some(Value::keyword("a")), restored.detail_id);
    let context = restored.discussion_context.expect("discussion context");
    assert_eq!(Some(&Value::keyword("plan")), get(&context, "proposal-id"));
    assert_eq!(
        Some(&Value::String("中文方案".into())),
        get(&context, "name")
    );
    assert_eq!(2, restored.mail_seen);
}

#[test]
fn a_love_written_session_restores_the_window_and_inspector_width() {
    let chrome = session::chrome(Some(&love()));
    assert_eq!(
        Chrome {
            window_w: Some(1100.0),
            window_h: Some(700.0),
            inspector_w: Some(340.0),
        },
        chrome
    );
    assert_eq!([1100.0, 700.0], session::window_size(&chrome, 1.0));
}

#[test]
fn a_legacy_session_without_chrome_opens_with_the_defaults() {
    let legacy = edn::parse("{:path \"d.edn\" :mail-seen 4 :zoom 1.25 :cam-x 7 :cam-y 0}").unwrap();
    let chrome = session::chrome(Some(&legacy));
    assert_eq!(Chrome::default(), chrome);
    assert_eq!([1500.0, 920.0], session::window_size(&chrome, 1.0));
    assert_eq!([3000.0, 1840.0], session::window_size(&chrome, 2.0));
    let restored = session::restore(&loaded(), &legacy);
    assert_eq!((1.25, 7.0), (restored.zoom, restored.cam_x));
    assert_eq!(4, restored.mail_seen);
}

#[test]
fn a_saved_window_never_opens_below_the_minimum() {
    let tiny = Chrome {
        window_w: Some(100.0),
        window_h: Some(90.0),
        inspector_w: None,
    };
    assert_eq!([640.0, 480.0], session::window_size(&tiny, 1.0));
}

#[test]
fn a_session_without_a_context_keeps_the_current_discussion_context() {
    let mut state = loaded();
    let real = edn::parse("{:context :real}").unwrap();
    state.discussion_context = Some(real.clone());
    let snapshot = edn::parse("{:zoom 2 :cam-x 1 :cam-y 2}").unwrap();
    assert_eq!(
        Some(real),
        session::restore(&state, &snapshot).discussion_context
    );
}

#[test]
fn the_written_snapshot_uses_the_love_schema_and_restores_the_same_view() {
    let mut state = session::restore(&loaded(), &love());
    state.path = Some("/tmp/uml142 love 项目/diagram.edn".into());
    let chrome = Chrome {
        window_w: Some(1100.0),
        window_h: Some(700.0),
        inspector_w: Some(340.0),
    };
    let written = edn::encode(&session::snapshot(&state, &chrome)).unwrap();
    let reread = edn::parse(&written).unwrap();
    for key in [
        "window-w",
        "window-h",
        "inspector-width",
        "path",
        "discussion-context",
        "mail-seen",
        "focus",
        "cam-x",
        "cam-y",
        "zoom",
        "selected",
        "detail-id",
    ] {
        assert!(
            get(&reread, key).is_some(),
            "snapshot lacks :{key}: {written}"
        );
    }
    assert_eq!(chrome, session::chrome(Some(&reread)));
    let again = session::restore(&loaded(), &reread);
    assert_eq!(
        (state.zoom, state.cam_x, state.cam_y),
        (again.zoom, again.cam_x, again.cam_y)
    );
    assert_eq!(state.selected, again.selected);
    assert_eq!(state.detail_id, again.detail_id);
    assert_eq!(state.discussion_context, again.discussion_context);
    assert_eq!(state.mail_seen, again.mail_seen);
}

#[test]
fn an_explicit_path_overrides_the_saved_one_which_overrides_the_default() {
    let saved = love();
    let default = std::path::Path::new("/p/diagram.edn");
    assert_eq!(
        std::path::PathBuf::from("/x.edn"),
        session::path(Some(std::path::Path::new("/x.edn")), Some(&saved), default)
    );
    assert_eq!(
        std::path::PathBuf::from("/tmp/uml142 love 项目/diagram.edn"),
        session::path(None, Some(&saved), default)
    );
    assert_eq!(default.to_path_buf(), session::path(None, None, default));
}

#[test]
fn a_restored_session_keeps_the_wide_layout_of_the_loaded_state() {
    let doc = scene::ir::read_document(DIAGRAM).unwrap();
    let state = document::new_state(doc, None, None, Some(&session::view_options()), &[], None);
    let restored = session::restore(&state, &love());
    assert_eq!(Some(&Value::Bool(true)), get(&restored.extra, "wide"));
    let direction = get(&restored.scene, "diagram").and_then(|d| get(d, "direction"));
    assert_eq!(Some(&Value::keyword("lr")), direction);
}
