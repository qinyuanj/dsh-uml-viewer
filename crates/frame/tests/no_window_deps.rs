//! The moved modules stay window-free (#16): the resolved dependency
//! closure of `frame` in the workspace `Cargo.lock` holds no egui, eframe,
//! winit or wgpu package, so the panel's host binary and the egui window
//! share one model and only the window links a GUI stack.

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

#[test]
fn the_frame_crate_links_no_window_or_gui_stack() {
    let graph = lock_graph();
    assert!(graph.contains_key("frame"), "frame is a workspace package");
    let mut seen = BTreeSet::new();
    let mut todo = vec!["frame".to_owned()];
    while let Some(name) = todo.pop() {
        if seen.insert(name.clone()) {
            todo.extend(graph.get(&name).cloned().unwrap_or_default());
        }
    }
    let gui: Vec<&String> = seen
        .iter()
        .filter(|n| {
            n.starts_with("egui")
                || n.starts_with("eframe")
                || n.starts_with("winit")
                || n.starts_with("wgpu")
                || n.as_str() == "viewer"
        })
        .collect();
    assert!(gui.is_empty(), "frame depends on {gui:?}");
    // The closure is the model's own: IR codec, scene core and JSON.
    assert!(seen.contains("scene") && seen.contains("edn"));
    // Sanity: the lock graph does see the window's stack.
    assert!(graph["viewer"].iter().any(|d| d == "eframe"));
}
