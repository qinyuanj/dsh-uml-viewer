//! The original dryer pytest cases, ported. Each file mirrors one Python
//! test module of the pinned dryer b3f72ad; the expectations are the
//! original assertions, not values recomputed from this implementation.
//!
//! `test_cli.py` is ported at the binary seam instead
//! (`crates/uml-viewer/tests/{dry.rs,dry_goldens.rs,dry_languages.rs}`),
//! because that is where the CLI contract is observable. `test_platform.py`
//! was ported with #6. `test_guardrails.py` and `test_crapper_link.py` police
//! the Python repository's own source layout and import fallback and have no
//! Rust behaviour to port; see `fixtures/dry/README.md`.

// Test code is the outside harness: the ported cases launch `git` to build
// real repositories, exactly as the original pytest cases do. The module lives
// under `#[cfg(test)]`, so this lift is test-only by construction and takes the
// documented `cfg_attr(test, allow(...))` form instead of a blanket allow.
#![cfg_attr(test, allow(clippy::disallowed_methods))]

mod cli;
mod clojure;
mod languages;
mod names;

use crate::extract::Entry;
use crate::scan::{Candidate, scan_files};
use quality_core::language::Language;
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "dsh-dry-original-{name}-{}-{unique}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    // Selection returns `Path.resolve()`d paths (without Windows' verbatim
    // prefix); the root must be resolved the same way for `relative_to`.
    quality_core::paths::resolve(&root)
}

fn write_source(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// `sorted(path for path in root.rglob("*") if path.is_file())`: every file,
/// not the discovery walk, exactly as the original helpers select.
fn all_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    fn walk(root: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else {
                found.push(path);
            }
        }
    }
    walk(root, &mut found);
    found.sort();
    found
}

fn scan(root: &Path, min_lines: usize, min_nodes: usize) -> Vec<Entry> {
    let files = all_files(root);
    let (entries, warnings) = scan_files(&files, root, min_lines, min_nodes).unwrap();
    assert_eq!(Vec::<String>::new(), warnings);
    entries
}

/// `pairs(root)`: every pair at threshold 0.99 with no size filter.
fn pairs(root: &Path) -> Vec<Candidate> {
    crate::scan::duplicates(scan(root, 1, 1), 0.99)
}

/// `duplicates(root, ...)` with the original defaults (0.82, 4, 20).
fn duplicates(root: &Path, threshold: f64, min_lines: usize, min_nodes: usize) -> Vec<Candidate> {
    crate::scan::duplicates(scan(root, min_lines, min_nodes), threshold)
}

fn forms(language: Language, source: &str, file: &str) -> Vec<Entry> {
    let (entries, warning) = crate::extract::entries_in_source(language, source, file, file);
    assert_eq!(None, warning);
    entries
}

fn spans(language: Language, source: &str, file: &str) -> Vec<(usize, usize)> {
    forms(language, source, file)
        .iter()
        .map(|entry| (entry.start_line, entry.end_line))
        .collect()
}

fn printed_fingerprints(entries: &[Entry]) -> String {
    entries
        .iter()
        .flat_map(|entry| entry.fingerprints.iter().cloned().collect::<Vec<_>>())
        .collect::<Vec<_>>()
        .join(" ")
}
