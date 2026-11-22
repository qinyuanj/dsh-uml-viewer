use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    Clojure,
    Java,
    Go,
    TypeScript,
    Rust,
    Python,
    Lua,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Language::Clojure => "clojure",
            Language::Java => "java",
            Language::Go => "go",
            Language::TypeScript => "typescript",
            Language::Rust => "rust",
            Language::Python => "python",
            Language::Lua => "lua",
        }
    }
}

const EXTENSIONS: &[(&str, Language)] = &[
    ("clj", Language::Clojure),
    ("cljc", Language::Clojure),
    ("cljs", Language::Clojure),
    ("bb", Language::Clojure),
    ("java", Language::Java),
    ("go", Language::Go),
    ("ts", Language::TypeScript),
    ("tsx", Language::TypeScript),
    ("mts", Language::TypeScript),
    ("cts", Language::TypeScript),
    ("rs", Language::Rust),
    ("py", Language::Python),
    ("lua", Language::Lua),
];

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".idea",
    ".venv",
    "venv",
    "node_modules",
    "vendor",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    ".metrics",
    ".uml-viewer",
    "testdata",
    "__pycache__",
    ".clj-kondo",
    "lua_modules",
    ".luarocks",
];

pub const TEST_DIRS: &[&str] = &["test", "tests", "spec", "specs", "__tests__"];

const TEST_SUFFIXES: &[&str] = &[
    "_test.go",
    "_test.clj",
    "_test.cljc",
    "_test.cljs",
    "_test.bb",
    ".test.ts",
    ".spec.ts",
    ".test.tsx",
    ".spec.tsx",
    ".test.mts",
    ".spec.mts",
    "_spec.lua",
    "_test.py",
];

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn language_of(path: &Path) -> Option<Language> {
    if file_name(path).ends_with(".d.ts") {
        return None;
    }
    let extension = path.extension()?.to_str()?;
    EXTENSIONS
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, language)| *language)
}

pub fn is_test_file(path: &Path) -> bool {
    let name = file_name(path);
    if TEST_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) || name == "conftest.py" {
        return true;
    }
    if name.starts_with("test_") && name.ends_with(".py") {
        return true;
    }
    path.components()
        .any(|part| TEST_DIRS.iter().any(|dir| part.as_os_str() == *dir))
}

pub fn is_source_file(path: &Path) -> bool {
    language_of(path).is_some() && !is_test_file(path)
}
