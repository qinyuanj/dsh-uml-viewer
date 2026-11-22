use std::path::Path;

use edn::Value;

use crate::files::Adapter;
use crate::value::get_truthy;

pub(crate) struct Uncovered {
    pub root: String,
    pub classes: usize,
}

fn normalized(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let path = path.strip_prefix("./").unwrap_or(&path);
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        ".".to_owned()
    } else {
        path.to_owned()
    }
}

fn covers(quality: &str, source: &str) -> bool {
    if quality == "." {
        return true;
    }
    source == quality || source.starts_with(&format!("{quality}/"))
}

fn quality_roots(adapter: &Adapter) -> Vec<String> {
    let path = Path::new(adapter.root()).join("uml-quality.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(config) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    if config.get("operations").is_some() {
        return Vec::new();
    }
    config
        .get("standardTools")
        .and_then(|standard| standard.get("sourceRoots"))
        .and_then(serde_json::Value::as_array)
        .map(|roots| {
            roots
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(normalized)
                .collect()
        })
        .unwrap_or_default()
}

fn policy_roots(policy: &Value) -> Vec<String> {
    let sources = edn::record::seq(get_truthy(policy, "sources"));
    let roots: Vec<String> = if sources.is_empty() {
        vec![root_of(policy, policy)]
    } else {
        sources
            .iter()
            .map(|source| root_of(source, policy))
            .collect()
    };
    let mut unique: Vec<String> = Vec::new();
    for root in roots {
        if !unique.contains(&root) {
            unique.push(root);
        }
    }
    unique
}

fn root_of(source: &Value, policy: &Value) -> String {
    get_truthy(source, "root")
        .or_else(|| get_truthy(source, "src"))
        .or_else(|| get_truthy(policy, "src"))
        .and_then(|value| match value {
            Value::String(path) => Some(path.as_str()),
            _ => None,
        })
        .map(normalized)
        .unwrap_or_else(|| "src".to_owned())
}

fn class_files(graph: &Value) -> Vec<String> {
    edn::record::seq(get_truthy(graph, "classes"))
        .iter()
        .filter(|class| !edn::record::flag(class, "foreign"))
        .filter_map(|class| match get_truthy(class, "file") {
            Some(Value::String(file)) => Some(normalized(file)),
            _ => None,
        })
        .collect()
}

pub(crate) fn uncovered(policy: &Value, graph: &Value, adapter: &Adapter) -> Vec<Uncovered> {
    let quality = quality_roots(adapter);
    if quality.is_empty() {
        return Vec::new();
    }
    let files = class_files(graph);
    let mut uncovered: Vec<Uncovered> = Vec::new();
    for root in policy_roots(policy) {
        if quality.iter().any(|declared| covers(declared, &root)) {
            continue;
        }
        let classes = files.iter().filter(|file| covers(&root, file)).count();
        if classes > 0 {
            uncovered.push(Uncovered { root, classes });
        }
    }
    uncovered
}

pub(crate) fn warning(root: &str, classes: usize) -> String {
    let noun = if classes == 1 { "class" } else { "classes" };
    format!(
        "uml: source root \"{root}\" has no quality provider in uml-quality.json: \
         {classes} {noun} get no CRAP or mutation data and stay red; add \"{root}\" \
         to standardTools.sourceRoots in uml-quality.json"
    )
}

pub(crate) fn warn(policy: &Value, graph: &Value, adapter: &Adapter) {
    for Uncovered { root, classes } in uncovered(policy, graph, adapter) {
        eprintln!("{}", warning(&root, classes));
    }
}
