//! The scanner seam and the four language scanners, ported from
//! `love/uml/<lang>_language/` and the shared parts of `love/uml/graph.lua`.
//! A scanner implements [`Scanner`] over [`SourceFile`] snapshots; the id and
//! path rules below are the ones every scanner applies, so adding a language
//! starts here. The JVM goldens pin existing fixture output; scanner fixes
//! add regression cases for source forms those fixtures do not cover.

use edn::Value;
use edn::record::record;

use crate::value::id_name;

pub mod lua;
pub mod python;
pub mod rust;
pub mod typescript;

/// A canonical source file and its raw bytes.
pub struct SourceFile {
    pub path: String,
    pub source: Vec<u8>,
}

/// A language scanner: file selection plus the project scan.
pub trait Scanner {
    fn accepts(&self, relative: &str, opts: &Value) -> bool;
    /// Whether a directory may contain scannable files; default true.
    fn accepts_directory(&self, relative: &str, opts: &Value) -> bool {
        let _ = (relative, opts);
        true
    }
    /// Explicitly named metadata collected outside the source tree.
    fn metadata_paths(&self, root: &str) -> Vec<String> {
        let _ = root;
        Vec::new()
    }
    fn scan(
        &self,
        root: &str,
        opts: &Value,
        files: &[SourceFile],
        cwd: &str,
    ) -> Result<Value, String>;
}

fn blank(s: Option<&Value>) -> bool {
    match s {
        _ if !edn::record::truthy(s) => true,
        Some(Value::String(s)) => s
            .trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'))
            .is_empty(),
        _ => false,
    }
}

fn as_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Keyword(n) | Value::Symbol(n) => n.clone(),
        _ => String::new(),
    }
}

/// `id_of(ns, prefix)`: strip a leading `prefix.`; the result is a keyword.
/// This is the join rule the quality overlay relies on: a class id is its
/// `:ns` without the policy `:prefix`, so `uml.ir.graph` has id `ir.graph`.
pub fn id_of(ns: &str, prefix: Option<&Value>) -> Value {
    let mut ns = ns.to_owned();
    if let Some(prefix) = prefix.filter(|p| !blank(Some(p))) {
        let prefix = as_str(prefix);
        if ns.starts_with(&format!("{prefix}.")) {
            ns = ns[prefix.len() + 1..].to_owned();
        }
    }
    Value::keyword(&ns)
}

/// `module_name(cid)`: the display name from the last dotted segment.
pub fn module_name(cid: &Value) -> String {
    let segment = {
        let name = id_name(cid);
        let stripped = name.strip_suffix('.').unwrap_or(&name);
        stripped.rsplit('.').next().unwrap_or(stripped).to_owned()
    };
    let mut words = Vec::new();
    for part in segment.split('-') {
        let mut chars = part.chars();
        let Some(first) = chars.next() else { continue };
        let rest: String = chars.collect();
        let rest = if part.chars().any(|c| c.is_ascii_uppercase()) {
            rest
        } else {
            rest.to_lowercase()
        };
        words.push(format!("{}{}", first.to_uppercase(), rest));
    }
    words.concat()
}

/// `relative_path(path, cwd)`: both sides canonicalized by the caller.
pub fn relative_path(path: &str, cwd: &str) -> String {
    let prefix = format!("{cwd}/");
    match path.strip_prefix(&prefix) {
        Some(rest) => rest.to_owned(),
        None => path.to_owned(),
    }
}

/// `file_base(path)`: last segment, allowing a trailing slash.
pub fn file_base(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().map(str::to_owned)
}

/// `ns_join(prefix, relative)`.
pub fn ns_join(prefix: &str, relative: &str) -> String {
    if relative.is_empty() {
        return prefix.to_owned();
    }
    if prefix.is_empty() {
        return relative.to_owned();
    }
    format!("{prefix}.{relative}")
}

/// `foreign_class(cid)`.
pub fn foreign_class(cid: &Value) -> Value {
    record(vec![
        ("id", cid.clone()),
        ("name", Value::String(id_name(cid))),
        ("ns", Value::String(id_name(cid))),
        ("foreign", Value::Bool(true)),
    ])
}
