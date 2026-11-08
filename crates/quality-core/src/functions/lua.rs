//! Lua functions and cyclomatic complexity (pinned crapper `lua.py`).
//! Nested functions stay within their enclosing entry; only bound closures
//! become entries. The namespace is the module's require path.
use super::Function;
use crate::grammar::{Grammar, children, end_line, start_line, text};
use crate::language::Language;
use std::path::Path;
use tree_sitter::Node;

fn namespace(path: &str) -> String {
    let mut path = path;
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    for root in ["src/lua/", "src/", "lua/"] {
        if let Some(rest) = path.strip_prefix(root) {
            path = rest;
            break;
        }
    }
    path.strip_suffix(".lua")
        .unwrap_or(path)
        .strip_suffix("/init")
        .unwrap_or(path.strip_suffix(".lua").unwrap_or(path))
        .replace('/', ".")
}

fn binary_logic(node: Node) -> bool {
    node.kind() == "binary_expression"
        && children(node)
            .iter()
            .any(|child| matches!(child.kind(), "and" | "or"))
}

pub(super) fn functions(source: &str, path: &str, _root: &Path) -> Vec<Function> {
    let tree = Grammar::Lua.parse(source);
    assert!(
        !tree.root_node().has_error(),
        "Lua source does not parse: {path}"
    );
    let nodes = crate::grammar::preorder(tree.root_node());
    let mut candidates: Vec<Node> = nodes
        .iter()
        .copied()
        .filter(|node| matches!(node.kind(), "function_declaration" | "function_definition"))
        .collect();
    candidates.sort_by_key(|node| (node.start_byte(), std::cmp::Reverse(node.end_byte())));
    let mut open_ends: Vec<usize> = Vec::new();
    let mut result = Vec::new();
    for node in candidates {
        while open_ends
            .last()
            .is_some_and(|end| *end <= node.start_byte())
        {
            open_ends.pop();
        }
        if !open_ends.is_empty() {
            open_ends.push(node.end_byte());
            continue;
        }
        open_ends.push(node.end_byte());
        let (name, header) = if node.kind() == "function_declaration" {
            let Some(name) = node.child_by_field_name("name") else {
                continue;
            };
            (text(source, name), node)
        } else {
            let Some(values) = node.parent() else {
                continue;
            };
            let Some(statement) = values.parent() else {
                continue;
            };
            if values.kind() != "expression_list" || statement.kind() != "assignment_statement" {
                continue;
            }
            let Some(targets) = crate::grammar::child_of_kind(statement, &["variable_list"]) else {
                continue;
            };
            let value_nodes: Vec<Node> = {
                let mut cursor = values.walk();
                values.named_children(&mut cursor).collect()
            };
            let names: Vec<Node> = {
                let mut cursor = targets.walk();
                targets.named_children(&mut cursor).collect()
            };
            let Some(index) = value_nodes.iter().position(|candidate| *candidate == node) else {
                continue;
            };
            let Some(target) = names.get(index) else {
                continue;
            };
            let declaration = statement
                .parent()
                .filter(|n| n.kind() == "variable_declaration");
            (text(source, *target), declaration.unwrap_or(statement))
        };
        let complexity = 1 + nodes
            .iter()
            .filter(|decision| {
                decision.start_byte() >= node.start_byte()
                    && decision.end_byte() <= node.end_byte()
                    && (matches!(
                        decision.kind(),
                        "if_statement"
                            | "elseif_statement"
                            | "while_statement"
                            | "repeat_statement"
                            | "for_statement"
                    ) || binary_logic(**decision))
            })
            .count() as u32;
        result.push(Function::new(
            name.to_string(),
            namespace(path),
            complexity,
            start_line(header),
            end_line(node),
            path.to_string(),
            Language::Lua,
        ));
    }
    result
}
