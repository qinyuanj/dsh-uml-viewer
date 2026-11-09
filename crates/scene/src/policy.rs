//! Policy helpers shared by namespace/proposal views (port of the view-side
//! half of `love/uml/domain/policy.lua`: merge/kinds/violations/proposals).
//! `apply_policy` itself belongs to the IR generator (#136).

use edn::Value;
use edn::record::{drop_keys, flag, get, local_name, name, numval, seq};

pub const PROPOSAL_NOTICE: &str = "PROPOSAL — not instantiated in code";

fn default<'a>(v: Option<&'a Value>, d: &'a Value) -> &'a Value {
    match v {
        Some(x) if edn::record::truthy(v) => x,
        _ => d,
    }
}

fn id(x: &Value) -> Value {
    Value::keyword(local_name(x).unwrap_or_default())
}

fn ids(xs: &[Value]) -> Vec<Value> {
    xs.iter().map(id).collect()
}

fn matches(x: &Value, p: &Value) -> bool {
    let (x, p) = (name(x), name(p));
    x == p || x.starts_with(&format!("{p}."))
}

/// Is the class id under any `:omit` prefix?
pub fn omitted_id(x: &Value, omit: Option<&Value>) -> bool {
    seq(omit).iter().any(|p| matches(x, p))
}

pub fn kind_rank(k: Option<&Value>) -> i64 {
    match k.map(name).as_deref() {
        Some("implements") | Some("inheritance") => 4,
        Some("composition") => 3,
        Some("aggregation") => 2,
        Some("association") => 1,
        _ => 0,
    }
}

/// The original leaf relationship carried by a visible edge. Keeping kind
/// with the endpoints lets dependency highlighting ignore stronger parallel
/// association/inheritance arrows without losing a bundled dependency.
pub(crate) fn leaf_dependency(e: &Value) -> Value {
    Value::Map(vec![
        (
            Value::keyword("from"),
            get(e, "orig-from")
                .or_else(|| get(e, "from"))
                .cloned()
                .unwrap_or(Value::Nil),
        ),
        (
            Value::keyword("to"),
            get(e, "orig-to")
                .or_else(|| get(e, "to"))
                .cloned()
                .unwrap_or(Value::Nil),
        ),
        (
            Value::keyword("kind"),
            get(e, "kind").cloned().unwrap_or(Value::Nil),
        ),
        (
            Value::keyword("violating"),
            Value::Bool(edn::record::is_true(e, "violating")),
        ),
    ])
}

/// Merge parallel edges per directed endpoint pair; the strongest kind wins,
/// violations are preserved, and output order follows Clojure's group-by.
pub fn merge_edges(edges: &[Value]) -> Value {
    struct Group {
        pair: (Value, Value),
        violating: bool,
        best: Value,
        deps: Vec<Value>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for e in edges {
        let pair = (
            get(e, "from").cloned().unwrap_or(Value::Nil),
            get(e, "to").cloned().unwrap_or(Value::Nil),
        );
        let gi = groups
            .iter()
            .position(|g| g.pair == pair)
            .unwrap_or_else(|| {
                groups.push(Group {
                    pair,
                    violating: false,
                    best: e.clone(),
                    deps: Vec::new(),
                });
                groups.len() - 1
            });
        let group = &mut groups[gi];
        group.deps.push(leaf_dependency(e));
        // The first edge of a new pair seeded `best` above; still apply the
        // kind comparison so an equal-rank later edge wins, as in Lua.
        group.violating = group.violating || edn::record::is_true(e, "violating");
        if kind_rank(get(e, "kind")) >= kind_rank(get(&group.best, "kind")) {
            group.best = e.clone();
        }
    }
    let best: Vec<Value> = groups
        .iter()
        .map(|group| {
            let mut b = drop_keys(&group.best, &["violating"]);
            edn::record::put(&mut b, "deps", Value::Vector(group.deps.clone()));
            if get(&b, "kind") == Some(&Value::keyword("dependency")) && group.violating {
                edn::record::put(&mut b, "violating", Value::Bool(true));
            }
            b
        })
        .collect();
    let pairs: Vec<(Value, Value)> = groups.into_iter().map(|g| g.pair).collect();
    let keys: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(from, to)| (keyword_name(from), keyword_name(to)))
        .collect();
    Value::Vector(
        edn::pair_order::indices(&keys)
            .into_iter()
            .map(|i| best[i].clone())
            .collect(),
    )
}

/// A pair key is a keyword (the IR's edge endpoints); anything else would
/// make Clojure's `group-by` key shape differ, so it is a programming error.
fn keyword_name(value: &Value) -> &str {
    match value {
        Value::Keyword(n) => n.as_str(),
        _ => panic!("pair order only hashes keyword pairs"),
    }
}

/// Restamp `:violating` on dependency edges that climb the rank order, and
/// `:level` on classes, from a rank map.
pub fn restamp_ranks(classes: &[Value], edges: &[Value], ranks: &Value) -> Value {
    Value::Map(vec![
        (
            Value::keyword("classes"),
            Value::Vector(with_levels(classes, ranks, true)),
        ),
        (Value::keyword("edges"), mark_violations(edges, ranks)),
    ])
}

pub fn rank_of(x: &Value, ranks: &Value) -> Option<f64> {
    let mut n = Some(name(x));
    while let Some(cur) = n {
        if let Some(v) = ranks.get(&Value::keyword(&cur))
            && edn::record::truthy(Some(v))
        {
            return edn::record::num(v);
        }
        n = cur.rsplit_once('.').map(|(parent, _)| parent.to_owned());
    }
    None
}

pub fn violating_dependency(e: &Value, ranks: &Value) -> bool {
    if get(e, "kind") != Some(&Value::keyword("dependency")) {
        return false;
    }
    match (
        rank_of(get(e, "from").unwrap(), ranks),
        rank_of(get(e, "to").unwrap(), ranks),
    ) {
        (Some(f), Some(t)) => f < t,
        _ => false,
    }
}

pub fn mark_violations(edges: &[Value], ranks: &Value) -> Value {
    Value::Vector(
        edges
            .iter()
            .map(|e| {
                let mut c = drop_keys(e, &["violating"]);
                if violating_dependency(e, ranks) {
                    edn::record::put(&mut c, "violating", Value::Bool(true));
                }
                c
            })
            .collect(),
    )
}

fn with_levels(classes: &[Value], ranks: &Value, clear: bool) -> Vec<Value> {
    classes
        .iter()
        .map(|c| {
            let mut out = if clear {
                drop_keys(c, &["level"])
            } else {
                c.clone()
            };
            if !flag(c, "foreign")
                && let Some(rank) = rank_of(get(c, "id").unwrap(), ranks)
            {
                edn::record::put(&mut out, "level", numval(rank));
            }
            out
        })
        .collect()
}

/// Apply `:edge-kinds` overrides and drop `:omit-edges` pairs.
pub fn apply_edge_kinds(edges: &[Value], kinds: Option<&Value>, omit: Option<&Value>) -> Value {
    let empty = Value::Map(vec![]);
    let kinds = default(kinds, &empty);
    let mut out = Vec::new();
    for e in edges {
        let pair = vec![
            get(e, "from").cloned().unwrap_or(Value::Nil),
            get(e, "to").cloned().unwrap_or(Value::Nil),
        ];
        let omitted = seq(omit).iter().any(|p| ids(seq(Some(p))) == pair);
        if omitted {
            continue;
        }
        let mut c = e.clone();
        if let Some(k) = kinds.get(&Value::Vector(pair.clone()))
            && edn::record::truthy(Some(k))
        {
            edn::record::put(&mut c, "kind", k.clone());
            // The override replaces the relation represented by this pair,
            // including leaves cached before merging. Highlighting traverses
            // those leaves, so retaining scanned kinds would contradict it.
            if let Some(Value::Vector(deps)) = get(&c, "deps") {
                let deps = deps
                    .iter()
                    .map(|dep| {
                        let mut dep = dep.clone();
                        edn::record::put(&mut dep, "kind", k.clone());
                        if k != &Value::keyword("dependency") {
                            dep = drop_keys(&dep, &["violating"]);
                        }
                        dep
                    })
                    .collect();
                edn::record::put(&mut c, "deps", Value::Vector(deps));
            }
        }
        if get(&c, "kind") != Some(&Value::keyword("dependency")) {
            c = drop_keys(&c, &["violating"]);
        }
        out.push(c);
    }
    Value::Vector(out)
}

/// Flatten an nse tree into its leaf/class ids, in encounter order.
pub fn nse_ids(nses: Option<&Value>) -> Vec<Value> {
    let mut out = Vec::new();
    for x in seq(nses) {
        if let Value::Map(_) = x {
            out.extend(nse_ids(get(x, "nses")));
        } else {
            out.push(id(x));
        }
    }
    out
}

fn nse_entry(x: &Value) -> Value {
    if !matches!(x, Value::Map(_)) {
        return id(x);
    }
    let cid = id(default(
        get(x, "id").or_else(|| get(x, "label")),
        &Value::Nil,
    ));
    let nses: Vec<Value> = seq(get(x, "nses")).iter().map(nse_entry).collect();
    let mut out = x.clone();
    edn::record::put(&mut out, "id", cid.clone());
    let label = Value::String(name(&cid));
    edn::record::put(&mut out, "label", default(get(x, "label"), &label).clone());
    edn::record::put(&mut out, "nses", Value::Vector(nses));
    out
}

fn layers_from(xs: Option<&Value>) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, x) in seq(xs).iter().enumerate() {
        if matches!(x, Value::Map(_)) {
            let fallback = Value::keyword(&format!("layer-{i}"));
            let cid = id(default(
                get(x, "id").or_else(|| get(x, "label")).or(Some(&fallback)),
                &fallback,
            ));
            let nses: Vec<Value> = seq(get(x, "nses")).iter().map(nse_entry).collect();
            let mut out_x = x.clone();
            edn::record::put(&mut out_x, "id", cid.clone());
            let label = Value::String(name(&cid));
            edn::record::put(
                &mut out_x,
                "label",
                default(get(x, "label"), &label).clone(),
            );
            edn::record::put(&mut out_x, "nses", Value::Vector(nses));
            out.push(out_x);
        }
    }
    out
}

/// Normalize a proposal body (map or bare layers vector); None when it has
/// no layers.
pub fn normalize_proposal(p: Option<&Value>) -> Option<Value> {
    let p = p?;
    let layers = match p {
        Value::Vector(_) => layers_from(Some(p)),
        Value::Map(_) => layers_from(get(p, "layers")),
        _ => return None,
    };
    if layers.is_empty() {
        return None;
    }
    let notice = Value::String(PROPOSAL_NOTICE.into());
    let mut r = Value::Map(vec![
        (
            Value::keyword("notice"),
            match p {
                Value::Map(_) => default(get(p, "notice"), &notice).clone(),
                _ => notice.clone(),
            },
        ),
        (Value::keyword("layers"), Value::Vector(layers)),
    ]);
    if matches!(p, Value::Map(_)) {
        edn::record::put(&mut r, "omit", Value::Vector(ids(seq(get(p, "omit")))));
    }
    Some(r)
}

fn named(p: &Value, i: usize) -> Option<Value> {
    if !matches!(p, Value::Map(_)) {
        return None;
    }
    let fallback = Value::keyword(&format!("proposal-{i}"));
    let cid = id(default(get(p, "id"), &fallback));
    let normalized = normalize_proposal(Some(p));
    let mut out = p.clone();
    edn::record::put(&mut out, "id", cid.clone());
    let fallback_name = Value::String(name(&cid));
    let name_v = default(get(p, "name").or_else(|| get(p, "label")), &fallback_name);
    let name_s = match name_v {
        Value::String(s) => Value::String(s.clone()),
        other => Value::String(name(other)),
    };
    edn::record::put(&mut out, "name", name_s);
    edn::record::put(
        &mut out,
        "layers",
        normalized
            .as_ref()
            .and_then(|n| get(n, "layers").cloned())
            .unwrap_or(Value::Vector(vec![])),
    );
    edn::record::put(&mut out, "omit", Value::Vector(ids(seq(get(p, "omit")))));
    let notice = Value::String(PROPOSAL_NOTICE.into());
    edn::record::put(
        &mut out,
        "notice",
        default(get(p, "notice"), &notice).clone(),
    );
    Some(out)
}

/// Named proposals of a policy or IR document (`:proposals`, then the
/// legacy singular `:proposal`).
pub fn named_proposals(x: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    match get(x, "proposals") {
        Some(Value::Vector(xs)) => {
            for (i, p) in xs.iter().enumerate() {
                if let Some(n) = named(p, i) {
                    out.push(n);
                }
            }
        }
        _ => {
            if let Some(one) = get(x, "proposal") {
                match one {
                    Value::Map(_)
                        if get(one, "name").is_some() || !seq(get(one, "layers")).is_empty() =>
                    {
                        if let Some(n) = named(one, 0) {
                            out.push(n);
                        }
                    }
                    Value::Vector(_) => {
                        if let Some(n) = normalize_proposal(Some(one)) {
                            out.push(Value::Map(vec![
                                (Value::keyword("id"), Value::keyword("proposal")),
                                (Value::keyword("name"), Value::String("Proposal".into())),
                                (
                                    Value::keyword("layers"),
                                    get(&n, "layers").cloned().unwrap_or(Value::Vector(vec![])),
                                ),
                                (
                                    Value::keyword("notice"),
                                    get(&n, "notice").cloned().unwrap_or(Value::Nil),
                                ),
                            ]));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

pub fn proposal_by_id(x: &Value, want: Option<&Value>) -> Option<Value> {
    let want = want?;
    if matches!(want, Value::Nil) {
        return None;
    }
    let want_id = id(want);
    named_proposals(x)
        .into_iter()
        .find(|p| get(p, "id") == Some(&want_id))
}

/// Layers of the first named proposal, else the normalized singular form.
#[cfg(test)]
pub fn proposal_layers(x: &Value) -> Vec<Value> {
    if let Some(first) = named_proposals(x).into_iter().next() {
        return seq(get(&first, "layers")).to_vec();
    }
    let value = if edn::record::get_raw(x, "proposal").is_some() {
        get(x, "proposal").cloned()
    } else {
        Some(x.clone())
    };
    normalize_proposal(value.as_ref())
        .map(|n| seq(get(&n, "layers")).to_vec())
        .unwrap_or_default()
}

pub fn ranks_from_layers(layers: &[Value]) -> Value {
    let levels: Vec<Value> = layers
        .iter()
        .map(|layer| {
            if matches!(layer, Value::Map(_)) {
                Value::Vector(nse_ids(get(layer, "nses")))
            } else {
                layer.clone()
            }
        })
        .collect();
    let mut r = Value::Map(vec![]);
    for (i, group) in levels.iter().enumerate() {
        for x in seq(Some(group)) {
            r.assoc(x.clone(), Value::Integer(i as i64));
        }
    }
    r
}
