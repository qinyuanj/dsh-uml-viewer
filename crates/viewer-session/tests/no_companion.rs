//! The viewer session does not depend on the companion (#17): the resolved
//! dependency closure of `viewer-session` in the workspace `Cargo.lock`
//! holds no `companion` package and no window stack, so the resident mode
//! the plugin host drives links neither Herdr nor a GUI toolkit.
//!
//! A lock entry lists a package's `[dev-dependencies]` beside the ones it
//! ships, and a test's dependencies never reach the binary, so the walk
//! leaves the dev-only names of every workspace crate out (#89).

use std::collections::{BTreeMap, BTreeSet};

/// `name -> dependency names` for every `[[package]]` of the lock file.
fn lock_graph() -> BTreeMap<String, Vec<String>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    let text = std::fs::read_to_string(path).unwrap();
    let mut graph = BTreeMap::new();
    for block in text.split("[[package]]").skip(1) {
        let mut name = None;
        let mut deps = Vec::new();
        let mut in_deps = false;
        for line in block.lines() {
            let line = line.trim();
            if let Some(n) = line.strip_prefix("name = ") {
                name = Some(n.trim_matches('"').to_owned());
            } else if line.starts_with("dependencies = [") {
                in_deps = true;
            } else if in_deps && line == "]" {
                in_deps = false;
            } else if in_deps {
                // `"name"` or `"name version"` (several versions locked).
                let dep = line.trim_end_matches(',').trim_matches('"');
                deps.push(dep.split(' ').next().unwrap().to_owned());
            }
        }
        if let Some(name) = name {
            graph.entry(name).or_insert_with(Vec::new).extend(deps);
        }
    }
    // Only the names a test alone needs leave the walk (#89).
    let names: Vec<String> = graph.keys().cloned().collect();
    for name in names {
        let (shipped, dev) = manifest_deps(&name);
        if let Some(deps) = graph.get_mut(&name) {
            deps.retain(|d| !dev.contains(d) || shipped.contains(d));
        }
    }
    graph
}

/// The dependency names a workspace crate's manifest declares: the ones it
/// ships, and the ones only its tests need.
fn manifest_deps(name: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let path = format!(
        "{}/../../crates/{name}/Cargo.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let Ok(text) = std::fs::read_to_string(path) else {
        return (BTreeSet::new(), BTreeSet::new());
    };
    let (mut shipped, mut dev) = (BTreeSet::new(), BTreeSet::new());
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_owned();
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(key) = line.split('=').next() else {
            continue;
        };
        let key = key.trim().to_owned();
        if section == "dev-dependencies" {
            dev.insert(key);
        } else if section == "dependencies" || section == "build-dependencies" {
            shipped.insert(key);
        }
    }
    (shipped, dev)
}

fn closure(graph: &BTreeMap<String, Vec<String>>, root: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![root.to_owned()];
    while let Some(name) = todo.pop() {
        if seen.insert(name.clone()) {
            todo.extend(graph.get(&name).cloned().unwrap_or_default());
        }
    }
    seen
}

#[test]
fn the_viewer_session_links_no_companion_and_no_window_stack() {
    let graph = lock_graph();
    assert!(graph.contains_key("viewer-session"), "a workspace package");
    let seen = closure(&graph, "viewer-session");
    let banned: Vec<&String> = seen
        .iter()
        .filter(|n| {
            matches!(n.as_str(), "companion" | "viewer" | "platform")
                || n.starts_with("egui")
                || n.starts_with("eframe")
                || n.starts_with("winit")
                || n.starts_with("wgpu")
        })
        .collect();
    assert!(banned.is_empty(), "viewer-session depends on {banned:?}");
    assert!(seen.contains("scene") && seen.contains("frame"));
    // #30 deleted the standalone window: its crate is not locked at all.
    assert!(!graph.contains_key("viewer"), "the window crate is locked");
}

#[test]
fn no_workspace_package_is_the_companion_any_more() {
    // #18 deleted the Herdr companion crate with its roles.
    let graph = lock_graph();
    assert!(!graph.contains_key("companion"), "the companion is locked");
    let users: Vec<&String> = graph
        .iter()
        .filter(|(_, deps)| deps.iter().any(|d| d == "companion"))
        .map(|(name, _)| name)
        .collect();
    assert!(users.is_empty(), "{users:?} still depend on the companion");
}
