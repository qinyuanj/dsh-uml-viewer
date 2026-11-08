//! Paths the way the Python tools see them: `Path.resolve()` (absolute,
//! symlinks resolved, a missing tail kept) and `as_posix()`.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// `Path.resolve()`: absolute against the current directory, symlinks and
/// `..` resolved as far as the path exists, the missing rest appended.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let simple = absolute
        .components()
        .all(|part| !matches!(part, Component::ParentDir | Component::CurDir));
    if simple && let Ok(found) = fs::canonicalize(&absolute) {
        return plain(found);
    }
    let mut out = PathBuf::new();
    let mut exists = true;
    for part in absolute.components() {
        match part {
            Component::Prefix(_) | Component::RootDir => out.push(part.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => {
                out.push(name);
                if exists {
                    match fs::canonicalize(&out) {
                        Ok(found) => out = plain(found),
                        Err(_) => exists = false,
                    }
                }
            }
        }
    }
    out
}

/// Without Windows' verbatim `\\?\` prefix, as Python prints paths.
fn plain(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

/// `as_posix()`: the path with `/` separators.
pub fn posix(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// `path.relative_to(root).as_posix()`, when `path` is under `root`.
pub fn relative_posix(path: &Path, root: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rest
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

/// `relative_posix`, or the whole path in posix form when it is outside.
pub fn shown(path: &Path, root: &Path) -> String {
    relative_posix(path, root).unwrap_or_else(|| posix(path))
}

/// `os.path.normcase`: case and separators folded on Windows.
pub fn normcase(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('/', "\\").to_lowercase()
    } else {
        text.into_owned()
    }
}

/// `Path.home()`.
pub fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Sort by `as_posix()` and drop duplicates, as the tools order files.
pub fn sorted_unique(mut files: Vec<PathBuf>) -> Vec<PathBuf> {
    files.sort_by_key(|path| posix(path));
    files.dedup();
    files
}
