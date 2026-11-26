use crate::config;
use crate::dry;
use edn::Value;
use edn::record::{flag, local_name, name, numval, put, seq};
use std::collections::BTreeMap;

fn g<'a>(m: &'a Value, key: &str) -> Option<&'a Value> {
    edn::record::get(m, key)
}

/// One snapshot file record, as the metrics adapter produced it.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub path: String,
    pub text: String,
    pub mtime: Value,
    pub size: usize,
}

impl Snapshot {
    pub fn new(path: &str, text: &str, mtime: Value) -> Snapshot {
        Snapshot {
            path: path.into(),
            text: text.into(),
            mtime,
            size: text.len(),
        }
    }
}

pub struct Metrics {
    pub crap: BTreeMap<String, Vec<Value>>,
    pub mutate: BTreeMap<String, Value>,
    pub dry: Option<Vec<dry::Candidate>>,
}

enum Kind {
    Crap,
    Dry,
    Mutate,
}

fn kind(path: &str) -> Kind {
    if path.ends_with(".metrics/crap.edn") {
        Kind::Crap
    } else if path.ends_with(".metrics/dry.edn") {
        Kind::Dry
    } else {
        Kind::Mutate
    }
}

/// A snapshot's order key: `(seconds, nanoseconds)`.
///
/// The discovery sites encode a file as `"sec:nsec:size"`
/// ([`crate::snapshots::stamp`]); a bare number is seconds. Only the first two
/// fields are the key, so the size never orders anything. Anything unparseable
/// sorts first, so a record with no usable stamp never outranks one that has
/// one (#163).
fn time_key(mtime: &Value) -> (u64, u64) {
    match mtime {
        Value::Integer(n) => (*n as u64, 0),
        Value::Float(x) => (*x as u64, 0),
        Value::String(s) => {
            let mut parts = s.split(':');
            let secs = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            let nanos = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            (secs, nanos)
        }
        _ => (0, 0),
    }
}

pub fn load(files: &[Snapshot]) -> Metrics {
    let mut crap: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut mutate: BTreeMap<String, Value> = BTreeMap::new();
    let mut dry: Option<Vec<dry::Candidate>> = None;
    let mut ordered: Vec<&Snapshot> = files.iter().collect();
    ordered.sort_by(|a, b| {
        let (ah, al) = time_key(&a.mtime);
        let (bh, bl) = time_key(&b.mtime);
        if ah != bh {
            ah.cmp(&bh)
        } else if al != bl {
            al.cmp(&bl)
        } else {
            a.path.cmp(&b.path)
        }
    });
    for f in ordered {
        let Ok(data) = edn::parse(&f.text) else {
            continue;
        };
        if let Kind::Crap = kind(&f.path) {
            for entry in seq(g(&data, "entries")) {
                if let Some(Value::String(ns)) = g(entry, "namespace") {
                    crap.entry(ns.clone()).or_default().push(entry.clone());
                }
            }
        } else if let Kind::Dry = kind(&f.path) {
            let parsed = dry::candidates(&data);
            dry = Some(parsed);
        } else {
            let mut ns = g(&data, "namespace").cloned();
            if ns.is_none()
                && let Some(Value::String(source)) = g(&data, "source")
            {
                let derived = source.strip_prefix("src/").unwrap_or(source).to_owned();
                let no_ext = match derived.rsplit_once('.') {
                    Some((stem, ext)) if !ext.is_empty() => stem,
                    _ => &derived,
                };
                ns = Some(Value::String(no_ext.replace('/', ".")));
            }
            if let Some(Value::String(ns)) = ns {
                mutate.insert(ns, data);
            }
        }
    }
    Metrics { crap, mutate, dry }
}

fn doc_classes(doc: &Value) -> Vec<Value> {
    if flag(doc, "hierarchical") {
        return seq(g(doc, "classes")).to_vec();
    }
    let mut cs = Vec::new();
    let diagrams = match g(doc, "diagrams") {
        Some(Value::Vector(ds)) => ds.clone(),
        _ => vec![doc.clone()],
    };
    for d in &diagrams {
        for p in seq(g(d, "packages")) {
            cs.extend(seq(g(p, "classes")).iter().cloned());
        }
    }
    cs
}

pub(crate) fn owner<'a>(
    classes: &'a [Value],
    prefix: Option<&Value>,
    ns: &str,
) -> Option<&'a Value> {
    let ns = ns.replace("::", ".");
    let ns = ns.as_str();
    let mut root: Option<&Value> = None;
    let mut count = 0;
    for c in classes {
        if g(c, "lang").and_then(local_name) == Some("rust")
            && !flag(c, "foreign")
            && !local_name(g(c, "id").unwrap())
                .unwrap_or_default()
                .contains('.')
        {
            root = Some(c);
            count += 1;
        }
    }
    let mut best: Option<&Value> = None;
    let mut best_rank = i64::MAX;
    let mut best_len = i64::MAX;
    for c in classes {
        if flag(c, "foreign") || g(c, "shape").and_then(local_name) == Some("oval") {
            continue;
        }
        let class_id = local_name(g(c, "id").unwrap())
            .unwrap_or_default()
            .to_owned();
        let class_ns = g(c, "ns").and_then(|v| match v {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        });
        let (r, l) = if class_ns == Some(ns) {
            (0, 0)
        } else if class_id == ns {
            (1, 0)
        } else if ns.starts_with(&format!("{class_id}.")) {
            (2, -(class_id.len() as i64))
        } else if count == 1 && Some(c) == root && prefix.map(name).as_deref() == Some(ns) {
            (3, 0)
        } else {
            continue;
        };
        if r < best_rank || (r == best_rank && l < best_len) {
            best = Some(c);
            best_rank = r;
            best_len = l;
        }
    }
    best
}

fn sites(f: &Value) -> f64 {
    if let Some(n) = g(f, "sites").and_then(edn::record::num) {
        return n;
    }
    g(f, "killed").and_then(edn::record::num).unwrap_or(0.0)
        + g(f, "survived").and_then(edn::record::num).unwrap_or(0.0)
        + g(f, "uncovered").and_then(edn::record::num).unwrap_or(0.0)
}

fn paint(c: &mut Value, nss: &[String], metrics: &Metrics) {
    let (mut entries, mut form_order) = (Vec::new(), Vec::new());
    let (mut by_name, mut forms): (Ordered<Value>, Ordered<(Value, bool)>) =
        (Ordered::default(), Ordered::default());
    for ns in nss {
        for e in metrics.crap.get(ns).into_iter().flatten() {
            entries.push(e.clone());
            if let Some(n) = g(e, "name").cloned() {
                by_name.insert(name(&n), e.clone());
            }
        }
        if let Some(data) = metrics.mutate.get(ns) {
            for f in seq(g(data, "forms")) {
                let fid = match g(f, "id") {
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                };
                let private = fid.starts_with("defn-/");
                let fname = if private {
                    Some(fid[6..].to_owned())
                } else {
                    fid.strip_prefix("defn/").map(|rest| rest.to_owned())
                };
                if let Some(fname) = fname {
                    if !forms.contains(&fname) {
                        form_order.push(fname.clone());
                    }
                    forms.insert(fname, (f.clone(), private));
                }
            }
        }
    }
    let (mut names, mut seen, mut existing): (Vec<String>, Vec<String>, Ordered<Value>) =
        (Vec::new(), Vec::new(), Ordered::default());
    for op in seq(g(c, "ops")) {
        if let Some(n) = g(op, "name") {
            let n = name(n);
            if !seen.contains(&n) {
                names.push(n.clone());
                seen.push(n.clone());
            }
            if !existing.contains(&n) {
                existing.insert(n, op.clone());
            }
        }
    }
    for e in &entries {
        if let Some(n) = g(e, "name") {
            let n = name(n);
            if !seen.contains(&n) {
                names.push(n.clone());
                seen.push(n);
            }
        }
    }
    for n in &form_order {
        if !seen.contains(n) {
            names.push(n.clone());
            seen.push(n.clone());
        }
    }
    let mut ops: Vec<Value> = Vec::new();
    for op in seq(g(c, "ops")) {
        if g(op, "name").is_none() {
            ops.push(op.clone());
        }
    }
    for n in &names {
        let mut op = match existing.get(n) {
            Some(op) => op.clone(),
            None => Value::Map(vec![
                (Value::keyword("name"), Value::String(n.clone())),
                (Value::keyword("text"), Value::String(n.clone())),
            ]),
        };
        if g(&op, "text").is_none() {
            put(&mut op, "text", Value::String(n.clone()));
        }
        if let Some(e) = by_name.get(n) {
            put(
                &mut op,
                "cc",
                g(e, "complexity").cloned().unwrap_or(Value::Nil),
            );
            put(&mut op, "crap", g(e, "crap").cloned().unwrap_or(Value::Nil));
            let cov = g(e, "coverage").and_then(edn::record::num);
            put(
                &mut op,
                "coverage",
                cov.map(|v| numval(v / 100.0)).unwrap_or(Value::Nil),
            );
        }
        if let Some((form, private)) = forms.get(n) {
            let form = form.clone();
            let private = *private;
            for k in ["killed", "survived", "uncovered"] {
                put(
                    &mut op,
                    k,
                    numval(g(&form, k).and_then(edn::record::num).unwrap_or(0.0)),
                );
            }
            put(&mut op, "sites", numval(sites(&form)));
            if private {
                put(&mut op, "private", Value::Bool(true));
            }
        }
        ops.push(op);
    }
    if !ops.is_empty() {
        put(c, "ops", Value::Vector(ops));
    }
    let (mut scores, mut cov, mut cc) = (Vec::new(), Vec::new(), 0.0);
    for e in &entries {
        if let Some(s) = g(e, "crap").and_then(edn::record::num) {
            scores.push(Value::Float(s));
        }
        if let Some(v) = g(e, "coverage").and_then(edn::record::num) {
            cov.push(v);
        }
        cc += g(e, "complexity").and_then(edn::record::num).unwrap_or(0.0);
    }
    if !scores.is_empty() {
        if let Some(summary) = config::summarize_scores(&scores) {
            put(c, "crap", summary);
        }
        put(c, "cc", numval(cc));
    }
    if !cov.is_empty() {
        let sum: f64 = cov.iter().sum();
        put(c, "coverage", numval(sum / cov.len() as f64 / 100.0));
    }
    if !form_order.is_empty() {
        let mut sums: BTreeMap<&str, f64> = BTreeMap::new();
        for (form, _) in forms.values() {
            for k in ["killed", "survived", "uncovered"] {
                *sums.entry(k).or_default() += g(form, k).and_then(edn::record::num).unwrap_or(0.0);
            }
            *sums.entry("sites").or_default() += sites(form);
        }
        for (k, v) in sums {
            put(c, k, numval(v));
        }
    }
}

struct Ordered<V> {
    entries: Vec<(String, V)>,
}

impl<V> Default for Ordered<V> {
    fn default() -> Ordered<V> {
        Ordered {
            entries: Vec::new(),
        }
    }
}

impl<V> Ordered<V> {
    fn insert(&mut self, k: String, v: V) {
        if let Some(e) = self.entries.iter_mut().find(|(key, _)| *key == k) {
            e.1 = v;
        } else {
            self.entries.push((k, v));
        }
    }

    fn get(&self, k: &str) -> Option<&V> {
        self.entries
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v)
    }

    fn contains(&self, k: &str) -> bool {
        self.entries.iter().any(|(key, _)| key == k)
    }

    fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }
}

pub fn apply_metrics(doc: &Value, metrics: &Metrics) -> Value {
    if metrics.crap.is_empty() && metrics.mutate.is_empty() && metrics.dry.is_none() {
        return doc.clone();
    }
    let mut doc = doc.clone();
    let classes = doc_classes(&doc);
    let prefix = g(&doc, "prefix").cloned();
    let duplicates = metrics
        .dry
        .as_ref()
        .map(|candidates| dry::Index::new(&classes, prefix.as_ref(), candidates));
    let mut names: Vec<&String> = metrics.crap.keys().collect();
    for ns in metrics.mutate.keys() {
        if !names.contains(&ns) {
            names.push(ns);
        }
    }
    names.sort();
    let mut owned: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ns in names {
        if let Some(c) = owner(&classes, prefix.as_ref(), ns)
            && let Some(Value::String(key)) = g(c, "ns")
        {
            owned.entry(key.clone()).or_default().push(ns.clone());
        }
    }
    let paint_class = |c: &mut Value| {
        let key = match g(c, "ns") {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        if let Some(key) = key
            && let Some(nss) = owned.get(&key)
        {
            paint(c, nss, metrics);
        }
        if let Some(index) = &duplicates {
            dry::apply(c, index);
        }
    };
    if flag(&doc, "hierarchical") {
        let classes = seq(g(&doc, "classes")).to_vec();
        let mut painted = Vec::with_capacity(classes.len());
        for mut c in classes.into_iter() {
            paint_class(&mut c);
            painted.push(c);
        }
        put(&mut doc, "classes", Value::Vector(painted));
    } else {
        let diagrams = match g(&doc, "diagrams").cloned() {
            Some(Value::Vector(ds)) => ds,
            _ => vec![doc.clone()],
        };
        let mut out = Vec::new();
        for d in diagrams {
            let mut d = d;
            let packages = seq(g(&d, "packages")).to_vec();
            let mut out_ps = Vec::new();
            for p in packages {
                let mut p = p;
                let classes = seq(g(&p, "classes")).to_vec();
                let mut painted = Vec::with_capacity(classes.len());
                for mut c in classes.into_iter() {
                    paint_class(&mut c);
                    painted.push(c);
                }
                put(&mut p, "classes", Value::Vector(painted));
                out_ps.push(p);
            }
            put(&mut d, "packages", Value::Vector(out_ps));
            out.push(d);
        }
        if g(&doc, "diagrams").is_some() {
            put(&mut doc, "diagrams", Value::Vector(out));
        } else {
            doc = out.into_iter().next().unwrap();
        }
    }
    if let Some(index) = &duplicates {
        put(&mut doc, "dry", index.summary());
    }
    doc
}

pub fn apply(doc: &Value, files: &[Snapshot]) -> Value {
    apply_metrics(doc, &load(files))
}
