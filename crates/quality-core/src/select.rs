//! Source file selection shared by the quality tools: the walk, `-s`
//! source roots, `--exclude` globs, named paths and text filters.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::language::{Language, SKIP_DIRS, TEST_DIRS, is_source_file, language_of};
use crate::paths::{posix, relative_posix, resolve, shown, sorted_unique};

/// Which paths a tool counts as source. crapper's table is the default;
/// dryer adds `.cljd` (dryer `discover.py`), and the same walk, roots,
/// exclusions, filters and changed-file narrowing serve both.
#[derive(Clone, Copy)]
pub struct Discovery {
    pub language_of: fn(&Path) -> Option<Language>,
    pub is_source: fn(&Path) -> bool,
}

impl Discovery {
    /// crapper's and mutator's tables.
    pub const CRAPPER: Discovery = Discovery {
        language_of,
        is_source: is_source_file,
    };
}

fn walk(root: &Path, found: &mut Vec<PathBuf>, discovery: Discovery) {
    if root.is_file() {
        found.push(root.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // os.walk lists a symlink to a directory as a directory and does not
        // descend into it.
        let is_dir = kind.is_dir() || (kind.is_symlink() && path.is_dir());
        if is_dir {
            let skipped = SKIP_DIRS.iter().chain(TEST_DIRS).any(|dir| name == *dir);
            if !skipped && !kind.is_symlink() {
                walk(&path, found, discovery);
            }
        } else if (discovery.is_source)(&path) {
            found.push(path);
        }
    }
}

/// Every source file under `roots` (a file root is itself), resolved,
/// deduplicated and sorted by posix path.
pub fn iter_source_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    iter_source_files_with(roots, Discovery::CRAPPER)
}

/// The same walk with another language table (dryer's `.cljd`).
pub fn iter_source_files_with(roots: &[PathBuf], discovery: Discovery) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in roots {
        walk(root, &mut found, discovery);
    }
    sorted_unique(found.iter().map(|path| resolve(path)).collect())
}

/// `fnmatch.fnmatchcase` for one path segment: `*`, `?`, `[abc]`, `[a-z]`,
/// `[!abc]`; an unclosed `[` is literal.
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    matches_from(&name, &pattern)
}

/// The class starting at `pattern[0] == '['`: its length, its members and
/// whether it is negated; None when unclosed (then `[` is literal).
fn class(pattern: &[char]) -> Option<(usize, &[char], bool)> {
    let mut index = 1;
    let negate = pattern.get(index) == Some(&'!');
    if negate {
        index += 1;
    }
    // A `]` right after `[` or `[!` belongs to the set.
    if pattern.get(index) == Some(&']') {
        index += 1;
    }
    while index < pattern.len() && pattern[index] != ']' {
        index += 1;
    }
    if index >= pattern.len() {
        return None;
    }
    let start = if negate { 2 } else { 1 };
    Some((index + 1, &pattern[start..index], negate))
}

/// Whether `c` is in the class `body` (`a-z` ranges and single members).
fn in_class(body: &[char], negate: bool, c: char) -> bool {
    let mut hit = false;
    let mut i = 0;
    while i < body.len() {
        if i + 2 < body.len() && body[i + 1] == '-' {
            hit |= body[i] <= c && c <= body[i + 2];
            i += 3;
        } else {
            hit |= body[i] == c;
            i += 1;
        }
    }
    hit != negate
}

fn matches_from(name: &[char], pattern: &[char]) -> bool {
    let Some(&first) = pattern.first() else {
        return name.is_empty();
    };
    match first {
        '*' => (0..=name.len()).any(|skip| matches_from(&name[skip..], &pattern[1..])),
        '?' => !name.is_empty() && matches_from(&name[1..], &pattern[1..]),
        '[' => match class(pattern) {
            Some((length, body, negate)) => {
                !name.is_empty()
                    && in_class(body, negate, name[0])
                    && matches_from(&name[1..], &pattern[length..])
            }
            None => name.first() == Some(&'[') && matches_from(&name[1..], &pattern[1..]),
        },
        literal => name.first() == Some(&literal) && matches_from(&name[1..], &pattern[1..]),
    }
}

fn matches_exclusion(relative: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    let segments: Vec<&str> = pattern.split('/').collect();
    fn step(parts: &[&str], segments: &[&str]) -> bool {
        let Some(&segment) = segments.first() else {
            return true; // A matching directory also excludes its descendants.
        };
        if segment == "**" {
            return step(parts, &segments[1..])
                || (!parts.is_empty() && step(&parts[1..], segments));
        }
        !parts.is_empty() && fnmatch(parts[0], segment) && step(&parts[1..], &segments[1..])
    }
    step(&parts, &segments)
}

fn normalized_pattern(value: &str) -> String {
    let value = value.replace('\\', "/");
    let value = value.strip_prefix("./").unwrap_or(&value);
    value.trim_end_matches('/').to_string()
}

/// Whether `--exclude` patterns exclude `path`. A match on a directory
/// excludes everything under it; a path outside `root` never matches.
pub fn excluded(path: &Path, root: &Path, exclusions: &[String]) -> bool {
    let Some(relative) = relative_posix(&resolve(path), &resolve(root)) else {
        return false;
    };
    exclusions
        .iter()
        .any(|value| matches_exclusion(&relative, &normalized_pattern(value)))
}

/// `files` without the excluded ones, in order.
pub fn exclude_files(files: Vec<PathBuf>, root: &Path, exclusions: &[String]) -> Vec<PathBuf> {
    if exclusions.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|path| !excluded(path, root, exclusions))
        .collect()
}

/// The usage error for an exclusion that leaves the project root.
pub fn exclusion_error(pattern: &str) -> Option<String> {
    let normalized = pattern.replace('\\', "/");
    let absolute = Path::new(pattern).is_absolute() || normalized.starts_with('/');
    if absolute || normalized.split('/').any(|part| part == "..") {
        return Some(format!(
            "--exclude must be a path or glob inside the project root: {pattern}"
        ));
    }
    None
}

/// Files inside at least one root, compared after resolving both sides.
pub fn under_roots(files: Vec<PathBuf>, roots: &[PathBuf]) -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = roots.iter().map(|root| resolve(root)).collect();
    files
        .into_iter()
        .filter(|path| {
            let path = resolve(path);
            roots.iter().any(|root| path.starts_with(root))
        })
        .collect()
}

/// A project's declared scope: its root, `-s` source roots and `--exclude`
/// patterns, as given on the command line.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub root: PathBuf,
    pub source_roots: Vec<String>,
    pub exclusions: Vec<String>,
}

/// What the selection named: existing paths and text filters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Named {
    pub paths: Vec<PathBuf>,
    pub filters: Vec<String>,
}

impl Scope {
    /// The resolved project root.
    pub fn project_root(&self) -> PathBuf {
        resolve(&self.root)
    }

    /// The resolved `-s` roots.
    pub fn roots(&self) -> Vec<PathBuf> {
        let root = self.project_root();
        self.source_roots
            .iter()
            .map(|value| resolve(&root.join(value)))
            .collect()
    }

    /// The first usage error among the exclusions.
    pub fn exclusion_error(&self) -> Option<String> {
        self.exclusions
            .iter()
            .find_map(|pattern| exclusion_error(pattern))
    }

    /// A whole-tree refresh needs every root to exist inside the project.
    pub fn check_whole_tree(&self) -> Result<(), String> {
        let root = self.project_root();
        let values = if self.source_roots.is_empty() {
            vec![root.to_string_lossy().into_owned()]
        } else {
            self.source_roots.clone()
        };
        for value in values {
            let source_root = resolve(&root.join(value));
            if !source_root.exists() {
                return Err(format!(
                    "Source root does not exist: {}",
                    source_root.display()
                ));
            }
            if !source_root.starts_with(&root) {
                return Err(format!(
                    "Source root must be inside project root: {}",
                    source_root.display()
                ));
            }
        }
        Ok(())
    }

    /// Positionals that exist (relative to the root) are paths; the rest
    /// are text filters on the posix path.
    pub fn named(&self, positionals: &[String]) -> Named {
        let root = self.project_root();
        let mut named = Named::default();
        for arg in positionals {
            let candidate = root.join(arg);
            if candidate.exists() {
                named.paths.push(resolve(&candidate));
            } else {
                named.filters.push(arg.clone());
            }
        }
        named
    }

    fn in_scope(&self, files: Vec<PathBuf>, roots: &[PathBuf]) -> Vec<PathBuf> {
        if roots.is_empty() {
            files
        } else {
            under_roots(files, roots)
        }
    }

    /// The walk of the source roots, or of the project root without any.
    pub fn walk(&self) -> Vec<PathBuf> {
        self.walk_with(Discovery::CRAPPER)
    }

    /// The same walk with another language table.
    pub fn walk_with(&self, discovery: Discovery) -> Vec<PathBuf> {
        let roots = self.roots();
        if roots.is_empty() {
            iter_source_files_with(&[self.project_root()], discovery)
        } else {
            iter_source_files_with(&roots, discovery)
        }
    }

    /// Named paths: a directory is walked and narrowed to the source roots;
    /// a file is kept when it is inside them, not excluded and in a known
    /// language (named test files are kept). `notes` gets one line per
    /// file dropped.
    pub fn explicit_files(&self, paths: &[PathBuf], notes: &mut dyn Write) -> Vec<PathBuf> {
        self.explicit_files_with(paths, notes, Discovery::CRAPPER)
    }

    /// Named paths under another language table.
    pub fn explicit_files_with(
        &self,
        paths: &[PathBuf],
        notes: &mut dyn Write,
        discovery: Discovery,
    ) -> Vec<PathBuf> {
        let root = self.project_root();
        let roots = self.roots();
        let mut files = Vec::new();
        for path in paths {
            if path.is_dir() {
                files.extend(self.in_scope(
                    iter_source_files_with(std::slice::from_ref(path), discovery),
                    &roots,
                ));
            } else if self.in_scope(vec![path.clone()], &roots).is_empty() {
                let _ = writeln!(
                    notes,
                    "Skipping {}; it is outside the source roots",
                    shown(path, &root)
                );
            } else if excluded(path, &root, &self.exclusions) {
                let _ = writeln!(notes, "Skipping excluded {}", shown(path, &root));
            } else if (discovery.language_of)(path).is_some() {
                files.push(path.clone());
            }
        }
        files
    }

    /// Git changes narrowed by source roots, text filters and exclusions.
    /// Existing positional paths do not replace the changed set.
    pub fn select_changed(&self, positionals: &[String], notes: &mut dyn Write) -> Vec<PathBuf> {
        self.select_changed_with(positionals, notes, Discovery::CRAPPER)
    }

    /// Git changes under another language table (dryer's `.cljd`).
    pub fn select_changed_with(
        &self,
        positionals: &[String],
        notes: &mut dyn Write,
        discovery: Discovery,
    ) -> Vec<PathBuf> {
        let files = crate::changed::changed_files(&self.project_root(), notes)
            .into_iter()
            .filter(|path| (discovery.is_source)(path))
            .collect();
        let selected = self.in_scope(files, &self.roots());
        let filtered = apply_filters(selected, &self.named(positionals).filters);
        exclude_files(filtered, &self.project_root(), &self.exclusions)
    }

    /// crapper's selection: named paths, or the walk; then text filters,
    /// then exclusions; sorted by posix path.
    pub fn select(&self, positionals: &[String], notes: &mut dyn Write) -> Vec<PathBuf> {
        self.select_with(positionals, notes, Discovery::CRAPPER)
    }

    /// The same selection with another language table.
    pub fn select_with(
        &self,
        positionals: &[String],
        notes: &mut dyn Write,
        discovery: Discovery,
    ) -> Vec<PathBuf> {
        let named = self.named(positionals);
        let selected = if named.paths.is_empty() {
            self.walk_with(discovery)
        } else {
            self.explicit_files_with(&named.paths, notes, discovery)
        };
        let filtered = apply_filters(selected, &named.filters);
        let kept = exclude_files(filtered, &self.project_root(), &self.exclusions);
        sorted_unique(kept.iter().map(|path| resolve(path)).collect())
    }
}

/// Keep files whose posix path contains any filter text.
pub fn apply_filters(files: Vec<PathBuf>, filters: &[String]) -> Vec<PathBuf> {
    if filters.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|path| {
            let text = posix(path);
            filters.iter().any(|filter| text.contains(filter.as_str()))
        })
        .collect()
}
