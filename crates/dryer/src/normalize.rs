//! Python dryer's normalized tree and structural fingerprint set.
use quality_core::grammar::{Node, children, text};
use quality_core::snapshot::string;
use std::collections::HashSet;

#[derive(Clone)]
enum Form {
    Keyword(String),
    Name(String),
    Vector(Vec<Form>),
}

impl Form {
    fn printed(&self) -> String {
        match self {
            Self::Keyword(s) => format!(":{s}"),
            Self::Name(s) => string(s),
            Self::Vector(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(Form::printed)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        }
    }

    fn collect(&self, found: &mut HashSet<String>) -> usize {
        found.insert(self.printed());
        1 + match self {
            Self::Vector(items) => items.iter().map(|item| item.collect(found)).sum(),
            _ => 0,
        }
    }
}

fn keyword(name: &str) -> Form {
    Form::Keyword(name.to_string())
}
fn symbol(source: &str, node: Node) -> Form {
    Form::Vector(vec![
        keyword("symbol"),
        Form::Name(text(source, node).to_string()),
    ])
}
fn identifier(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "shorthand_property_identifier"
            | "shorthand_property_identifier_pattern"
            | "package_identifier"
    )
}
fn attribute(kind: &str) -> bool {
    matches!(
        kind,
        "attribute"
            | "member_expression"
            | "selector_expression"
            | "field_access"
            | "field_expression"
            | "dot_index_expression"
            | "method_index_expression"
    )
}
fn path(kind: &str) -> bool {
    matches!(
        kind,
        "scoped_identifier" | "scoped_type_identifier" | "generic_type"
    )
}
fn type_args(kind: &str) -> bool {
    matches!(kind, "type_arguments" | "type_parameters")
}
fn operator(kind: &str) -> bool {
    matches!(
        kind,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "**"
            | "=="
            | "!="
            | "<"
            | ">"
            | "<="
            | ">="
            | "==="
            | "!=="
            | "&&"
            | "||"
            | "&"
            | "|"
            | "^"
            | "<<"
            | ">>"
            | ">>>"
            | "+="
            | "-="
            | "*="
            | "/="
            | "%="
            | "&="
            | "|="
            | "^="
            | "<<="
            | ">>="
            | ">>>="
            | "!"
            | "~"
            | "++"
            | "--"
            | "and"
            | "or"
            | "not"
            | "??"
            | "?."
            | "="
            | ":="
            | "=>"
            | "?"
    )
}
fn literal(node: Node) -> bool {
    let kind = node.kind();
    matches!(
        kind,
        "string"
            | "string_literal"
            | "interpreted_string_literal"
            | "raw_string_literal"
            | "char_literal"
            | "rune_literal"
            | "character_literal"
            | "number"
            | "integer"
            | "float"
            | "decimal_integer_literal"
            | "hex_integer_literal"
            | "octal_integer_literal"
            | "binary_integer_literal"
            | "decimal_floating_point_literal"
            | "hex_floating_point_literal"
            | "int_literal"
            | "float_literal"
            | "imaginary_literal"
            | "integer_literal"
            | "true"
            | "false"
            | "null"
            | "nil"
            | "none"
            | "undefined"
            | "null_literal"
            | "boolean_literal"
            | "template_string"
            | "regex"
            | "regex_literal"
    ) && !(kind == "string" && children(node).iter().any(|n| n.kind() == "interpolation"))
}
fn named(node: Node) -> Vec<Node> {
    children(node).into_iter().filter(Node::is_named).collect()
}
fn extend<'a>(out: &mut Vec<Form>, nodes: impl IntoIterator<Item = Node<'a>>, source: &str) {
    out.extend(
        nodes
            .into_iter()
            .filter_map(|n| normalize(n, source, false)),
    );
}

fn normalize(node: Node, source: &str, head: bool) -> Option<Form> {
    let kind = node.kind();
    if matches!(
        kind,
        "comment" | "line_comment" | "block_comment" | "documentation_comment" | "doc_comment"
    ) {
        return None;
    }
    if operator(kind) {
        return Some(Form::Vector(vec![
            keyword("symbol"),
            Form::Name(kind.to_string()),
        ]));
    }
    if !node.is_named() {
        return None;
    }
    if identifier(kind) {
        return Some(if head {
            symbol(source, node)
        } else {
            keyword("symbol")
        });
    }
    if literal(node) {
        return Some(keyword("literal"));
    }
    let kids = named(node);
    if kind == "parenthesized_expression" && kids.len() == 1 {
        return normalize(kids[0], source, head);
    }
    if matches!(
        kind,
        "call"
            | "function_call"
            | "call_expression"
            | "method_invocation"
            | "object_creation_expression"
            | "new_expression"
    ) {
        let mut out = vec![keyword(kind)];
        let split = kids
            .iter()
            .position(|n| matches!(n.kind(), "argument_list" | "arguments"))
            .unwrap_or(kids.len());
        let incoming = &kids[..split];
        let rest: Vec<_> = incoming
            .iter()
            .copied()
            .filter(|n| !type_args(n.kind()))
            .collect();
        if let Some((callee, receivers)) = rest.split_last() {
            extend(&mut out, receivers.iter().copied(), source);
            out.push(normalize_callee(*callee, source));
        }
        extend(
            &mut out,
            incoming.iter().copied().filter(|n| type_args(n.kind())),
            source,
        );
        if let Some(args) = kids.get(split) {
            extend(&mut out, [*args], source);
        }
        return Some(Form::Vector(out));
    }
    if attribute(kind) {
        return Some(normalize_attribute(node, source, head));
    }
    let mut out = vec![keyword(kind)];
    extend(&mut out, children(node), source);
    Some(Form::Vector(out))
}

fn normalize_callee(node: Node, source: &str) -> Form {
    if identifier(node.kind()) {
        return symbol(source, node);
    }
    if attribute(node.kind()) {
        return normalize_attribute(node, source, true);
    }
    if path(node.kind()) {
        let mut out = vec![keyword(node.kind())];
        for child in named(node) {
            if identifier(child.kind()) {
                out.push(symbol(source, child));
            } else if path(child.kind()) {
                out.push(normalize_callee(child, source));
            } else {
                extend(&mut out, [child], source);
            }
        }
        return Form::Vector(out);
    }
    // A named callee always normalizes to a form for Rust's grammar.
    normalize(node, source, true).unwrap_or_else(|| keyword("symbol"))
}
fn normalize_attribute(node: Node, source: &str, head: bool) -> Form {
    let mut out = vec![keyword(node.kind())];
    if let Some((name, objects)) = named(node).split_last() {
        extend(&mut out, objects.iter().copied(), source);
        if head {
            out.push(normalize_callee(*name, source));
        } else {
            extend(&mut out, [*name], source);
        }
    }
    Form::Vector(out)
}

pub(crate) fn shape(node: Node, source: &str) -> (usize, HashSet<String>) {
    let mut found = HashSet::new();
    let count = normalize(node, source, false).map_or(0, |form| form.collect(&mut found));
    (count, found)
}
