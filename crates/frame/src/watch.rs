use std::io;
use std::path::Path;

/// `"sec:nsec:size"` for the file, or an error when it cannot be stat'ed.
///
/// The encoding is [`scene::snapshots::stamp`]'s, so the diagram's change
/// detection and a quality snapshot's record carry one stamp shape.
pub fn mtime(path: &Path) -> io::Result<String> {
    scene::snapshots::stamp(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the file cannot be stat'ed"))
}
