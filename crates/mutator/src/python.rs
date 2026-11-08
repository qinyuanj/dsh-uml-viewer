//! Python mutation: sites, privacy, the per-file test plan and coverage.
use crate::operators;
use quality_core::grammar::Node;

/// `_python_private`: a leading underscore, except a dunder.
pub fn private(name: &str) -> bool {
    if name.starts_with("__") && name.ends_with("__") {
        return false;
    }
    name.starts_with('_')
}

/// `_python_annotation`: union annotations share `a | b`'s syntax tree.
fn in_annotation(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "type" {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

/// The leaf's site under the original's Python rules.
pub fn site(node: Node, token: &str) -> Option<(String, &'static str)> {
    operators::leaf(node, token, |token| {
        if let Some(mutant) = operators::bitwise(token) {
            return (!in_annotation(node)).then_some(mutant);
        }
        operators::binary(token)
    })
}
