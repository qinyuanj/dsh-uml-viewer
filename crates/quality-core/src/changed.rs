//! Git's changed set, shared by all quality commands.
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::language::SKIP_DIRS;
use crate::paths::{resolve, sorted_unique};
use crate::process::{Program, output};

fn git(root: &Path, args: &[&str], notes: &mut dyn Write) -> Option<Vec<u8>> {
    let argv: Vec<String> = std::iter::once("git")
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect();
    match output(&Program::Argv(&argv), root) {
        Ok(result) if result.status.success() => Some(result.stdout),
        Ok(result) => {
            let text = String::from_utf8_lossy(&result.stderr);
            let _ = writeln!(
                notes,
                "{}",
                if text.trim().is_empty() {
                    "git status failed"
                } else {
                    text.trim()
                }
            );
            None
        }
        Err(error) => {
            let _ = writeln!(notes, "git status failed: {error}");
            None
        }
    }
}

/// Existing added, modified, renamed and untracked files below `root`.
/// Renames are requested as delete/add pairs; skipped directories are ignored.
/// A git failure reports its diagnostic and yields an empty selection.
pub fn changed_files(root: &Path, notes: &mut dyn Write) -> Vec<PathBuf> {
    let root = resolve(root);
    let Some(top) = git(&root, &["rev-parse", "--show-toplevel"], notes) else {
        return Vec::new();
    };
    let Some(status) = git(
        &root,
        &[
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ],
        notes,
    ) else {
        return Vec::new();
    };
    let top = PathBuf::from(String::from_utf8_lossy(&top).trim_end_matches('\n'));
    let found = status
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let name = entry.get(3..)?;
            let path = resolve(&top.join(String::from_utf8_lossy(name).as_ref()));
            let relative = path.strip_prefix(&root).ok()?;
            let parent = relative.parent()?;
            (path.is_file()
                && !parent
                    .components()
                    .any(|part| SKIP_DIRS.iter().any(|skip| part.as_os_str() == *skip)))
            .then_some(path)
        })
        .collect();
    sorted_unique(found)
}
