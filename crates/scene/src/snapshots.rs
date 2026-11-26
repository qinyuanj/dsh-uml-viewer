//! Quality snapshot discovery (#163): which files under `.metrics` are
//! snapshots, and what stamp each one carries.
//!
//! Two callers read the same project: the `scene` CLI export (and the
//! `--frame` oracle built on it) and the resident the panel talks to. They must
//! find the same records, or they lay the same diagram out differently. So the
//! traversal lives here, once, and both call it.

use std::path::Path;

use crate::overlay::Snapshot;

/// Load the snapshots of `<root>/.metrics`: the CRAP snapshot, the
/// duplicate-structure snapshot, and every mutation snapshot at any depth under
/// `.metrics/mutate`, sorted by path.
pub fn load_snapshots(root: &Path) -> Vec<Snapshot> {
    let mut files = Vec::new();
    for rel in [".metrics/crap.edn", ".metrics/dry.edn"] {
        let path = root.join(rel);
        if let Ok(text) = std::fs::read_to_string(&path) {
            files.push(Snapshot::new(rel, &text, mtime(&path)));
        }
    }
    collect(root, ".metrics/mutate", &mut files);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// Depth-first walk of one snapshot directory. `rel` is the forward-slash path
/// under the metrics root; every `.edn` file becomes a record, whether it sits
/// directly in `.metrics/mutate` or in a crate subdirectory.
fn collect(base: &Path, rel: &str, files: &mut Vec<Snapshot>) {
    let Ok(entries) = std::fs::read_dir(base.join(rel)) else {
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
            files.push(Snapshot::new(&next, &text, mtime(&path)));
        }
    }
}

/// `"sec:nsec:size"` for the file, or the empty string when it cannot be
/// stat'ed. The overlay orders records by it, so both discovery sites must
/// encode it the same way; layout never depends on it.
fn mtime(path: &Path) -> edn::Value {
    edn::Value::String(stamp(path).unwrap_or_default())
}

/// The raw stamp: `"sec:nsec:size"` for the file, or `None` when it cannot be
/// stat'ed. The one encoder for a file's identity, shared with the viewer's
/// change detection (`frame::watch::mtime`).
pub fn stamp(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let since = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(format!(
        "{}:{}:{}",
        since.as_secs(),
        since.subsec_nanos(),
        meta.len()
    ))
}
