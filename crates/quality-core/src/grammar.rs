//! The tree-sitter language table and the node helpers every tool reads
//! syntax trees with. Grammar crates are pinned to the revisions the Python
//! tools parse with (tree-sitter-language-pack 1.21.0), so a tree here is
//! node for node the tree there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use tree_sitter::{Node, Parser, Tree};

/// A tree-sitter grammar. Languages without one (Clojure in crapper) are
/// read by hand-written scanners.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Grammar {
    Rust,
    Go,
    Java,
}

impl Grammar {
    pub fn language(self) -> tree_sitter::Language {
        match self {
            Grammar::Rust => tree_sitter_rust::LANGUAGE.into(),
            Grammar::Go => tree_sitter_go::LANGUAGE.into(),
            Grammar::Java => tree_sitter_java::LANGUAGE.into(),
        }
    }

    /// Parse `source` (UTF-8 bytes, as the Python tools encode it).
    pub fn parse(self, source: &str) -> Tree {
        thread_local! {
            static PARSERS: RefCell<HashMap<Grammar, Parser>> = RefCell::new(HashMap::new());
        }
        PARSERS.with(|parsers| {
            let mut parsers = parsers.borrow_mut();
            let parser = parsers.entry(self).or_insert_with(|| {
                let mut parser = Parser::new();
                parser
                    .set_language(&self.language())
                    .expect("a pinned grammar matches the tree-sitter ABI");
                parser
            });
            parser
                .parse(source, None)
                .expect("parsing without a timeout")
        })
    }
}

/// `Path.read_text(encoding="utf-8", errors="replace")`: invalid UTF-8
/// replaced, and universal newlines (`\r\n` and `\r` read as `\n`).
pub fn read_source(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(universal_newlines(&String::from_utf8_lossy(&bytes)))
}

/// `\r\n` and lone `\r` as `\n`.
pub fn universal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The source text of `node`.
pub fn text<'a>(source: &'a str, node: Node) -> &'a str {
    &source[node.start_byte()..node.end_byte()]
}

/// 1-based first line of `node`.
pub fn start_line(node: Node) -> usize {
    node.start_position().row + 1
}

/// 1-based last line of `node`; a node ending at column 0 ends on the
/// line before.
pub fn end_line(node: Node) -> usize {
    let end = node.end_position();
    if end.column == 0 {
        start_line(node).max(end.row)
    } else {
        end.row + 1
    }
}

/// All children, named and anonymous, in order.
pub fn children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// The first child whose kind is one of `kinds`.
pub fn child_of_kind<'t>(node: Node<'t>, kinds: &[&str]) -> Option<Node<'t>> {
    children(node)
        .into_iter()
        .find(|child| kinds.contains(&child.kind()))
}

/// `node` and its descendants, in preorder.
pub fn preorder(node: Node) -> Vec<Node> {
    let mut found = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        found.push(current);
        let mut kids = children(current);
        kids.reverse();
        stack.extend(kids);
    }
    found
}

/// The first node of `kind` in `node`'s subtree, `node` included.
pub fn first_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    preorder(node).into_iter().find(|item| item.kind() == kind)
}

/// McCabe complexity: 1 plus each decision in the subtree.
pub fn complexity(node: Node, is_decision: &dyn Fn(Node) -> bool) -> u32 {
    1 + preorder(node)
        .into_iter()
        .filter(|item| is_decision(*item))
        .count() as u32
}

/// A `binary_expression` whose operator is one of `operators`.
pub fn binary_logic(node: Node, operators: &[&str]) -> bool {
    node.kind() == "binary_expression"
        && children(node)
            .iter()
            .any(|child| operators.contains(&child.kind()))
}
