//! Coverage freshness compatible with crapper's version-1 stamp.
use std::cmp::Ordering;
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

/// One file's row in the tree fingerprint: relative posix path, size, and
/// mtime in nanoseconds since the epoch.
type Row = (String, u64, i128);

/// How many paths each difference names before it counts the rest.
const NAMED: usize = 8;

/// A fingerprint and the rows it hashes. Keeping the rows lets a later
/// comparison name the files that differ without walking the tree again.
pub struct Snapshot {
    rows: Vec<Row>,
    digest: String,
}

impl Snapshot {
    /// The version-1 stamp's fingerprint value.
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Every file under `root` as [relative posix path, size, mtime_ns], sorted.
/// Includes tests and configuration; excludes tool output and cache artifacts.
fn sorted_rows(root: &Path) -> Vec<Row> {
    let mut rows = Vec::new();
    tree(root, root, &mut rows);
    rows.sort();
    rows
}

fn sha256(rows: &[Row]) -> String {
    let mut digest = Sha256::new();
    for (path, size, nanos) in rows {
        let path = ascii_json(&serde_json::to_string(path).expect("JSON string"));
        digest.update(format!("[{path}, {size}, {nanos}]\n"));
    }
    format!("{:x}", digest.finalize())
}

/// The tree's fingerprint rows and their SHA-256, taken together.
pub fn snapshot(root: &Path) -> Snapshot {
    let rows = sorted_rows(root);
    let digest = sha256(&rows);
    Snapshot { rows, digest }
}

/// SHA-256 of the sorted Python JSON rows. A caller that compares two
/// fingerprints afterwards takes `snapshot` instead, so the difference can be
/// named from rows it already has.
pub fn fingerprint(root: &Path) -> String {
    snapshot(root).digest
}

/// The files one fingerprint has and the other does not, or has differently.
#[derive(Default)]
struct Differences {
    added: Vec<String>,
    removed: Vec<String>,
    changed: Vec<String>,
}

/// Compare two sorted row tables in one pass: a path on one side only was
/// added or removed, and the same path with another size or mtime changed.
fn differences(before: &[Row], after: &[Row]) -> Differences {
    let mut found = Differences::default();
    let (mut left, mut right) = (0, 0);
    while let (Some((path, size, nanos)), Some((other, other_size, other_nanos))) =
        (before.get(left), after.get(right))
    {
        match path.cmp(other) {
            Ordering::Less => {
                found.removed.push(path.clone());
                left += 1;
            }
            Ordering::Greater => {
                found.added.push(other.clone());
                right += 1;
            }
            Ordering::Equal => {
                if size != other_size || nanos != other_nanos {
                    found.changed.push(path.clone());
                }
                left += 1;
                right += 1;
            }
        }
    }
    found
        .removed
        .extend(before[left..].iter().map(|row| row.0.clone()));
    found
        .added
        .extend(after[right..].iter().map(|row| row.0.clone()));
    found
}

/// One category of the report: its name, then up to `NAMED` paths, then how
/// many follow. An empty category says `(none)` rather than going unmentioned,
/// which would read as "not checked".
fn category(lines: &mut Vec<String>, name: &str, paths: &[String]) {
    lines.push(format!("  {name}:"));
    if paths.is_empty() {
        lines.push("    (none)".to_owned());
        return;
    }
    for path in paths.iter().take(NAMED) {
        lines.push(format!("    {path}"));
    }
    if let Some(more) = paths.len().checked_sub(NAMED).filter(|more| *more > 0) {
        lines.push(format!("    ... and {more} more"));
    }
}

/// The lines appended after the freshness warning: what differs between two
/// snapshots, grouped as added, removed and changed.
fn changes(before: &[Row], after: &[Row]) -> String {
    let found = differences(before, after);
    let mut lines = Vec::new();
    category(&mut lines, "added", &found.added);
    category(&mut lines, "removed", &found.removed);
    category(&mut lines, "changed", &found.changed);
    lines.join("\n")
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
/// edit during coverage drops every language participating in that run and
/// names the files that differ. Stamp publication is best effort and never
/// changes the command's status.
pub fn record(
    root: &Path,
    files: &[PathBuf],
    command: Option<&str>,
    failed: &[&str],
    before: &Snapshot,
) {
    let mut data = base(root, command);
    let current = snapshot(root);
    let settled = before.digest == current.digest;
    if !settled {
        eprintln!("Files changed while coverage ran. Its reports are not stamped as fresh.");
        eprintln!("{}", changes(&before.rows, &current.rows));
    }
    for (language, paths) in covered(root, files) {
        if failed.contains(&language) || !settled {
            data["languages"].as_object_mut().unwrap().remove(language);
            data["files"].as_object_mut().unwrap().remove(language);
        } else {
            data["languages"][language] = json!(current.digest());
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
    let current = snapshot(&root);
    files
        .iter()
        .filter(|file| {
            let Some(language) = language_of(file) else {
                return true;
            };
            data["languages"][language.name()].as_str() != Some(current.digest())
                || !data["files"][language.name()]
                    .as_array()
                    .is_some_and(|paths| paths.contains(&json!(shown(&resolve(file), &root))))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// A scratch tree under the system temp directory, removed on drop.
    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "quality-core-stamp-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// What `record` reports for one scratch tree: fingerprinted, changed while
    /// "coverage ran", fingerprinted again.
    fn report(change: impl FnOnce(&Scratch)) -> String {
        let scratch = Scratch::new();
        scratch.write("a.rs", "pub fn a() {}\n");
        let before = snapshot(&scratch.root);
        change(&scratch);
        let after = snapshot(&scratch.root);
        changes(&before.rows, &after.rows)
    }

    #[test]
    fn a_file_added_while_coverage_ran_is_named_as_added() {
        let text = report(|scratch| {
            scratch.write(
                "crates/mutator/outer-contamination/default.profraw",
                "rforpl",
            )
        });
        assert_eq!(
            text,
            "  added:\n    crates/mutator/outer-contamination/default.profraw\n  removed:\n    (none)\n  changed:\n    (none)"
        );
    }

    #[test]
    fn a_file_removed_while_coverage_ran_is_named_as_removed() {
        let text = report(|scratch| fs::remove_file(scratch.root.join("a.rs")).unwrap());
        assert_eq!(
            text,
            "  added:\n    (none)\n  removed:\n    a.rs\n  changed:\n    (none)"
        );
    }

    #[test]
    fn a_file_rewritten_while_coverage_ran_is_named_as_changed() {
        let text = report(|scratch| scratch.write("a.rs", "pub fn a() {}\npub fn b() {}\n"));
        assert_eq!(
            text,
            "  added:\n    (none)\n  removed:\n    (none)\n  changed:\n    a.rs"
        );
    }

    #[test]
    fn a_category_names_eight_paths_and_counts_the_rest() {
        let text = report(|scratch| {
            for index in 0..10 {
                scratch.write(&format!("added/{index}.rs"), "x");
            }
        });
        assert!(text.contains("  added:\n    added/0.rs\n"), "{text}");
        assert!(
            text.contains("    added/7.rs\n    ... and 2 more"),
            "{text}"
        );
        assert!(!text.contains("added/8.rs"), "{text}");
        assert!(text.contains("  removed:\n    (none)"), "{text}");
    }

    #[test]
    fn an_unchanged_tree_differs_by_nothing() {
        let scratch = Scratch::new();
        scratch.write("a.rs", "pub fn a() {}\n");
        let before = snapshot(&scratch.root);
        let after = snapshot(&scratch.root);
        assert_eq!(before.digest(), after.digest());
        assert_eq!(
            changes(&before.rows, &after.rows),
            "  added:\n    (none)\n  removed:\n    (none)\n  changed:\n    (none)"
        );
    }
}
