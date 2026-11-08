//! Mutation replacements bound to the functions scored by quality-core.
use quality_core::functions::functions_in_file;
use quality_core::grammar::{Grammar, preorder, text};
use quality_core::language::{Language, language_of};
use quality_core::paths::{posix, resolve};
use quality_core::serde_json::{self, Value};
use quality_core::sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub file: String,
    pub namespace: String,
    pub form_id: String,
    pub name: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub mutant: String,
    pub category: String,
    pub mutation_id: String,
}
impl Site {
    pub fn description(&self) -> String {
        if self.mutant.is_empty() {
            format!("delete {}", self.original)
        } else {
            format!("{} -> {}", self.original, self.mutant)
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub id: String,
    pub namespace: String,
    pub name: String,
    pub private: bool,
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub digest: String,
    pub killed: usize,
    pub survived: usize,
    pub uncovered: usize,
    pub sites: usize,
}
fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

// Python str.splitlines recognizes CRLF and these Unicode separators too.
fn lines(source: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        if matches!(
            ch,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            found.push(&source[start..at]);
            start = at + ch.len_utf8();
            if ch == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
                let (at, _) = chars.next().unwrap();
                start = at + 1;
            }
        }
    }
    if start < source.len() {
        found.push(&source[start..]);
    }
    found
}
/// Hash an inclusive function span after trimming each line's trailing whitespace.
pub fn digest(source: &str, start: usize, end: usize) -> String {
    let selected: Vec<_> = lines(source)
        .into_iter()
        .skip(start.saturating_sub(1))
        .take(end.saturating_sub(start) + 1)
        .map(|line| {
            line.trim_end_matches(|ch: char| {
                ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
            })
        })
        .collect();
    hash(&selected.join("\n"))
}
fn private(source: &str, start: usize, end: usize) -> bool {
    let selected = lines(source)
        .into_iter()
        .skip(start.saturating_sub(1))
        .take(end.min(start + 6).saturating_sub(start) + 1)
        .collect::<Vec<_>>()
        .join("\n");
    let mut header = selected.trim_start();
    while header.starts_with("#[") {
        let Some((_, rest)) = header.split_once(']') else {
            break;
        };
        header = rest.trim_start();
    }
    // Python checks pub\s: pub(crate) deliberately remains private.
    !header
        .strip_prefix("pub")
        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}
/// Compact identity JSON, including Python json.dumps' default ensure_ascii.
pub fn mutation_id(
    file: &str,
    namespace: &str,
    form: &str,
    start: usize,
    end: usize,
    original: &str,
    mutant: &str,
) -> String {
    let json = serde_json::to_string(&serde_json::json!([
        file, namespace, form, start, end, original, mutant
    ]))
    .expect("serializing a mutation identity");
    let mut escaped = String::new();
    for ch in json.chars() {
        if ch < '\u{7f}' {
            escaped.push(ch);
        } else {
            let mut units = [0; 2];
            for unit in ch.encode_utf16(&mut units) {
                escaped.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    escaped
}
fn mutation_field(id: &str, index: usize) -> Option<String> {
    let Value::Array(values) = serde_json::from_str::<Value>(id).ok()? else {
        return None;
    };
    values.get(index)?.as_str().map(str::to_owned)
}
pub fn mutation_file(id: &str) -> Option<String> {
    mutation_field(id, 0)
}
pub fn mutation_namespace(id: &str) -> Option<String> {
    mutation_field(id, 1)
}
fn replacement(token: &str, parent: &str) -> Option<(&'static str, &'static str)> {
    if parent == "binary_expression" {
        return Some(match token {
            "+" => ("-", "arithmetic"),
            "-" => ("+", "arithmetic"),
            "*" => ("/", "arithmetic"),
            "/" => ("*", "arithmetic"),
            "%" => ("*", "arithmetic"),
            ">" => (">=", "comparison"),
            ">=" => (">", "comparison"),
            "<" => ("<=", "comparison"),
            "<=" => ("<", "comparison"),
            "==" => ("!=", "equality"),
            "!=" => ("==", "equality"),
            "&&" => ("||", "logical"),
            "||" => ("&&", "logical"),
            "^" | "&" => ("|", "bitwise"),
            "|" => ("&", "bitwise"),
            "<<" => (">>", "bitwise"),
            ">>" => ("<<", "bitwise"),
            _ => return None,
        });
    }
    if parent == "unary_expression" && matches!(token, "!" | "-") {
        return Some(("", "unary"));
    }
    None
}
type FormParts = (usize, Vec<(usize, usize, String)>);

/// Rust sites in source order and forms in first-line order. Test code is excluded
/// by shared function discovery; private forms retain their defn-/ IDs.
pub fn discover(source: &str, path: &Path, root: &Path, file_key: &str) -> (Vec<Site>, Vec<Form>) {
    if language_of(path) != Some(Language::Rust) {
        return (Vec::new(), Vec::new());
    }
    let located = if path.is_absolute() {
        resolve(path)
    } else {
        resolve(&root.join(path))
    };
    let functions = functions_in_file(Language::Rust, source, &posix(&located), root)
        .expect("Rust function discovery is infallible");
    let mut forms: Vec<Form> = Vec::new();
    let mut grouped: BTreeMap<(String, String), FormParts> = BTreeMap::new();
    let mut owners = Vec::new();
    for function in &functions {
        let private = private(source, function.start_line, function.end_line);
        let id = format!(
            "{}/{}",
            if private { "defn-" } else { "defn" },
            function.name
        );
        let key = (function.namespace.clone(), id.clone());
        let position = if let Some((position, parts)) = grouped.get_mut(&key) {
            let form = &mut forms[*position];
            form.line = form.line.min(function.start_line);
            form.end_line = form.end_line.max(function.end_line);
            parts.push((
                function.start_line,
                function.end_line,
                digest(source, function.start_line, function.end_line),
            ));
            *position
        } else {
            let position = forms.len();
            forms.push(Form {
                id,
                namespace: function.namespace.clone(),
                name: function.name.clone(),
                private,
                file: file_key.to_string(),
                line: function.start_line,
                end_line: function.end_line,
                ..Form::default()
            });
            grouped.insert(
                key,
                (
                    position,
                    vec![(
                        function.start_line,
                        function.end_line,
                        digest(source, function.start_line, function.end_line),
                    )],
                ),
            );
            position
        };
        owners.push((function, position));
    }
    for (position, mut parts) in grouped.into_values() {
        parts.sort_by_key(|(start, end, _)| (*start, *end));
        forms[position].digest = hash(
            &parts
                .into_iter()
                .map(|(_, _, hash)| hash)
                .collect::<String>(),
        );
    }
    let tree = Grammar::Rust.parse(source);
    let mut found = Vec::new();
    for node in preorder(tree.root_node()) {
        if node.child_count() != 0 {
            continue;
        }
        let token = text(source, node);
        let parent = node.parent().map_or("", |node| node.kind());
        let changed = replacement(token, parent).or_else(|| match (node.kind(), token) {
            ("true", "true") => Some(("false", "boolean")),
            ("false", "false") => Some(("true", "boolean")),
            ("integer_literal", "0") => Some(("1", "constant")),
            ("integer_literal", "1") => Some(("0", "constant")),
            _ => None,
        });
        let Some((mutant, category)) = changed else {
            continue;
        };
        let line = node.start_position().row + 1;
        let Some((function, position)) = owners
            .iter()
            .filter(|(function, _)| function.start_line <= line && line <= function.end_line)
            .min_by_key(|(function, _)| {
                (function.end_line - function.start_line, function.start_line)
            })
        else {
            continue;
        };
        let form = &forms[*position];
        found.push(Site {
            file: file_key.to_string(),
            namespace: function.namespace.clone(),
            form_id: form.id.clone(),
            name: function.name.clone(),
            line,
            start: node.start_byte(),
            end: node.end_byte(),
            original: token.to_string(),
            mutant: mutant.to_string(),
            category: category.to_string(),
            mutation_id: mutation_id(
                file_key,
                &function.namespace,
                &form.id,
                node.start_byte(),
                node.end_byte(),
                token,
                mutant,
            ),
        });
    }
    found.sort_by_key(|site| (site.line, site.start, site.end));
    forms.sort_by_key(|form| form.line);
    (found, forms)
}
/// Replace the site's UTF-8 byte span.
pub fn apply_site(source: &str, site: &Site) -> String {
    format!(
        "{}{}{}",
        &source[..site.start],
        site.mutant,
        &source[site.end..]
    )
}
