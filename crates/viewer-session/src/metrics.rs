use edn::Value;
use scene::Snapshot;
use std::path::{Path, PathBuf};

fn canonical(path: &Path) -> PathBuf {
    frame::project_files::canonical(path)
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

/// Snapshots of `.metrics/crap.edn`, `.metrics/dry.edn` and
/// `.metrics/mutate/**/*.edn`, sorted by path, plus the stamp Value (the
/// EDN-encoded `[path mtime size]` vector).
///
/// The discovery is [`scene::snapshots::load_snapshots`], the one the `scene`
/// CLI export reads: sharing it keeps `scene --frame` and the panel on the same
/// records and therefore the same geometry (#163).
pub fn snapshot(base: &Path) -> (Vec<Snapshot>, Value) {
    let files = scene::snapshots::load_snapshots(base);
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
