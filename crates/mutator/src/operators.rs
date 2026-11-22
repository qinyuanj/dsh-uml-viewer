use quality_core::grammar::Node;

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
    found.filter(|(mutant, _)| mutant != token)
}
