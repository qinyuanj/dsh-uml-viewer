//! IR normalization on load (port of `love/uml/domain/ir.lua`): validate and
//! canonicalize the flat/diagram documents the scanners emit.

use edn::Value;
use edn::record::{flag, local_name, name, numval, seq};
use std::collections::HashSet;

fn g<'a>(m: &'a Value, key: &str) -> Option<&'a Value> {
    edn::record::get(m, key)
}

type Result<T> = std::result::Result<T, String>;

fn id(x: &Value) -> Result<Value> {
    match x {
        Value::Keyword(_) => Ok(x.clone()),
        Value::String(s) => {
            let mut out = Vec::with_capacity(s.len());
            let mut in_ws = false;
            for b in s.bytes() {
                if matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
                    in_ws = true;
                } else {
                    if in_ws {
                        out.push(b'-');
                    }
                    in_ws = false;
                    out.push(b.to_ascii_lowercase());
                }
            }
            Ok(Value::keyword(&String::from_utf8(out).unwrap()))
        }
        Value::Symbol(_) => Ok(Value::keyword(local_name(x).unwrap_or_default())),
        _ => Err("id must be a keyword or string".into()),
    }
}

fn number(x: Option<&Value>, kind: &str) -> Result<Value> {
    let Some(x) = x else { return Ok(Value::Nil) };
    let Some(n) = edn::record::num(x) else {
        return Err(format!("{kind} must be a number"));
    };
    if kind == "coverage" {
        if !(0.0..=1.0).contains(&n) {
            return Err("coverage must be between 0 and 1".into());
        }
    } else {
        if n < 0.0 {
            return Err(format!("{kind} must be non-negative"));
        }
        if kind != "cc" && n != n.floor() {
            return Err(format!("{kind} must be an integer"));
        }
    }
    Ok(x.clone())
}

fn crap(x: Option<&Value>) -> Result<Value> {
    let Some(x) = x else { return Ok(Value::Nil) };
    if let Some(n) = edn::record::num(x) {
        return Ok(Value::Map(vec![
            (Value::keyword("mu"), numval(n)),
            (Value::keyword("max"), numval(n)),
            (Value::keyword("sigma"), Value::Integer(0)),
        ]));
    }
    if !matches!(x, Value::Map(_)) {
        return Err("crap must be a number or map".into());
    }
    let mut r = Value::Map(vec![
        (
            Value::keyword("mu"),
            g(x, "mu").cloned().unwrap_or(Value::Nil),
        ),
        (
            Value::keyword("max"),
            g(x, "max")
                .or_else(|| g(x, "mu"))
                .cloned()
                .unwrap_or(Value::Integer(0)),
        ),
        (
            Value::keyword("sigma"),
            g(x, "sigma")
                .or_else(|| g(x, "sd"))
                .cloned()
                .unwrap_or(Value::Integer(0)),
        ),
    ]);
    if let Some(n) = g(x, "n").and_then(edn::record::num)
        && n > 0.0
    {
        edn::record::put(&mut r, "n", numval(n.floor()));
    }
    Ok(r)
}

fn member(x: &Value) -> Result<Value> {
    if let Value::String(_) = x {
        return Ok(Value::Map(vec![(Value::keyword("text"), x.clone())]));
    }
    if !matches!(x, Value::Map(_)) {
        return Err("member must be a string or map".into());
    }
    let text = match g(x, "text") {
        Some(t) => t.clone(),
        None => {
            let mut text = match g(x, "name") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => name(other),
                None => String::new(),
            };
            if let Some(Value::Vector(args)) = g(x, "args")
                && !args.is_empty()
            {
                let parts: Vec<String> = args
                    .iter()
                    .map(|a| match a {
                        Value::String(s) => s.clone(),
                        other => name(other),
                    })
                    .collect();
                text = format!("{}({})", text, parts.join(", "));
            }
            if let Some(t) = g(x, "type") {
                text = format!(
                    "{} : {}",
                    text,
                    match t {
                        Value::String(s) => s.clone(),
                        other => name(other),
                    }
                );
            }
            if let Some(r) = g(x, "returns") {
                text = format!(
                    "{} : {}",
                    text,
                    match r {
                        Value::String(s) => s.clone(),
                        other => name(other),
                    }
                );
            }
            Value::String(text)
        }
    };
    let mut r = Value::Map(vec![(Value::keyword("text"), text)]);
    for k in [
        "coverage",
        "killed",
        "survived",
        "uncovered",
        "sites",
        "cc",
        "crap",
    ] {
        if let Some(raw) = edn::record::get_raw(x, k) {
            let v = if k == "crap" {
                crap(Some(raw))?
            } else {
                number(Some(raw), k)?
            };
            edn::record::put(&mut r, k, v);
        }
    }
    if edn::record::is_true(x, "private") {
        edn::record::put(&mut r, "private", Value::Bool(true));
    }
    if let Some(n) = g(x, "name") {
        edn::record::put(&mut r, "name", n.clone());
    }
    Ok(r)
}

fn class(c: &Value) -> Result<Value> {
    let label = match g(c, "name") {
        Some(v) => v.clone(),
        None => match g(c, "id") {
            Some(v) if !matches!(v, Value::Nil) => Value::String(name(v)),
            _ => return Err("class needs :name or :id".into()),
        },
    };
    let idv = id(g(c, "id").unwrap_or(&label))?;
    let oval = flag(c, "foreign") || g(c, "shape").and_then(local_name) == Some("oval");
    let mut r = Value::Map(vec![
        (Value::keyword("id"), idv),
        (Value::keyword("name"), label),
        (
            Value::keyword("shape"),
            if oval {
                Value::keyword("oval")
            } else {
                Value::Nil
            },
        ),
        (
            Value::keyword("stereotype"),
            g(c, "stereotype").cloned().unwrap_or(Value::Nil),
        ),
        (Value::keyword("crap"), crap(g(c, "crap"))?),
        (
            Value::keyword("hide-members"),
            Value::Bool(edn::record::is_true(c, "hide-members")),
        ),
    ]);
    for k in ["coverage", "cc", "killed", "survived", "uncovered", "sites"] {
        edn::record::put(&mut r, k, number(g(c, k), k)?);
    }
    for k in ["fields", "ops"] {
        let mut xs = Vec::new();
        for x in seq(g(c, k)) {
            xs.push(member(x)?);
        }
        edn::record::put(&mut r, k, Value::Vector(xs));
    }
    for k in ["ns", "lang", "file", "level"] {
        if let Some(v) = g(c, k) {
            edn::record::put(&mut r, k, v.clone());
        }
    }
    Ok(r)
}

pub fn normalize(raw: &Value) -> Result<Value> {
    let (mut ps, mut fs, mut es) = (Vec::new(), Vec::new(), Vec::new());
    let mut ids: HashSet<String> = HashSet::new();
    let mut add = |c: &Value| -> Result<Value> {
        let x = class(c)?;
        let k = edn::encode(g(&x, "id").unwrap()).unwrap();
        if !ids.insert(k.clone()) {
            return Err(format!("duplicate class id: {k}"));
        }
        Ok(x)
    };
    for p in seq(g(raw, "packages")) {
        let label = match g(p, "label") {
            Some(v) => v.clone(),
            None => match g(p, "id") {
                Some(v) if !matches!(v, Value::Nil) => Value::String(name(v)),
                _ => return Err("package needs :label or :id".into()),
            },
        };
        let mut cs = Vec::new();
        for c in seq(g(p, "classes")) {
            cs.push(add(c)?);
        }
        ps.push(Value::Map(vec![
            (Value::keyword("id"), id(g(p, "id").unwrap_or(&label))?),
            (Value::keyword("label"), label),
            (Value::keyword("crap"), crap(g(p, "crap"))?),
            (Value::keyword("classes"), Value::Vector(cs)),
        ]));
    }
    for c in seq(g(raw, "foreign")) {
        fs.push(add(c)?);
    }
    for e in seq(g(raw, "edges")) {
        if g(e, "from").is_none() || g(e, "to").is_none() {
            return Err("edge needs :from and :to".into());
        }
        let f = id(g(e, "from").unwrap())?;
        let t = id(g(e, "to").unwrap())?;
        if !ids.contains(&edn::encode(&f).unwrap()) {
            return Err("edge :from unknown class".into());
        }
        if !ids.contains(&edn::encode(&t).unwrap()) {
            return Err("edge :to unknown class".into());
        }
        let mut x = Value::Map(vec![
            (Value::keyword("from"), f),
            (Value::keyword("to"), t),
            (
                Value::keyword("kind"),
                id(default_or(g(e, "kind"), &Value::keyword("association")))?,
            ),
            (
                Value::keyword("label"),
                g(e, "label").cloned().unwrap_or(Value::Nil),
            ),
        ]);
        if edn::record::is_true(e, "violating") {
            edn::record::put(&mut x, "violating", Value::Bool(true));
        }
        es.push(x);
    }
    Ok(Value::Map(vec![
        (
            Value::keyword("title"),
            g(raw, "title")
                .cloned()
                .unwrap_or(Value::String("UML".into())),
        ),
        (
            Value::keyword("direction"),
            id(default_or(g(raw, "direction"), &Value::keyword("tb")))?,
        ),
        (Value::keyword("packages"), Value::Vector(ps)),
        (Value::keyword("foreign"), Value::Vector(fs)),
        (Value::keyword("edges"), Value::Vector(es)),
    ]))
}

fn default_or<'a>(v: Option<&'a Value>, d: &'a Value) -> &'a Value {
    match v {
        Some(x) if edn::record::truthy(v) => x,
        _ => d,
    }
}

/// Parse an IR document; hierarchical documents pass through unchanged.
pub fn read_document(text: &str) -> Result<Value> {
    let raw = edn::parse(text).map_err(|e| e.to_string())?;
    if !matches!(raw, Value::Map(_)) {
        return Err("document must be an EDN map".into());
    }
    if flag(&raw, "hierarchical") {
        return Ok(raw);
    }
    let mut ds = Vec::new();
    let diagrams = match g(&raw, "diagrams") {
        Some(Value::Vector(items)) => items.clone(),
        _ => vec![raw.clone()],
    };
    for d in &diagrams {
        ds.push(normalize(d)?);
    }
    Ok(Value::Map(vec![
        (
            Value::keyword("title"),
            g(&raw, "title")
                .cloned()
                .unwrap_or(Value::String("UML".into())),
        ),
        (Value::keyword("diagrams"), Value::Vector(ds)),
    ]))
}
