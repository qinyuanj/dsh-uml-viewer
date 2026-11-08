//! Read source files and pair the forms that clear the similarity threshold
//! (`dryer/scan.py`).

use crate::discover;
use crate::extract::{Entry, entries_in_source};
use crate::names;
use quality_core::grammar::read_source;
use quality_core::language::Language;
use quality_core::paths::{posix, relative_posix};
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
pub(crate) struct Candidate {
    pub score: f64,
    pub language: Language,
    pub left: Span,
    pub right: Span,
    pub left_nodes: usize,
    pub right_nodes: usize,
}

/// `lines >= min_lines and nodes >= min_nodes`.
pub(crate) fn keep(entry: &Entry, min_lines: usize, min_nodes: usize) -> bool {
    // Written as `end + 1 - start >= min_lines` because a huge count
    // (`int()` accepts arbitrary precision) must not overflow.
    entry.end_line + 1 - entry.start_line >= min_lines && entry.nodes >= min_nodes
}

/// The forms of every selected file, named after crapper's functions, plus
/// one warning per unreadable Clojure tail (`"<file>:<line>: <message>"`).
pub(crate) fn scan_files(
    files: &[PathBuf],
    root: &Path,
    min_lines: usize,
    min_nodes: usize,
) -> io::Result<(Vec<Entry>, Vec<String>)> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for path in files {
        let Some(language) = discover::language_of(path) else {
            continue;
        };
        let source = read_source(path)?;
        let file = relative_posix(path, root).expect("source inside project root");
        let (found, warning) = entries_in_source(language, &source, &posix(path), &file);
        if let Some(warning) = warning {
            warnings.push(format!("{file}:{warning}"));
        }
        let kept: Vec<Entry> = found
            .into_iter()
            .filter(|entry| keep(entry, min_lines, min_nodes))
            .collect();
        entries.extend(names::named(kept, language, &source, &file, root)?);
    }
    Ok((entries, warnings))
}

fn span(entry: &Entry) -> Span {
    Span {
        file: entry.file.clone(),
        start: entry.start_line,
        end: entry.end_line,
        namespace: entry.namespace.clone(),
        name: entry.name.clone(),
    }
}

/// Pair forms of the same language; each unordered pair is reported once.
pub(crate) fn duplicates(entries: Vec<Entry>, threshold: f64) -> Vec<Candidate> {
    let mut found = Vec::new();
    for (i, left) in entries.iter().enumerate() {
        for right in &entries[i + 1..] {
            if left.language != right.language
                || (left.file == right.file
                    && left.start_line == right.start_line
                    && left.end_line == right.end_line)
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
                    left: span(left),
                    right: span(right),
                    left_nodes: left.nodes,
                    right_nodes: right.nodes,
                });
            }
        }
    }
    // Python groups by language before pairing; within one language the order
    // is the same, so the stable sort breaks ties identically.
    found.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.language.name().cmp(b.language.name()))
            .then_with(|| a.left.file.cmp(&b.left.file))
            .then_with(|| a.left.start.cmp(&b.left.start))
            .then_with(|| a.right.file.cmp(&b.right.file))
            .then_with(|| a.right.start.cmp(&b.right.start))
    });
    found
}
