//! The project boundary: canonical project and diagram paths. The window's
//! proposal write-back (#141) left with the window (dsh-uml-viewer#30);
//! proposals are edited in the policy by the companion.

use std::path::{Path, PathBuf};

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
