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

fn pairs(root: &Path) -> Vec<Candidate> {
    crate::scan::duplicates(scan(root, 1, 1), 0.99)
}

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
