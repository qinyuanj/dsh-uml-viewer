use crate::config;
use crate::geom::{self, Rect};
use edn::Value;
use edn::record::{flag, get, local_name, name, num, numval, put, seq};

/// Estimated advance of one half-width character at the 16px body font.
pub const CHAR_W: f64 = 8.0;

// East Asian Wide/Fullwidth BMP blocks: CJK, kana, Hangul, full-width forms.
const WIDE: &[(u32, u32)] = &[
    (0x1100, 0x115F),
    (0x2E80, 0x303E),
    (0x3041, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xA000, 0xA4CF),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE30, 0xFE4F),
    (0xFF00, 0xFF60),
    (0xFFE0, 0xFFE6),
];

fn units(c: char) -> usize {
    if c.len_utf8() == 4 {
        return 2; // one JVM UTF-16 surrogate pair
    }
    if c.len_utf8() == 3 {
        let code = c as u32;
        if WIDE.iter().any(|&(lo, hi)| code >= lo && code <= hi) {
            return 2;
        }
    }
    1
}

/// JVM String.count in UTF-16 units, except full-width CJK counts double.
pub fn text_w(s: Option<&str>) -> f64 {
    let n: usize = s.unwrap_or("").chars().map(units).sum();
    CHAR_W * n as f64
}

fn line(kind: &str, text: Option<Value>, extra: Vec<(Value, Value)>) -> Value {
    let mut l = Value::Map(vec![
        (Value::keyword("kind"), Value::keyword(kind)),
        (Value::keyword("text"), text.unwrap_or(Value::Nil)),
    ]);
    for (k, v) in extra {
        l.assoc(k, v);
    }
    l
}

pub fn class_lines(c: &Value) -> Vec<Value> {
    let mut lines = Vec::new();
    if let Some(stereotype) = get(c, "stereotype") {
        lines.push(line(
            "stereo",
            Some(Value::String(format!("«{}»", name(stereotype)))),
            vec![],
        ));
    }
    lines.push(line("name", get(c, "name").cloned(), vec![]));
    if !flag(c, "hide-members") {
        let kids = seq(get(c, "contents"));
        if !kids.is_empty() {
            lines.push(line("rule", None, vec![]));
            for ch in kids {
                lines.push(line(
                    "child",
                    get(ch, "name").cloned(),
                    vec![
                        (
                            Value::keyword("id"),
                            get(ch, "id").cloned().unwrap_or(Value::Nil),
                        ),
                        (
                            Value::keyword("drill?"),
                            Value::Bool(edn::record::is_true(ch, "drill?")),
                        ),
                    ],
                ));
            }
        } else {
            for kind in ["fields", "ops"] {
                let shown: Vec<&Value> = seq(get(c, kind))
                    .iter()
                    .filter(|x| kind != "ops" || !flag(x, "private"))
                    .collect();
                if !shown.is_empty() {
                    lines.push(line("rule", None, vec![]));
                    for x in shown {
                        lines.push(line(
                            if kind == "fields" { "field" } else { "op" },
                            get(x, "text").cloned(),
                            vec![],
                        ));
                    }
                }
            }
        }
    }
    lines
}

pub fn class_box_size(c: &Value) -> (f64, f64, Vec<Value>) {
    let lines = class_lines(c);
    let (mut w, mut h): (f64, f64) = (120.0, 24.0);
    for l in &lines {
        if local_name(get(l, "kind").unwrap()).unwrap_or_default() == "rule" {
            h += 12.0;
        } else {
            h += 18.0;
            w = w.max(
                24.0 + text_w(get(l, "text").and_then(|t| match t {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                })),
            );
        }
    }
    (w, h, lines)
}

fn port_w(d: &Value) -> f64 {
    (44.0_f64).max(
        10.0 + text_w(Some(&match get(d, "name") {
            Some(v @ Value::String(_)) => name(v),
            _ => local_name(get(d, "id").unwrap())
                .unwrap_or_default()
                .to_owned(),
        })),
    )
}

fn pack(c: &Value) -> Value {
    let mut c = c.clone();
    let (w, h, lines) = class_box_size(&c);
    let ins = seq(get(&c, "in-deps"));
    let outs = seq(get(&c, "out-deps"));
    let mut iw: f64 = 0.0;
    for (i, d) in ins.iter().enumerate() {
        iw += port_w(d) + if i > 0 { 6.0 } else { 0.0 };
    }
    let mut ow: f64 = 0.0;
    for d in outs {
        ow = ow.max(port_w(d));
    }
    if outs.len() >= 2 {
        ow += 16.0;
    }
    let isp = if ins.is_empty() { 0.0 } else { 40.0 };
    let osp = if outs.is_empty() {
        0.0
    } else {
        58.0 + (outs.len() as f64 - 1.0) * 28.0
    };
    for (k, v) in [
        ("content-w", w),
        ("content-h", h),
        ("in-space", isp),
        ("out-space", osp),
        ("w", w.max(iw).max(ow)),
        ("h", isp + h + osp),
    ] {
        put(&mut c, k, numval(v));
    }
    put(&mut c, "lines", Value::Vector(lines));
    c
}

fn ports(c: &mut Value) {
    let r = Rect::from_value(get(c, "rect").unwrap()).unwrap();
    let ins = seq(get(c, "in-deps")).to_vec();
    let outs = seq(get(c, "out-deps")).to_vec();
    let mut total: f64 = 0.0;
    for (i, d) in ins.iter().enumerate() {
        total += port_w(d) + if i > 0 { 6.0 } else { 0.0 };
    }
    let mut ip = Vec::new();
    let mut x = r.cx() - total / 2.0;
    for d in &ins {
        let mut p = (*d).clone();
        put(
            &mut p,
            "rect",
            Rect::new(x, r.y - 40.0, port_w(d), 18.0).to_value(),
        );
        ip.push(p);
        x += port_w(d) + 6.0;
    }
    let mut widest: f64 = 0.0;
    for d in &outs {
        widest = widest.max(port_w(d));
    }
    x = r.cx() - (widest + if outs.len() >= 2 { 16.0 } else { 0.0 }) / 2.0;
    let mut op = Vec::new();
    for (i, d) in outs.iter().enumerate() {
        let mut p = (*d).clone();
        put(
            &mut p,
            "rect",
            Rect::new(
                x + (i % 2) as f64 * 16.0,
                r.bottom() + 40.0 + i as f64 * 28.0,
                port_w(d),
                18.0,
            )
            .to_value(),
        );
        op.push(p);
    }
    put(c, "in-ports", Value::Vector(ip));
    put(c, "out-ports", Value::Vector(op));
}

const SUB_ROWS: usize = 3;

const GRID: usize = 4;

const LEVEL_GAP: f64 = 220.0;
const RANK_GAP: f64 = 140.0;

const SHELF_GAP: f64 = LEVEL_GAP;

const LANE_GAP: f64 = 12.0;
const LANE_MARGIN: f64 = 20.0;
const LANE_REACH: f64 = 50.0;

type Lane = (usize, Vec<[f64; 2]>);
type Pass = (usize, [f64; 2], [f64; 2]);

type Sunk = (String, Rect, f64);

fn place(
    classes: &[Value],
    edges: &[Value],
    direction: &str,
    sinks: &[(String, f64, f64)],
    wide: Option<f64>,
) -> (Vec<Value>, Vec<Lane>, Vec<Sunk>) {
    use std::collections::HashMap;
    let mut cs: Vec<Value> = Vec::with_capacity(classes.len());
    let mut ids: Vec<String> = Vec::with_capacity(classes.len());
    let mut idx: HashMap<String, usize> = HashMap::new();
    let mut out_edges: HashMap<String, Vec<String>> = HashMap::new();
    let mut nbr: HashMap<String, Vec<String>> = HashMap::new();
    let mut high: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (i, c) in classes.iter().enumerate() {
        let packed = pack(c);
        let id = edn::encode(get(&packed, "id").unwrap()).unwrap();
        idx.insert(id.clone(), i);
        ids.push(id.clone());
        out_edges.insert(id.clone(), Vec::new());
        nbr.insert(id.clone(), Vec::new());
        cs.push(packed);
    }
    let sink: HashMap<String, (f64, f64)> = sinks
        .iter()
        .map(|(id, w, h)| (id.clone(), (*w, *h)))
        .collect();
    for id in sink.keys() {
        nbr.insert(id.clone(), Vec::new());
    }
    for e in edges {
        let f = edn::encode(get(e, "from").unwrap()).unwrap();
        let t = edn::encode(get(e, "to").unwrap()).unwrap();
        let known = |id: &String| idx.contains_key(id) || sink.contains_key(id);
        if !known(&f) || !known(&t) {
            continue;
        }
        if !nbr[&f].contains(&t) {
            nbr.get_mut(&f).unwrap().push(t.clone());
        }
        if !nbr[&t].contains(&f) {
            nbr.get_mut(&t).unwrap().push(f.clone());
        }
        if sink.contains_key(&f) || sink.contains_key(&t) {
            continue;
        }
        let k = get(e, "kind")
            .and_then(local_name)
            .unwrap_or_default()
            .to_owned();
        let (f, t) = if k == "inheritance" || k == "implements" {
            (t, f)
        } else {
            (f, t)
        };
        out_edges.get_mut(&f).unwrap().push(t.clone());
        high.insert(t);
    }
    let mut queue: Vec<String> = Vec::new();
    let mut ranks: HashMap<String, f64> = HashMap::new();
    for id in &ids {
        if !high.contains(id) {
            queue.push(id.clone());
            ranks.insert(id.clone(), 0.0);
        }
    }
    if queue.is_empty() {
        for id in &ids {
            queue.push(id.clone());
            ranks.insert(id.clone(), 0.0);
        }
    }
    let mut qi = 0;
    while qi < queue.len() {
        let id = queue[qi].clone();
        qi += 1;
        for t in out_edges[&id].clone() {
            if !ranks.contains_key(&t) {
                ranks.insert(t.clone(), ranks[&id] + 1.0);
                queue.push(t);
            }
        }
    }
    let horizontal = direction == "lr" || direction == "rl";
    let mut levels = false;
    for c in &cs {
        if get(c, "level").is_some() {
            levels = true;
        }
    }
    let levels = levels && (!horizontal || wide.is_some());
    let level_of = |id: &str| get(&cs[idx[id]], "level").and_then(num).unwrap_or(0.0);
    let sub = if levels {
        let inner: HashMap<String, Vec<String>> = ids
            .iter()
            .map(|id| {
                let same = out_edges[id]
                    .iter()
                    .filter(|t| level_of(t) == level_of(id))
                    .cloned()
                    .collect();
                (id.clone(), same)
            })
            .collect();
        let mut sub = sublayers(&ids, &inner);
        for row in sub.values_mut() {
            *row = (*row).min(SUB_ROWS - 1);
        }
        sub
    } else {
        HashMap::new()
    };
    let rows = sub.values().max().map_or(1, |d| d + 1) as f64;
    let class_rank = |id: &str| {
        if levels {
            level_of(id) * rows + sub[id] as f64
        } else {
            ranks.get(id).copied().unwrap_or(0.0)
        }
    };
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let mut keys: Vec<f64> = Vec::new();
    let mut pos: HashMap<String, f64> = HashMap::new();
    let mut alone: Vec<String> = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        pos.insert(id.clone(), (i as f64) * 80.0);
        if levels && nbr[id].is_empty() {
            alone.push(id.clone());
            continue;
        }
        let r = class_rank(id);
        let key = rank_key(r);
        if !groups.contains_key(&key) {
            groups.insert(key.clone(), Vec::new());
            keys.push(r);
        }
        groups.get_mut(&key).unwrap().push(id.clone());
    }
    keys.sort_by(|a, b| {
        if levels {
            b.partial_cmp(a).unwrap()
        } else {
            a.partial_cmp(b).unwrap()
        }
    });
    let sink_rank = keys.last().copied().unwrap_or(0.0) - 1.0;
    if !sink.is_empty() {
        keys.push(sink_rank);
        let mut ids: Vec<String> = sinks.iter().map(|s| s.0.clone()).collect();
        for (i, id) in ids.iter().enumerate() {
            pos.insert(id.clone(), (cs.len() + i) as f64 * 80.0);
        }
        groups
            .entry(rank_key(sink_rank))
            .or_default()
            .append(&mut ids);
    }
    let level_keys: std::collections::HashSet<String> = keys.iter().map(|r| rank_key(*r)).collect();
    let grid_top = if sink.is_empty() {
        sink_rank
    } else {
        sink_rank - 1.0
    };
    let mut grid: HashMap<String, f64> = HashMap::new();
    for (row, chunk) in alone.chunks(GRID).enumerate() {
        let r = grid_top - row as f64;
        keys.push(r);
        groups.insert(rank_key(r), chunk.to_vec());
        for id in chunk {
            grid.insert(id.clone(), r);
        }
    }
    let size = |id: &str| match sink.get(id) {
        Some(&wh) => wh,
        None => {
            let c = &cs[idx[id]];
            (
                num(get(c, "w").unwrap()).unwrap(),
                num(get(c, "h").unwrap()).unwrap(),
            )
        }
    };
    let mut rank_at: HashMap<String, f64> = groups
        .values()
        .flatten()
        .map(|id| {
            let r = if sink.contains_key(id) {
                sink_rank
            } else if let Some(&r) = grid.get(id) {
                r
            } else {
                class_rank(id)
            };
            (id.clone(), r)
        })
        .collect();
    let mut band: HashMap<String, f64> = keys
        .iter()
        .map(|&r| {
            let b = if level_keys.contains(&rank_key(r)) {
                (r / rows).floor()
            } else {
                f64::NEG_INFINITY
            };
            (rank_key(r), b)
        })
        .collect();
    let mut shelf_of: HashMap<String, usize> = HashMap::new();
    if let Some(aspect) = wide {
        sweep(&keys, &groups, &mut pos, &nbr);
        let ranks: Vec<Column> = keys
            .iter()
            .map(|r| {
                let ids = ordered(&groups[&rank_key(*r)], &pos, &nbr);
                let sized = ids
                    .into_iter()
                    .map(|id| {
                        let (w, h) = size(&id);
                        (id, w, h)
                    })
                    .collect();
                (band[&rank_key(*r)], sized)
            })
            .collect();
        let links: Vec<(String, String)> = edges
            .iter()
            .filter_map(|e| {
                let f = edn::encode(get(e, "from").unwrap()).unwrap();
                let t = edn::encode(get(e, "to").unwrap()).unwrap();
                (rank_at.contains_key(&f) && rank_at.contains_key(&t)).then_some((f, t))
            })
            .collect();
        let ovals: std::collections::HashSet<String> = sink.keys().cloned().collect();
        let columns = fold(&ranks, &links, aspect, levels, &ovals);
        let n = columns.len();
        (keys, groups, band) = (Vec::new(), HashMap::new(), HashMap::new());
        for (j, (b, ids, nth)) in columns.into_iter().enumerate() {
            let r = (n - 1 - j) as f64;
            for id in &ids {
                rank_at.insert(id.clone(), r);
            }
            keys.push(r);
            groups.insert(rank_key(r), ids);
            band.insert(rank_key(r), b);
            shelf_of.insert(rank_key(r), nth);
        }
    }
    let rank_of = |id: &str| rank_at[id];
    let order: HashMap<String, usize> = keys
        .iter()
        .enumerate()
        .map(|(k, r)| (rank_key(*r), k))
        .collect();
    let mut lane_of: HashMap<String, (usize, usize, bool)> = HashMap::new();
    for (ei, e) in edges.iter().enumerate() {
        let f = edn::encode(get(e, "from").unwrap()).unwrap();
        let t = edn::encode(get(e, "to").unwrap()).unwrap();
        let known = |id: &String| idx.contains_key(id) || sink.contains_key(id);
        if !known(&f) || !known(&t) {
            continue;
        }
        let (kf, kt) = (order[&rank_key(rank_of(&f))], order[&rank_key(rank_of(&t))]);
        if kf.abs_diff(kt) < 2 {
            continue;
        }
        let on_shelf = |k: usize| shelf_of.get(&rank_key(keys[k])).copied().unwrap_or(0);
        if on_shelf(kf) != on_shelf(kt) {
            continue;
        }
        let steps: Vec<usize> = if kf < kt {
            (kf + 1..kt).collect()
        } else {
            (kt + 1..kf).rev().collect()
        };
        let chain: Vec<String> = (0..steps.len()).map(|j| format!("lane:{ei}:{j}")).collect();
        for (j, (k, id)) in steps.iter().zip(&chain).enumerate() {
            let prev = if j == 0 {
                f.clone()
            } else {
                chain[j - 1].clone()
            };
            let next = chain.get(j + 1).cloned().unwrap_or_else(|| t.clone());
            nbr.insert(id.clone(), vec![prev, next]);
            groups
                .get_mut(&rank_key(keys[*k]))
                .unwrap()
                .push(id.clone());
            pos.insert(id.clone(), pos[&f]);
            lane_of.insert(id.clone(), (ei, j, kf < kt));
        }
    }
    let gap = |a: bool, b: bool| {
        if a && b {
            LANE_GAP
        } else if a || b {
            LANE_MARGIN
        } else {
            BOX_GAP
        }
    };
    sweep(&keys, &groups, &mut pos, &nbr);
    let mut gs: Vec<(f64, Vec<String>, f64, f64, f64)> = Vec::new();
    for r in &keys {
        let xs = ordered(&groups[&rank_key(*r)], &pos, &nbr);
        let (mut w, mut h, mut colh): (f64, f64, f64) = (0.0, 0.0, 0.0);
        for (i, id) in xs.iter().enumerate() {
            let lane = lane_of.contains_key(id);
            if i > 0 {
                colh += gap(lane_of.contains_key(&xs[i - 1]), lane);
            }
            if lane {
                continue;
            }
            let (cw, ch) = size(id);
            w = w.max(cw);
            h = h.max(ch);
            colh += ch;
        }
        gs.push((*r, xs, w, h, colh));
    }
    let nth: Vec<usize> = gs
        .iter()
        .map(|g| shelf_of.get(&rank_key(g.0)).copied().unwrap_or(0))
        .collect();
    let mut shelves: Vec<std::ops::Range<usize>> = Vec::new();
    for k in 0..gs.len() {
        match shelves.last_mut() {
            Some(r) if nth[r.start] == nth[k] => r.end = k + 1,
            _ => shelves.push(k..k + 1),
        }
    }
    let start: Option<HashMap<String, f64>> = levels.then(|| {
        let mut out: HashMap<String, f64> = HashMap::new();
        for run in &shelves {
            let rows: Vec<Vec<Slot>> = gs[run.clone()]
                .iter()
                .map(|g| {
                    g.1.iter()
                        .map(|id| {
                            let lane = lane_of.contains_key(id);
                            let (w, h) = if lane { (0.0, 0.0) } else { size(id) };
                            let w = if horizontal { h } else { w };
                            (id.clone(), w, lane)
                        })
                        .collect()
                })
                .collect();
            out.extend(nearby(&rows, &nbr, &gap));
        }
        out
    });
    let mut result = Vec::new();
    let mut sunk: Vec<Sunk> = Vec::new();
    let mut through: HashMap<usize, Vec<Pass>> = HashMap::new();
    let mut offset = 0.0;
    for run in &shelves {
        let tall = gs[run.clone()].iter().map(|g| g.4).fold(0.0, f64::max);
        let mut along = 0.0;
        for (k, (rank, group_ids, gw, gh, colh)) in gs[run.clone()].iter().enumerate() {
            let mut across = offset + if horizontal { (tall - colh) / 2.0 } else { 0.0 };
            let depth = if horizontal { *gw } else { *gh };
            for (i, id) in group_ids.iter().enumerate() {
                if i > 0 {
                    across += gap(
                        lane_of.contains_key(&group_ids[i - 1]),
                        lane_of.contains_key(id),
                    );
                }
                if let Some(start) = &start {
                    across = offset + start[id];
                }
                if let Some(&(ei, j, forward)) = lane_of.get(id) {
                    let at = |d: f64| if horizontal { [d, across] } else { [across, d] };
                    let (near, far) = (at(along - LANE_REACH), at(along + depth + LANE_REACH));
                    let (enter, exit) = if forward { (near, far) } else { (far, near) };
                    through.entry(ei).or_default().push((j, enter, exit));
                    continue;
                }
                if let Some(&(cw, ch)) = sink.get(id) {
                    let r = if horizontal {
                        Rect::new(along + (gw - cw) / 2.0, across, cw, ch)
                    } else {
                        Rect::new(across, along, cw, ch)
                    };
                    sunk.push((id.clone(), r, *rank));
                    across += if horizontal { ch } else { cw };
                    continue;
                }
                let mut c = cs[idx[id]].clone();
                let (cw, ch, content_w, content_h, in_space) = (
                    num(get(&c, "w").unwrap()).unwrap(),
                    num(get(&c, "h").unwrap()).unwrap(),
                    num(get(&c, "content-w").unwrap()).unwrap(),
                    num(get(&c, "content-h").unwrap()).unwrap(),
                    num(get(&c, "in-space").unwrap()).unwrap(),
                );
                let (x, y) = if horizontal {
                    (along, across)
                } else {
                    (across, along)
                };
                let slot = if wide.is_some() { *gw } else { cw };
                put(
                    &mut c,
                    "rect",
                    Rect::new(
                        x + (slot - content_w) / 2.0,
                        y + in_space,
                        content_w,
                        content_h,
                    )
                    .to_value(),
                );
                put(&mut c, "rank", numval(*rank));
                result.push(c);
                across += if horizontal { ch } else { cw };
            }
            let next = (k + 1 < run.end - run.start).then(|| gs[run.start + k + 1].0);
            along += depth
                + match next {
                    Some(n) if levels && band[&rank_key(n)] != band[&rank_key(*rank)] => LEVEL_GAP,
                    _ => RANK_GAP,
                };
        }
        offset += tall + SHELF_GAP;
    }
    let mut lanes: Vec<Lane> = through
        .into_iter()
        .map(|(ei, mut steps)| {
            steps.sort_by_key(|s| s.0);
            (ei, steps.into_iter().flat_map(|s| [s.1, s.2]).collect())
        })
        .collect();
    lanes.sort_by_key(|l| l.0);
    (result, lanes, sunk)
}

fn ordered(
    xs: &[String],
    pos: &std::collections::HashMap<String, f64>,
    nbr: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<String> {
    let score = |id: &str| -> f64 {
        let mut sum = 0.0;
        let mut n = 0.0;
        for t in &nbr[id] {
            if let Some(p) = pos.get(t) {
                sum += p;
                n += 1.0;
            }
        }
        if n > 0.0 {
            sum / n
        } else {
            pos.get(id).copied().unwrap_or(0.0)
        }
    };
    let mut list: Vec<(String, usize, f64)> = xs
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i + 1, score(id)))
        .collect();
    list.sort_by(|a, b| {
        if a.2 == b.2 {
            a.1.cmp(&b.1)
        } else {
            a.2.partial_cmp(&b.2).unwrap()
        }
    });
    list.into_iter().map(|x| x.0).collect()
}

fn sweep(
    keys: &[f64],
    groups: &std::collections::HashMap<String, Vec<String>>,
    pos: &mut std::collections::HashMap<String, f64>,
    nbr: &std::collections::HashMap<String, Vec<String>>,
) {
    for _ in 0..5 {
        for r in keys {
            for (i, id) in ordered(&groups[&rank_key(*r)], pos, nbr).iter().enumerate() {
                pos.insert(id.clone(), (i as f64) * 80.0);
            }
        }
    }
}

type Sized = (String, f64, f64);

type Column = (f64, Vec<Sized>);

const BOX_GAP: f64 = 40.0;

fn fold(
    ranks: &[Column],
    links: &[(String, String)],
    aspect: f64,
    levels: bool,
    ovals: &std::collections::HashSet<String>,
) -> Vec<(f64, Vec<String>, usize)> {
    let split = |nodes: &[Sized], limit: f64| -> Vec<std::ops::Range<usize>> {
        let mut runs = Vec::new();
        let (mut start, mut h) = (0, 0.0);
        for (i, n) in nodes.iter().enumerate() {
            if i > start && h + BOX_GAP + n.2 > limit {
                runs.push(start..i);
                (start, h) = (i, 0.0);
            }
            h += if i > start { BOX_GAP } else { 0.0 } + n.2;
        }
        if start < nodes.len() {
            runs.push(start..nodes.len());
        }
        runs
    };
    let columns = |limit: f64| -> Vec<(f64, &[Sized])> {
        ranks
            .iter()
            .flat_map(|(band, nodes)| {
                split(nodes, limit)
                    .into_iter()
                    .map(move |run| (*band, &nodes[run]))
            })
            .collect()
    };
    let gap = |a: f64, b: f64| {
        if levels && a != b {
            LEVEL_GAP
        } else {
            RANK_GAP
        }
    };
    let sizes = |cols: &[(f64, &[Sized])]| -> (Vec<f64>, Vec<f64>) {
        let at: std::collections::HashMap<&str, usize> = cols
            .iter()
            .enumerate()
            .flat_map(|(k, (_, nodes))| nodes.iter().map(move |n| (n.0.as_str(), k)))
            .collect();
        let mut lanes = vec![0usize; cols.len()];
        for (f, t) in links {
            let (a, b) = (at[f.as_str()], at[t.as_str()]);
            for l in lanes.iter_mut().take(a.max(b)).skip(a.min(b) + 1) {
                *l += 1;
            }
        }
        let mut ws = Vec::with_capacity(cols.len());
        let mut hs = Vec::with_capacity(cols.len());
        for (k, (_, nodes)) in cols.iter().enumerate() {
            ws.push(nodes.iter().map(|n| n.1).fold(0.0, f64::max));
            let tall: f64 = nodes.iter().map(|n| n.2).sum();
            let lane = match lanes[k] {
                0 => 0.0,
                m => 2.0 * LANE_MARGIN + LANE_GAP * (m - 1) as f64,
            };
            hs.push(tall + BOX_GAP * nodes.len().saturating_sub(1) as f64 + lane);
        }
        (ws, hs)
    };
    let shelves = |cols: &[(f64, &[Sized])], ws: &[f64], hs: &[f64], shelf: f64| {
        let (mut which, mut xs, mut nth) = (
            Vec::with_capacity(cols.len()),
            Vec::with_capacity(cols.len()),
            0usize,
        );
        let (mut x, mut widest, mut y, mut row): (f64, f64, f64, f64) = (0.0, 0.0, 0.0, 0.0);
        for k in 0..cols.len() {
            if x > 0.0 && x + gap(cols[k - 1].0, cols[k].0) + ws[k] > shelf {
                widest = widest.max(x);
                (y, x, row) = (y + row + SHELF_GAP, 0.0, 0.0);
                nth += 1;
            } else if x > 0.0 {
                x += gap(cols[k - 1].0, cols[k].0);
            }
            which.push(nth);
            xs.push(x);
            x += ws[k];
            row = row.max(hs[k]);
        }
        (widest.max(x), y + row, which, xs)
    };
    let covers = |cols: &[(f64, &[Sized])], which: &[usize], xs: &[f64], ws: &[f64]| -> bool {
        let (mut l, mut r) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut low, mut high) = (usize::MAX, 0usize);
        for (k, (_, nodes)) in cols.iter().enumerate() {
            if nodes.iter().all(|n| ovals.contains(&n.0)) {
                continue;
            }
            l = l.min(xs[k]);
            r = r.max(xs[k] + ws[k]);
            low = low.min(which[k]);
            high = high.max(which[k]);
        }
        if low >= high {
            return false;
        }
        cols.iter().enumerate().any(|(k, (_, nodes))| {
            nodes.iter().any(|n| ovals.contains(&n.0))
                && low <= which[k]
                && which[k] <= high
                && l <= xs[k]
                && xs[k] + ws[k] <= r
        })
    };
    let mut limits: Vec<f64> = ranks
        .iter()
        .flat_map(|(_, nodes)| {
            let mut h = 0.0;
            nodes.iter().enumerate().map(move |(i, n)| {
                h += if i > 0 { BOX_GAP } else { 0.0 } + n.2;
                h
            })
        })
        .collect();
    limits.sort_by(|a, b| b.partial_cmp(a).unwrap());
    limits.dedup();
    let mut best: Option<(f64, f64, f64)> = None;
    for &limit in &limits {
        let cols = columns(limit);
        let (ws, hs) = sizes(&cols);
        let mut cum = Vec::with_capacity(cols.len());
        let mut edge = 0.0;
        for k in 0..cols.len() {
            if k > 0 {
                edge += gap(cols[k - 1].0, cols[k].0);
            }
            edge += ws[k];
            cum.push(edge);
        }
        for &shelf in &cum {
            let (w, h, which, xs) = shelves(&cols, &ws, &hs, shelf);
            if covers(&cols, &which, &xs, &ws) {
                continue;
            }
            let e = (w / aspect).max(h);
            let better = match best {
                Some((b, _, _)) => e < b,
                None => true,
            };
            if better {
                best = Some((e, limit, shelf));
            }
        }
    }
    let (limit, shelf) = best.map_or((0.0, 0.0), |(_, l, s)| (l, s));
    let cols = columns(limit);
    let (ws, hs) = sizes(&cols);
    let (_, _, which, _) = shelves(&cols, &ws, &hs, shelf);
    cols.into_iter()
        .zip(which)
        .map(|((band, nodes), nth)| (band, nodes.iter().map(|n| n.0.clone()).collect(), nth))
        .collect()
}

type Slot = (String, f64, bool);

const NEARBY_SWEEPS: usize = 8;

fn isotonic(targets: &[(f64, f64)], lo: f64, hi: f64) -> Vec<f64> {
    let mut blocks: Vec<(f64, f64, usize)> = Vec::new();
    for &(t, w) in targets {
        blocks.push((t * w, w, 1));
        while let [.., a, b] = blocks[..] {
            if a.0 / a.1 <= b.0 / b.1 {
                break;
            }
            blocks.pop();
            let a = blocks.last_mut().unwrap();
            *a = (a.0 + b.0, a.1 + b.1, a.2 + b.2);
        }
    }
    blocks
        .iter()
        .flat_map(|&(s, w, n)| std::iter::repeat_n((s / w).clamp(lo, hi.max(lo)), n))
        .collect()
}

fn nearby(
    rows: &[Vec<Slot>],
    nbr: &std::collections::HashMap<String, Vec<String>>,
    gap: &impl Fn(bool, bool) -> f64,
) -> std::collections::HashMap<String, f64> {
    use std::collections::HashMap;
    let packed: Vec<Vec<f64>> = rows
        .iter()
        .map(|row| {
            let mut d = 0.0;
            (0..row.len())
                .map(|i| {
                    if i > 0 {
                        d += row[i - 1].1 + gap(row[i - 1].2, row[i].2);
                    }
                    d
                })
                .collect()
        })
        .collect();
    let width = |k: usize| {
        rows[k]
            .last()
            .map_or(0.0, |l| packed[k].last().unwrap() + l.1)
    };
    let widest = (0..rows.len()).map(width).fold(0.0, f64::max);
    let mut left: HashMap<&str, f64> = HashMap::new();
    let mut centre: HashMap<&str, f64> = HashMap::new();
    for (row, offs) in rows.iter().zip(&packed) {
        for ((id, w, _), x) in row.iter().zip(offs) {
            left.insert(id, *x);
            centre.insert(id, x + w / 2.0);
        }
    }
    let lanes: std::collections::HashSet<&str> = rows
        .iter()
        .flatten()
        .filter(|n| n.2)
        .map(|n| n.0.as_str())
        .collect();
    let lane = |id: &str| lanes.contains(id);
    let mean = |ids: &mut dyn Iterator<Item = &String>, centre: &HashMap<&str, f64>| {
        let (mut sum, mut n) = (0.0, 0.0);
        for t in ids {
            if let Some(c) = centre.get(t.as_str()) {
                sum += c;
                n += 1.0;
            }
        }
        (n > 0.0).then(|| (sum / n, n))
    };
    for sweep in 0..NEARBY_SWEEPS {
        let order: Vec<usize> = if sweep % 2 == 0 {
            (0..rows.len()).collect()
        } else {
            (0..rows.len()).rev().collect()
        };
        for k in order {
            let (row, offs) = (&rows[k], &packed[k]);
            let boxes: Vec<usize> = (0..row.len()).filter(|&i| !row[i].2).collect();
            let targets: Vec<(f64, f64)> = boxes
                .iter()
                .map(|&i| {
                    let (id, w, _) = &row[i];
                    let near = mean(&mut nbr[id].iter().filter(|t| !lane(t)), &centre);
                    let (target, weight) = near.unwrap_or((centre[id.as_str()], 0.01));
                    (target - offs[i] - w / 2.0, weight)
                })
                .collect();
            let shifts = isotonic(&targets, 0.0, widest - width(k));
            for (&i, z) in boxes.iter().zip(shifts) {
                left.insert(&row[i].0, offs[i] + z);
            }
            let mut i = 0;
            while i < row.len() {
                if !row[i].2 {
                    i += 1;
                    continue;
                }
                let first = i;
                while i < row.len() && row[i].2 {
                    i += 1;
                }
                let lo = match first.checked_sub(1) {
                    Some(b) => left[row[b].0.as_str()] + row[b].1 + gap(false, true),
                    None => 0.0,
                };
                let hi = match row.get(i) {
                    Some(b) => left[b.0.as_str()] - gap(true, false),
                    None => widest,
                };
                let step = gap(true, true);
                let targets: Vec<(f64, f64)> = (first..i)
                    .enumerate()
                    .map(|(j, l)| {
                        let at = mean(&mut nbr[&row[l].0].iter(), &centre)
                            .map_or(left[row[l].0.as_str()], |m| m.0);
                        (at - j as f64 * step, 1.0)
                    })
                    .collect();
                let top = hi - (i - first - 1) as f64 * step;
                for (j, x) in isotonic(&targets, lo, top).into_iter().enumerate() {
                    left.insert(&row[first + j].0, x + j as f64 * step);
                }
            }
            for (id, w, _) in row {
                centre.insert(id, left[id.as_str()] + w / 2.0);
            }
        }
    }
    left.into_iter().map(|(id, x)| (id.to_owned(), x)).collect()
}

fn sublayers(
    ids: &[String],
    next: &std::collections::HashMap<String, Vec<String>>,
) -> std::collections::HashMap<String, usize> {
    use std::collections::{HashMap, HashSet};
    struct Walk<'a> {
        next: &'a HashMap<String, Vec<String>>,
        index: HashMap<String, usize>,
        low: HashMap<String, usize>,
        stack: Vec<String>,
        open: HashSet<String>,
        row: HashMap<String, usize>,
    }
    impl Walk<'_> {
        fn visit(&mut self, u: &str) {
            let i = self.index.len();
            self.index.insert(u.to_owned(), i);
            self.low.insert(u.to_owned(), i);
            self.stack.push(u.to_owned());
            self.open.insert(u.to_owned());
            for v in &self.next[u] {
                if !self.index.contains_key(v) {
                    self.visit(v);
                    let low = self.low[u].min(self.low[v]);
                    self.low.insert(u.to_owned(), low);
                } else if self.open.contains(v) {
                    let low = self.low[u].min(self.index[v]);
                    self.low.insert(u.to_owned(), low);
                }
            }
            if self.low[u] != i {
                return;
            }
            let mut members = Vec::new();
            loop {
                let w = self.stack.pop().unwrap();
                self.open.remove(&w);
                let done = w == u;
                members.push(w);
                if done {
                    break;
                }
            }
            let row = members
                .iter()
                .flat_map(|m| &self.next[m])
                .filter(|v| !members.contains(v))
                .map(|v| self.row[v] + 1)
                .max()
                .unwrap_or(0);
            for m in members {
                self.row.insert(m, row);
            }
        }
    }
    let mut walk = Walk {
        next,
        index: HashMap::new(),
        low: HashMap::new(),
        stack: Vec::new(),
        open: HashSet::new(),
        row: HashMap::new(),
    };
    for id in ids {
        if !walk.index.contains_key(id) {
            walk.visit(id);
        }
    }
    walk.row
}

fn rank_key(r: f64) -> String {
    format!("{r:?}")
}

fn oval(c0: &Value) -> (Value, f64, f64) {
    let mut c = c0.clone();
    let w = (80.0_f64).max(
        24.0 + text_w(get(&c, "name").and_then(|n| match n {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        })),
    );
    let h = (44.0_f64).max((0.55 * w).floor());
    put(&mut c, "shape", Value::keyword("oval"));
    let name_line = line("name", get(&c, "name").cloned(), vec![]);
    put(&mut c, "lines", Value::Vector(vec![name_line]));
    (c, w, h)
}

pub fn layout(diagram: &Value) -> Value {
    let mut pkgs: Vec<Value> = Vec::new();
    let mut classes: Vec<Value> = Vec::new();
    let mut lanes: Vec<Value> = Vec::new();
    let mut y = 40.0;
    let mut stride: f64 = 2.0;
    for p in seq(get(diagram, "packages")) {
        stride = stride.max(1.0 + seq(get(p, "classes")).len() as f64);
    }
    let mut ovals: Vec<(Value, f64, f64)> = seq(get(diagram, "foreign")).iter().map(oval).collect();
    let direction = get(diagram, "direction").map(name);
    let horizontal = matches!(direction.as_deref(), Some("lr" | "rl"));
    let wide = get(diagram, "wide").and_then(num);
    let sunk = match seq(get(diagram, "packages")) {
        [p] => {
            (!horizontal || wide.is_some())
                && seq(get(p, "classes"))
                    .iter()
                    .any(|c| get(c, "level").is_some())
        }
        _ => false,
    };
    let sinks: Vec<(String, f64, f64)> = if sunk {
        ovals
            .iter()
            .map(|(c, w, h)| (edn::encode(get(c, "id").unwrap()).unwrap(), *w, *h))
            .collect()
    } else {
        Vec::new()
    };
    let placed: Vec<(Vec<Value>, Vec<Lane>, Vec<Sunk>)> = seq(get(diagram, "packages"))
        .iter()
        .map(|p| {
            place(
                seq(get(p, "classes")),
                seq(get(diagram, "edges")),
                &get(diagram, "direction")
                    .map(name)
                    .unwrap_or_else(|| "tb".into()),
                &sinks,
                wide,
            )
        })
        .collect();
    for c in placed.iter().flat_map(|(inner, _, _)| inner) {
        stride = stride.max(1.0 + num(get(c, "rank").unwrap()).unwrap());
    }
    for ((i, p), (inner, through, sunk)) in
        seq(get(diagram, "packages")).iter().enumerate().zip(placed)
    {
        for (id, r, rank) in sunk {
            let (c, _, _) = ovals
                .iter_mut()
                .find(|(c, _, _)| edn::encode(get(c, "id").unwrap()).unwrap() == id)
                .unwrap();
            put(c, "rect", r.translate(52.0, y + 44.0).to_value());
            put(c, "rank", numval(i as f64 * stride + rank));
        }
        let mut across: Vec<f64> = Vec::new();
        for (ei, points) in through {
            across.extend(points.iter().map(|q| if horizontal { q[1] } else { q[0] }));
            let points = points
                .iter()
                .map(|q| geom::point([q[0] + 52.0, q[1] + y + 44.0]))
                .collect();
            lanes.push(edn::record::record(vec![
                ("edge", numval(ei as f64)),
                ("points", Value::Vector(points)),
            ]));
        }
        let mut rects: Vec<Rect> = Vec::new();
        let mut real: Vec<Value> = Vec::new();
        let mut inner = inner;
        for c in inner.iter_mut() {
            let moved = Rect::from_value(get(c, "rect").unwrap())
                .unwrap()
                .translate(52.0, y + 44.0);
            put(c, "rect", moved.to_value());
            put(c, "package", get(p, "id").cloned().unwrap_or(Value::Nil));
            let rank = num(get(c, "rank").unwrap()).unwrap();
            put(c, "rank", numval(i as f64 * stride + rank));
            ports(c);
            if !flag(c, "dummy?") {
                real.push(c.clone());
                rects.push(Rect::from_value(get(c, "rect").unwrap()).unwrap());
                for k in ["in-ports", "out-ports"] {
                    for port in seq(get(c, k)) {
                        rects.push(Rect::from_value(get(port, "rect").unwrap()).unwrap());
                    }
                }
            }
        }
        let mut body = union_all(&rects).unwrap_or(Rect::new(52.0, y + 44.0, 160.0, 40.0));
        for a in across {
            body = if horizontal {
                union_all(&[body, Rect::new(body.x, a + y + 44.0, 0.0, 0.0)]).unwrap()
            } else {
                union_all(&[body, Rect::new(a + 52.0, body.y, 0.0, 0.0)]).unwrap()
            };
        }
        let pr = Rect::new(
            40.0,
            y,
            (body.right() + 12.0 - 40.0)
                .max(
                    24.0 + text_w(get(p, "label").and_then(|l| match l {
                        Value::String(s) => Some(s.as_str()),
                        _ => None,
                    })),
                )
                .max(180.0),
            body.bottom() + 12.0 - y,
        );
        let mut pkg = Value::Map(vec![
            (
                Value::keyword("id"),
                get(p, "id").cloned().unwrap_or(Value::Nil),
            ),
            (
                Value::keyword("label"),
                get(p, "label").cloned().unwrap_or(Value::Nil),
            ),
            (
                Value::keyword("title"),
                get(p, "label").cloned().unwrap_or(Value::Nil),
            ),
            (Value::keyword("rect"), pr.to_value()),
        ]);
        let source = if !real.is_empty() { &real } else { &inner };
        if let Some(crap) = config::pool_crap(source, true) {
            put(&mut pkg, "crap", crap);
        }
        if let Some(Value::Map(entries)) = config::pool_mutants(source) {
            for (k, v) in entries {
                pkg.assoc(k, v);
            }
        }
        pkgs.push(pkg);
        for c in inner.iter_mut() {
            if flag(c, "dummy?") {
                put(c, "rect", pr.to_value());
                ports(c);
            }
            classes.push(c.clone());
        }
        y += pr.h + 80.0;
    }
    let rs: Vec<Rect> = pkgs
        .iter()
        .map(|p| Rect::from_value(get(p, "rect").unwrap()).unwrap())
        .collect();
    let bounds = union_all(&rs).unwrap_or(Rect::new(40.0, 40.0, 400.0, 300.0));
    let mut oy = bounds.y;
    let mut stacked: Vec<Value> = Vec::new();
    for (c, w, h) in ovals.iter_mut() {
        if get(c, "rect").is_some() {
            classes.push(c.clone());
            continue;
        }
        put(
            c,
            "rect",
            Rect::new(bounds.right() + 40.0, oy, *w, *h).to_value(),
        );
        stacked.push(c.clone());
        oy += *h + 40.0;
    }
    let oh = if stacked.is_empty() {
        0.0
    } else {
        oy - 40.0 - bounds.y
    };
    let dy = if stacked.is_empty() {
        0.0
    } else {
        ((bounds.h - oh) / 2.0).max(0.0)
    };
    for c in stacked.iter_mut() {
        let moved = Rect::from_value(get(c, "rect").unwrap())
            .unwrap()
            .translate(0.0, dy);
        put(c, "rect", moved.to_value());
        classes.push(c.clone());
    }
    Value::Map(vec![
        (Value::keyword("diagram"), diagram.clone()),
        (Value::keyword("packages"), Value::Vector(pkgs)),
        (Value::keyword("classes"), Value::Vector(classes)),
        (Value::keyword("edges"), Value::Vector(vec![])),
        (Value::keyword("lanes"), Value::Vector(lanes)),
    ])
}

fn union_all(rects: &[Rect]) -> Option<Rect> {
    geom::union(&rects.iter().map(|r| Some(*r)).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests;
