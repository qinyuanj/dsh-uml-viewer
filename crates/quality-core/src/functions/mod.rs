//! The scored units of a source file: functions and methods with their
//! namespace, span and cyclomatic complexity. crapper scores them, mutator
//! groups mutation sites by them and dryer names duplicates after them.

use std::path::Path;

use crate::language::Language;

mod lua;
mod python;
mod rust;
mod typescript;

/// One function or method.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    /// `crate::module[::Type]` for Rust; the uml-viewer join key.
    pub namespace: String,
    pub complexity: u32,
    pub start_line: usize,
    pub end_line: usize,
    /// Project-relative posix path, or the absolute posix path outside it.
    pub path: String,
    pub language: Language,
}

/// Whether crapper can extract this language. Go, Java and Clojure arrive
/// with qinyuanj/dsh-uml-viewer#5; their registrations merge additively.
pub fn supported(language: Language) -> bool {
    matches!(
        language,
        Language::Rust | Language::Python | Language::TypeScript | Language::Lua
    )
}

/// Functions in one file. `path` is project-relative (either separator);
/// `project_root` anchors it.
pub fn functions_in_file(
    language: Language,
    source: &str,
    path: &str,
    project_root: &Path,
) -> Vec<Function> {
    let path = path.replace('\\', "/");
    match language {
        Language::Rust => rust::functions(source, &path, project_root),
        Language::Python => python::functions(source, &path, project_root),
        Language::TypeScript => typescript::functions(source, &path, project_root),
        Language::Lua => lua::functions(source, &path, project_root),
        _ => Vec::new(),
    }
}
