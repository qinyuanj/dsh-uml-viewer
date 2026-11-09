//! The original `sites.py` leaf rules shared by Python and TypeScript.
//!
//! Every language but Clojure walks the syntax tree's leaves with the same
//! rule: a token under a binary parent is swapped through the Java table (plus
//! the bitwise, shift and remainder swaps), `!`/`not` and unary `-` are
//! deleted, booleans swap and the literals `0` and `1` flip. The language
//! modules supply only their grammar-specific exceptions.
use quality_core::grammar::Node;

/// `_BINARY`: Java's decisions, spelled for every language that uses them.
pub fn binary(token: &str) -> Option<&'static str> {
    Some(match token {
        "+" => "-",
        "-" => "+",
        "*" => "/",
        "/" => "*",
        ">" => ">=",
        ">=" => ">",
        "<" => "<=",
        "<=" => "<",
        "==" => "!=",
        "!=" => "==",
        "===" => "!==",
        "!==" => "===",
        "&&" => "||",
        "||" => "&&",
        "and" => "or",
        "or" => "and",
        _ => return None,
    })
}

/// `_BITWISE_BINARY`: the swaps TypeScript, Rust and Python add.
pub fn bitwise(token: &str) -> Option<&'static str> {
    Some(match token {
        "^" | "&" => "|",
        "|" => "&",
        "<<" => ">>",
        ">>" => "<<",
        "%" => "*",
        _ => return None,
    })
}

/// `_category`: the original falls back to "arithmetic" (so `%` is one).
pub fn category(token: &str) -> &'static str {
    match token {
        ">" | ">=" | "<" | "<=" => "comparison",
        "==" | "!=" | "===" | "!==" | "~=" => "equality",
        "^" | "&" | "|" | "<<" | ">>" | ">>>" => "bitwise",
        "&&" | "||" | "and" | "or" => "logical",
        _ => "arithmetic",
    }
}

const BINARY_PARENTS: &[&str] = &[
    "binary_expression",
    "binary_operator",
    "comparison_operator",
    "boolean_operator",
];
const UNARY_PARENTS: &[&str] = &["unary_expression", "unary_operator", "not_operator"];
const NUMBERS: &[&str] = &[
    "decimal_integer_literal",
    "integer_literal",
    "int_literal",
    "integer",
    "number",
    "num_lit",
];

/// One leaf's site under `_operator` then `_literal`; `binary_mutant` is the
/// language's binary table, already refusing tokens it must not mutate.
pub fn leaf(
    node: Node,
    token: &str,
    binary_mutant: impl Fn(&str) -> Option<&'static str>,
) -> Option<(String, &'static str)> {
    let parent = node.parent().map_or("", |parent| parent.kind());
    let mut found = None;
    if BINARY_PARENTS.contains(&parent)
        && let Some(mutant) = binary_mutant(token)
    {
        found = Some((mutant.to_string(), category(token)));
    }
    if found.is_none()
        && matches!(token, "!" | "not")
        && (UNARY_PARENTS.contains(&parent) || parent == "comparison_operator")
    {
        found = Some((String::new(), "unary"));
    }
    if found.is_none() && token == "-" && UNARY_PARENTS.contains(&parent) {
        found = Some((String::new(), "unary"));
    }
    if found.is_none() && matches!(node.kind(), "true" | "false") {
        let swapped = match token {
            "true" => Some("false"),
            "false" => Some("true"),
            "True" => Some("False"),
            "False" => Some("True"),
            _ => None,
        };
        found = swapped.map(|mutant| (mutant.to_string(), "boolean"));
    }
    if found.is_none() && NUMBERS.contains(&node.kind()) && matches!(token, "0" | "1") {
        let flipped = if token == "0" { "1" } else { "0" };
        found = Some((flipped.to_string(), "constant"));
    }
    // `_add` drops a replacement that equals the original.
    found.filter(|(mutant, _)| mutant != token)
}
