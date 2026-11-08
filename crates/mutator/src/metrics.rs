//! Namespace snapshots and the differential mutation policy.
use crate::sites::{Form, Site, mutation_file, mutation_namespace};
use quality_core::edn::Value;
use quality_core::paths::{relative_posix, resolve};
use quality_core::snapshot::{decimal, string};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct PriorForm {
    pub digest: String,
}
#[derive(Clone, Debug, Default)]
pub struct History {
    pub forms: BTreeMap<(String, String), PriorForm>,
    pub outcomes: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub action: String,
    pub reason: String,
    pub previous: String,
}
/// A parsed artifact. Ordered EDN fields preserve legacy form field order.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub path: PathBuf,
    pub data: Value,
}

pub fn source_key(path: &Path, root: &Path) -> io::Result<String> {
    relative_posix(&resolve(path), &resolve(root)).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "source is outside project root",
        )
    })
}
pub fn snapshot_path(root: &Path, namespace: &str) -> io::Result<PathBuf> {
    let normalized = namespace.replace('\\', "/").replace("::", "/");
    let mut path = root.join(".metrics").join("mutate");
    let parts: Vec<_> = normalized.split('/').collect();
    for part in &parts {
        if matches!(*part, "" | "." | "..")
            || Path::new(part).components().count() != 1
            || part.contains(':')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsafe namespace {namespace:?}"),
            ));
        }
        path.push(part);
    }
    // Append, rather than replace an extension: dots are namespace characters.
    path.set_file_name(format!("{}.edn", parts.last().unwrap()));
    Ok(path)
}

fn value_string(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}
fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let Value::Map(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find(|(name, _)| value_string(name) == Some(key))
        .map(|(_, value)| value)
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    get(value, key).and_then(value_string)
}
fn assoc(value: &mut Value, key: &str, item: Value) {
    value.assoc(Value::String(key.to_string()), item);
}
fn forms_of(data: &Value) -> Vec<Value> {
    match get(data, "forms") {
        Some(Value::Vector(forms)) => forms
            .iter()
            .filter(|form| matches!(form, Value::Map(_)))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}
fn outcomes_of(data: &Value) -> BTreeMap<String, String> {
    let Some(Value::Map(pairs)) = get(data, "outcomes") else {
        return BTreeMap::new();
    };
    pairs
        .iter()
        .filter_map(|(key, value)| {
            let key = value_string(key)?;
            let status = value_string(value)?;
            let status = status.strip_prefix(':').unwrap_or(status);
            matches!(status, "killed" | "survived").then(|| (key.to_string(), status.to_string()))
        })
        .collect()
}
fn form_file<'a>(form: &'a Value, data: &'a Value) -> Option<&'a str> {
    text(form, "file")
        .filter(|file| !file.is_empty())
        .or_else(|| text(data, "source"))
}
fn edn_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            edn_files(&path, files);
        } else if path.is_file() && path.extension().is_some_and(|ext| ext == "edn") {
            files.push(path);
        }
    }
}
pub fn read_snapshots(root: &Path) -> Vec<Snapshot> {
    let mut paths = Vec::new();
    edn_files(&root.join(".metrics").join("mutate"), &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let data = parse(&fs::read_to_string(&path).ok()?)?;
            matches!(data, Value::Map(_)).then_some(Snapshot { path, data })
        })
        .collect()
}
pub fn load_history(root: &Path, file_key: &str) -> History {
    let mut history = History::default();
    for snapshot in read_snapshots(root) {
        let data = &snapshot.data;
        let Some(namespace) = text(data, "namespace") else {
            continue;
        };
        for form in forms_of(data) {
            if form_file(&form, data) != Some(file_key) {
                continue;
            }
            let (Some(id), Some(digest)) = (text(&form, "id"), text(&form, "hash")) else {
                continue;
            };
            history.forms.insert(
                (namespace.to_string(), id.to_string()),
                PriorForm {
                    digest: digest.to_string(),
                },
            );
        }
        history.outcomes.extend(
            outcomes_of(data)
                .into_iter()
                .filter(|(id, _)| mutation_file(id).as_deref() == Some(file_key)),
        );
    }
    history
}

pub fn decide(
    sites: &[Site],
    covered: &BTreeMap<String, bool>,
    history: &History,
    forms: &[Form],
    mutate_all: bool,
    lines: Option<&BTreeSet<usize>>,
) -> BTreeMap<String, Decision> {
    let digests: BTreeMap<_, _> = forms
        .iter()
        .map(|form| ((form.namespace.clone(), form.id.clone()), &form.digest))
        .collect();
    sites
        .iter()
        .map(|site| {
            let previous = history
                .outcomes
                .get(&site.mutation_id)
                .map_or("none", String::as_str);
            let key = (site.namespace.clone(), site.form_id.clone());
            let prior = history.forms.get(&key);
            let (action, reason) = if !covered.get(&site.mutation_id).copied().unwrap_or(false) {
                ("uncovered", "uncovered")
            } else if lines.is_some_and(|lines| !lines.contains(&site.line)) {
                ("excluded", "line-filter")
            } else if mutate_all {
                ("executed", "explicit-all")
            } else if prior.is_none() {
                ("executed", "no-history")
            } else if digests.get(&key).copied() != prior.map(|prior| &prior.digest) {
                ("executed", "digest-changed")
            } else if previous == "killed" {
                ("reused", "unchanged-killed")
            } else if previous == "survived" {
                ("executed", "previous-survived")
            } else {
                ("executed", "id-changed")
            };
            (
                site.mutation_id.clone(),
                Decision {
                    action: action.to_string(),
                    reason: reason.to_string(),
                    previous: previous.to_string(),
                },
            )
        })
        .collect()
}
pub fn carry_forward(
    sites: &[Site],
    covered: &BTreeMap<String, bool>,
    history: &History,
    forms: &[Form],
) -> BTreeMap<String, String> {
    let digests: BTreeMap<_, _> = forms
        .iter()
        .map(|form| ((form.namespace.clone(), form.id.clone()), &form.digest))
        .collect();
    sites
        .iter()
        .filter_map(|site| {
            if !covered.get(&site.mutation_id).copied().unwrap_or(false) {
                return None;
            }
            let key = (site.namespace.clone(), site.form_id.clone());
            let prior = history.forms.get(&key)?;
            if digests.get(&key).copied() != Some(&prior.digest) {
                return None;
            }
            Some((
                site.mutation_id.clone(),
                history.outcomes.get(&site.mutation_id)?.clone(),
            ))
        })
        .collect()
}
pub fn tally_forms(
    forms: &mut [Form],
    sites: &[Site],
    covered: &BTreeMap<String, bool>,
    outcomes: &BTreeMap<String, String>,
    lines: Option<&BTreeSet<usize>>,
) {
    for form in forms {
        form.killed = 0;
        form.survived = 0;
        form.uncovered = 0;
        form.sites = 0;
        for site in sites
            .iter()
            .filter(|site| site.namespace == form.namespace && site.form_id == form.id)
        {
            form.sites += 1;
            if !covered.get(&site.mutation_id).copied().unwrap_or(false) {
                form.uncovered += 1;
                continue;
            }
            match outcomes.get(&site.mutation_id).map(String::as_str) {
                Some("killed") => form.killed += 1,
                Some("survived") => form.survived += 1,
                _ if lines.is_none() => form.survived += 1,
                _ => (),
            }
        }
    }
}

fn map(fields: Vec<(&str, Value)>) -> Value {
    Value::Map(
        fields
            .into_iter()
            .map(|(key, value)| (Value::String(key.to_string()), value))
            .collect(),
    )
}
fn s(value: &str) -> Value {
    Value::String(value.to_string())
}
fn n(value: usize) -> Value {
    Value::Integer(value as i64)
}
fn render_form(form: &Form) -> Value {
    map(vec![
        ("id", s(&form.id)),
        ("kind", s(if form.private { "defn-" } else { "defn" })),
        ("file", s(&form.file)),
        ("line", n(form.line)),
        ("end-line", n(form.end_line)),
        ("hash", s(&form.digest)),
        ("killed", n(form.killed)),
        ("survived", n(form.survived)),
        ("uncovered", n(form.uncovered)),
        ("sites", n(form.sites)),
    ])
}
fn keyword_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || "*+!_?".contains(ch))
        && chars.all(|ch| ch.is_ascii_alphanumeric() || "*+!_?-./".contains(ch))
}
fn render(value: &Value) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Integer(value) => value.to_string(),
        Value::Float(value) => decimal(*value, 4),
        Value::String(value) | Value::Symbol(value) => string(value),
        Value::Keyword(value) => format!(":{value}"),
        Value::Vector(items) | Value::Set(items) => format!(
            "[{}]",
            items.iter().map(render).collect::<Vec<_>>().join(" ")
        ),
        Value::Map(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        Value::String(key) if keyword_key(key) => format!(":{key}"),
                        _ => render(key),
                    };
                    format!("{key} {}", render(value))
                })
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Value::Tagged(_, value) => render(value),
    }
}
pub fn render_snapshot(
    namespace: &str,
    source: &str,
    forms: Vec<Value>,
    outcomes: &BTreeMap<String, String>,
    tested_at: &str,
    isolation: &str,
) -> String {
    let outcomes = Value::Map(
        outcomes
            .iter()
            .map(|(id, status)| (s(id), Value::Keyword(status.clone())))
            .collect(),
    );
    let data = map(vec![
        ("version", n(1)),
        ("tested-at", s(tested_at)),
        ("isolation", Value::Keyword(isolation.to_string())),
        ("source", s(source)),
        ("namespace", s(namespace)),
        ("outcomes", outcomes),
        ("forms", Value::Vector(forms)),
    ]);
    format!("{}\n", render(&data))
}
fn mentions_file(data: &Value, file_key: &str) -> bool {
    text(data, "source") == Some(file_key)
        || outcomes_of(data)
            .keys()
            .any(|id| mutation_file(id).as_deref() == Some(file_key))
        || forms_of(data)
            .iter()
            .any(|form| form_file(form, data) == Some(file_key))
}
fn file_only(data: &Value, file_key: &str) -> bool {
    let forms = forms_of(data);
    if forms.is_empty() {
        text(data, "source") == Some(file_key)
    } else {
        forms
            .iter()
            .all(|form| form_file(form, data) == Some(file_key))
    }
}
fn form_order(form: &Value) -> (String, i64, String) {
    let file = text(form, "file").unwrap_or("None").to_string();
    let line = match get(form, "line") {
        Some(Value::Integer(line)) => *line,
        Some(Value::String(line)) => line.parse().unwrap_or(0),
        _ => 0,
    };
    (file, line, text(form, "id").unwrap_or("None").to_string())
}
fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
fn remove_empty_dirs(directory: &Path) -> io::Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            remove_empty_dirs(&path)?;
            if fs::read_dir(&path)?.next().is_none() {
                fs::remove_dir(path)?;
            }
        }
    }
    Ok(())
}
/// Replace one source's results, retaining other sources in shared namespaces.
/// Publication is atomic per namespace; legacy artifacts are removed afterwards.
pub fn write_results(
    root: &Path,
    file_key: &str,
    forms: &[Form],
    outcomes: &BTreeMap<String, String>,
    tested_at: &str,
    isolation: &str,
) -> io::Result<Vec<PathBuf>> {
    let root = resolve(root);
    let existing = read_snapshots(&root);
    let mut retained: BTreeMap<String, (Vec<Value>, BTreeMap<String, String>)> = BTreeMap::new();
    let mut touched = BTreeSet::new();
    for snapshot in &existing {
        let data = &snapshot.data;
        let Some(namespace) = text(data, "namespace") else {
            continue;
        };
        if mentions_file(data, file_key) {
            touched.insert(namespace.to_string());
        }
        let (kept_forms, kept_outcomes) = retained.entry(namespace.to_string()).or_default();
        for mut form in forms_of(data) {
            let file = form_file(&form, data).map(str::to_string);
            if file.as_deref() == Some(file_key) {
                continue;
            }
            assoc(&mut form, "file", file.map_or(Value::Nil, |file| s(&file)));
            kept_forms.push(form);
        }
        kept_outcomes.extend(
            outcomes_of(data)
                .into_iter()
                .filter(|(id, _)| mutation_file(id).as_deref() != Some(file_key)),
        );
    }
    let mut incoming: BTreeMap<String, Vec<&Form>> = BTreeMap::new();
    for form in forms {
        incoming
            .entry(form.namespace.clone())
            .or_default()
            .push(form);
        touched.insert(form.namespace.clone());
    }
    let mut written = Vec::new();
    let mut canonical = BTreeSet::new();
    for namespace in &touched {
        let path = snapshot_path(&root, namespace)?;
        canonical.insert(resolve(&path));
        let (mut kept, mut merged) = retained.remove(namespace).unwrap_or_default();
        let fresh = incoming.get(namespace).cloned().unwrap_or_default();
        let source = fresh
            .first()
            .map(|form| form.file.clone())
            .or_else(|| {
                kept.first()
                    .and_then(|form| text(form, "file"))
                    .filter(|file| !file.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| file_key.to_string());
        kept.extend(fresh.iter().map(|form| render_form(form)));
        let mut unique = Vec::new();
        let mut seen = Vec::new();
        for form in kept {
            let key = ["file", "id", "line", "hash"]
                .map(|key| get(&form, key).cloned().unwrap_or(Value::Nil));
            if !seen.contains(&key) {
                seen.push(key);
                unique.push(form);
            }
        }
        unique.sort_by_key(form_order);
        if !fresh.is_empty() {
            merged.extend(
                outcomes
                    .iter()
                    .filter(|(id, status)| {
                        mutation_file(id).as_deref() == Some(file_key)
                            && mutation_namespace(id).as_deref() == Some(namespace)
                            && matches!(status.as_str(), "killed" | "survived")
                    })
                    .map(|(id, status)| (id.clone(), status.clone())),
            );
        }
        if unique.is_empty() && merged.is_empty() {
            remove_if_exists(&path)?;
        } else {
            quality_core::atomic::write_bytes(
                &path,
                render_snapshot(namespace, &source, unique, &merged, tested_at, isolation)
                    .as_bytes(),
            )?;
            written.push(path);
        }
    }
    for snapshot in existing {
        if canonical.contains(&resolve(&snapshot.path)) {
            continue;
        }
        if text(&snapshot.data, "namespace").is_some_and(|namespace| touched.contains(namespace))
            || file_only(&snapshot.data, file_key)
        {
            remove_if_exists(&snapshot.path)?;
        }
    }
    remove_empty_dirs(&root.join(".metrics").join("mutate"))?;
    Ok(written)
}

// The Python snapshot reader is deliberately a small permissive EDN subset:
// keywords/symbols become strings; lists become vectors; duplicate keys overwrite
// their old value without moving its insertion position; unknown escapes lose \\.
fn parse(source: &str) -> Option<Value> {
    let mut reader = Reader {
        chars: source.chars().collect(),
        at: 0,
    };
    let value = reader.value()?;
    reader.skip();
    (reader.at == reader.chars.len()).then_some(value)
}
struct Reader {
    chars: Vec<char>,
    at: usize,
}
impl Reader {
    fn skip(&mut self) {
        while let Some(ch) = self.chars.get(self.at) {
            if ch.is_whitespace() || *ch == ',' {
                self.at += 1;
            } else if *ch == ';' {
                while self.chars.get(self.at).is_some_and(|ch| *ch != '\n') {
                    self.at += 1;
                }
            } else {
                break;
            }
        }
    }
    fn value(&mut self) -> Option<Value> {
        self.skip();
        let ch = *self.chars.get(self.at)?;
        self.at += 1;
        match ch {
            '"' => {
                let mut value = String::new();
                loop {
                    let ch = *self.chars.get(self.at)?;
                    self.at += 1;
                    if ch == '"' {
                        break;
                    }
                    if ch == '\\' {
                        let escaped = *self.chars.get(self.at)?;
                        self.at += 1;
                        value.push(match escaped {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            ch => ch,
                        });
                    } else {
                        value.push(ch);
                    }
                }
                Some(s(&value))
            }
            '{' => {
                let mut map = Value::Map(Vec::new());
                loop {
                    self.skip();
                    if *self.chars.get(self.at)? == '}' {
                        self.at += 1;
                        return Some(map);
                    }
                    let key = self.value()?;
                    let value = self.value()?;
                    map.assoc(key, value);
                }
            }
            '[' | '(' => {
                let closing = if ch == '[' { ']' } else { ')' };
                let mut values = Vec::new();
                loop {
                    self.skip();
                    if *self.chars.get(self.at)? == closing {
                        self.at += 1;
                        return Some(Value::Vector(values));
                    }
                    values.push(self.value()?);
                }
            }
            '#' => {
                if self.chars.get(self.at) != Some(&'{') {
                    return None;
                }
                self.at += 1;
                let mut values = Vec::new();
                loop {
                    self.skip();
                    if *self.chars.get(self.at)? == '}' {
                        self.at += 1;
                        return Some(Value::Set(values));
                    }
                    let value = self.value()?;
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
            }
            _ => {
                let keyword = ch == ':';
                let start = if keyword { self.at } else { self.at - 1 };
                while self
                    .chars
                    .get(self.at)
                    .is_some_and(|ch| !" \t\r\n,{}[]()\"\\;".contains(*ch))
                {
                    self.at += 1;
                }
                if self.at == start {
                    return None;
                }
                let token: String = self.chars[start..self.at].iter().collect();
                if keyword {
                    return Some(s(&token));
                }
                Some(match token.as_str() {
                    "nil" => Value::Nil,
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => {
                        let digits = token.strip_prefix('-').unwrap_or(&token);
                        if !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()) {
                            Value::Integer(token.parse().ok()?)
                        } else if let Some((left, right)) = digits.split_once('.') {
                            if !left.is_empty()
                                && !right.is_empty()
                                && left
                                    .chars()
                                    .chain(right.chars())
                                    .all(|ch| ch.is_ascii_digit())
                            {
                                Value::Float(token.parse().ok()?)
                            } else {
                                s(&token)
                            }
                        } else {
                            s(&token)
                        }
                    }
                })
            }
        }
    }
}
