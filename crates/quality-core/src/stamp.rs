//! Coverage freshness compatible with crapper's version-1 stamp.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::language::{SKIP_DIRS, language_of};
use crate::paths::{resolve, shown};

const CACHES: &[&str] = &[
    ".pytest_cache",
    ".hypothesis",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
    ".cpcache",
    ".nyc_output",
    "htmlcov",
];

pub fn stamp_path(root: &Path) -> PathBuf {
    root.join("target/coverage/crapper-stamp.json")
}

// Python json.dumps defaults to ensure_ascii=True, including surrogate pairs.
fn ascii_json(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii() && ch != '\u{7f}' {
            out.push(ch);
        } else {
            let mut units = [0; 2];
            for unit in ch.encode_utf16(&mut units) {
                use std::fmt::Write;
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out
}

fn mtime_nanos(modified: std::time::SystemTime) -> i128 {
    match modified.duration_since(UNIX_EPOCH) {
        Ok(value) => value.as_nanos() as i128,
        Err(value) => -(value.duration().as_nanos() as i128),
    }
}

fn tree(root: &Path, directory: &Path, rows: &mut Vec<(String, u64, i128)>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if path.is_dir() {
            if !kind.is_symlink()
                && !SKIP_DIRS.contains(&name.as_ref())
                && !CACHES.contains(&name.as_ref())
            {
                tree(root, &path, rows);
            }
        } else if name != ".coverage" && !name.starts_with(".coverage.") {
            let Ok(meta) = path.metadata() else { continue };
            // Python falls back to a platform's last-resort timestamp only on
            // an unavailable mtime; standard files use their exact mtime.
            let nanos = meta.modified().map(mtime_nanos).unwrap_or(0);
            rows.push((shown(&path, root), meta.len(), nanos));
        }
    }
}

/// SHA-256 of sorted Python JSON [relative posix path, size, mtime_ns] rows.
/// Includes tests and configuration; excludes tool output and cache artifacts.
pub fn fingerprint(root: &Path) -> String {
    let mut rows = Vec::new();
    tree(root, root, &mut rows);
    rows.sort();
    let mut digest = Sha256::new();
    for (path, size, nanos) in rows {
        let path = ascii_json(&serde_json::to_string(&path).expect("JSON string"));
        digest.update(format!("[{path}, {size}, {nanos}]\n"));
    }
    format!("{:x}", digest.finalize())
}

fn read(root: &Path) -> Option<Value> {
    let data: Value = serde_json::from_slice(&fs::read(stamp_path(root)).ok()?).ok()?;
    if data.get("version")? != 1 {
        return None;
    }
    let languages = data.get("languages")?.as_object()?;
    let files = data.get("files")?.as_object()?;
    (languages.values().all(Value::is_string) && files.values().all(Value::is_array))
        .then_some(data)
}

fn base(root: &Path, command: Option<&str>) -> Value {
    read(root)
        .filter(|data| data.get("command").unwrap_or(&Value::Null) == &json!(command))
        .unwrap_or_else(|| json!({"version": 1, "command": command, "languages": {}, "files": {}}))
}

fn covered(root: &Path, files: &[PathBuf]) -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut found = BTreeMap::new();
    for file in files {
        if let Some(language) = language_of(file) {
            found
                .entry(language.name())
                .or_insert_with(BTreeSet::new)
                .insert(shown(&resolve(file), root));
        }
    }
    found
}

/// Record successful languages, dropping failed/replaced languages. A tree
/// edit during coverage drops every language participating in that run.
/// Stamp publication is best effort and never changes the command's status.
pub fn record(
    root: &Path,
    files: &[PathBuf],
    command: Option<&str>,
    failed: &[&str],
    before: &str,
) {
    let mut data = base(root, command);
    let current = fingerprint(root);
    let settled = before == current;
    if !settled {
        eprintln!("Files changed while coverage ran. Its reports are not stamped as fresh.");
    }
    for (language, paths) in covered(root, files) {
        if failed.contains(&language) || !settled {
            data["languages"].as_object_mut().unwrap().remove(language);
            data["files"].as_object_mut().unwrap().remove(language);
        } else {
            data["languages"][language] = json!(current);
            data["files"][language] = json!(paths);
        }
    }
    let result = if data["languages"].as_object().unwrap().is_empty() {
        match fs::remove_file(stamp_path(root)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    } else {
        let bytes = ascii_json(&(serde_json::to_string_pretty(&data).expect("stamp JSON") + "\n"));
        crate::atomic::write_bytes(&stamp_path(root), bytes.as_bytes())
    };
    if let Err(error) = result {
        eprintln!("Could not record the coverage stamp: {error}");
    }
}

/// Files whose language fingerprint, command or covered-file membership is stale.
/// Preserve input order, including files whose extension is not recognized.
pub fn stale_files(root: &Path, files: &[PathBuf], command: Option<&str>) -> Vec<PathBuf> {
    let root = resolve(root);
    let Some(data) =
        read(&root).filter(|data| data.get("command").unwrap_or(&Value::Null) == &json!(command))
    else {
        return files.to_vec();
    };
    let current = fingerprint(&root);
    files
        .iter()
        .filter(|file| {
            let Some(language) = language_of(file) else {
                return true;
            };
            data["languages"][language.name()].as_str() != Some(&current)
                || !data["files"][language.name()]
                    .as_array()
                    .is_some_and(|paths| paths.contains(&json!(shown(&resolve(file), &root))))
        })
        .cloned()
        .collect()
}
