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
