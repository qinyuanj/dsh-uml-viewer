//! qinyuanj/dsh-uml-viewer#83: a policy `:sources` root that
//! `uml-quality.json` gives no quality provider for can never receive CRAP or
//! mutation data, so its classes stay red for a configuration reason. The red
//! rule is unchanged — a class with no data is red, never hidden (AGENTS.md) —
//! this module only names the gap so a reader cannot mistake it for a score.
//!
//! This is the one seam where both scopes are known at once: the `ir` role has
//! read the policy, built the class graph (each real class carries its
//! project-relative `:file`) and knows the project root that holds
//! `uml-quality.json`. `quality` alone never constructs classes, so it cannot
//! count what a missing provider strands.

use std::path::Path;

use edn::Value;

use crate::files::Adapter;
use crate::value::get_truthy;

/// A policy source root with no quality provider, and the classes it strands.
pub(crate) struct Uncovered {
    pub root: String,
    pub classes: usize,
}

/// Project-relative, `/`-separated, no `./`, no trailing `/`; the empty root
/// is the project root.
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

/// Whether quality source root `quality` scores everything under `source`.
/// An empty declared set is the whole project (the default dispatch), which
/// covers every policy root.
fn covers(quality: &str, source: &str) -> bool {
    if quality == "." {
        return true;
    }
    source == quality || source.starts_with(&format!("{quality}/"))
}

/// The standard-tools scope `uml-quality.json` declares. Empty means the whole
/// tree: no file, no `standardTools`, no `sourceRoots`, or an `operations`
/// override (which defines its own scope and is not described here).
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

/// The policy's source roots exactly as the scanner resolves them: one per
/// `:sources` entry, or the single `:src` (default `src`).
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

/// A source's `:root`/`:src`, falling back to the policy's `:src`, then `src`.
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

/// The project-relative files of the real classes the graph holds. Foreign
/// stubs are ovals with no quality data by design, not stranded classes.
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

/// The policy source roots no quality provider reaches, each with the number
/// of classes it strands. A root without a class is not reported: it leaves
/// nothing red.
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

/// The warning line: the root, how many classes it strands, and the exact edit
/// `uml-quality.json` needs.
pub(crate) fn warning(root: &str, classes: usize) -> String {
    let noun = if classes == 1 { "class" } else { "classes" };
    format!(
        "uml: source root \"{root}\" has no quality provider in uml-quality.json: \
         {classes} {noun} get no CRAP or mutation data and stay red; add \"{root}\" \
         to standardTools.sourceRoots in uml-quality.json"
    )
}

/// Warn on stderr for every uncovered root (CI and local output both see it).
pub(crate) fn warn(policy: &Value, graph: &Value, adapter: &Adapter) {
    for Uncovered { root, classes } in uncovered(policy, graph, adapter) {
        eprintln!("{}", warning(&root, classes));
    }
}
