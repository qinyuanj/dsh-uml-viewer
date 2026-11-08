//! TypeScript and TSX functions, methods and structural complexity.

use std::path::Path;

use tree_sitter::Node;

use super::Function;
use crate::grammar::{
    Grammar, binary_logic, child_of_kind, children, end_line, preorder, start_line, text,
};
use crate::language::Language;

const DECISIONS: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "ternary_expression",
    "switch_case",
    "switch_default",
];
const CLASSES: &[&str] = &["class_declaration", "abstract_class_declaration"];
const SKIP: &[&str] = &[
    "class_declaration",
    "abstract_class_declaration",
    "interface_declaration",
];
const FUNCTION_TYPES: &[&str] = &[
    "function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
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
    let relative = [".tsx", ".mts", ".cts", ".ts"]
        .iter()
        .find_map(|suffix| relative.strip_suffix(suffix))
        .unwrap_or(relative);
    relative.replace('/', ".")
}

fn has_function_ancestor(node: Node) -> bool {
    let mut parent = node.parent();
    while let Some(owner) = parent {
        if FUNCTION_TYPES.contains(&owner.kind()) {
            return true;
        }
        parent = owner.parent();
    }
    false
}

fn method_namespace(source: &str, node: Node, module: &str) -> Option<String> {
    let mut classes = Vec::new();
    let mut parent = node.parent();
    while let Some(owner) = parent {
        if CLASSES.contains(&owner.kind())
            && let Some(ident) = child_of_kind(owner, &["type_identifier", "identifier"])
        {
            classes.push(text(source, ident));
        }
        parent = owner.parent();
    }
    if classes.is_empty() {
        return None;
    }
    classes.reverse();
    Some(
        std::iter::once(module)
            .chain(classes)
            .collect::<Vec<_>>()
            .join("."),
    )
}

fn complexity(node: Node) -> u32 {
    let mut score = 1;
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if SKIP.contains(&current.kind()) {
            continue;
        }
        score +=
            u32::from(DECISIONS.contains(&current.kind()) || binary_logic(current, &["&&", "||"]));
        stack.extend(children(current));
    }
    score
}

fn entry(
    source: &str,
    path: &str,
    name: Node,
    namespace: String,
    span: Node,
    body: Node,
) -> Function {
    Function {
        name: text(source, name).to_string(),
        namespace,
        complexity: complexity(body),
        start_line: start_line(span),
        end_line: end_line(span),
        path: path.to_string(),
        language: Language::TypeScript,
    }
}

pub(super) fn functions(source: &str, path: &str, root: &Path) -> Vec<Function> {
    let grammar = if path.ends_with(".tsx") {
        Grammar::Tsx
    } else {
        Grammar::TypeScript
    };
    let tree = grammar.parse(source);
    let module = module_namespace(path, root);
    let mut found = Vec::new();
    for node in preorder(tree.root_node()) {
        match node.kind() {
            "function_declaration" => {
                if has_function_ancestor(node)
                    || child_of_kind(node, &["statement_block"]).is_none()
                {
                    continue;
                }
                if let Some(ident) = child_of_kind(node, &["identifier"]) {
                    found.push(entry(source, path, ident, module.clone(), node, node));
                }
            }
            "method_definition" => {
                if child_of_kind(node, &["statement_block"]).is_none() {
                    continue;
                }
                if let Some(ident) = child_of_kind(node, &["property_identifier", "identifier"])
                    && let Some(namespace) = method_namespace(source, node, &module)
                {
                    found.push(entry(source, path, ident, namespace, node, node));
                }
            }
            "lexical_declaration" => {
                if has_function_ancestor(node) {
                    continue;
                }
                for declarator in children(node) {
                    if declarator.kind() != "variable_declarator" {
                        continue;
                    }
                    if let Some(ident) = child_of_kind(declarator, &["identifier"])
                        && let Some(value) =
                            child_of_kind(declarator, &["arrow_function", "function_expression"])
                    {
                        found.push(entry(
                            source,
                            path,
                            ident,
                            module.clone(),
                            declarator,
                            value,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    found
}
