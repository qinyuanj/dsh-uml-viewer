use std::path::Path;

use crate::language::Language;

mod clojure;
mod go;
mod java;
mod lua;
mod python;
mod rust;
mod typescript;

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub namespace: String,
    #[doc(hidden)]
    pub jacoco_class: Option<String>,
    pub complexity: u32,
    pub start_line: usize,
    pub end_line: usize,
    pub path: String,
    pub language: Language,
}

impl Function {
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

    pub fn with_jacoco_class(mut self, binary: String) -> Function {
        self.jacoco_class = Some(binary);
        self
    }
}

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

pub fn functions_in_file(
    language: Language,
    source: &str,
    path: &str,
    project_root: &Path,
) -> std::io::Result<Vec<Function>> {
    let path = path.replace('\\', "/");
    let functions = match language {
        Language::Rust => rust::functions(source, &path, project_root),
        Language::Go => go::functions(source, &path, project_root),
        Language::Java => java::functions(source, &path, project_root),
        Language::Clojure => clojure::functions(source, &path, project_root),
        Language::Python => python::functions(source, &path, project_root),
        Language::TypeScript => typescript::functions(source, &path, project_root),
        Language::Lua => lua::functions(source, &path, project_root)?,
    };
    Ok(functions)
}
