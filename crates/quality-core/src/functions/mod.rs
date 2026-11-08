//! The scored units of a source file: functions and methods with their
//! namespace, span and cyclomatic complexity. crapper scores them, mutator
//! groups mutation sites by them and dryer names duplicates after them.

use std::path::Path;

use crate::language::Language;

mod clojure;
mod go;
mod java;
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
    /// Java binary class name; nested types use `$`, not `.`.
    #[doc(hidden)]
    pub jacoco_class: Option<String>,
    pub complexity: u32,
    pub start_line: usize,
    pub end_line: usize,
    /// Project-relative posix path, or the absolute posix path outside it.
    pub path: String,
    pub language: Language,
}

impl Function {
    /// The only construction site, so a new field (such as the Java binary
    /// class #5 adds) is set once rather than in every language scanner.
    pub fn new(
        name: String,
        namespace: String,
        complexity: u32,
        start_line: usize,
        end_line: usize,
        path: String,
        language: Language,
    ) -> Function {
        Function {
            name,
            namespace,
            jacoco_class: None,
            complexity,
            start_line,
            end_line,
            path,
            language,
        }
    }

    /// The Java binary class name JaCoCo keys a nested type by.
    pub fn with_jacoco_class(mut self, binary: String) -> Function {
        self.jacoco_class = Some(binary);
        self
    }
}

/// Whether this port reads `language` yet. Every analysed language is ported:
/// Python, TypeScript and Lua with qinyuanj/dsh-uml-viewer#4, Go, Java and
/// Clojure with #5.
pub fn supported(language: Language) -> bool {
    matches!(
        language,
        Language::Rust
            | Language::Go
            | Language::Java
            | Language::Clojure
            | Language::Python
            | Language::TypeScript
            | Language::Lua
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
        Language::Go => go::functions(source, &path, project_root),
        Language::Java => java::functions(source, &path, project_root),
        Language::Clojure => clojure::functions(source, &path, project_root),
        Language::Python => python::functions(source, &path, project_root),
        Language::TypeScript => typescript::functions(source, &path, project_root),
        Language::Lua => lua::functions(source, &path, project_root),
    }
}
