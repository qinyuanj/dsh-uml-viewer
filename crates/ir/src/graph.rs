//! Port of `love/uml/graph.lua`'s pipeline: select the scanner for a
//! language, scan a project, merge the scans. `register`/`lookup`
//! indirection exists in Lua for dynamic loading; in Rust the four scanners
//! are static modules behind [`scan_project`]. The scanner seam and the
//! id/path rules the scanners share live in [`crate::scan`].

use std::collections::HashMap;

use edn::Value;

use crate::files::Adapter;
use crate::scan::{self, Scanner};
use crate::value::encode_key;
use edn::record::{flag, get, local_name, put, record, remove};

fn lookup(lang: &Value) -> Result<&'static dyn Scanner, String> {
    let name = match lang {
        Value::Keyword(n) | Value::Symbol(n) | Value::String(n) => n.as_str(),
        _ => "",
    };
    match name {
        "lua" => Ok(&scan::lua::SCANNER),
        "typescript" => Ok(&scan::typescript::SCANNER),
        "rust" => Ok(&scan::rust::SCANNER),
        "python" => Ok(&scan::python::SCANNER),
        other => Err(format!("no LanguageGraph for :{other}")),
    }
}

/// `scan_project(lang, root, opts)`: snapshots the source tree for the
/// language and runs its scanner. `root` is adapter-relative.
pub fn scan_project(
    lang: &Value,
    root: &str,
    opts: &Value,
    adapter: &Adapter,
) -> Result<Value, String> {
    let scanner = lookup(lang)?;
    let (canonical, files) = adapter.snapshot(root, scanner, opts)?;
    let cwd = adapter.root().to_owned();
    scanner.scan(&canonical, opts, &files, &cwd)
}

/// `merge_scans(scans)`: classes dedup by id (a foreign stub yields to the
/// real class; two real classes with one id are an error), a qualified stub
/// stands for the deepest real class on its path, edges dedup structurally,
/// and invokes add dependency edges to command owners.
pub fn merge_scans(scans: &[Value]) -> Result<Value, String> {
    let mut classes: Vec<Value> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut owners: HashMap<String, Value> = HashMap::new();
    for scan in scans {
        for c in edn::record::seq(get(scan, "classes")) {
            let key = encode_key(get(c, "id").unwrap_or(&Value::Nil));
            match by_id.get(&key) {
                None => {
                    classes.push(c.clone());
                    by_id.insert(key, classes.len() - 1);
                }
                Some(&at) => {
                    if !flag(c, "foreign") {
                        if !flag(&classes[at], "foreign") {
                            return Err(format!("duplicate class id: {key}"));
                        }
                        classes[at] = c.clone();
                    }
                }
            }
        }
        if let Some(Value::Map(entries)) = get(scan, "commands") {
            for (id, cmds) in entries {
                for cmd in edn::record::seq(Some(cmds)) {
                    owners.insert(encode_key(cmd), id.clone());
                }
            }
        }
    }
    let remap = qualified_stubs(&mut classes);
    let mut edges: Vec<Value> = Vec::new();
    let add = |edges: &mut Vec<Value>, e: Value| {
        if !edges.contains(&e) {
            edges.push(e);
        }
    };
    for scan in scans {
        for e in edn::record::seq(get(scan, "edges")) {
            let mut e = e.clone();
            for end in ["from", "to"] {
                if let Some(to) = get(&e, end).and_then(|id| remap.get(&encode_key(id))) {
                    put(&mut e, end, to.clone());
                }
            }
            if get(&e, "from") != get(&e, "to") {
                add(&mut edges, e);
            }
        }
    }
    for scan in scans {
        if let Some(Value::Map(entries)) = get(scan, "invokes") {
            for (from, commands) in entries {
                for command in edn::record::seq(Some(commands)) {
                    if let Some(to) = owners.get(&encode_key(command))
                        && to != from
                    {
                        add(
                            &mut edges,
                            record(vec![
                                ("from", from.clone()),
                                ("to", to.clone()),
                                ("kind", Value::keyword("dependency")),
                            ]),
                        );
                    }
                }
            }
        }
    }
    Ok(record(vec![
        ("classes", Value::Vector(classes)),
        ("edges", Value::Vector(edges)),
    ]))
}

/// A `:qualified` stub names a path into another scanned project. It joins
/// the deepest real class on that path only when both carry the same explicit
/// crate identity. Scanner-only identity never leaves this merge.
fn qualified_stubs(classes: &mut Vec<Value>) -> HashMap<String, Value> {
    fn krate(c: &Value) -> Option<&str> {
        match get(c, "scan-crate") {
            Some(Value::String(name)) => Some(name),
            _ => None,
        }
    }
    let real: HashMap<(&str, &str), &Value> = classes
        .iter()
        .filter(|c| !flag(c, "foreign"))
        .filter_map(|c| {
            let id = get(c, "id")?;
            Some(((krate(c)?, local_name(id)?), id))
        })
        .collect();
    let deepest = |c: &Value| {
        let krate = krate(c)?;
        let mut name = local_name(get(c, "id")?)?;
        loop {
            if let Some(&to) = real.get(&(krate, name)) {
                return Some(to.clone());
            }
            name = name.rsplit_once('.')?.0;
        }
    };
    let remap: HashMap<String, Value> = classes
        .iter()
        .filter(|c| flag(c, "foreign") && flag(c, "qualified"))
        .filter_map(|c| Some((encode_key(get(c, "id")?), deepest(c)?)))
        .collect();
    classes.retain(|c| !get(c, "id").is_some_and(|id| remap.contains_key(&encode_key(id))));
    for c in classes {
        remove(c, "scan-crate");
    }
    remap
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(id: &str, foreign: bool) -> Value {
        let mut c = record(vec![("id", Value::keyword(id))]);
        put(
            &mut c,
            "scan-crate",
            Value::String(id.split('.').next().unwrap_or_default().to_owned()),
        );
        if foreign {
            put(&mut c, "foreign", Value::Bool(true));
            put(&mut c, "qualified", Value::Bool(true));
        }
        c
    }

    fn dep(from: &str, to: &str) -> Value {
        record(vec![
            ("from", Value::keyword(from)),
            ("to", Value::keyword(to)),
            ("kind", Value::keyword("dependency")),
        ])
    }

    fn ids(v: &Value, key: &str) -> Vec<String> {
        edn::record::seq(get(v, key))
            .iter()
            .map(|x| encode_key(get(x, "id").unwrap_or(x)))
            .collect()
    }

    #[test]
    fn a_qualified_stub_lands_on_the_deepest_real_class_on_its_path() {
        let app = record(vec![
            (
                "classes",
                Value::Vector(vec![
                    class("main", false),
                    class("scene.document.ViewState", true),
                    class("scene.run", true),
                    class("egui.Context", true),
                ]),
            ),
            (
                "edges",
                Value::Vector(vec![
                    dep("main", "scene.document.ViewState"),
                    dep("main", "scene.run"),
                    dep("main", "egui.Context"),
                ]),
            ),
        ]);
        let scene = record(vec![
            (
                "classes",
                Value::Vector(vec![class("scene", false), class("scene.document", false)]),
            ),
            ("edges", Value::Vector(vec![dep("scene", "scene.document")])),
        ]);
        let g = merge_scans(&[app, scene]).unwrap();
        assert_eq!(
            vec![":main", ":egui.Context", ":scene", ":scene.document"],
            ids(&g, "classes")
        );
        let edges: Vec<String> = edn::record::seq(get(&g, "edges"))
            .iter()
            .map(|e| {
                let end = |k| encode_key(get(e, k).unwrap());
                format!("{} -> {}", end("from"), end("to"))
            })
            .collect();
        assert_eq!(
            vec![
                ":main -> :scene.document",
                ":main -> :scene",
                ":main -> :egui.Context",
                ":scene -> :scene.document",
            ],
            edges
        );
    }
}
