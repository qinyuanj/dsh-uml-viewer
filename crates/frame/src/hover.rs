//! Optional bounded hover metadata for the viewer panel (#26).
//!
//! Selection, 依赖链高亮 and 关系淡化 are the resident's: a `select` answers a
//! frame that already carries them. Hover is the client's (ADR 0013), but two
//! things it needs are not visible in the frame's `edges`: which classes a
//! bundled relation passes through (`scene::highlight::touches` reads the
//! scene's `:via-ids`), and the dependency tooltip rows
//! (`commands::dependency_rows`). This module derives both from the scene, so
//! the panel renders them verbatim instead of inventing a dependency rule or a
//! metric of its own. The field is optional and additive to frame version 1;
//! `scene --frame` writes it only with `--hover-metadata`, so the checked-in
//! goldens are unchanged.
//!
//! Every bound below fails closed: exceeding one yields
//! `{"complete": false, "edges": [], "indicators": []}` rather than a silently
//! truncated list, and the client then falls back to the relation's own
//! endpoints and no tooltip.

use std::collections::HashSet;

use edn::Value;
use edn::record::{get, seq};
use serde_json::{Map, Value as Json, json};

use crate::commands::{self, PopupRow};
use crate::frame::role_json;

/// Visible relations plus 精简依赖标记 in one frame.
pub const MAX_ITEMS: usize = 1024;
/// `viaIds` on one relation.
pub const MAX_VIA_IDS: usize = 256;
/// Tooltip rows on one relation or indicator.
pub const MAX_ROWS_PER_ITEM: usize = 128;
/// Tooltip rows in the whole capability.
pub const MAX_ROWS: usize = 2048;
/// One id or one row text, in UTF-8 bytes.
pub const MAX_TEXT_BYTES: usize = 4096;
/// The serialized capability, in UTF-8 bytes.
pub const MAX_METADATA_BYTES: usize = 512 * 1024;

/// The optional `hover` field of a resident frame.
pub fn metadata(scene: &Value) -> Json {
    match build(scene) {
        Some((edges, indicators)) => {
            let value = json!({"complete": true, "edges": edges, "indicators": indicators});
            if compact_len(&value) <= MAX_METADATA_BYTES {
                value
            } else {
                incomplete()
            }
        }
        None => incomplete(),
    }
}

/// No usable capability; the shape stays the same so a consumer can check it.
fn incomplete() -> Json {
    json!({"complete": false, "edges": Vec::<Json>::new(), "indicators": Vec::<Json>::new()})
}

fn compact_len(value: &Json) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

/// An element id as the frame writes it: its EDN text, which round-trips.
fn edn_text(id: &Value) -> Option<String> {
    edn::encode(id).ok()
}

fn id_text(record: &Value, key: &str) -> Option<String> {
    get(record, key).and_then(edn_text)
}

/// `scene::highlight::touches`'s third clause: the classes a bundled relation
/// passes through. The scene writes them as `:via-ids`.
fn via_ids(edge: &Value) -> Vec<String> {
    match get(edge, "via-ids") {
        Some(Value::Vector(ids)) => ids.iter().filter_map(edn_text).collect(),
        _ => Vec::new(),
    }
}

/// `scene::hit::deps_of`: the relation's collapsed `:deps`, else a single leaf
/// record built from its own endpoints.
fn deps_of(edge: &Value) -> Vec<Value> {
    if let Some(Value::Vector(items)) = get(edge, "deps")
        && !items.is_empty()
    {
        return items.clone();
    }
    let (Some(from), Some(to)) = (get(edge, "from"), get(edge, "to")) else {
        return Vec::new();
    };
    vec![Value::Map(vec![
        (Value::keyword("from"), from.clone()),
        (Value::keyword("to"), to.clone()),
        (
            Value::keyword("violating"),
            Value::Bool(edn::record::flag(edge, "violating")),
        ),
    ])]
}

/// The tooltip rows of one relation or indicator: `commands::dependency_rows`.
fn rows(deps: &[Value]) -> Vec<PopupRow> {
    if deps.is_empty() {
        return Vec::new();
    }
    commands::dependency_rows(deps)
}

fn row_json(row: &PopupRow) -> Json {
    let mut object = Map::new();
    object.insert("text".into(), json!(row.text));
    object.insert("violating".into(), json!(row.violating));
    object.insert("ink".into(), role_json(row.color));
    Json::Object(object)
}

fn rows_json(rows: &[PopupRow]) -> Json {
    Json::Array(rows.iter().map(row_json).collect())
}

/// One `[x, y]` of a scene point vector, if it is one.
fn point_of(value: &Value) -> Option<[f64; 2]> {
    let Value::Vector(items) = value else {
        return None;
    };
    let [Value::Float(x), Value::Float(y)] = items.as_slice() else {
        return None;
    };
    Some([*x, *y])
}

/// Build the capability, or `None` when any bound is exceeded.
fn build(scene: &Value) -> Option<(Vec<Json>, Vec<Json>)> {
    let edges = seq(get(scene, "edges"));
    let indicators = seq(get(scene, "dep-indicators"));
    if edges.len() + indicators.len() > MAX_ITEMS {
        return None;
    }
    let mut total_rows = 0usize;
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut edge_entries = Vec::new();
    for edge in edges {
        let (Some(from), Some(to)) = (id_text(edge, "from"), id_text(edge, "to")) else {
            continue;
        };
        // Every stroke of one relation carries the same metadata; keep one.
        if !seen.insert((from.clone(), to.clone())) {
            continue;
        }
        let via = via_ids(edge);
        if via.len() > MAX_VIA_IDS {
            return None;
        }
        let deps = rows(&deps_of(edge));
        if deps.len() > MAX_ROWS_PER_ITEM {
            return None;
        }
        total_rows += deps.len();
        let mut object = Map::new();
        object.insert("from".into(), json!(from));
        object.insert("to".into(), json!(to));
        object.insert("viaIds".into(), json!(via));
        object.insert("rows".into(), rows_json(&deps));
        edge_entries.push(Json::Object(object));
    }
    let mut indicator_entries = Vec::new();
    for indicator in indicators {
        let Some(id) = id_text(indicator, "id") else {
            continue;
        };
        let triangle: Vec<[f64; 2]> = seq(get(indicator, "triangle"))
            .iter()
            .filter_map(point_of)
            .collect();
        if triangle.len() != 3 {
            continue;
        }
        let rows_src: Vec<Value> = match get(indicator, "deps") {
            Some(Value::Vector(items)) => items.clone(),
            _ => Vec::new(),
        };
        let deps = rows(&rows_src);
        if deps.len() > MAX_ROWS_PER_ITEM {
            return None;
        }
        total_rows += deps.len();
        let mut object = Map::new();
        object.insert("id".into(), json!(id));
        object.insert(
            "points".into(),
            Json::Array(
                triangle
                    .iter()
                    .map(|point| json!([point[0], point[1]]))
                    .collect(),
            ),
        );
        object.insert("rows".into(), rows_json(&deps));
        indicator_entries.push(Json::Object(object));
    }
    if total_rows > MAX_ROWS {
        return None;
    }
    // Ids and row text share the per-string bound: every string the panel draws.
    let mut strings: Vec<&str> = Vec::new();
    for entry in edge_entries.iter().chain(indicator_entries.iter()) {
        collect_strings(entry, &mut strings);
    }
    if strings.iter().any(|text| text.len() > MAX_TEXT_BYTES) {
        return None;
    }
    Some((edge_entries, indicator_entries))
}

/// Every string a JSON value carries, for the per-string byte bound.
fn collect_strings<'a>(value: &'a Json, out: &mut Vec<&'a str>) {
    match value {
        Json::String(text) => out.push(text),
        Json::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Json::Object(object) => {
            for (key, item) in object {
                out.push(key);
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    //! Source-only tests (#26): NOT RUN under the current resource restriction.
    use super::*;

    fn kw(name: &str) -> Value {
        Value::keyword(name)
    }

    fn edge(from: &str, to: &str, extra: Vec<(&str, Value)>) -> Value {
        let mut entries = vec![
            (Value::keyword("from"), kw(from)),
            (Value::keyword("to"), kw(to)),
        ];
        for (key, value) in extra {
            entries.push((Value::keyword(key), value));
        }
        Value::Map(entries)
    }

    fn scene(edges: Vec<Value>, indicators: Vec<Value>) -> Value {
        Value::Map(vec![
            (Value::keyword("edges"), Value::Vector(edges)),
            (Value::keyword("dep-indicators"), Value::Vector(indicators)),
        ])
    }

    fn dep(from: &str, to: &str, violating: bool) -> Value {
        Value::Map(vec![
            (Value::keyword("from"), kw(from)),
            (Value::keyword("to"), kw(to)),
            (Value::keyword("violating"), Value::Bool(violating)),
        ])
    }

    #[test]
    fn carries_bundled_edge_via_ids_and_violation_first_rows() {
        let bundled = edge(
            "api",
            "serde",
            vec![
                (
                    "via-ids",
                    Value::Vector(vec![kw("core.order"), kw("core.price")]),
                ),
                (
                    "deps",
                    Value::Vector(vec![
                        dep("api", "core.order", false),
                        dep("core.order", "serde", true),
                    ]),
                ),
            ],
        );
        assert_eq!(
            metadata(&scene(vec![bundled], vec![])),
            json!({
                "complete": true,
                "edges": [{
                    "from": ":api",
                    "to": ":serde",
                    "viaIds": [":core.order", ":core.price"],
                    "rows": [
                        {"text": "core.order -> serde", "violating": true, "ink": {"role": "violation"}},
                        {"text": "api -> core.order", "violating": false, "ink": {"role": "ink"}},
                    ],
                }],
                "indicators": [],
            })
        );
    }

    #[test]
    fn one_entry_per_relation_and_a_plain_edge_names_its_endpoints() {
        let pair = edge("api", "store", vec![]);
        let value = metadata(&scene(vec![pair.clone(), pair], vec![]));
        let edges = value["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["rows"][0]["text"], json!("api -> store"));
        assert_eq!(edges[0]["viaIds"], json!([]));
    }

    #[test]
    fn indicators_carry_their_triangle_and_rows() {
        let indicator = Value::Map(vec![
            (Value::keyword("id"), kw("core")),
            (
                Value::keyword("triangle"),
                Value::Vector(vec![
                    Value::Vector(vec![Value::Float(10.0), Value::Float(20.0)]),
                    Value::Vector(vec![Value::Float(16.0), Value::Float(24.0)]),
                    Value::Vector(vec![Value::Float(10.0), Value::Float(28.0)]),
                ]),
            ),
            (
                Value::keyword("deps"),
                Value::Vector(vec![dep("core", "util", true)]),
            ),
        ]);
        assert_eq!(
            metadata(&scene(vec![], vec![indicator]))["indicators"],
            json!([{
                "id": ":core",
                "points": [[10.0, 20.0], [16.0, 24.0], [10.0, 28.0]],
                "rows": [{"text": "core -> util", "violating": true, "ink": {"role": "violation"}}],
            }])
        );
    }

    #[test]
    fn every_bound_fails_closed_instead_of_truncating() {
        let too_many: Vec<Value> = (0..=MAX_ITEMS)
            .map(|n| edge(&format!("c{n}"), "b", vec![]))
            .collect();
        assert_eq!(metadata(&scene(too_many, vec![]))["complete"], json!(false));
        let long_via = edge(
            "a",
            "b",
            vec![(
                "via-ids",
                Value::Vector((0..=MAX_VIA_IDS).map(|n| kw(&format!("c{n}"))).collect()),
            )],
        );
        assert_eq!(
            metadata(&scene(vec![long_via], vec![]))["complete"],
            json!(false)
        );
        let many_rows = edge(
            "a",
            "b",
            vec![(
                "deps",
                Value::Vector(
                    (0..=MAX_ROWS_PER_ITEM)
                        .map(|_| dep("a", "b", false))
                        .collect(),
                ),
            )],
        );
        assert_eq!(
            metadata(&scene(vec![many_rows], vec![]))["complete"],
            json!(false)
        );
        let long_text = "x".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(
            metadata(&scene(vec![edge(&long_text, "b", vec![])], vec![]))["complete"],
            json!(false)
        );
        // The whole-metadata byte bound: 16 relations of 128 maximal rows is
        // exactly MAX_ROWS rows, each within MAX_TEXT_BYTES, and still 8 MiB.
        let text = "y".repeat(MAX_TEXT_BYTES / 2 - 2);
        let mut wide = Vec::new();
        for n in 0..(MAX_ROWS / MAX_ROWS_PER_ITEM) {
            wide.push(edge(
                &format!("from{n}"),
                "b",
                vec![(
                    "deps",
                    Value::Vector(
                        (0..MAX_ROWS_PER_ITEM)
                            .map(|_| dep(&text, &text, false))
                            .collect(),
                    ),
                )],
            ));
        }
        let value = metadata(&scene(wide, vec![]));
        assert_eq!(value["complete"], json!(false));
        assert_eq!(value["edges"], json!([]));
    }

    #[test]
    fn an_empty_scene_is_complete_and_empty() {
        assert_eq!(
            metadata(&Value::Map(vec![])),
            json!({"complete": true, "edges": [], "indicators": []})
        );
        assert_eq!(metadata(&Value::Nil)["edges"], json!([]));
    }
}
