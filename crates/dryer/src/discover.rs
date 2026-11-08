//! Which files dryer compares: crapper's tables, plus `.cljd`
//! (`dryer/discover.py`: dry4clj reads it, so it is Clojure here and
//! `_test.cljd` is a test). The walk, roots, exclusions and changed-file
//! narrowing stay in `quality_core::select`; there is no second walk.

use quality_core::language::{self, Language};
use quality_core::select::{Discovery, Scope};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The dryer's language table: crapper's, with `.cljd` added.
pub(crate) fn language_of(path: &Path) -> Option<Language> {
    if path.extension().and_then(|ext| ext.to_str()) == Some("cljd") {
        return Some(Language::Clojure);
    }
    language::language_of(path)
}

pub(crate) fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with("_test.cljd"))
        || language::is_test_file(path)
}

pub(crate) fn is_source_file(path: &Path) -> bool {
    language_of(path).is_some() && !is_test_file(path)
}

pub(crate) const DISCOVERY: Discovery = Discovery {
    language_of,
    is_source: is_source_file,
};

/// Source files under `roots` (a file root is itself).
#[cfg(test)]
pub(crate) fn iter_source_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    quality_core::select::iter_source_files_with(roots, DISCOVERY)
}

pub(crate) fn select(scope: &Scope, positionals: &[String], notes: &mut dyn Write) -> Vec<PathBuf> {
    scope.select_with(positionals, notes, DISCOVERY)
}

pub(crate) fn select_changed(
    scope: &Scope,
    positionals: &[String],
    notes: &mut dyn Write,
) -> Vec<PathBuf> {
    scope.select_changed_with(positionals, notes, DISCOVERY)
}
