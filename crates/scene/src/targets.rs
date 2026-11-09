//! Quality refresh targets (#28): the class or component a panel names,
//! resolved against the loaded IR with the scene's own grouping rules.
//!
//! The panel sends one element id; this module says what that element is and
//! which classes belong to it. Nothing is re-derived elsewhere: the resident
//! serializes this answer (`crate::targets` in `viewer-session`) and the
//! plugin host only validates and queues it.
//!
//! Which tree a target is looked up in is exactly the tree
//! [`crate::document::compile_view`] shows, because two groupings exist:
//!
//! - a **proposal view** while the session's `:proposal-id` names an existing
//!   proposal and no namespace focus is entered: its layer packages
//!   (`:proposal.<layer>`), their nested group boxes, and the
//!   `:proposal.unassigned` leftovers, whose members are
//!   [`crate::hierarchy::proposal_view`]'s own picks (an exact module first,
//!   otherwise the namespace subtree; policy and proposal `:omit` subtracted;
//!   foreign classes never included);
//! - the **source diagram** otherwise: a class id, or every class under a
//!   namespace id, with policy `:omit` and foreign classes excluded, in the
//!   diagram's order.
//!
//! A name the shown tree does not have is a refusal, never an empty answer.

use crate::document;
use crate::hierarchy;
use crate::policy;
use edn::Value;
use edn::record::{flag, get, name, seq};

/// What the named element is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One class, whose own source file is the whole target.
    Class,
    /// A namespace, proposal layer, proposal group or proposal leftover set.
    Component,
}

impl Kind {
    /// The wire name (`"class"` or `"component"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Class => "class",
            Kind::Component => "component",
        }
    }
}

/// One member class of a target: its EDN id text and its IR `:file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub file: Option<String>,
}

/// The named element and its member classes, in the shown tree's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub kind: Kind,
    pub members: Vec<Member>,
}

fn id_text(value: &Value) -> String {
    edn::encode(value).unwrap_or_default()
}

fn member(class: &Value) -> Member {
    Member {
        id: get(class, "id").map(id_text).unwrap_or_default(),
        file: match get(class, "file") {
            Some(Value::String(file)) if !file.is_empty() => Some(file.clone()),
            _ => None,
        },
    }
}

/// The leaf classes of a group's children, in display order; a nested group
/// box contributes its own members, which is what the scene paints.
fn leaves(children: &[Value], out: &mut Vec<Member>) {
    for child in children {
        if flag(child, "proposal-group?") {
            leaves(seq(get(child, "members")), out);
        } else {
            out.push(member(child));
        }
    }
}

fn members(children: &[Value]) -> Vec<Member> {
    let mut out = Vec::new();
    leaves(children, &mut out);
    out
}

/// The children of the nested group box `want`, depth first.
fn group_children<'a>(children: &'a [Value], want: &Value) -> Option<&'a [Value]> {
    for child in children {
        if !flag(child, "proposal-group?") {
            continue;
        }
        if get(child, "id") == Some(want) {
            return Some(seq(get(child, "members")));
        }
        if let Some(found) = group_children(seq(get(child, "members")), want) {
            return Some(found);
        }
    }
    None
}

/// The leaf class `want` in a group tree, depth first.
fn leaf<'a>(children: &'a [Value], want: &Value) -> Option<&'a Value> {
    for child in children {
        if flag(child, "proposal-group?") {
            if let Some(found) = leaf(seq(get(child, "members")), want) {
                return Some(found);
            }
        } else if get(child, "id") == Some(want) {
            return Some(child);
        }
    }
    None
}

/// A layer package, `:proposal.unassigned`, a nested group or a class of the
/// shown proposal view.
fn in_proposal(packages: &[Value], want: &Value) -> Option<Target> {
    for package in packages {
        if get(package, "id") == Some(want) {
            return Some(Target {
                kind: Kind::Component,
                members: members(seq(get(package, "classes"))),
            });
        }
    }
    for package in packages {
        if let Some(children) = group_children(seq(get(package, "classes")), want) {
            return Some(Target {
                kind: Kind::Component,
                members: members(children),
            });
        }
    }
    for package in packages {
        if let Some(class) = leaf(seq(get(package, "classes")), want) {
            return Some(Target {
                kind: Kind::Class,
                members: vec![member(class)],
            });
        }
    }
    None
}

/// A class, or every visible class under a namespace id, in the diagram's own
/// order. The classes a namespace covers are exactly the ones the source view
/// paints: policy `:omit` and foreign classes never join.
fn in_source(doc: &Value, want: &Value) -> Option<Target> {
    let omit = get(doc, "omit");
    let mut found: Vec<Member> = Vec::new();
    let mut nested = false;
    for class in seq(get(doc, "classes")) {
        let Some(class_id) = get(class, "id") else {
            continue;
        };
        if flag(class, "foreign") || policy::omitted_id(class_id, omit) {
            continue;
        }
        if name(class_id) == name(want) {
            found.push(member(class));
        } else if hierarchy::under(class_id, want) {
            nested = true;
            found.push(member(class));
        }
    }
    if found.is_empty() {
        return None;
    }
    // An id with classes under it is a component even when a class carries the
    // same id (a Rust `lib.rs`): the panel refreshes the subtree, not one file.
    let kind = if nested { Kind::Component } else { Kind::Class };
    Some(Target {
        kind,
        members: found,
    })
}

/// Resolve `id` in the tree the session shows: the current proposal view
/// (`:proposal-id` in `extra`, with an empty `focus`), else the source
/// diagram. These are the two session state fields
/// [`crate::document::compile_view`] reads to pick its tree, so the answer
/// describes what the panel can actually see. A drilled-in namespace shows
/// the source tree, and membership is the same either way: which level is on
/// screen does not change which classes a name covers.
///
/// # Errors
///
/// The shown tree has no class, layer, group or unassigned set with that id.
pub fn resolve(doc: &Value, focus: &[Value], extra: &Value, id: &Value) -> Result<Target, String> {
    let named = hierarchy::named_proposals(doc);
    let proposal = document::active_proposal_id(&named, extra);
    let shown = match proposal {
        Some(proposal) if focus.is_empty() && !named.is_empty() => {
            let view = hierarchy::proposal_view(doc, Some(&proposal));
            seq(get(&view, "packages")).to_vec()
        }
        _ => Vec::new(),
    };
    if !shown.is_empty() {
        return in_proposal(&shown, id).ok_or_else(|| {
            format!(
                "no class, proposal layer, proposal group or :proposal.unassigned named {} in the shown proposal",
                id_text(id)
            )
        });
    }
    // `proposal_view` paints no package at all when a proposal's layers and
    // leftovers are both empty; it leaves the source packages in place, so
    // the source tree is then what `compile_view` shows too.
    in_source(doc, id).ok_or_else(|| {
        format!(
            "no class or namespace component {} in the shown diagram",
            id_text(id)
        )
    })
}
