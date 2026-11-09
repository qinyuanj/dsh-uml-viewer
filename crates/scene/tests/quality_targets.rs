//! Quality refresh targets (#28): what a panel's right-click names, resolved
//! with the scene's own grouping rules — source class, namespace component,
//! proposal layer, nested proposal group, `:proposal.unassigned`, `:omit`,
//! foreign and unknown ids.
//!
//! Authored under the RESOURCE-POLICY of spec #203 and **NOT RUN**: no Rust
//! test binary may be built or executed in this session. Run
//! `cargo test -p scene --locked` when the policy allows it.

use edn::Value;
use scene::targets::{Kind, Target, resolve};

/// A hierarchical IR document, as `uml-viewer ir` writes it.
fn doc(text: &str) -> Value {
    scene::ir::read_document(text).expect("the fixture is a hierarchical IR document")
}

fn keyword(text: &str) -> Value {
    edn::parse(text).expect("a keyword")
}

fn extra(proposal: Option<&str>) -> Value {
    match proposal {
        Some(id) => Value::Map(vec![(Value::keyword("proposal-id"), keyword(id))]),
        None => Value::Map(vec![]),
    }
}

/// The member ids, sorted: membership is the contract here, and the file
/// answer sorts its own paths (`viewer-session::targets`).
fn members(target: &Target) -> Vec<String> {
    let mut ids: Vec<String> = target.members.iter().map(|m| m.id.clone()).collect();
    ids.sort();
    ids
}

fn files(target: &Target) -> Vec<String> {
    target
        .members
        .iter()
        .filter_map(|m| m.file.clone())
        .collect()
}

/// One source diagram: a plain class, a namespace component (including a
/// nested one that only shares a file), a dash-prefixed sibling that is not
/// inside it, `:omit`ped classes and a foreign one.
const SOURCE: &str = r#"{:hierarchical true :prefix "demo" :omit [:hidden]
  :classes [{:id :main :file "src/main.rs"}
            {:id :api :file "src/api.rs"}
            {:id :api.handler :file "src/api/handler.rs"}
            {:id :core :file "src/core/lib.rs"}
            {:id :core.order :file "src/core/order.rs"}
            {:id :core.order.line :file "src/core/order.rs"}
            {:id :core.extra :file "src/core/extra.rs"}
            {:id :core-extra :file "src/core-extra/x.rs"}
            {:id :hidden :file "src/hidden.rs"}
            {:id :hidden.child :file "src/hidden/child.rs"}
            {:id :quil :name "quil" :foreign true}]}"#;

#[test]
fn a_plain_class_is_the_class_itself() {
    let target = resolve(&doc(SOURCE), &[], &extra(None), &keyword(":main")).unwrap();
    assert_eq!(target.kind, Kind::Class);
    assert_eq!(members(&target), [":main"]);
    assert_eq!(files(&target), ["src/main.rs"]);
}

#[test]
fn a_namespace_id_is_every_class_under_it_and_never_a_dash_sibling() {
    let target = resolve(&doc(SOURCE), &[], &extra(None), &keyword(":core")).unwrap();
    assert_eq!(target.kind, Kind::Component);
    assert_eq!(
        members(&target),
        [":core", ":core.extra", ":core.order", ":core.order.line"]
    );
    // The namespace's own class file and its subtree, one entry per class.
    assert_eq!(
        files(&target),
        [
            "src/core/lib.rs",
            "src/core/order.rs",
            "src/core/order.rs",
            "src/core/extra.rs"
        ]
    );
    // `:core-extra` is a different namespace, not a child of `:core`.
    let sibling = resolve(&doc(SOURCE), &[], &extra(None), &keyword(":core-extra")).unwrap();
    assert_eq!(sibling.kind, Kind::Class);
    assert_eq!(members(&sibling), [":core-extra"]);
}

#[test]
fn a_nested_namespace_component_is_still_a_component() {
    let target = resolve(&doc(SOURCE), &[], &extra(None), &keyword(":core.order")).unwrap();
    assert_eq!(target.kind, Kind::Component);
    assert_eq!(members(&target), [":core.order", ":core.order.line"]);
}

#[test]
fn omitted_foreign_and_unknown_ids_are_refused() {
    let source = doc(SOURCE);
    for id in [":hidden", ":hidden.child"] {
        let error = resolve(&source, &[], &extra(None), &keyword(id)).unwrap_err();
        assert!(
            error.contains("no class or namespace component"),
            "{id}: {error}"
        );
    }
    let error = resolve(&source, &[], &extra(None), &keyword(":quil")).unwrap_err();
    assert!(
        error.contains("no class or namespace component :quil"),
        "{error}"
    );
    let error = resolve(&source, &[], &extra(None), &keyword(":nowhere")).unwrap_err();
    assert!(error.contains(":nowhere"), "{error}");
    // An omitted class never joins a component either.
    let component = resolve(&source, &[], &extra(None), &keyword(":core")).unwrap();
    assert!(!files(&component).contains(&"src/hidden.rs".to_owned()));
}

/// One proposal over classes that exercise the scene's pick rule: an exact
/// module in a group (`:api`, not its `:api.handler` child), a namespace
/// subtree (`:core`), a leftover set, proposal `:omit` and a foreign class.
const PROPOSAL: &str = r#"{:hierarchical true :prefix "demo" :omit []
  :classes [{:id :core.order :file "src/core/order.rs"}
            {:id :core.order.total :file "src/core/order.rs"}
            {:id :core.cart :file "src/core/cart.rs"}
            {:id :api :file "src/api.rs"}
            {:id :api.handler :file "src/api/handler.rs"}
            {:id :util :file "src/util.rs"}
            {:id :other :file "src/other.rs"}
            {:id :quil :name "quil" :foreign true}]
  :proposals [{:id :p-1 :name "2026-11-09 10:00" :omit [:other]
               :layers [{:id :domain :label "Domain"
                         :nses [:core {:id :orders :label "Orders" :nses [:api]}]}
                        {:id :io :label "IO" :nses [:core.cart]}]}]}"#;

#[test]
fn a_proposal_layer_is_its_members_with_omit_subtracted() {
    let proposal = doc(PROPOSAL);
    let target = resolve(
        &proposal,
        &[],
        &extra(Some(":p-1")),
        &keyword(":proposal.domain"),
    )
    .unwrap();
    assert_eq!(target.kind, Kind::Component);
    assert_eq!(
        members(&target),
        [":api", ":core.cart", ":core.order", ":core.order.total"]
    );
    let io = resolve(
        &proposal,
        &[],
        &extra(Some(":p-1")),
        &keyword(":proposal.io"),
    )
    .unwrap();
    assert_eq!(members(&io), [":core.cart"]);
}

#[test]
fn a_nested_proposal_group_takes_the_exact_module_or_else_the_subtree() {
    let proposal = doc(PROPOSAL);
    // `:api` is an exact class, so the group is that module alone: its
    // `:api.handler` child is not pulled in, exactly as the scene paints it.
    let exact = resolve(&proposal, &[], &extra(Some(":p-1")), &keyword(":orders")).unwrap();
    assert_eq!(exact.kind, Kind::Component);
    assert_eq!(members(&exact), [":api"]);
    // A name with no exact class is the namespace subtree.
    let subtree = resolve(
        &proposal,
        &[],
        &extra(Some(":p-1")),
        &keyword(":proposal.domain"),
    )
    .unwrap();
    assert!(members(&subtree).contains(&":core.order".to_owned()));
    assert!(members(&subtree).contains(&":core.cart".to_owned()));
}

#[test]
fn the_unassigned_leftovers_are_the_classes_no_layer_claims() {
    let target = resolve(
        &doc(PROPOSAL),
        &[],
        &extra(Some(":p-1")),
        &keyword(":proposal.unassigned"),
    )
    .unwrap();
    assert_eq!(target.kind, Kind::Component);
    // `:other` is omitted, `:quil` is foreign, and `:api.handler` is covered
    // by the `:api` claim of the `:orders` group.
    assert_eq!(members(&target), [":util"]);
    assert_eq!(files(&target), ["src/util.rs"]);
}

#[test]
fn a_class_inside_the_shown_proposal_is_the_class_itself() {
    let target = resolve(&doc(PROPOSAL), &[], &extra(Some(":p-1")), &keyword(":api")).unwrap();
    assert_eq!(target.kind, Kind::Class);
    assert_eq!(members(&target), [":api"]);
    // A proposal paints only what its layers and leftovers hold: this class
    // is claimed but picked by no group, so it cannot be named there.
    let error = resolve(
        &doc(PROPOSAL),
        &[],
        &extra(Some(":p-1")),
        &keyword(":api.handler"),
    )
    .unwrap_err();
    assert!(error.contains("in the shown proposal"), "{error}");
}

#[test]
fn proposal_names_need_the_proposal_tree_and_source_names_need_the_source_tree() {
    let proposal = doc(PROPOSAL);
    let error = resolve(&proposal, &[], &extra(None), &keyword(":proposal.domain")).unwrap_err();
    assert!(error.contains("no class or namespace component"), "{error}");
    let error = resolve(&proposal, &[], &extra(Some(":p-1")), &keyword(":other")).unwrap_err();
    assert!(error.contains("in the shown proposal"), "{error}");
    // The source tree of the same document still resolves its namespaces.
    let source = resolve(&proposal, &[], &extra(None), &keyword(":core")).unwrap();
    assert_eq!(source.kind, Kind::Component);
    assert_eq!(
        members(&source),
        [":core.cart", ":core.order", ":core.order.total"]
    );
}

#[test]
fn an_entered_namespace_focus_shows_the_source_tree_even_with_a_proposal() {
    // `compile_view` paints `view_at(doc, focus)` whenever a focus is set, so
    // a target is looked up in that same tree.
    let proposal = doc(PROPOSAL);
    let focus = [keyword(":core")];
    let target = resolve(
        &proposal,
        &focus,
        &extra(Some(":p-1")),
        &keyword(":core.order"),
    )
    .unwrap();
    assert_eq!(target.kind, Kind::Component);
    assert_eq!(members(&target), [":core.order", ":core.order.total"]);
}
