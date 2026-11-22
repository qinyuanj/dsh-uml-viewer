use std::collections::HashMap;

use crate::value::{contains, copy_map, encode_key, get_truthy, id, id_name, ids};
use edn::Value;
use edn::pair_order;
use edn::record::{flag, get, num, put, record, remove, seq, truthy};

fn matches_prefix(x: &Value, p: &Value) -> bool {
    let (x, p) = (id_name(x), id_name(p));
    x == p || x.starts_with(&format!("{p}."))
}

pub fn hierarchical(p: &Value) -> bool {
    edn::record::is_true(p, "hierarchical")
        || (get(p, "packages").is_none() && get(p, "diagrams").is_none())
}

pub fn assigned(p: &Value) -> Vec<Value> {
    let mut r = Vec::new();
    for pkg in seq(get(p, "packages")) {
        for x in ids(seq(get(pkg, "nses"))) {
            if !contains(&r, &x) {
                r.push(x);
            }
        }
    }
    r
}

pub fn unassigned(p: &Value, g: &Value) -> Vec<Value> {
    let mut r = Vec::new();
    if hierarchical(p) {
        return r;
    }
    let assigned = assigned(p);
    for c in seq(get(g, "classes")) {
        if !flag(c, "foreign") && !contains(&assigned, get(c, "id").unwrap()) {
            r.push(c.clone());
        }
    }
    r.sort_by_key(|a| id_name(get(a, "id").unwrap()));
    r
}

fn kind_rank(k: Option<&Value>) -> i64 {
    match k.map(id_name) {
        Some(n) if n == "implements" || n == "inheritance" => 4,
        Some(n) if n == "composition" => 3,
        Some(n) if n == "aggregation" => 2,
        Some(n) if n == "association" => 1,
        _ => 0,
    }
}

fn pair_names(pair: &Value) -> (String, String) {
    let raw = |v: &Value| match v {
        Value::Keyword(n) | Value::Symbol(n) | Value::String(n) => n.clone(),
        other => id_name(other),
    };
    match pair {
        Value::Vector(items) if items.len() == 2 => (raw(&items[0]), raw(&items[1])),
        _ => (String::new(), String::new()),
    }
}

pub fn merge_edges(edges: &[Value]) -> Value {
    struct Group {
        index: usize,
        violating: bool,
        best: Option<Value>,
    }
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut out: Vec<Value> = Vec::new();
    let mut pairs: Vec<Value> = Vec::new();
    for e in edges {
        let pair = Value::Vector(vec![
            get(e, "from").cloned().unwrap_or(Value::Nil),
            get(e, "to").cloned().unwrap_or(Value::Nil),
        ]);
        let key = encode_key(&pair);
        let group = groups.entry(key).or_insert_with(|| {
            out.push(e.clone());
            pairs.push(pair.clone());
            Group {
                index: out.len() - 1,
                violating: false,
                best: None,
            }
        });
        group.violating = group.violating || edn::record::is_true(e, "violating");
        let better = match &group.best {
            None => true,
            Some(best) => kind_rank(get(e, "kind")) >= kind_rank(get(best, "kind")),
        };
        if better {
            group.best = Some(e.clone());
        }
        let mut best = copy_map(group.best.as_ref().unwrap());
        remove(&mut best, "violating");
        if get(&best, "kind") == Some(&Value::keyword("dependency")) && group.violating {
            put(&mut best, "violating", Value::Bool(true));
        }
        out[group.index] = best;
    }
    let names: Vec<(String, String)> = pairs.iter().map(pair_names).collect();
    let keys: Vec<(&str, &str)> = names
        .iter()
        .map(|(f, t)| (f.as_str(), t.as_str()))
        .collect();
    Value::Vector(
        pair_order::indices(&keys)
            .into_iter()
            .map(|i| out[i].clone())
            .collect(),
    )
}

pub fn collapse_graph(p: &Value, g: &Value) -> Value {
    let mut remap: HashMap<String, Value> = HashMap::new();
    let mut classes: Vec<Value> = Vec::new();
    let mut foreigns: HashMap<String, Value> = HashMap::new();
    for c in seq(get(g, "classes")) {
        let cid = get(c, "id").cloned().unwrap_or(Value::Nil);
        if flag(c, "foreign") {
            let mut best: Option<Value> = None;
            for prefix in ids(seq(get(p, "foreign"))) {
                if matches_prefix(&cid, &prefix)
                    && best
                        .as_ref()
                        .is_none_or(|b| id_name(&prefix).len() > id_name(b).len())
                {
                    best = Some(prefix);
                }
            }
            if let Some(best) = best {
                remap.insert(encode_key(&cid), best.clone());
                foreigns.insert(id_name(&best), best);
            }
        } else {
            classes.push(c.clone());
        }
    }
    let mut sorted: Vec<String> = foreigns.keys().cloned().collect();
    sorted.sort();
    for n in sorted {
        let best = &foreigns[&n];
        classes.push(record(vec![
            ("id", best.clone()),
            ("name", Value::String(n.clone())),
            ("foreign", Value::Bool(true)),
        ]));
    }
    let present: std::collections::HashSet<String> = classes
        .iter()
        .filter_map(|c| get(c, "id"))
        .map(encode_key)
        .collect();
    let mut edges: Vec<Value> = Vec::new();
    for e in seq(get(g, "edges")) {
        let from = get(e, "from").cloned().unwrap_or(Value::Nil);
        let to = get(e, "to").cloned().unwrap_or(Value::Nil);
        let from = remap.get(&encode_key(&from)).cloned().unwrap_or(from);
        let to = remap.get(&encode_key(&to)).cloned().unwrap_or(to);
        if present.contains(&encode_key(&from)) && present.contains(&encode_key(&to)) && from != to
        {
            let mut c = copy_map(e);
            put(&mut c, "from", from);
            put(&mut c, "to", to);
            edges.push(c);
        }
    }
    record(vec![
        ("classes", Value::Vector(classes)),
        ("edges", merge_edges(&edges)),
    ])
}

pub const PROPOSAL_NOTICE: &str = "PROPOSAL — not instantiated in code";

fn nse_entry(x: &Value) -> Value {
    if !matches!(x, Value::Map(_)) {
        return id(x);
    }
    let cid = id(get_truthy(x, "id")
        .or_else(|| get_truthy(x, "label"))
        .unwrap_or(&Value::Nil));
    let nses: Vec<Value> = seq(get(x, "nses")).iter().map(nse_entry).collect();
    let mut out = copy_map(x);
    put(&mut out, "id", cid.clone());
    put(
        &mut out,
        "label",
        get_truthy(x, "label")
            .unwrap_or(&Value::String(id_name(&cid)))
            .clone(),
    );
    put(&mut out, "nses", Value::Vector(nses));
    out
}

pub fn nse_ids(nses: &[Value]) -> Vec<Value> {
    let mut r = Vec::new();
    for x in nses {
        if matches!(x, Value::Map(_)) {
            r.extend(nse_ids(seq(get(x, "nses"))));
        } else {
            r.push(id(x));
        }
    }
    r
}

fn layers_from(xs: &[Value]) -> Vec<Value> {
    let mut r = Vec::new();
    for (i, x) in xs.iter().enumerate() {
        if matches!(x, Value::Map(_)) {
            let fallback = Value::keyword(&format!("layer-{i}"));
            let cid = id(get_truthy(x, "id")
                .or_else(|| get_truthy(x, "label"))
                .unwrap_or(&fallback));
            let nses: Vec<Value> = seq(get(x, "nses")).iter().map(nse_entry).collect();
            let mut out = copy_map(x);
            put(&mut out, "id", cid.clone());
            put(
                &mut out,
                "label",
                get_truthy(x, "label")
                    .unwrap_or(&Value::String(id_name(&cid)))
                    .clone(),
            );
            put(&mut out, "nses", Value::Vector(nses));
            r.push(out);
        }
    }
    r
}

pub fn normalize_proposal(p: &Value) -> Option<Value> {
    if !matches!(p, Value::Map(_) | Value::Vector(_)) {
        return None;
    }
    let layers = match p {
        Value::Vector(items) => layers_from(items),
        _ => layers_from(seq(get(p, "layers"))),
    };
    if layers.is_empty() {
        return None;
    }
    let mut r = record(vec![
        (
            "notice",
            get_truthy(p, "notice")
                .unwrap_or(&Value::String(PROPOSAL_NOTICE.to_owned()))
                .clone(),
        ),
        ("layers", Value::Vector(layers)),
    ]);
    if matches!(p, Value::Map(_)) {
        put(&mut r, "omit", Value::Vector(ids(seq(get(p, "omit")))));
    }
    Some(r)
}

fn named(p: &Value, i: usize) -> Option<Value> {
    if !matches!(p, Value::Map(_)) {
        return None;
    }
    let cid = id(get_truthy(p, "id").unwrap_or(&Value::keyword(&format!("proposal-{i}"))));
    let normalized = normalize_proposal(p);
    let mut out = copy_map(p);
    put(&mut out, "id", cid.clone());
    let fallback = id_name(get_truthy(p, "label").unwrap_or(&cid));
    put(
        &mut out,
        "name",
        get_truthy(p, "name")
            .unwrap_or(&Value::String(fallback))
            .clone(),
    );
    put(
        &mut out,
        "layers",
        normalized
            .as_ref()
            .and_then(|n| get(n, "layers"))
            .cloned()
            .unwrap_or(Value::Vector(Vec::new())),
    );
    put(&mut out, "omit", Value::Vector(ids(seq(get(p, "omit")))));
    put(
        &mut out,
        "notice",
        get_truthy(p, "notice")
            .unwrap_or(&Value::String(PROPOSAL_NOTICE.to_owned()))
            .clone(),
    );
    Some(out)
}

pub fn named_proposals(x: &Value) -> Vec<Value> {
    let (xs, one) = (get(x, "proposals"), get(x, "proposal"));
    let mut r = Vec::new();
    if let Some(Value::Vector(items)) = xs {
        for (i, p) in items.iter().enumerate() {
            if let Some(n) = named(p, i) {
                r.push(n);
            }
        }
    } else if let Some(map @ Value::Map(_)) = one {
        if (flag(map, "name") || !seq(get(map, "layers")).is_empty())
            && let Some(n) = named(map, 0)
        {
            r.push(n);
        }
    } else if let Some(Value::Vector(_)) = one
        && let Some(n) = normalize_proposal(one.unwrap())
    {
        r.push(record(vec![
            ("id", Value::keyword("proposal")),
            ("name", Value::String("Proposal".to_owned())),
            ("layers", get(&n, "layers").cloned().unwrap_or(Value::Nil)),
            ("notice", get(&n, "notice").cloned().unwrap_or(Value::Nil)),
        ]));
    }
    r
}

pub fn proposal_layers(x: &Value) -> Vec<Value> {
    if let Some(first) = named_proposals(x).into_iter().next() {
        return seq(get(&first, "layers")).to_vec();
    }
    let value = match x {
        Value::Map(_) if edn::record::get_raw(x, "proposal").is_some() => {
            get(x, "proposal").cloned().unwrap_or(Value::Nil)
        }
        _ => x.clone(),
    };
    normalize_proposal(&value)
        .map(|n| seq(get(&n, "layers")).to_vec())
        .unwrap_or_default()
}

pub fn level_groups(p: &Value) -> Vec<Value> {
    let mut r = Vec::new();
    let levels = seq(get(p, "levels"));
    if !levels.is_empty() {
        for group in levels {
            r.push(Value::Vector(ids(match group {
                Value::Map(_) => seq(get(group, "nses")),
                other => seq(Some(other)),
            })));
        }
    } else {
        for layer in proposal_layers(p) {
            r.push(Value::Vector(nse_ids(seq(get(&layer, "nses")))));
        }
    }
    r
}

pub fn level_ranks(p: &Value) -> Value {
    let mut r = Value::Map(Vec::new());
    for (i, group) in level_groups(p).iter().enumerate() {
        for x in seq(Some(group)) {
            r.assoc(x.clone(), Value::Integer(i as i64));
        }
    }
    r
}

pub fn rank_of<'a>(x: &Value, ranks: &'a Value) -> Option<&'a Value> {
    let mut n = Some(id_name(x));
    while let Some(current) = n {
        if let Some(rank) = ranks.get(&Value::keyword(&current))
            && truthy(Some(rank))
        {
            return Some(rank);
        }
        n = current
            .rsplit_once('.')
            .map(|(parent, _)| parent.to_owned());
    }
    None
}

fn violating_dependency(e: &Value, ranks: &Value) -> bool {
    if get(e, "kind") != Some(&Value::keyword("dependency")) {
        return false;
    }
    let f = rank_of(get(e, "from").unwrap(), ranks);
    let t = rank_of(get(e, "to").unwrap(), ranks);
    match (f, t) {
        (Some(a), Some(b)) => num(a).zip(num(b)).is_some_and(|(a, b)| a < b),
        _ => false,
    }
}

pub fn mark_violations(edges: &[Value], ranks: &Value) -> Vec<Value> {
    let mut r = Vec::new();
    for e in edges {
        let mut c = copy_map(e);
        remove(&mut c, "violating");
        if violating_dependency(e, ranks) {
            put(&mut c, "violating", Value::Bool(true));
        }
        r.push(c);
    }
    r
}

fn with_levels(classes: &[Value], ranks: &Value, clear: bool) -> Vec<Value> {
    let mut r = Vec::new();
    for c in classes {
        let mut out = if clear {
            {
                let mut m = copy_map(c);
                remove(&mut m, "level");
                m
            }
        } else {
            copy_map(c)
        };
        let rank = if !flag(c, "foreign") {
            get(c, "id").and_then(|cid| rank_of(cid, ranks))
        } else {
            None
        };
        if let Some(rank) = rank {
            put(&mut out, "level", rank.clone());
        }
        r.push(out);
    }
    r
}

pub fn apply_edge_kinds(edges: &[Value], kinds: &Value, omit: &[Value]) -> Vec<Value> {
    let empty = Value::Map(Vec::new());
    let kinds = if truthy(Some(kinds)) { kinds } else { &empty };
    let mut r = Vec::new();
    for e in edges {
        let pair = Value::Vector(vec![
            get(e, "from").cloned().unwrap_or(Value::Nil),
            get(e, "to").cloned().unwrap_or(Value::Nil),
        ]);
        let omitted = omit
            .iter()
            .any(|p| Value::Vector(ids(seq(Some(p)))) == pair);
        if omitted {
            continue;
        }
        let mut c = copy_map(e);
        if let Some(k) = kinds.get(&pair)
            && truthy(Some(k))
        {
            put(&mut c, "kind", k.clone());
        }
        if get(&c, "kind") != Some(&Value::keyword("dependency")) {
            remove(&mut c, "violating");
        }
        r.push(c);
    }
    r
}

fn ir_class(c: &Value, hide: bool) -> Value {
    let mut r = record(vec![
        ("id", get(c, "id").cloned().unwrap_or(Value::Nil)),
        ("name", get_truthy(c, "name").unwrap_or(&Value::Nil).clone()),
    ]);
    for key in ["ns", "lang", "file", "stereotype"] {
        if let Some(v) = get(c, key)
            && truthy(Some(v))
        {
            put(&mut r, key, v.clone());
        }
    }
    if flag(c, "foreign") {
        put(&mut r, "shape", Value::keyword("oval"));
        r
    } else if hide {
        put(&mut r, "hide-members", Value::Bool(true));
        r
    } else {
        r
    }
}

fn packaged_document(p: &Value, g: &Value) -> Value {
    let index: HashMap<String, &Value> = seq(get(g, "classes"))
        .iter()
        .map(|c| (encode_key(get(c, "id").unwrap()), c))
        .collect();
    let lookup = |cid: &Value| index.get(&encode_key(&id(cid))).copied();
    let visible = |nses: Vec<Value>, hide: bool| {
        nses.iter()
            .filter_map(&lookup)
            .map(|c| ir_class(c, hide))
            .collect::<Vec<_>>()
    };
    let mut diagrams = Vec::new();
    for d in seq(get(p, "diagrams")) {
        let overview = get(d, "view") == Some(&Value::keyword("overview"));
        let mut packages: Vec<Value> = Vec::new();
        let mut home: Vec<Value> = Vec::new();
        let mut all: Vec<Value> = Vec::new();
        let hide = flag(d, "hide-members");
        if overview {
            for pkg in seq(get(p, "packages")) {
                packages.push(record(vec![
                    ("id", id(get(pkg, "id").unwrap())),
                    (
                        "label",
                        get_truthy(pkg, "label").unwrap_or(&Value::Nil).clone(),
                    ),
                    (
                        "classes",
                        Value::Vector(visible(ids(seq(get(pkg, "nses"))), hide)),
                    ),
                ]));
            }
            let extra = unassigned(p, g);
            if !extra.is_empty() {
                let cs: Vec<Value> = extra.iter().map(|c| ir_class(c, hide)).collect();
                packages.push(record(vec![
                    ("id", Value::keyword("unassigned")),
                    ("label", Value::String("Unassigned".to_owned())),
                    ("classes", Value::Vector(cs)),
                ]));
            }
            for pkg in &packages {
                for c in seq(get(pkg, "classes")) {
                    home.push(get(c, "id").cloned().unwrap_or(Value::Nil));
                }
            }
            all = home.clone();
        } else {
            let pkg = seq(get(p, "packages"))
                .iter()
                .find(|candidate| {
                    id(get(candidate, "id").unwrap()) == id(get(d, "package").unwrap())
                })
                .unwrap_or_else(|| {
                    panic!(
                        "diagram package not in policy: {}",
                        id_name(get(d, "package").unwrap())
                    )
                });
            home = ids(seq(get(pkg, "nses")));
            let mut cs = visible(home.clone(), hide);
            let mut stubs: Vec<Value> = Vec::new();
            for e in seq(get(g, "edges")) {
                let target = get(e, "to").unwrap();
                if contains(&home, get(e, "from").unwrap())
                    && !contains(&home, target)
                    && !contains(&stubs, target)
                {
                    stubs.push(target.clone());
                }
            }
            for cid in &stubs {
                if let Some(c) = lookup(cid)
                    && !flag(c, "foreign")
                {
                    cs.push(ir_class(c, true));
                }
            }
            packages.push(record(vec![
                ("id", id(get(pkg, "id").unwrap())),
                (
                    "label",
                    get_truthy(pkg, "label").unwrap_or(&Value::Nil).clone(),
                ),
                ("classes", Value::Vector(cs.clone())),
            ]));
            for c in &cs {
                all.push(get(c, "id").cloned().unwrap_or(Value::Nil));
            }
        }
        let mut foreign: Vec<Value> = Vec::new();
        let mut wanted: Vec<Value> = Vec::new();
        for e in seq(get(g, "edges")) {
            let target = get(e, "to").unwrap();
            if let Some(c) = lookup(target)
                && contains(&home, get(e, "from").unwrap())
                && flag(c, "foreign")
                && !contains(&wanted, target)
            {
                wanted.push(target.clone());
            }
        }
        wanted.sort_by_key(id_name);
        for cid in &wanted {
            foreign.push(ir_class(lookup(cid).unwrap(), false));
            all.push(cid.clone());
        }
        let mut edges: Vec<Value> = Vec::new();
        for e in seq(get(g, "edges")) {
            if contains(&all, get(e, "from").unwrap())
                && contains(&all, get(e, "to").unwrap())
                && (overview
                    || contains(&home, get(e, "from").unwrap())
                    || contains(&home, get(e, "to").unwrap())
                    || get(e, "kind") == Some(&Value::keyword("implements")))
            {
                edges.push(e.clone());
            }
        }
        let mut diagram = record(vec![
            (
                "title",
                get_truthy(d, "title").unwrap_or(&Value::Nil).clone(),
            ),
            (
                "direction",
                id(
                    get_truthy(d, "direction").unwrap_or(&Value::String(if overview {
                        "tb".into()
                    } else {
                        "lr".into()
                    })),
                ),
            ),
            ("packages", Value::Vector(packages)),
            (
                "edges",
                Value::Vector(apply_edge_kinds(
                    &match merge_edges(&edges) {
                        Value::Vector(items) => items,
                        other => vec![other],
                    },
                    get_truthy(d, "edge-kinds").unwrap_or(&Value::Map(Vec::new())),
                    seq(get(d, "omit-edges")),
                )),
            ),
        ]);
        if !foreign.is_empty() {
            put(&mut diagram, "foreign", Value::Vector(foreign));
        }
        diagrams.push(diagram);
    }
    record(vec![
        (
            "title",
            get_truthy(p, "title")
                .unwrap_or(&Value::String("UML".to_owned()))
                .clone(),
        ),
        ("diagrams", Value::Vector(diagrams)),
    ])
}

pub fn apply_policy(p: &Value, g: &Value) -> Value {
    let g = collapse_graph(p, g);
    let proposals = named_proposals(p);
    let proposal = proposals
        .first()
        .and_then(normalize_proposal)
        .or_else(|| get(p, "proposal").and_then(normalize_proposal));
    let ranks = level_ranks(p);
    let classes = with_levels(seq(get(&g, "classes")), &ranks, false);
    let edges = mark_violations(seq(get(&g, "edges")), &ranks);
    let g = record(vec![
        ("classes", Value::Vector(classes)),
        ("edges", Value::Vector(edges)),
    ]);
    if !hierarchical(p) {
        return packaged_document(p, &g);
    }
    let mut doc = record(vec![
        (
            "title",
            get_truthy(p, "title")
                .unwrap_or(&Value::String("UML".to_owned()))
                .clone(),
        ),
        ("hierarchical", Value::Bool(true)),
        (
            "prefix",
            get_truthy(p, "prefix")
                .unwrap_or(&Value::String("uml-viewer".to_owned()))
                .clone(),
        ),
        ("order", Value::Vector(ids(seq(get(p, "order"))))),
        ("levels", Value::Vector(level_groups(p))),
        (
            "edge-kinds",
            get_truthy(p, "edge-kinds")
                .unwrap_or(&Value::Map(Vec::new()))
                .clone(),
        ),
        (
            "omit-edges",
            get_truthy(p, "omit-edges")
                .unwrap_or(&Value::Vector(Vec::new()))
                .clone(),
        ),
        ("omit", Value::Vector(ids(seq(get(p, "omit"))))),
        ("classes", get(&g, "classes").cloned().unwrap_or(Value::Nil)),
        ("edges", get(&g, "edges").cloned().unwrap_or(Value::Nil)),
    ]);
    if !proposals.is_empty() {
        put(&mut doc, "proposals", Value::Vector(proposals));
    }
    if let Some(proposal) = proposal {
        put(&mut doc, "proposal", proposal);
    }
    doc
}
