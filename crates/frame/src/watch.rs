//! Nanosecond mtime stamps (port of `love/uml/adapters/posix_watch.lua`).
//! Whole-second mtimes would hide two edits inside one second. The stamp is
//! opaque: consumers compare it for equality, never parse it.

use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// `"sec:nsec:size"` for the file, or an error when it cannot be stat'ed.
pub fn mtime(path: &Path) -> io::Result<String> {
    let meta = std::fs::metadata(path)?;
    let modified = meta.modified()?;
    let since = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    Ok(format!(
        "{}:{}:{}",
        since.as_secs(),
        since.subsec_nanos(),
        meta.len()
    ))
}

/// Idle wake-up stamp (#143): stat-only fingerprint of every input a poll
/// can reload (diagram, `.metrics/crap.edn`, `.metrics/mutate/**/*.edn`, and
/// the caller's extra inputs such as the `to-viewer.edn` mailbox). The viewer repaints while idle only when it
/// changes; the frame's own poll then does the full reload comparison.
#[derive(Debug)]
pub struct Wake {
    diagram: std::path::PathBuf,
    root: std::path::PathBuf,
    inputs: Vec<std::path::PathBuf>,
    last: String,
}

fn stamp_into(out: &mut String, path: &Path) {
    out.push_str(&path.to_string_lossy());
    out.push('=');
    out.push_str(&mtime(path).unwrap_or_else(|_| "-".into()));
    out.push('\n');
}

fn stamp_tree(out: &mut String, dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            stamp_tree(out, &path);
        } else if path.extension().is_some_and(|e| e == "edn") {
            stamp_into(out, &path);
        }
    }
}

impl Wake {
    pub fn new(diagram: &Path, root: &Path, inputs: &[&Path]) -> Wake {
        let mut wake = Wake {
            diagram: diagram.to_path_buf(),
            root: root.to_path_buf(),
            inputs: inputs.iter().map(|p| p.to_path_buf()).collect(),
            last: String::new(),
        };
        wake.last = wake.stamp();
        wake
    }

    fn stamp(&self) -> String {
        let mut out = String::new();
        stamp_into(&mut out, &self.diagram);
        stamp_into(&mut out, &self.root.join(".metrics/crap.edn"));
        stamp_tree(&mut out, &self.root.join(".metrics/mutate"));
        for path in &self.inputs {
            stamp_into(&mut out, path);
        }
        out
    }

    /// True once per change since the last poll: request one repaint.
    pub fn poll(&mut self) -> bool {
        let stamp = self.stamp();
        if stamp == self.last {
            return false;
        }
        self.last = stamp;
        true
    }
}
