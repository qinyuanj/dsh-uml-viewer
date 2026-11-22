//! Relationship highlighting: the selected class's outgoing dependency chain,
//! plus temporary edge hover. Membership is derived here; draw commands only
//! consume the resulting focus, chain and dimming predicates.

use edn::Value;
use edn::record::{get, local_name, name, seq};
use std::collections::HashSet;

pub(crate) fn box_id(hit: Option<&Value>) -> Option<Value> {
    let hit = hit?;
    match get(hit, "kind").map(name).as_deref() {
        Some("class") => get(hit, "id").cloned(),
        Some("child") | Some("port") => get(hit, "parent").cloned(),
        _ => None,
    }
}

pub fn touches(e: &Value, id: Option<&Value>) -> bool {
    let Some(id) = id else {
        return false;
    };
    if get(e, "from") == Some(id) || get(e, "to") == Some(id) {
        return true;
    }
    match get(e, "via-ids") {
        Some(Value::Vector(via)) => via.iter().any(|v| v == id),
        _ => false,
    }
}

fn same_edge(e: &Value, hit: Option<&Value>) -> bool {
    let Some(hit) = hit else {
        return false;
    };
    get(hit, "kind").and_then(local_name) == Some("edge")
        && get(e, "from") == get(hit, "from")
        && get(e, "to") == get(hit, "to")
}

fn key(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| edn::encode(v).ok())
}

fn collect_descendants(class: &Value, ids: &mut HashSet<String>) {
    for child in seq(get(class, "contents")) {
        if let Some(id) = key(get(child, "id")) {
            ids.insert(id);
        }
        collect_descendants(child, ids);
    }
    for ns in seq(get(class, "nses")) {
        if let Some(id) = key(Some(ns)) {
            ids.insert(id);
        }
    }
}

fn dependencies(e: &Value) -> Vec<&Value> {
    match get(e, "deps") {
        Some(Value::Vector(deps)) if !deps.is_empty() => deps
            .iter()
            .filter(|d| {
                get(d, "kind")
                    .or_else(|| get(e, "kind"))
                    .and_then(local_name)
                    == Some("dependency")
            })
            .collect(),
        _ if get(e, "kind").and_then(local_name) == Some("dependency") => vec![e],
        _ => vec![],
    }
}

/// Focus state derived from selection and hover.
pub struct Highlight {
    pub focus: Option<Value>,
    pub hover: Option<Value>,
    reachable: HashSet<String>,
    related: HashSet<String>,
}

impl Highlight {
    /// Build from a scene and a `{selected, hover}` view record.
    pub fn new(scene: &Value, selected: Option<&Value>, hover: Option<&Value>) -> Highlight {
        let focus = box_id(selected);
        let hover = box_id(hover);
        let edges = seq(get(scene, "edges"));
        let mut reachable = HashSet::new();
        if let Some(focus_key) = key(focus.as_ref()) {
            reachable.insert(focus_key);
            for class in seq(get(scene, "classes")) {
                if get(class, "id") == focus.as_ref() {
                    collect_descendants(class, &mut reachable);
                }
            }
            loop {
                let mut changed = false;
                for e in edges {
                    for dep in dependencies(e) {
                        if key(get(dep, "from")).is_some_and(|id| reachable.contains(&id))
                            && let Some(to) = key(get(dep, "to"))
                        {
                            changed |= reachable.insert(to);
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        let mut related = HashSet::new();
        for e in edges {
            if dependencies(e)
                .iter()
                .any(|dep| key(get(dep, "from")).is_some_and(|id| reachable.contains(&id)))
            {
                if let Some(id) = key(get(e, "from")) {
                    related.insert(id);
                }
                if let Some(id) = key(get(e, "to")) {
                    related.insert(id);
                }
            }
        }
        Highlight {
            focus,
            hover,
            reachable,
            related,
        }
    }

    /// A visible class or package on the focus's dependency chain, but not the
    /// focus itself. No off-screen classes are added to the scene.
    pub fn related(&self, id: &Value) -> bool {
        self.focus.as_ref().is_some_and(|f| f != id)
            && key(Some(id)).is_some_and(|k| self.related.contains(&k))
    }

    /// A visible class or package outside the focused dependency chain.
    pub fn dimmed(&self, id: &Value) -> bool {
        self.focus.is_some()
            && self.focus.as_ref() != Some(id)
            && !key(Some(id)).is_some_and(|k| self.related.contains(&k))
    }

    /// A visible edge carrying a dependency reachable from the focus.
    pub fn on_chain(&self, e: &Value) -> bool {
        self.focus.is_some()
            && dependencies(e)
                .iter()
                .any(|dep| key(get(dep, "from")).is_some_and(|id| self.reachable.contains(&id)))
    }

    /// The edge directly under the pointer.
    pub fn hot(&self, e: &Value, hovered: Option<&Value>) -> bool {
        same_edge(e, hovered)
    }

    /// An edge touching the hovered class.
    pub fn warm(&self, e: &Value) -> bool {
        touches(e, self.hover.as_ref())
    }
}
