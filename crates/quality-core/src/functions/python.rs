//! Python function entries and structural cyclomatic complexity.

use std::path::Path;

use tree_sitter::Node;

use super::Function;
use crate::grammar::{Grammar, child_of_kind, children, end_line, preorder, start_line, text};
use crate::language::Language;

const DECISIONS: &[&str] = &[
    "if_statement",
    "elif_clause",
    "for_statement",
    "while_statement",
    "except_clause",
    "conditional_expression",
    "case_clause",
    "if_clause",
    "boolean_operator",
];

fn module_namespace(path: &str, root: &Path) -> String {
    let path = path.replace('\\', "/");
    let root = root.to_string_lossy().replace('\\', "/");
    let root = root.trim_end_matches('/');
    let relative = if !root.is_empty() && root != "." {
        path.strip_prefix(&format!("{root}/"))
            .unwrap_or_else(|| path.strip_prefix("./").unwrap_or(&path))
    } else {
        path.strip_prefix("./").unwrap_or(&path)
    };
    let relative = relative.strip_prefix("src/").unwrap_or_else(|| {
        relative
            .split_once("/src/")
            .map_or(relative, |(_, rest)| rest)
    });
    let relative = relative.strip_suffix(".py").unwrap_or(relative);
    let relative = relative.strip_suffix("/__init__").unwrap_or(relative);
    if relative.is_empty() || relative == "__init__" {
        "__init__".to_string()
    } else {
        relative.replace('/', ".")
    }
}

fn is_nested_function(node: Node) -> bool {
    let mut parent = node.parent();
    while let Some(owner) = parent {
        match owner.kind() {
            "function_definition" => return true,
            "class_definition" => return false,
            _ => parent = owner.parent(),
        }
    }
    false
}

fn namespace(source: &str, node: Node, module: &str) -> String {
    let mut classes = Vec::new();
    let mut parent = node.parent();
    while let Some(owner) = parent {
        if owner.kind() == "class_definition"
            && let Some(ident) = child_of_kind(owner, &["identifier"])
        {
            classes.push(text(source, ident));
        }
        parent = owner.parent();
    }
    classes.reverse();
    std::iter::once(module)
        .chain(classes)
        .collect::<Vec<_>>()
        .join(".")
}

fn complexity(node: Node) -> u32 {
    let mut score = 1;
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "class_definition" {
            continue;
        }
        score += u32::from(DECISIONS.contains(&current.kind()));
        stack.extend(children(current));
    }
    score
}

pub(super) fn functions(source: &str, path: &str, root: &Path) -> Vec<Function> {
    let tree = Grammar::Python.parse(source);
    let module = module_namespace(path, root);
    let mut found = Vec::new();
    for node in preorder(tree.root_node()) {
        if node.kind() != "function_definition" || is_nested_function(node) {
            continue;
        }
        let Some(ident) = child_of_kind(node, &["identifier"]) else {
            continue;
        };
        found.push(Function {
            name: text(source, ident).to_string(),
            namespace: namespace(source, node, &module),
            complexity: complexity(node),
            start_line: start_line(node),
            end_line: end_line(node),
            path: path.to_string(),
            language: Language::Python,
        });
    }
    found
}
