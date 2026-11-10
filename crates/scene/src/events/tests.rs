//! Breadcrumbs and returning to a named level (#45): the tree path the panel
//! shows in its top bar, and the `back` target that returns to any of it.
//! Documents are written literally, as in `hierarchy/tests.rs`.

use crate::document::{self, ViewState};
use crate::events::{self, Crumb, CrumbView};
use edn::record::get;
use edn::{Value, parse};

fn kw(s: &str) -> Value {
    Value::keyword(s)
}

const LEVELS: &str = r#"{:hierarchical true :title "Levels"
 :classes [{:id :a :name "a" :ns "levels.a"}
           {:id :a.b :name "b" :ns "levels.a.b"}
           {:id :a.b.c :name "c" :ns "levels.a.b.c"}
           {:id :x :name "x" :ns "levels.x"}
           ;; No layer claims :y, so the proposal shows :proposal.unassigned.
           {:id :y :name "y" :ns "levels.y"}]
 :edges [{:from :a.b :to :x :kind :dependency}]
 :proposals [{:id :p-levels :name "Levels proposal"
              :layers [{:id :domain :label "Domain"
                        :nses [{:id :core-group :label "Core" :nses [:a]}]}
                       {:id :edge :label "Edge" :nses [:x]}]}]}"#;

fn state(focus: &[Value], extra: &str) -> ViewState {
    let doc = parse(LEVELS).unwrap();
    document::new_state(
        doc,
        None,
        Some(focus),
        Some(&parse(extra).unwrap()),
        &[],
        None,
    )
}

fn crumb(id: Option<&str>, name: &str, view: CrumbView) -> Crumb {
    Crumb {
        id: id.map(kw),
        name: name.to_owned(),
        view,
    }
}

fn source(depth: usize) -> CrumbView {
    CrumbView::Source { depth }
}

#[test]
fn the_breadcrumb_path_names_every_namespace_from_the_root_down() {
    assert_eq!(
        vec![crumb(None, "Levels", source(0))],
        events::breadcrumbs(&state(&[], "{}"))
    );
    assert_eq!(
        vec![
            crumb(None, "Levels", source(0)),
            crumb(Some("a"), "a", source(1)),
            crumb(Some("a.b"), "b", source(2)),
        ],
        events::breadcrumbs(&state(&[kw("a"), kw("b")], "{}"))
    );
    // A diagram is not loaded yet: there is no tree to show.
    assert!(events::breadcrumbs(&ViewState::default()).is_empty());
}

#[test]
fn a_proposal_breadcrumb_path_starts_at_the_proposal_and_names_its_layers() {
    let proposal = ":proposal true :proposal-id :p-levels";
    assert_eq!(
        vec![crumb(
            Some("p-levels"),
            "Levels proposal",
            CrumbView::Proposal { layer: None }
        )],
        events::breadcrumbs(&state(&[], &format!("{{{proposal}}}")))
    );
    // A declared layer package (`:proposal.<layer>`) is one level.
    assert_eq!(
        vec![
            crumb(
                Some("p-levels"),
                "Levels proposal",
                CrumbView::Proposal { layer: None }
            ),
            crumb(
                Some("proposal.domain"),
                "Domain",
                CrumbView::Proposal {
                    layer: Some(kw("proposal.domain"))
                }
            ),
        ],
        events::breadcrumbs(&state(
            &[],
            "{:proposal true :proposal-id :p-levels :open-layer :proposal.domain}"
        ))
    );
    // A nested proposal group keeps the levels above it.
    assert_eq!(
        vec![
            crumb(
                Some("p-levels"),
                "Levels proposal",
                CrumbView::Proposal { layer: None }
            ),
            crumb(
                Some("proposal.domain"),
                "Domain",
                CrumbView::Proposal {
                    layer: Some(kw("proposal.domain"))
                }
            ),
            crumb(
                Some("core-group"),
                "Core",
                CrumbView::Proposal {
                    layer: Some(kw("core-group"))
                }
            ),
        ],
        events::breadcrumbs(&state(
            &[],
            "{:proposal true :proposal-id :p-levels :open-layer :core-group}"
        ))
    );
}

#[test]
fn a_generated_proposal_layer_is_named_the_way_the_canvas_names_it() {
    // Classes no declared layer claims land in `:proposal.unassigned`; its
    // label exists only in the scene, so the crumb reads it from there.
    let unassigned = state(
        &[],
        "{:proposal true :proposal-id :p-levels :open-layer :proposal.unassigned}",
    );
    assert_eq!(
        vec![
            crumb(
                Some("p-levels"),
                "Levels proposal",
                CrumbView::Proposal { layer: None }
            ),
            crumb(
                Some("proposal.unassigned"),
                "Unassigned",
                CrumbView::Proposal {
                    layer: Some(kw("proposal.unassigned"))
                }
            ),
        ],
        events::breadcrumbs(&unassigned)
    );
    // A layer the proposal no longer declares is not a level; the path stops
    // at the proposal rather than inventing one.
    let stale = state(
        &[],
        "{:proposal true :proposal-id :p-levels :open-layer :proposal.gone}",
    );
    assert_eq!(1, events::breadcrumbs(&stale).len());
}

#[test]
fn returning_to_a_level_restores_it_and_forgets_everything_below() {
    let deep = state(&[kw("a"), kw("b")], "{}");
    let middle = events::back_to(&deep, Some(&kw("a"))).expect("a is a level");
    assert_eq!(vec![kw("a")], middle.focus);
    assert!(middle.fit_pending, "returning refits the view");
    assert_eq!(None, middle.selected);
    let root = events::back_to(&deep, None).expect("the root names no id");
    assert!(root.focus.is_empty());
    // A level the path does not carry is refused, not guessed.
    assert!(events::back_to(&deep, Some(&kw("x"))).is_none());
    // The deepest level is where the view already is: nothing moves.
    let mut shown = deep.clone();
    shown.cam_x = 12.0;
    let same = events::back_to(&shown, Some(&kw("a.b"))).expect("a.b is the current level");
    assert_eq!(12.0, same.cam_x);
    assert!(!same.fit_pending);
}

#[test]
fn returning_to_a_proposal_level_reopens_that_layer() {
    let group = state(
        &[],
        "{:proposal true :proposal-id :p-levels :open-layer :core-group}",
    );
    let layer = events::back_to(&group, Some(&kw("proposal.domain"))).expect("a layer is a level");
    assert_eq!(
        Some(kw("proposal.domain")),
        get(&layer.extra, "open-layer").cloned()
    );
    let root =
        events::back_to(&group, Some(&kw("p-levels"))).expect("the proposal root is a level");
    assert_eq!(None, get(&root.extra, "open-layer"));
    // The source root's id is `null`, which a proposal path does not carry.
    assert!(events::back_to(&group, None).is_none());
    // A proposal's `back` never falls into the source tree.
    assert!(events::back_to(&group, Some(&kw("a"))).is_none());
}
