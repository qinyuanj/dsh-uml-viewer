//! Class search (port of the deleted inspector's search,
//! `application/inspector.lua`): UTF-8 substring over class names and
//! namespaces, ASCII case-insensitive.

use edn::Value;
use edn::record::{get, name};

/// One search result row.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub id: Value,
    pub name: String,
    pub ns: String,
    pub label: String,
}

/// ASCII-only lowercase, leaving non-ASCII bytes untouched (Lua `:lower()`).
fn ascii_lower(s: &str) -> String {
    s.bytes()
        .map(|b| b.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .iter()
        .map(|&b| b as char)
        .collect()
}

/// Search the loaded document's classes; results sort by namespace then id.
pub fn search(doc: &Value, query: &str) -> Vec<Match> {
    let query = ascii_lower(query.trim());
    if query.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut add = |c: &Value| {
        let Some(id) = get(c, "id") else { return };
        let class_name = match get(c, "name") {
            Some(Value::String(s)) => s.clone(),
            _ => name(id),
        };
        let ns = match get(c, "ns") {
            Some(Value::String(s)) => s.clone(),
            _ => name(id),
        };
        if ascii_lower(&class_name).contains(&query) || ascii_lower(&ns).contains(&query) {
            out.push(Match {
                id: id.clone(),
                label: format!("{class_name}  ·  {ns}"),
                name: class_name,
                ns,
            });
        }
    };
    for c in edn::record::seq(get(doc, "classes")) {
        add(c);
    }
    for p in edn::record::seq(get(doc, "packages")) {
        for c in edn::record::seq(get(p, "classes")) {
            add(c);
        }
    }
    out.sort_by(|a, b| {
        if a.ns != b.ns {
            a.ns.cmp(&b.ns)
        } else {
            edn::encode(&a.id)
                .unwrap()
                .cmp(&edn::encode(&b.id).unwrap())
        }
    });
    out
}
