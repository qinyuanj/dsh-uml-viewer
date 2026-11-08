use crate::normalize::shape;
use quality_core::functions::functions_in_file;
use quality_core::grammar::{Grammar, Node, children, end_line, read_source, start_line, text};
use quality_core::language::Language;
use quality_core::paths::relative_posix;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct Span {
    pub file: String,
    pub start: usize,
    pub end: usize,
    pub namespace: Option<String>,
    pub name: Option<String>,
}
pub(crate) struct Entry {
    span: Span,
    language: Language,
    nodes: usize,
    fingerprints: HashSet<String>,
}
pub(crate) struct Candidate {
    pub score: f64,
    pub language: Language,
    pub left: Span,
    pub right: Span,
    pub left_nodes: usize,
    pub right_nodes: usize,
}
fn nodes<'t>(root: Node<'t>, source: &str) -> Vec<Node<'t>> {
    quality_core::grammar::preorder(root)
        .into_iter()
        .filter(|node| {
            if node.kind() != "function_item"
                || !children(*node).iter().any(|n| n.kind() == "block")
            {
                return false;
            }
            if node.parent().is_some_and(|parent| parent.kind() == "block") {
                return false;
            }
            let mut parent = node.parent();
            while let Some(current) = parent {
                if current.kind() == "mod_item"
                    && let Some(name) = children(current)
                        .into_iter()
                        .find(|n| n.kind() == "identifier")
                    && text(source, name) == "tests"
                {
                    return false;
                }
                parent = current.parent();
            }
            true
        })
        .collect()
}
pub(crate) fn scan(
    files: &[PathBuf],
    root: &Path,
    min_lines: usize,
    min_nodes: usize,
) -> io::Result<Vec<Entry>> {
    let mut found = Vec::new();
    for path in files {
        let language = quality_core::language::language_of(path).expect("selected language");
        if language != Language::Rust {
            return Err(io::Error::other(format!(
                "dry does not support {} yet (qinyuanj/dsh-uml-viewer#7)",
                language.name()
            )));
        }
        let source = read_source(path)?;
        let file = relative_posix(path, root).expect("source inside project root");
        let tree = Grammar::Rust.parse(&source);
        let functions = functions_in_file(language, &source, &file, root);
        for node in nodes(tree.root_node(), &source) {
            let (nodes, fingerprints) = shape(node, &source);
            let (start, end) = (start_line(node), end_line(node));
            if end + 1 - start < min_lines || nodes < min_nodes {
                continue;
            }
            let owner = functions
                .iter()
                .filter(|f| f.start_line == start)
                .min_by_key(|f| (f.end_line - f.start_line, f.start_line))
                .or_else(|| {
                    functions
                        .iter()
                        .filter(|f| f.start_line <= start && end <= f.end_line)
                        .min_by_key(|f| (f.end_line - f.start_line, f.start_line))
                });
            found.push(Entry {
                span: Span {
                    file: file.clone(),
                    start,
                    end,
                    namespace: owner.map(|f| f.namespace.clone()),
                    name: owner.map(|f| f.name.clone()),
                },
                language,
                nodes,
                fingerprints,
            });
        }
    }
    Ok(found)
}
pub(crate) fn duplicates(entries: Vec<Entry>, threshold: f64) -> Vec<Candidate> {
    let mut found = Vec::new();
    for (i, left) in entries.iter().enumerate() {
        for right in &entries[i + 1..] {
            if left.language != right.language
                || (left.span.file == right.span.file
                    && left.span.start == right.span.start
                    && left.span.end == right.span.end)
            {
                continue;
            }
            let intersection = left.fingerprints.intersection(&right.fingerprints).count();
            let union = left.fingerprints.union(&right.fingerprints).count();
            let score = if union == 0 {
                0.0
            } else {
                intersection as f64 / union as f64
            };
            // Python skips only `score < threshold`; NaN therefore keeps pairs.
            if score.partial_cmp(&threshold) != Some(std::cmp::Ordering::Less) {
                found.push(Candidate {
                    score,
                    language: left.language,
                    left: left.span.clone(),
                    right: right.span.clone(),
                    left_nodes: left.nodes,
                    right_nodes: right.nodes,
                });
            }
        }
    }
    found.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.language.cmp(&b.language))
            .then_with(|| a.left.file.cmp(&b.left.file))
            .then_with(|| a.left.start.cmp(&b.left.start))
            .then_with(|| a.right.file.cmp(&b.right.file))
            .then_with(|| a.right.start.cmp(&b.right.start))
    });
    found
}
