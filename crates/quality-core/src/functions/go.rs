//! Go functions and cyclomatic complexity, following crap4go.
//!
//! Package namespaces are import paths from the nearest go.mod; methods
//! append the receiver type. Function literals contribute their decisions
//! to the enclosing declaration rather than becoming separate functions.

use std::fs;
use std::path::Path;

use tree_sitter::Node;

use super::Function;
use crate::grammar::{
    Grammar, binary_logic, child_of_kind, complexity, end_line, first_of_kind, preorder,
    start_line, text,
};
use crate::language::Language;
use crate::paths::{relative_posix, resolve};

const DECISIONS: &[&str] = &[
    "if_statement",
    "for_statement",
    "expression_case",
    "type_case",
    "communication_case",
    "default_case",
];

fn is_decision(node: Node) -> bool {
    DECISIONS.contains(&node.kind()) || binary_logic(node, &["&&", "||"])
}

fn name_and_namespace(source: &str, node: Node, package: &str) -> Option<(String, String)> {
    if node.kind() == "function_declaration" {
        let name = text(source, child_of_kind(node, &["identifier"])?);
        return (!name.is_empty()).then(|| (name.to_string(), package.to_string()));
    }
    let name = text(source, child_of_kind(node, &["field_identifier"])?);
    let receiver = child_of_kind(node, &["parameter_list"])?;
    let receiver = text(source, first_of_kind(receiver, "type_identifier")?);
    if name.is_empty() || receiver.is_empty() {
        return None;
    }
    Some((name.to_string(), format!("{package}.{receiver}")))
}

/// The nearest go.mod shadows all outer ones, including when it has no
/// module directive. Like the Python scanner, keep the directive's value
/// verbatim rather than interpreting Go string literals or comments.
fn package_namespace(path: &str, root: &Path, package: &str) -> String {
    let absolute = resolve(&root.join(path));
    let Some(directory) = absolute.parent() else {
        return package.to_string();
    };
    for ancestor in directory.ancestors() {
        let candidate = ancestor.join("go.mod");
        if !candidate.is_file() {
            continue;
        }
        let Ok(contents) = fs::read_to_string(candidate) else {
            return package.to_string();
        };
        // Python str.splitlines() also recognizes these Unicode separators.
        for line in contents.split([
            '\n', '\r', '\u{000b}', '\u{000c}', '\u{001c}', '\u{001d}', '\u{001e}', '\u{0085}',
            '\u{2028}', '\u{2029}',
        ]) {
            if let Some(module) = line.trim().strip_prefix("module ") {
                let module = module.trim();
                let relative = relative_posix(directory, ancestor)
                    .expect("the module directory is an ancestor of the source directory");
                return if relative == "." {
                    module.to_string()
                } else {
                    format!("{module}/{relative}")
                };
            }
        }
        return package.to_string();
    }
    package.to_string()
}

/// Functions with bodies, in source order. `root` anchors relative paths;
/// the original `path` is retained in each scored function.
pub fn functions(source: &str, path: &str, root: &Path) -> Vec<Function> {
    let tree = Grammar::Go.parse(source);
    let package = first_of_kind(tree.root_node(), "package_identifier")
        .map(|node| text(source, node))
        .unwrap_or("");
    let package = package_namespace(path, root, package);
    preorder(tree.root_node())
        .into_iter()
        .filter(|node| {
            matches!(node.kind(), "function_declaration" | "method_declaration")
                && child_of_kind(*node, &["block"]).is_some()
        })
        .filter_map(|node| {
            let (name, namespace) = name_and_namespace(source, node, &package)?;
            Some(Function::new(
                name,
                namespace,
                complexity(node, &is_decision),
                start_line(node),
                end_line(node),
                path.to_string(),
                Language::Go,
            ))
        })
        .collect()
}
