use edn::Value;
use edn::record::record;

use crate::value::id_name;

pub mod lua;
pub mod python;
pub mod rust;
pub mod typescript;

pub struct SourceFile {
    pub path: String,
    pub source: Vec<u8>,
}

pub trait Scanner {
    fn accepts(&self, relative: &str, opts: &Value) -> bool;
    fn accepts_directory(&self, relative: &str, opts: &Value) -> bool {
        let _ = (relative, opts);
        true
    }
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

pub fn relative_path(path: &str, cwd: &str) -> String {
    let prefix = format!("{cwd}/");
    match path.strip_prefix(&prefix) {
        Some(rest) => rest.to_owned(),
        None => path.to_owned(),
    }
}

pub fn file_base(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().map(str::to_owned)
}

pub fn ns_join(prefix: &str, relative: &str) -> String {
    if relative.is_empty() {
        return prefix.to_owned();
    }
    if prefix.is_empty() {
        return relative.to_owned();
    }
    format!("{prefix}.{relative}")
}

pub fn foreign_class(cid: &Value) -> Value {
    record(vec![
        ("id", cid.clone()),
        ("name", Value::String(id_name(cid))),
        ("ns", Value::String(id_name(cid))),
        ("foreign", Value::Bool(true)),
    ])
}
