//! The report paths and texts shared by crapper and mutator: `root.glob`
//! with literal segments, at most one `**`, and files in name order.
use crate::paths::{posix, resolve};
use std::fs;
use std::path::{Path, PathBuf};

/// `root.glob(pattern)` for patterns of literal segments with at most one
/// `**`; directory entries are visited in name order.
pub(crate) fn glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    if pattern.starts_with("*/") || pattern.starts_with("*/*/") {
        let depth = usize::from(pattern.starts_with("*/*/")) + 1;
        let rest = pattern.splitn(depth + 1, '/').last().unwrap();
        let mut found = Vec::new();
        fn children_at(root: &Path, depth: usize, found: &mut Vec<PathBuf>) {
            if depth == 0 {
                found.push(root.to_path_buf());
                return;
            }
            let Ok(entries) = fs::read_dir(root) else {
                return;
            };
            let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            entries.sort();
            for entry in entries {
                if entry.is_dir() {
                    children_at(&entry, depth - 1, found);
                }
            }
        }
        children_at(root, depth, &mut found);
        return found
            .into_iter()
            .flat_map(|directory| glob(&directory, rest))
            .collect();
    }
    let Some((prefix, rest)) = pattern.split_once("/**/") else {
        let path = root.join(pattern);
        return if path.exists() {
            vec![path]
        } else {
            Vec::new()
        };
    };
    let mut found = Vec::new();
    fn descend(directory: &Path, rest: &str, found: &mut Vec<PathBuf>) {
        let candidate = directory.join(rest);
        if candidate.exists() {
            found.push(candidate);
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        dirs.sort();
        for dir in dirs {
            descend(&dir, rest, found);
        }
    }
    let base = root.join(prefix);
    if base.is_dir() {
        descend(&base, rest, &mut found);
    }
    found
}

/// The text of each report once, in pattern order.
pub(crate) fn report_texts(root: &Path, patterns: &[&str]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut texts = Vec::new();
    for path in patterns.iter().flat_map(|pattern| glob(root, pattern)) {
        let key = posix(&resolve(&path));
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if !path.is_file() {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if !text.is_empty() {
                texts.push(text);
            }
        }
    }
    texts
}
