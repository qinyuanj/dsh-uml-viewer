//! Project files the inspector writes (#141): proposal write-back through
//! the scene [`Boundary`].

use std::path::{Path, PathBuf};

use scene::document::Boundary;

/// `std::fs::canonicalize`, or `path` itself when it does not resolve. On
/// Windows the verbatim prefix is stripped (`\\?\C:\x` → `C:\x`,
/// `\\?\UNC\s\sh` → `\\s\sh`): a verbatim path takes no `/` joins, and
/// displayed paths are joined with `/`.
pub fn canonical(path: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if cfg!(windows) {
        let text = resolved.to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(local) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(local);
        }
    }
    resolved
}

/// Write `text` next to `path` and rename it into place.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The filesystem as the proposal write-back boundary.
pub struct Files;

impl Boundary for Files {
    fn exists(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn write(&self, path: &str, text: &str) -> bool {
        match write_atomic(Path::new(path), text) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("uml viewer: could not write {path}: {error}");
                false
            }
        }
    }
}
