//! Metrics snapshot loading (port of `love/uml/adapters/metrics.lua`): the
//! `.metrics` root walk and the immutable snapshot records the pure overlay
//! consumes. The stamp is the encoded vector of `[path mtime size]`; mtimes
//! carry nanoseconds so equal-byte-length edits still reload.

use edn::Value;
use scene::Snapshot;
use std::path::{Path, PathBuf};

use crate::watch;

fn canonical(path: &Path) -> PathBuf {
    crate::project_files::canonical(path)
}

/// The directory holding `.metrics`, walking up from the diagram path;
/// without one the current directory is the root (Lua `metrics.root`).
pub fn root(path: &Path) -> PathBuf {
    let mut dir = canonical(path);
    if !dir.is_dir() {
        dir = dir.parent().map(Path::to_path_buf).unwrap_or(dir);
    }
    loop {
        if dir.join(".metrics").is_dir() {
            return dir;
        }
        match dir.parent() {
            Some(up) if up != dir => dir = up.to_path_buf(),
            _ => {
                return canonical(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            }
        }
    }
}

/// `rel` is the `/`-separated snapshot path below `base` on every host: it is
/// EDN-visible (the snapshot name and the reload stamp).
fn collect(base: &Path, rel: &str, files: &mut Vec<Snapshot>) {
    let dir = base.join(rel);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let next = format!("{rel}/{}", name.to_string_lossy());
        let path = entry.path();
        if path.is_dir() {
            collect(base, &next, files);
        } else if name.to_string_lossy().ends_with(".edn")
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            let mtime = watch::mtime(&path).unwrap_or_default();
            files.push(Snapshot::new(&next, &text, Value::String(mtime)));
        }
    }
}

/// Snapshots of `.metrics/crap.edn` and `.metrics/mutate/**/*.edn`, sorted by
/// path, plus the stamp Value (the EDN-encoded `[path mtime size]` vector).
pub fn snapshot(base: &Path) -> (Vec<Snapshot>, Value) {
    let mut files = Vec::new();
    let crap = base.join(".metrics/crap.edn");
    if crap.is_file()
        && let Ok(text) = std::fs::read_to_string(&crap)
    {
        let mtime = watch::mtime(&crap).unwrap_or_default();
        files.push(Snapshot::new(
            ".metrics/crap.edn",
            &text,
            Value::String(mtime),
        ));
    }
    collect(base, ".metrics/mutate", &mut files);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let stamp: Vec<Value> = files
        .iter()
        .map(|f| {
            Value::Vector(vec![
                Value::String(f.path.clone()),
                f.mtime.clone(),
                Value::Integer(f.text.len() as i64),
            ])
        })
        .collect();
    (
        files,
        Value::String(edn::encode(&Value::Vector(stamp)).unwrap_or_default()),
    )
}
