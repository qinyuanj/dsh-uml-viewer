//! The viewer session does not depend on the companion (#17): the resolved
//! dependency closure of `viewer-session` in the workspace `Cargo.lock`
//! holds no `companion` package and no window stack, so the resident mode
//! the plugin host drives links neither Herdr nor egui.

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
    graph
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
    // Sanity: the lock graph does see the window stack where it is.
    assert!(graph["viewer"].iter().any(|d| d == "eframe"));
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
