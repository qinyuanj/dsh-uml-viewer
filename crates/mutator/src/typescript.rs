//! TypeScript mutation: sites, privacy, the per-file test plan and coverage.
use crate::operators;
use quality_core::grammar::{Grammar, Node};
use std::path::Path;

/// `_grammar`: `.tsx` parses with the TSX grammar, every other suffix with
/// TypeScript's.
pub fn grammar(path: &Path) -> Grammar {
    if path.to_string_lossy().ends_with(".tsx") {
        Grammar::Tsx
    } else {
        Grammar::TypeScript
    }
}

/// Python `re.search(r"\bprivate\b", header)` with Unicode word characters.
fn mentions_private(header: &str) -> bool {
    let word = |ch: char| ch.is_alphanumeric() || ch == '_';
    header.match_indices("private").any(|(at, _)| {
        let before = header[..at].chars().next_back();
        let after = header[at + "private".len()..].chars().next();
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// `_typescript_private`: an ECMAScript `#name` or the `private` keyword in
/// the declaration's first lines.
pub fn private(name: &str, header: &str) -> bool {
    name.starts_with('#') || mentions_private(header)
}

/// The leaf's site under the original's TypeScript rules: `>>>` has one
/// mutant, `<<`.
pub fn site(node: Node, token: &str) -> Option<(String, &'static str)> {
    operators::leaf(node, token, |token| {
        if token == ">>>" {
            return Some("<<");
        }
        operators::bitwise(token).or_else(|| operators::binary(token))
    })
}
