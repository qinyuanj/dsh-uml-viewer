use edn::Value;
use edn::record::{flag, get, num, numval};

pub const GRADE_BEST: f64 = 10.0;
pub const GRADE_MID: f64 = 5.5;
pub const GRADE_WORST: f64 = 1.0;

pub fn crap_band(score: Option<f64>) -> Value {
    Value::keyword(match score {
        None => "red",
        Some(s) if s <= 8.0 => "green",
        Some(s) if s <= 12.0 => "yellow",
        _ => "red",
    })
}

pub fn mutation_band(score: Option<f64>) -> Value {
    Value::keyword(match score {
        None => "red",
        Some(s) if s <= 0.8 => "red",
        Some(s) if s <= 0.9 => "yellow",
        _ => "green",
    })
}

fn along(x: f64, a: f64, b: f64, c: f64, y: f64, z: f64, w: f64) -> f64 {
    if x <= a {
        y
    } else if x <= b {
        y + (x - a) / (b - a) * (z - y)
    } else if x <= c {
        z + (x - b) / (c - b) * (w - z)
    } else {
        w
    }
}

pub fn crap_grade(score: Option<f64>) -> f64 {
    match score {
        None => 1.0,
        Some(s) => along(s, 8.0, 12.0, 20.0, 10.0, 5.5, 1.0),
    }
}

pub fn mutation_grade(score: Option<f64>) -> f64 {
    match score {
        None => 1.0,
        Some(s) => along(s, 0.8, 0.9, 1.0, 1.0, 5.5, 10.0),
    }
}

pub fn crap_risk(crap: Option<&Value>) -> Option<f64> {
    let mu = num(get(crap?, "mu")?)?;
    Some(mu + get(crap?, "sigma").and_then(num).unwrap_or(0.0))
}

pub fn combined_grade(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some((a + b) / 2.0),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

/// A traffic-light band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Red,
    Yellow,
    Green,
}

impl Band {
    fn of(band: &Value) -> Band {
        match edn::record::local_name(band).unwrap_or_default() {
            "green" => Band::Green,
            "yellow" => Band::Yellow,
            _ => Band::Red,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Band::Red => "red",
            Band::Yellow => "yellow",
            Band::Green => "green",
        }
    }
}

/// The quality rating of a class or package record: the C and M bands and
/// 1-10 grades its box and badges are drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rating {
    pub crap: Band,
    pub mutation: Band,
    pub crap_grade: f64,
    pub mutation_grade: f64,
    /// Either metric is absent: decided from the nil metrics, never
    /// inferred from a red band.
    pub missing: bool,
}

impl Rating {
    /// The worse of the two bands.
    pub fn band(&self) -> Band {
        if self.crap == Band::Red || self.mutation == Band::Red {
            Band::Red
        } else if self.crap == Band::Yellow || self.mutation == Band::Yellow {
            Band::Yellow
        } else {
            Band::Green
        }
    }

    /// The 4e69e34 box grade: the average of the CRAP and mutation grades.
    pub fn grade(&self) -> Option<f64> {
        combined_grade(Some(self.crap_grade), Some(self.mutation_grade))
    }

    /// A class box grade: an unmeasured class is the worst grade.
    pub fn class_grade(&self) -> Option<f64> {
        if self.missing {
            Some(GRADE_WORST)
        } else {
            self.grade()
        }
    }
}

/// Rate a class or package record of the overlaid scene.
pub fn rating(record: &Value) -> Rating {
    let risk = crap_risk(get(record, "crap"));
    let ratio = mutation_ratio(record);
    Rating {
        crap: Band::of(&crap_band(risk)),
        mutation: Band::of(&mutation_band(ratio)),
        crap_grade: crap_grade(risk),
        mutation_grade: mutation_grade(ratio),
        missing: risk.is_none() || ratio.is_none(),
    }
}

/// The rating of one class-card stats row (the class or one operation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricRating {
    pub crap_grade: f64,
    /// killed / (killed + survived + uncovered), None when nothing counted.
    pub kill_ratio: Option<f64>,
    pub mutation_grade: f64,
    /// Averaged swatch grade; a missing metric enters as grade 1.
    pub grade: Option<f64>,
}

/// Rate a card row from its CRAP `mu` and mutation counts.
pub fn metric_rating(
    crap_mu: Option<f64>,
    killed: Option<f64>,
    survived: Option<f64>,
    uncovered: Option<f64>,
) -> MetricRating {
    let total = killed.unwrap_or(0.0) + survived.unwrap_or(0.0) + uncovered.unwrap_or(0.0);
    let kill_ratio = (total > 0.0).then(|| killed.unwrap_or(0.0) / total);
    let (c, m) = (crap_grade(crap_mu), mutation_grade(kill_ratio));
    MetricRating {
        crap_grade: c,
        kill_ratio,
        mutation_grade: m,
        grade: combined_grade(Some(c), Some(m)),
    }
}

pub fn mutation_ratio(m: &Value) -> Option<f64> {
    let k = get(m, "killed").and_then(num);
    let s = get(m, "survived").and_then(num);
    let g = get(m, "mut-gap").and_then(num);
    if k.is_some() || s.is_some() || g.is_some_and(|g| g > 0.0) {
        let n = k.unwrap_or(0.0) + s.unwrap_or(0.0) + g.unwrap_or(0.0);
        if n > 0.0 {
            return Some(k.unwrap_or(0.0) / n);
        }
    }
    None
}

pub fn function_weight(c: &Value) -> f64 {
    match get(c, "ops") {
        Some(Value::Vector(ops)) => (ops.len().max(1)) as f64,
        _ => 1.0,
    }
}

struct Group {
    n: f64,
    mu: f64,
    sigma: f64,
    max: f64,
}

fn group(xs: &[f64]) -> Group {
    let mut sum = 0.0;
    let mut mx = f64::NEG_INFINITY;
    for x in xs {
        sum += x;
        mx = mx.max(*x);
    }
    let mu = sum / xs.len() as f64;
    let mut variance = 0.0;
    for x in xs {
        variance += (x - mu) * (x - mu);
    }
    Group {
        n: xs.len() as f64,
        mu,
        sigma: (variance / xs.len() as f64).sqrt(),
        max: mx,
    }
}

fn rounded(g: &Group) -> Value {
    let r = |x: f64| (x * 10.0 + 0.5).floor() / 10.0;
    Value::Map(vec![
        (Value::keyword("n"), numval(g.n)),
        (Value::keyword("mu"), numval(r(g.mu))),
        (Value::keyword("sigma"), numval(r(g.sigma))),
        (Value::keyword("max"), numval(r(g.max))),
    ])
}

pub fn summarize_scores(scores: &[Value]) -> Option<Value> {
    let xs: Vec<f64> = scores
        .iter()
        .filter_map(|x| match x {
            Value::Integer(_) | Value::Float(_) => num(x),
            _ => num(get(x, "mu")?),
        })
        .collect();
    if xs.is_empty() {
        return None;
    }
    Some(rounded(&group(&xs)))
}

fn module(c: &Value) -> bool {
    !flag(c, "foreign") && get(c, "shape") != Some(&Value::keyword("oval"))
}

fn score(x: &Value) -> Option<f64> {
    match x {
        Value::Integer(_) | Value::Float(_) => num(x),
        _ => get(x, "mu").and_then(num),
    }
}

pub fn pool_crap(classes: &[Value], rolled: bool) -> Option<Value> {
    let mut groups: Vec<Group> = Vec::new();
    for c in classes {
        if !module(c) {
            continue;
        }
        let xs: Vec<f64> = match get(c, "ops") {
            Some(Value::Vector(ops)) => ops
                .iter()
                .filter_map(|op| get(op, "crap").and_then(score))
                .collect(),
            _ => Vec::new(),
        };
        let summary = get(c, "crap");
        let mu = summary.and_then(|s| get(s, "mu")).and_then(num);
        let g = if !xs.is_empty() && !(rolled && mu.is_some()) {
            group(&xs)
        } else if let Some(mu) = mu {
            let n = match summary.and_then(|s| get(s, "n")).and_then(num) {
                Some(n) if n > 0.0 => n,
                _ => {
                    if !xs.is_empty() {
                        xs.len() as f64
                    } else {
                        function_weight(c)
                    }
                }
            };
            Group {
                n: n.floor(),
                mu,
                sigma: summary
                    .and_then(|s| get(s, "sigma"))
                    .and_then(num)
                    .unwrap_or(0.0),
                max: summary
                    .and_then(|s| get(s, "max"))
                    .and_then(num)
                    .unwrap_or(mu),
            }
        } else {
            Group {
                n: function_weight(c),
                mu: 20.0,
                sigma: 0.0,
                max: 20.0,
            }
        };
        groups.push(g);
    }
    if groups.is_empty() {
        return None;
    }
    let (mut n, mut sum, mut m2, mut mx) = (0.0, 0.0, 0.0, f64::NEG_INFINITY);
    for g in &groups {
        n += g.n;
        sum += g.n * g.mu;
        m2 += g.n * (g.sigma * g.sigma + g.mu * g.mu);
        mx = mx.max(g.max);
    }
    let mu = sum / n;
    Some(rounded(&Group {
        n,
        mu,
        sigma: (m2 / n - mu * mu).max(0.0).sqrt(),
        max: mx,
    }))
}

pub fn pool_mutants(classes: &[Value]) -> Option<Value> {
    let (mut k, mut s, mut g, mut u) = (0.0, 0.0, 0.0, 0.0);
    let (mut measured, mut uncovered) = (false, false);
    for c in classes {
        if !module(c) {
            continue;
        }
        let ck = get(c, "killed").and_then(num);
        let cs = get(c, "survived").and_then(num);
        let cg = get(c, "mut-gap").and_then(num);
        let cu = get(c, "uncovered").and_then(num);
        let has = ck.is_some() || cs.is_some();
        if has {
            measured = true;
            k += ck.unwrap_or(0.0);
            s += cs.unwrap_or(0.0);
        }
        if let Some(cu) = cu {
            uncovered = true;
            u += cu;
        }
        g += if has || cg.is_some() {
            cg.unwrap_or(0.0)
        } else {
            function_weight(c)
        };
    }
    if !measured && g <= 0.0 {
        return None;
    }
    let mut r = Value::Map(vec![]);
    if measured {
        r.assoc(Value::keyword("killed"), numval(k));
        r.assoc(Value::keyword("survived"), numval(s));
    }
    if uncovered {
        r.assoc(Value::keyword("uncovered"), numval(u));
    }
    if g > 0.0 {
        r.assoc(Value::keyword("mut-gap"), numval(g));
    }
    Some(r)
}

#[cfg(test)]
mod tests;
