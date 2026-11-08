//! The forms each language compares: dryer's `extract.py`, ported per
//! language. Clojure compares every top-level list except `ns`; the other
//! languages compare the functions and methods crapper scores, with each
//! language's own picker. These pickers are deliberately not unified.

use crate::clojure;
use crate::normalize::normalized_shape;
use quality_core::grammar::{Grammar, Node, children, end_line, preorder, start_line, text};
use quality_core::language::Language;
use std::collections::HashSet;

/// One normalized form ready to compare (dryer `model.Entry`).
#[derive(Clone)]
pub(crate) struct Entry {
    pub language: Language,
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub nodes: usize,
    pub fingerprints: HashSet<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
}

const TS_FUNCTION: &[&str] = &[
    "function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
];
const DEFINITION_PARENTS: &[&str] = &[
    "variable_declarator",
    "public_field_definition",
    "property_definition",
];
const LUA_FUNCTION: &[&str] = &["function_declaration", "function_definition"];

fn grammar(language: Language, path: &str) -> Option<Grammar> {
    match language {
        Language::Python => Some(Grammar::Python),
        Language::TypeScript => Some(if path.ends_with(".tsx") {
            Grammar::Tsx
        } else {
            Grammar::TypeScript
        }),
        Language::Lua => Some(Grammar::Lua),
        Language::Rust => Some(Grammar::Rust),
        Language::Go => Some(Grammar::Go),
        Language::Java => Some(Grammar::Java),
        Language::Clojure => None,
    }
}

fn has(node: Node, kinds: &[&str]) -> bool {
    children(node)
        .iter()
        .any(|child| kinds.contains(&child.kind()))
}

fn inside(node: Node, kinds: &[&str]) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if kinds.contains(&parent.kind()) {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// A function inside another function; a class in between keeps the method.
fn python_nested(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "class_definition" => return false,
            "function_definition" => return true,
            _ => current = parent.parent(),
        }
    }
    false
}

fn rust_in_tests(source: &str, node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "mod_item"
            && let Some(name) = children(parent)
                .into_iter()
                .find(|child| child.kind() == "identifier")
            && text(source, name) == "tests"
        {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Only the immediate `block` parent is skipped, and only `mod tests`; a
/// `#[test]` or `cfg(test)` function is still a candidate here, unlike crap.
fn rust_nodes<'t>(source: &str, root: Node<'t>) -> Vec<Node<'t>> {
    preorder(root)
        .into_iter()
        .filter(|node| {
            node.kind() == "function_item"
                && has(*node, &["block"])
                && node.parent().is_none_or(|parent| parent.kind() != "block")
                && !rust_in_tests(source, *node)
        })
        .collect()
}

/// Methods with their own body. A method is not descended into, so a local
/// class inside it never contributes its own method.
fn java_nodes(root: Node) -> Vec<Node> {
    let mut found = Vec::new();
    fn visit<'t>(node: Node<'t>, found: &mut Vec<Node<'t>>) {
        if node.kind() == "method_declaration" {
            let nested = inside(
                node,
                &[
                    "method_declaration",
                    "constructor_declaration",
                    "lambda_expression",
                ],
            );
            if has(node, &["block"]) && !nested {
                found.push(node);
            }
            return;
        }
        for child in children(node) {
            visit(child, found);
        }
    }
    visit(root, &mut found);
    found
}

fn go_nodes(root: Node) -> Vec<Node> {
    preorder(root)
        .into_iter()
        .filter(|node| {
            matches!(node.kind(), "function_declaration" | "method_declaration")
                && has(*node, &["block"])
        })
        .collect()
}

fn python_nodes(root: Node) -> Vec<Node> {
    preorder(root)
        .into_iter()
        .filter(|node| node.kind() == "function_definition" && !python_nested(*node))
        .collect()
}

fn typescript_nodes(root: Node) -> Vec<Node> {
    let mut found = Vec::new();
    for node in preorder(root) {
        let picked = match node.kind() {
            "function_declaration" | "method_definition" => {
                (has(node, &["statement_block"]) && !inside(node, TS_FUNCTION)).then_some(node)
            }
            "arrow_function" | "function_expression" => node.parent().filter(|parent| {
                DEFINITION_PARENTS.contains(&parent.kind()) && !inside(*parent, TS_FUNCTION)
            }),
            _ => None,
        };
        if let Some(picked) = picked {
            found.push(picked);
        }
    }
    found
}

/// Function declarations and function expressions assigned to a name; a
/// returned or argument-position function is not an entry.
fn lua_nodes(root: Node) -> Vec<Node> {
    preorder(root)
        .into_iter()
        .filter(|node| {
            if !LUA_FUNCTION.contains(&node.kind()) || inside(*node, LUA_FUNCTION) {
                return false;
            }
            if node.kind() == "function_declaration" {
                return true;
            }
            node.parent().is_some_and(|values| {
                values.kind() == "expression_list"
                    && values
                        .parent()
                        .is_some_and(|parent| parent.kind() == "assignment_statement")
            })
        })
        .collect()
}

fn nodes<'t>(language: Language, source: &str, root: Node<'t>) -> Vec<Node<'t>> {
    match language {
        Language::Java => java_nodes(root),
        Language::Go => go_nodes(root),
        Language::Python => python_nodes(root),
        Language::TypeScript => typescript_nodes(root),
        Language::Rust => rust_nodes(source, root),
        Language::Lua => lua_nodes(root),
        Language::Clojure => Vec::new(),
    }
}

/// The entries of one file, plus a Clojure reader warning
/// (`dryer.extract.entries_in_source`). `path` picks the TypeScript grammar.
pub(crate) fn entries_in_source(
    language: Language,
    source: &str,
    path: &str,
    file: &str,
) -> (Vec<Entry>, Option<String>) {
    if language == Language::Clojure {
        let (forms, warning) = clojure::read_source(source);
        let entries = forms
            .iter()
            .filter(|form| clojure::is_candidate_form(form))
            .map(|form| {
                let normalized = clojure::normalize(form, false);
                let mut fingerprints = HashSet::new();
                let nodes = normalized.collect(&mut fingerprints);
                Entry {
                    language,
                    file: file.to_string(),
                    start_line: form.line(),
                    end_line: clojure::max_line(form),
                    nodes,
                    fingerprints,
                    namespace: None,
                    name: None,
                }
            })
            .collect();
        return (entries, warning);
    }
    let Some(grammar) = grammar(language, path) else {
        return (Vec::new(), None);
    };
    let tree = grammar.parse(source);
    let entries = nodes(language, source, tree.root_node())
        .into_iter()
        .filter_map(|node| {
            let (nodes, fingerprints) = normalized_shape(node, source, Some(language))?;
            Some(Entry {
                language,
                file: file.to_string(),
                start_line: start_line(node),
                end_line: end_line(node),
                nodes,
                fingerprints,
                namespace: None,
                name: None,
            })
        })
        .collect();
    (entries, None)
}
