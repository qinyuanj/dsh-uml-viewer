//! The report paths and texts shared by crapper and mutator: `root.glob`
//! with literal segments, at most one `**`, and files in name order.
use crate::paths::{posix, resolve};
use std::fs;
use std::path::{Path, PathBuf};

/// `root.glob(pattern)` for patterns of literal segments with at most one
/// `**`; directory entries are visited in name order.
pub(crate) fn glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    if pattern.starts_with("*/") || pattern.starts_with("*/*/") {
        let depth = usize::from(pattern.starts_with("*/*/")) + 1;
        let rest = pattern.splitn(depth + 1, '/').last().unwrap();
        let mut found = Vec::new();
        fn children_at(root: &Path, depth: usize, found: &mut Vec<PathBuf>) {
            if depth == 0 {
                found.push(root.to_path_buf());
                return;
            }
            let Ok(entries) = fs::read_dir(root) else {
                return;
            };
            let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            entries.sort();
            for entry in entries {
                if entry.is_dir() {
                    children_at(&entry, depth - 1, found);
                }
            }
        }
        children_at(root, depth, &mut found);
        return found
            .into_iter()
            .flat_map(|directory| glob(&directory, rest))
            .collect();
    }
    let Some((prefix, rest)) = pattern.split_once("/**/") else {
        let path = root.join(pattern);
        return if path.exists() {
            vec![path]
        } else {
            Vec::new()
        };
    };
    let mut found = Vec::new();
    fn descend(directory: &Path, rest: &str, found: &mut Vec<PathBuf>) {
        let candidate = directory.join(rest);
        if candidate.exists() {
            found.push(candidate);
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        dirs.sort();
        for dir in dirs {
            descend(&dir, rest, found);
        }
    }
    let base = root.join(prefix);
    if base.is_dir() {
        descend(&base, rest, &mut found);
    }
    found
}

/// The directory reports are read from: one plain tree, or an overlayfs
/// upper layer read through to its lower layer after the mount is gone.
///
/// The overlay form shows what the mounted view showed: an upper entry
/// shadows the lower one, a whiteout (a 0/0 character device) deletes it, a
/// non-directory hides everything below it, and an opaque upper directory
/// (`user.overlay.opaque` or `trusted.overlay.opaque` = `y`) hides the lower
/// directory's contents.
#[derive(Clone, Copy)]
pub struct ReportTree<'a> {
    top: &'a Path,
    lower: Option<&'a Path>,
}

impl<'a> ReportTree<'a> {
    /// Every report as `root` holds it.
    pub fn plain(root: &'a Path) -> Self {
        Self {
            top: root,
            lower: None,
        }
    }

    /// An overlay's `upper` directory over its `lower` directory.
    pub fn overlay(upper: &'a Path, lower: &'a Path) -> Self {
        Self {
            top: upper,
            lower: Some(lower),
        }
    }

    /// Where `relative` is visible as a regular file, if it is.
    pub fn file(&self, relative: &Path) -> Option<PathBuf> {
        let Some(lower) = self.lower else {
            let path = self.top.join(relative);
            return path.is_file().then_some(path);
        };
        let names: Vec<_> = relative.components().collect();
        let mut upper = self.top.to_path_buf();
        // The lower layer stays visible until an opaque or replaced directory.
        let mut through = true;
        for (index, name) in names.iter().enumerate() {
            upper.push(name);
            let Ok(metadata) = fs::symlink_metadata(&upper) else {
                // Absent from upper from here down: the lower layer decides.
                if !through {
                    return None;
                }
                let path = lower.join(relative);
                return path.is_file().then_some(path);
            };
            if whiteout(&metadata) {
                return None;
            }
            if index + 1 == names.len() {
                return upper.is_file().then_some(upper);
            }
            if metadata.is_dir() {
                through &= !opaque(&upper);
            } else if upper.is_dir() {
                // A link copied up replaces the lower directory.
                through = false;
            } else {
                return None;
            }
        }
        None
    }

    /// `glob(pattern)` over the merged view, in the order a glob over the
    /// mounted view lists it: each matched directory in component order.
    fn glob(&self, pattern: &str) -> Vec<PathBuf> {
        let Some(lower) = self.lower else {
            return glob(self.top, pattern);
        };
        // The literal segments after the last wildcard, the same in every match.
        let tail = pattern
            .rsplit('/')
            .take_while(|segment| !segment.contains('*'))
            .count();
        let head = |path: &Path| -> PathBuf {
            let names: Vec<_> = path.components().collect();
            names[..names.len().saturating_sub(tail)].iter().collect()
        };
        let mut found: Vec<PathBuf> = [lower, self.top]
            .into_iter()
            .flat_map(|layer| {
                glob(layer, pattern)
                    .into_iter()
                    .filter_map(move |path| path.strip_prefix(layer).ok().map(Path::to_path_buf))
            })
            .collect();
        found.sort_by_key(|path| head(path));
        found.dedup();
        found
            .iter()
            .filter_map(|relative| self.file(relative))
            .collect()
    }

    /// Every `.html` file under `directory`, as (path, path below
    /// `directory`), in the sorted depth-first order of a plain walk.
    pub(crate) fn htmls(&self, directory: &Path) -> Vec<(PathBuf, PathBuf)> {
        fn walk(root: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(root) else {
                return;
            };
            let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "html") {
                    out.push(path);
                }
            }
        }
        let layers: Vec<&Path> = match self.lower {
            None => vec![self.top],
            Some(lower) => vec![lower, self.top],
        };
        let mut found: Vec<PathBuf> = Vec::new();
        for layer in &layers {
            let base = layer.join(directory);
            let mut paths = Vec::new();
            walk(&base, &mut paths);
            found.extend(
                paths
                    .into_iter()
                    .filter_map(|path| path.strip_prefix(&base).ok().map(Path::to_path_buf)),
            );
        }
        if self.lower.is_none() {
            return found
                .into_iter()
                .map(|below| (self.top.join(directory).join(&below), below))
                .collect();
        }
        found.sort();
        found.dedup();
        found
            .into_iter()
            .filter_map(|below| self.file(&directory.join(&below)).map(|path| (path, below)))
            .collect()
    }
}

/// An overlayfs whiteout: a character device numbered 0/0.
#[cfg(unix)]
fn whiteout(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    metadata.file_type().is_char_device() && metadata.rdev() == 0
}

#[cfg(not(unix))]
fn whiteout(_: &fs::Metadata) -> bool {
    false
}

/// An overlayfs opaque directory: its lower directory's entries are hidden.
#[cfg(target_os = "linux")]
fn opaque(directory: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = CString::new(directory.as_os_str().as_bytes()) else {
        return false;
    };
    [c"user.overlay.opaque", c"trusted.overlay.opaque"]
        .into_iter()
        .any(|name| {
            let mut value = [0u8; 2];
            // SAFETY: both names are NUL-terminated and `value` is writable for
            // its whole length; the call does not retain either pointer.
            let read = unsafe {
                libc::lgetxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            read == 1 && value[0] == b'y'
        })
}

#[cfg(not(target_os = "linux"))]
fn opaque(_: &Path) -> bool {
    false
}

/// The text of each report once, in pattern order.
pub(crate) fn report_texts(root: &Path, patterns: &[&str]) -> Vec<String> {
    tree_report_texts(ReportTree::plain(root), patterns)
}

/// [`report_texts`] over a plain or overlay report tree.
pub(crate) fn tree_report_texts(tree: ReportTree<'_>, patterns: &[&str]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut texts = Vec::new();
    for path in patterns.iter().flat_map(|pattern| tree.glob(pattern)) {
        let key = posix(&resolve(&path));
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if !path.is_file() {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if !text.is_empty() {
                texts.push(text);
            }
        }
    }
    texts
}
