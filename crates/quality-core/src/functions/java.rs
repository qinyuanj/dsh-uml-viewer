//! Java methods and cyclomatic complexity, following crap4java.
//!
//! Constructors, declarations without bodies and methods nested inside an
//! executable are omitted. Member types keep their own dotted namespace.

use std::path::Path;

use tree_sitter::Node;

use super::Function;
use crate::grammar::{Grammar, binary_logic, child_of_kind, children, end_line, start_line, text};
use crate::language::Language;

const NESTED_TYPES: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];
const EXECUTABLES: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "lambda_expression",
];
const DECISIONS: &[&str] = &[
    "if_statement",
    "for_statement",
    "enhanced_for_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "ternary_expression",
    "switch_label",
];

fn is_decision(node: Node) -> bool {
    DECISIONS.contains(&node.kind()) || binary_logic(node, &["&&", "||"])
}

/// Unlike the generic complexity walk, Java skips nested type declarations
/// and anonymous class bodies. Lambda decisions still belong to the method.
fn method_complexity(node: Node) -> u32 {
    let mut score = 1;
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if NESTED_TYPES.contains(&current.kind()) || current.kind() == "class_body" {
            continue;
        }
        score += u32::from(is_decision(current));
        stack.extend(children(current));
    }
    score
}

fn whitespace(ch: char) -> bool {
    // Python's Unicode \\s also includes these four ASCII separators.
    ch.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&ch)
}

/// The reference uses ^\\s*package\\s+([a-zA-Z_][\\w.]*)\\s*; in multiline
/// mode, rather than the syntax tree. Preserve its line anchoring and its
/// treatment of package-looking text inside comments.
fn package_name(source: &str) -> Option<&str> {
    let starts = std::iter::once(0).chain(source.match_indices('\n').map(|(at, _)| at + 1));
    for at in starts {
        let rest = source[at..].trim_start_matches(whitespace);
        let Some(rest) = rest.strip_prefix("package") else {
            continue;
        };
        if !rest.starts_with(whitespace) {
            continue;
        }
        let rest = rest.trim_start_matches(whitespace);
        if !rest.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_') {
            continue;
        }
        let end = rest
            .find(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '.'))
            .unwrap_or(rest.len());
        if rest[end..].trim_start_matches(whitespace).starts_with(';') {
            return Some(&rest[..end]);
        }
    }
    None
}

fn enclosing_names<'a>(source: &'a str, node: Node) -> Vec<&'a str> {
    let mut names = Vec::new();
    let mut current = node.parent();
    while let Some(owner) = current {
        if NESTED_TYPES.contains(&owner.kind())
            && let Some(ident) = child_of_kind(owner, &["identifier"])
        {
            names.push(text(source, ident));
        }
        current = owner.parent();
    }
    names.reverse();
    names
}

fn record(source: &str, node: Node, package: Option<&str>, path: &str) -> Option<Function> {
    child_of_kind(node, &["block"])?;
    let ident = child_of_kind(node, &["identifier"])?;
    let names = enclosing_names(source, node);
    if names.is_empty() {
        return None;
    }
    let qualify = |separator: &str| {
        let body = names.join(separator);
        match package {
            Some(package) => format!("{package}.{body}"),
            None => body,
        }
    };
    Some(Function {
        name: text(source, ident).to_string(),
        namespace: qualify("."),
        jacoco_class: Some(qualify("$")),
        complexity: method_complexity(node),
        start_line: start_line(node),
        end_line: end_line(node),
        path: path.to_string(),
        language: Language::Java,
    })
}

pub fn functions(source: &str, path: &str, _root: &Path) -> Vec<Function> {
    let tree = Grammar::Java.parse(source);
    let package = package_name(source);
    let mut found = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "method_declaration" {
            found.extend(record(source, node, package, path));
        }
        // No method below an executable can be an entry. Pruning here also
        // prevents local and anonymous classes from contributing entries.
        if EXECUTABLES.contains(&node.kind()) {
            continue;
        }
        let mut kids = children(node);
        kids.reverse();
        stack.extend(kids);
    }
    found
}
