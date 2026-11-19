//! Duplicate structure from the dryer's pairwise snapshot (.metrics/dry.edn).
//!
//! The snapshot has one key, :candidates: pairs of source spans with a
//! similarity score. It holds no classes, no clusters and no summary, so this
//! module derives the three views the class record reads (#92):
//!
//! * by class -- how many of its functions take part in a pair, the weakest
//!   score, and the classes on the other side. The count is over functions,
//!   not pairs: most of this repository's pairs are one file duplicating
//!   itself, so counting pairs would score that duplication twice;
//! * by function -- the counterpart span and score of each participating
//!   function, for the member rows;
//! * globally -- candidates, attributed and unattributed pairs, and clusters.
//!
//! A **cluster** is a connected component of the pairs (a span is a node, a
//! pair an edge) and its similarity is its weakest edge (GLOSSARY.md).
//! A pair that joins no class on either side is counted, never dropped: the
//! snapshot measuring code the diagram does not draw must stay visible.
//!
//! The index is a pure function of (classes, candidates) and is rebuilt on
//! every overlay run -- opening a class card re-applies the overlay, so
//! nothing may be cached across calls. The marker is additive: it never
//! enters the CRAP or mutation rating, so it can never turn a class red, and
//! dry stays an optional quality step.

use crate::overlay::owner;
use edn::Value;
use edn::record::{flag, get, local_name, num, put, record, seq, text};
use std::collections::{BTreeMap, BTreeSet};

/// One side of a candidate pair: a function's source span.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub file: String,
    pub start: i64,
    pub end: i64,
    /// crapper's function namespace (crate::module::Type); None in a
    /// snapshot written before the name was reported.
    pub namespace: Option<String>,
    pub name: Option<String>,
}

/// One duplicate pair, as :candidates reports it.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub score: f64,
    pub left: Span,
    pub right: Span,
}

/// The counterpart of a participating function: one member row's data.
#[derive(Debug, Clone)]
pub struct Hit {
    pub score: f64,
    /// The counterpart class's :id, None when it joins no class.
    pub class: Option<Value>,
    pub span: Span,
}

/// What one class knows about duplicate structure.
#[derive(Debug, Clone, Default)]
pub struct ClassDry {
    /// The class's distinct function spans that take part in a pair.
    pub spans: BTreeSet<Span>,
    /// Pairs that touch the class, counted once even when both sides are its
    /// own functions (a file duplicating itself is one duplication).
    pub pairs: usize,
    /// The lowest score among those pairs.
    pub weakest: Option<f64>,
    /// The counterpart classes by :id local name.
    pub others: BTreeMap<String, Value>,
    /// Pairs whose other side joins no class, counted rather than dropped.
    pub unattributed: usize,
}

/// A connected component of pairs; its similarity is its weakest edge.
#[derive(Debug, Clone)]
pub struct Cluster {
    pub spans: Vec<Span>,
    pub weakest: f64,
}

/// The duplicate-structure index, derived from a document's classes and a
/// snapshot's candidates.
pub struct Index {
    pub classes: BTreeMap<String, ClassDry>,
    /// (class id, function name) -> the pairs that function takes part in.
    pub functions: BTreeMap<(String, String), Vec<Hit>>,
    pub clusters: Vec<Cluster>,
    pub candidates: usize,
    /// A pair is attributed when both sides join a class; anything else is
    /// counted unattributed.
    pub attributed: usize,
    pub unattributed: usize,
}

/// The :candidates of one dry snapshot. A malformed pair (a missing span or
/// score) is skipped: the dryer always writes all three, so there is nothing
/// to guess.
pub fn candidates(data: &Value) -> Vec<Candidate> {
    let mut out = Vec::new();
    for c in seq(get(data, "candidates")) {
        let (Some(left), Some(right), Some(score)) = (
            get(c, "left").and_then(span),
            get(c, "right").and_then(span),
            get(c, "score").and_then(num),
        ) else {
            continue;
        };
        out.push(Candidate { score, left, right });
    }
    out
}

fn span(v: &Value) -> Option<Span> {
    Some(Span {
        file: text(get(v, "file")?)?.to_owned(),
        start: get(v, "start-line").and_then(num)? as i64,
        end: get(v, "end-line").and_then(num)? as i64,
        namespace: get(v, "namespace").and_then(text).map(str::to_owned),
        name: get(v, "name").and_then(text).map(str::to_owned),
    })
}

/// A class's :id local name, the key the index is built and read with.
fn class_key(class: &Value) -> String {
    get(class, "id")
        .and_then(local_name)
        .unwrap_or_default()
        .to_owned()
}

fn class_id(class: &Value) -> Option<Value> {
    get(class, "id").cloned()
}

/// The class that owns `span`, through the overlay's join rule: the function
/// namespace first, then the file.
///
/// The file fallback exists because snapshots written before the dryer
/// reported :namespace/:name (the committed .metrics/dry.edn is one) carry
/// the file and lines only. It is exact while a file belongs to one class;
/// when several classes share a file it is not, and the pair is reported
/// unattributed instead of being attached to a guessed class.
fn class_of<'a>(classes: &'a [Value], prefix: Option<&Value>, span: &Span) -> Option<&'a Value> {
    if let Some(ns) = &span.namespace {
        return owner(classes, prefix, ns);
    }
    let mut found = classes.iter().filter(|c| {
        !flag(c, "foreign")
            && get(c, "shape").and_then(local_name) != Some("oval")
            && get(c, "file").and_then(text) == Some(span.file.as_str())
    });
    let first = found.next()?;
    if found.next().is_some() {
        return None;
    }
    Some(first)
}

impl Index {
    /// Derive the index. Pure: the same classes and candidates always give
    /// the same index, and the overlay may run it once per compile and once
    /// per opened class card.
    pub fn new(classes: &[Value], prefix: Option<&Value>, candidates: &[Candidate]) -> Index {
        let mut index = Index {
            classes: BTreeMap::new(),
            functions: BTreeMap::new(),
            clusters: Vec::new(),
            candidates: candidates.len(),
            attributed: 0,
            unattributed: 0,
        };
        let mut nodes: BTreeMap<Span, usize> = BTreeMap::new();
        let mut spans: Vec<Span> = Vec::new();
        let mut parents: Vec<usize> = Vec::new();
        let mut edges: Vec<(usize, usize, f64)> = Vec::new();
        for c in candidates {
            let left = class_of(classes, prefix, &c.left);
            let right = class_of(classes, prefix, &c.right);
            if left.is_some() && right.is_some() {
                index.attributed += 1;
            } else {
                index.unattributed += 1;
            }
            let mut touched: BTreeSet<String> = BTreeSet::new();
            for (own, other, span, counterpart) in [
                (left, right, &c.left, &c.right),
                (right, left, &c.right, &c.left),
            ] {
                let Some(own) = own else {
                    continue;
                };
                let key = class_key(own);
                let entry = index.classes.entry(key.clone()).or_default();
                entry.spans.insert(span.clone());
                entry.weakest = Some(entry.weakest.map_or(c.score, |w| w.min(c.score)));
                match other {
                    Some(other) => {
                        entry
                            .others
                            .insert(class_key(other), class_id(other).unwrap_or(Value::Nil));
                    }
                    None => entry.unattributed += 1,
                }
                touched.insert(key.clone());
                if let Some(name) = &span.name {
                    index
                        .functions
                        .entry((key, name.clone()))
                        .or_default()
                        .push(Hit {
                            score: c.score,
                            class: other.and_then(class_id),
                            span: counterpart.clone(),
                        });
                }
            }
            for key in touched {
                index.classes.entry(key).or_default().pairs += 1;
            }
            let a = node(&mut nodes, &mut spans, &mut parents, &c.left);
            let b = node(&mut nodes, &mut spans, &mut parents, &c.right);
            union(&mut parents, a, b);
            edges.push((a, b, c.score));
        }
        index.clusters = clusters(&spans, &mut parents, &edges);
        index
    }

    /// The document-level summary: how many pairs the snapshot holds, how
    /// many join a class, and the clusters derived from them.
    pub fn summary(&self) -> Value {
        let functions: usize = self.clusters.iter().map(|c| c.spans.len()).sum();
        let weakest = self
            .clusters
            .iter()
            .map(|c| c.weakest)
            .fold(f64::INFINITY, f64::min);
        record(vec![
            ("candidates", Value::Integer(self.candidates as i64)),
            ("attributed", Value::Integer(self.attributed as i64)),
            ("unattributed", Value::Integer(self.unattributed as i64)),
            ("clusters", Value::Integer(self.clusters.len() as i64)),
            ("functions", Value::Integer(functions as i64)),
            (
                "weakest",
                if weakest.is_finite() {
                    Value::Float(weakest)
                } else {
                    Value::Nil
                },
            ),
        ])
    }
}

/// Mark one class in place: its :dry state map and its members' pairs.
///
/// An absent :dry key is the third state -- no snapshot measured the class.
/// A present one says either :overlap (with the numbers) or :none (measured,
/// no overlap); an absent or stale snapshot is never read as "no
/// duplication".
pub fn apply(class: &mut Value, index: &Index) {
    let key = class_key(class);
    let state = match index.classes.get(&key) {
        Some(entry) => record(vec![
            ("state", Value::keyword("overlap")),
            ("functions", Value::Integer(entry.spans.len() as i64)),
            ("pairs", Value::Integer(entry.pairs as i64)),
            (
                "weakest",
                entry
                    .weakest
                    .map_or(Value::Nil, |weakest| Value::Float(weakest)),
            ),
            (
                "others",
                Value::Vector(entry.others.values().cloned().collect()),
            ),
            ("unattributed", Value::Integer(entry.unattributed as i64)),
        ]),
        None => record(vec![("state", Value::keyword("none"))]),
    };
    put(class, "dry", state);
    let ops: Vec<Value> = seq(get(class, "ops")).to_vec();
    let mut marked = Vec::with_capacity(ops.len());
    for mut op in ops {
        let name = match get(&op, "name").and_then(local_name) {
            Some(name) => name.to_owned(),
            None => {
                marked.push(op);
                continue;
            }
        };
        if let Some(hits) = index.functions.get(&(key.clone(), name)) {
            let pairs: Vec<Value> = hits.iter().map(hit).collect();
            put(
                &mut op,
                "dry",
                record(vec![("pairs", Value::Vector(pairs))]),
            );
        }
        marked.push(op);
    }
    if !marked.is_empty() {
        put(class, "ops", Value::Vector(marked));
    }
}

fn hit(hit: &Hit) -> Value {
    let mut entries = vec![
        ("score", Value::Float(hit.score)),
        ("file", Value::String(hit.span.file.clone())),
        ("start-line", Value::Integer(hit.span.start)),
        ("end-line", Value::Integer(hit.span.end)),
    ];
    if let Some(class) = &hit.class {
        entries.push(("class", class.clone()));
    }
    if let Some(name) = &hit.span.name {
        entries.push(("name", Value::String(name.clone())));
    }
    record(entries)
}

fn node(
    nodes: &mut BTreeMap<Span, usize>,
    spans: &mut Vec<Span>,
    parents: &mut Vec<usize>,
    span: &Span,
) -> usize {
    if let Some(i) = nodes.get(span) {
        return *i;
    }
    let i = spans.len();
    nodes.insert(span.clone(), i);
    spans.push(span.clone());
    parents.push(i);
    i
}

fn find(parents: &mut [usize], mut i: usize) -> usize {
    while parents[i] != i {
        parents[i] = parents[parents[i]];
        i = parents[i];
    }
    i
}

fn union(parents: &mut [usize], a: usize, b: usize) {
    let (a, b) = (find(parents, a), find(parents, b));
    if a != b {
        parents[b] = a;
    }
}

/// Group the spans into connected components. The weakest edge of a
/// component is its similarity, and the components are ordered worst first so
/// a caller can truncate from the least confident end.
fn clusters(spans: &[Span], parents: &mut [usize], edges: &[(usize, usize, f64)]) -> Vec<Cluster> {
    let roots: Vec<usize> = (0..spans.len()).map(|i| find(parents, i)).collect();
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, root) in roots.iter().enumerate() {
        groups.entry(*root).or_default().push(i);
    }
    let mut out = Vec::with_capacity(groups.len());
    for (root, members) in &groups {
        let mut cluster = Cluster {
            spans: members.iter().map(|i| spans[*i].clone()).collect(),
            weakest: f64::INFINITY,
        };
        for (a, _, score) in edges {
            if roots[*a] == *root {
                cluster.weakest = cluster.weakest.min(*score);
            }
        }
        out.push(cluster);
    }
    out.sort_by(|a, b| {
        a.weakest
            .total_cmp(&b.weakest)
            .then_with(|| a.spans.cmp(&b.spans))
    });
    out
}
