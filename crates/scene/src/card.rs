use crate::document::ViewState;
use crate::hierarchy;
use crate::overlay;
use edn::Value;
use edn::record::{flag, get, local_name, seq};

/// One relationship of the carded class, seen from that class.
#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    /// The class at the other end.
    pub other: Value,
    /// The other class's `:name`, when its record is known.
    pub other_name: Option<Value>,
    pub kind: Option<Value>,
    pub label: Option<Value>,
    /// The carded class is the edge's `:from`.
    pub outgoing: bool,
}

/// The neighbourhood a class card is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbourhood {
    pub class: Value,
    pub relations: Vec<Relation>,
    /// The record of the class's `:package`, when the scene has it.
    pub package: Option<Value>,
    /// The diagram `:title`.
    pub title: Option<Value>,
    /// The document-level duplicate-structure summary (`:candidates`,
    /// `:attributed`, `:unattributed`, `:clusters`), `None` when no dry
    /// snapshot was read. The per-class state is on the class record's `:dry`.
    pub dry: Option<Value>,
}

fn named<'a>(scene: &'a Value, key: &str, id: &Value) -> Option<&'a Value> {
    seq(get(scene, key))
        .iter()
        .find(|c| get(c, "id") == Some(id))
}

/// The package box that holds class `id` in `scene`.
fn package_of(scene: &Value, id: &Value) -> Option<Value> {
    named(scene, "classes", id)
        .and_then(|c| get(c, "package"))
        .and_then(|p| named(scene, "packages", p))
        .cloned()
}

/// The neighbourhood of class `id` within one scene, or None when absent.
pub fn class_card_in(scene: &Value, id: &Value) -> Option<Neighbourhood> {
    let c = named(scene, "classes", id)?;
    let mut relations = Vec::new();
    for e in seq(get(scene, "edges")) {
        let outgoing = get(e, "from") == Some(id);
        if !(outgoing || get(e, "to") == Some(id)) {
            continue;
        }
        let other = get(e, if outgoing { "to" } else { "from" })
            .cloned()
            .unwrap_or(Value::Nil);
        relations.push(Relation {
            other_name: named(scene, "classes", &other).and_then(|o| get(o, "name").cloned()),
            other,
            kind: get(e, "kind").cloned(),
            label: get(e, "label").cloned(),
            outgoing,
        });
    }
    Some(Neighbourhood {
        class: c.clone(),
        relations,
        package: package_of(scene, id),
        title: get(scene, "diagram").and_then(|d| get(d, "title")).cloned(),
        dry: get(scene, "dry").cloned(),
    })
}

#[derive(Default)]
struct Merge {
    classes: Vec<Value>,
    seen: Vec<Value>,
    edges: Vec<Value>,
    seen_edges: Vec<Value>,
}

impl Merge {
    fn edges(&mut self, list: &[Value]) {
        for e in list {
            let key = Value::Vector(
                ["from", "to", "kind", "label"]
                    .iter()
                    .map(|k| get(e, k).cloned().unwrap_or(Value::Nil))
                    .collect(),
            );
            if !self.seen_edges.contains(&key) {
                self.seen_edges.push(key);
                self.edges.push(e.clone());
            }
        }
    }

    fn class(&mut self, c: &Value) {
        let cid = get(c, "id").cloned().unwrap_or(Value::Nil);
        if !self.seen.contains(&cid) {
            self.seen.push(cid);
            self.classes.push(c.clone());
        }
        for child in seq(get(c, "contents")) {
            self.class(child);
        }
    }

    fn view(&mut self, view: &Value) {
        self.edges(seq(get(view, "edges")));
        for p in seq(get(view, "packages")) {
            for c in seq(get(p, "classes")) {
                self.class(c);
            }
        }
        for key in ["classes", "foreign"] {
            for c in seq(get(view, key)) {
                self.class(c);
            }
        }
    }
}

/// The neighbourhood of class `id` in the current view: the visible scene
/// merged with the full overlaid document, so a card keeps its leaf
/// identities and relationships across namespaces.
pub fn class_card(state: &ViewState, id: &Value) -> Option<Neighbourhood> {
    let Some(doc) = &state.document else {
        return class_card_in(&state.scene, id);
    };
    let doc = overlay::apply(doc, &state.snapshots);
    let mut m = Merge::default();
    m.edges(seq(get(&doc, "edges")));
    for c in seq(get(&doc, "classes")) {
        m.class(c);
    }
    for p in seq(get(&doc, "packages")) {
        for c in seq(get(p, "classes")) {
            m.class(c);
        }
    }
    if flag(&doc, "hierarchical") {
        m.view(&hierarchy::view_at(&doc, &[]));
        let mut parent: Vec<Value> = local_name(id)
            .unwrap_or_default()
            .split('.')
            .map(Value::keyword)
            .collect();
        parent.pop();
        if !parent.is_empty() {
            let target = hierarchy::view_at(&doc, &parent);
            m.view(&target);
            for p in seq(get(&target, "packages")) {
                for c in seq(get(p, "classes")) {
                    if get(c, "id") != Some(id) || !flag(c, "drill?") {
                        continue;
                    }
                    for dir in ["in-deps", "out-deps"] {
                        for dep in seq(get(c, dir)) {
                            m.class(dep);
                            let other = get(dep, "id").cloned().unwrap_or(Value::Nil);
                            let (from, to) = if dir == "out-deps" {
                                (id.clone(), other)
                            } else {
                                (other, id.clone())
                            };
                            m.edges(&[Value::Map(vec![
                                (Value::keyword("from"), from),
                                (Value::keyword("to"), to),
                                (Value::keyword("kind"), Value::keyword("dependency")),
                            ])]);
                        }
                    }
                }
            }
        }
    }
    m.edges(seq(get(&state.scene, "edges")));
    for c in seq(get(&state.scene, "classes")) {
        m.class(c);
    }
    let merged = Value::Map(vec![
        (Value::keyword("classes"), Value::Vector(m.classes)),
        (Value::keyword("edges"), Value::Vector(m.edges)),
        (
            Value::keyword("packages"),
            get(&state.scene, "packages")
                .cloned()
                .unwrap_or(Value::Vector(vec![])),
        ),
        (
            Value::keyword("diagram"),
            get(&state.scene, "diagram").cloned().unwrap_or(Value::Nil),
        ),
    ]);
    let mut card = class_card_in(&merged, id)?;
    card.package = card.package.or_else(|| package_of(&state.scene, id));
    card.dry = get(&doc, "dry").cloned();
    Some(card)
}
