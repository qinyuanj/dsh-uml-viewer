//! Port of `love/uml/lua_language/graph_lua.lua`: a pure Lua module scanner
//! over caller-supplied canonical source snapshots. Scanner limitations are
//! preserved (the goldens encode them); positions are 1-based like Lua.

use edn::Value;

use crate::scan::{self, Scanner, SourceFile};
use crate::text::{self, at, is_alpha_, is_space, is_word};
use crate::value::get_truthy as get;
use edn::record::{flag, record, seq};

const SKIP_DIRS: &[&str] = &[
    "lua_modules",
    "spec",
    "tests",
    "test",
    "dist",
    "build",
    "target",
    "out",
    "coverage",
];

fn ident(c: Option<u8>) -> bool {
    c.is_some_and(is_word)
}

/// `keyword(s, i, w)`: literal `w` at 1-based `i` with non-identifier flanks.
fn keyword(s: &[u8], i: usize, w: &[u8]) -> bool {
    text::sub(s, i as i64, i as i64 + w.len() as i64 - 1) == w
        && !ident(at(s, i.wrapping_sub(1)))
        && !ident(at(s, i + w.len()))
}

/// `long_end(s, i)`: a long bracket `[=*[` at `i`; returns one past `]=*]`.
fn long_end(s: &[u8], i: usize) -> Option<usize> {
    if at(s, i) != Some(b'[') {
        return None;
    }
    let mut j = i + 1;
    while at(s, j) == Some(b'=') {
        j += 1;
    }
    if at(s, j) != Some(b'[') {
        return None;
    }
    let eq_len = j - (i + 1);
    let mut close = vec![b']'];
    close.extend(std::iter::repeat_n(b'=', eq_len));
    close.push(b']');
    let start = i + eq_len + 2;
    match text::find_plain(s, &close, start) {
        Some(b) => Some(b + close.len()),
        None => Some(s.len() + 1),
    }
}

fn in_hole(holes: &[(usize, usize)], i: usize) -> bool {
    holes.iter().any(|&(a, b)| i >= a && i < b)
}

/// `mask(source)`: comments and long brackets blanked (newlines kept);
/// quoted strings kept with their interior recorded as a hole.
fn mask(source: &[u8]) -> (Vec<u8>, Vec<(usize, usize)>) {
    let mut out = Vec::new();
    let mut holes = Vec::new();
    let mut i = 1;
    while i <= source.len() {
        let c = at(source, i).unwrap();
        let mut finish = None;
        let mut quoted = false;
        if at(source, i) == Some(b'-') && at(source, i + 1) == Some(b'-') {
            finish = Some(
                long_end(source, i + 2)
                    .or_else(|| text::find_plain(source, b"\n", i))
                    .unwrap_or(source.len() + 1),
            );
        } else if c == b'"' || c == b'\'' {
            finish = Some(text::scan_quoted(source, i, c, None));
            quoted = true;
        } else if c == b'[' {
            finish = long_end(source, i);
        }
        if let Some(finish) = finish {
            let text = &source[i - 1..finish - 1];
            if quoted {
                out.extend_from_slice(text);
                holes.push((i + 1, (source.len() + 1).min(finish - 1)));
            } else {
                out.extend(text.iter().map(|&c| if c == b'\n' { c } else { b' ' }));
            }
            i = finish;
        } else {
            out.push(c);
            i += 1;
        }
    }
    (out, holes)
}

fn strip_spaces(s: &[u8]) -> usize {
    s.iter().take_while(|&&c| is_space(c)).count()
}

/// `[%a_][%w_%.:]*` capture starting at 0 (byte offset into the slice).
fn capture_dotted(s: &[u8]) -> Option<usize> {
    if !s.first().is_some_and(|&c| is_alpha_(c)) {
        return None;
    }
    let len = s
        .iter()
        .take_while(|&&c| is_word(c) || c == b'.' || c == b':')
        .count();
    Some(len)
}

/// `^local%s+function%s+([%a_][%w_%.:]*)`.
fn match_local_function(rest: &[u8]) -> Option<Vec<u8>> {
    let s = rest.strip_prefix(b"local")?;
    let n = strip_spaces(s);
    if n == 0 {
        return None;
    }
    let s = s[n..].strip_prefix(b"function")?;
    let n = strip_spaces(s);
    if n == 0 {
        return None;
    }
    let len = capture_dotted(&s[n..])?;
    Some(s[n..n + len].to_vec())
}

/// `^local%s+([%a_][%w_%.:]*)%s*=%s*function%f[^%w_]`.
fn match_local_assign_function(rest: &[u8]) -> Option<Vec<u8>> {
    let s = rest.strip_prefix(b"local")?;
    let n = strip_spaces(s);
    if n == 0 {
        return None;
    }
    let s = &s[n..];
    let len = capture_dotted(s)?;
    let captured = s[..len].to_vec();
    let s = &s[len..];
    let s = &s[strip_spaces(s)..];
    let s = s.strip_prefix(b"=")?;
    let s = &s[strip_spaces(s)..];
    let s = s.strip_prefix(b"function")?;
    if s.first().is_some_and(|&c| is_word(c)) {
        return None;
    }
    Some(captured)
}

/// `^function%s+([%a_][%w_%.:]*)`.
fn match_function(rest: &[u8]) -> Option<Vec<u8>> {
    let s = rest.strip_prefix(b"function")?;
    let n = strip_spaces(s);
    if n == 0 {
        return None;
    }
    let len = capture_dotted(&s[n..])?;
    Some(s[n..n + len].to_vec())
}

/// `^([%a_][%w_%.:]*)%s*=%s*function%f[^%w_]`.
fn match_assign_function(rest: &[u8]) -> Option<Vec<u8>> {
    let len = capture_dotted(rest)?;
    let s = &rest[len..];
    let s = &s[strip_spaces(s)..];
    let s = s.strip_prefix(b"=")?;
    let s = &s[strip_spaces(s)..];
    let s = s.strip_prefix(b"function")?;
    if s.first().is_some_and(|&c| is_word(c)) {
        return None;
    }
    Some(rest[..len].to_vec())
}

/// `declaration(rest, kw)`.
fn declaration(rest: &[u8], kw: Option<&str>) -> (Option<Vec<u8>>, bool) {
    match kw {
        Some("local") => (
            match_local_function(rest).or_else(|| match_local_assign_function(rest)),
            true,
        ),
        Some("function") => (match_function(rest), false),
        None => (match_assign_function(rest), false),
        _ => (None, false),
    }
}

fn statement_before(text: &[u8], i: usize) -> Vec<u8> {
    let before = &text[..i - 1];
    let start = before
        .iter()
        .rposition(|&c| c == b'\n' || c == b';')
        .map_or(0, |p| p + 1);
    before[start..].to_vec()
}

/// `statement:match('local%s*$')`.
fn ends_with_local(s: &[u8]) -> bool {
    let end = s.iter().rposition(|&c| !is_space(c)).map_or(0, |p| p + 1);
    s[..end].ends_with(b"local")
}

/// The `local X = require "mod"` scan: returns (start, end, binding, module).
fn find_binding(text: &[u8], pos: usize) -> Option<(usize, usize, Vec<u8>, Vec<u8>)> {
    for i in pos..=text.len() {
        if let Some((b, binding, module)) = match_binding_at(text, i) {
            return Some((i, b, binding, module));
        }
    }
    None
}

/// `local%s+([%a_][%w_]*)%s*=%s*require` at `i`, then the literal module that
/// `require_at` reads there (so a concatenated name binds nothing).
/// Returns (end, binding, module) with `end` the 1-based inclusive end of
/// `require`, like Lua's `b`.
fn match_binding_at(t: &[u8], i: usize) -> Option<(usize, Vec<u8>, Vec<u8>)> {
    let mut p = i - 1;
    if !t.get(p..)?.starts_with(b"local") {
        return None;
    }
    p += 5;
    let spaces = t[p..].iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    p += spaces;
    if !t.get(p).is_some_and(|&c| is_alpha_(c)) {
        return None;
    }
    let binding_start = p;
    while t.get(p).is_some_and(|&c| is_word(c)) {
        p += 1;
    }
    let binding = t[binding_start..p].to_vec();
    p += t[p..].iter().take_while(|&&c| is_space(c)).count();
    if t.get(p) != Some(&b'=') {
        return None;
    }
    p += 1;
    p += t[p..].iter().take_while(|&&c| is_space(c)).count();
    match require_at(t, p + 1)? {
        Require::Module(module) => Some((p + 7, binding, module)),
        Require::Dynamic => None,
    }
}

/// A `require` call at `i`: a literal module name, or a call whose argument
/// is an expression (`require(name)`) and so cannot be resolved statically.
enum Require {
    Module(Vec<u8>),
    Dynamic,
}

/// `[%w_%.%-/]`: a byte of a module name as the searchers read it.
fn module_byte(c: u8) -> bool {
    is_word(c) || matches!(c, b'.' | b'-' | b'/')
}

/// `^([%a_][%w_%.%-/]*)`: the module name opening `s`, `/` read as `.`.
/// The searchers turn `.` into the directory separator, so
/// `require "lib/lume"` loads the file `require "lib.lume"` does; `-` is an
/// ordinary name byte (`game-state.lua`).
fn required_name(s: &[u8]) -> Option<Vec<u8>> {
    if !s.first().is_some_and(|&c| is_alpha_(c)) {
        return None;
    }
    let len = s.iter().take_while(|&&c| module_byte(c)).count();
    Some(
        s[..len]
            .iter()
            .map(|&c| if c == b'/' { b'.' } else { c })
            .collect(),
    )
}

/// `s:gsub('%s+$', '')`.
fn trim_end(s: &[u8]) -> &[u8] {
    &s[..s.iter().rposition(|&c| !is_space(c)).map_or(0, |p| p + 1)]
}

/// `pcall%s*%(%s*$` before the `require` at 1-based `i`:
/// `pcall(require, "m")` loads `m` like `require "m"`, failing softly.
fn protected(text: &[u8], i: usize) -> bool {
    let Some(before) = trim_end(&text[..i - 1]).strip_suffix(b"(") else {
        return false;
    };
    let before = trim_end(before);
    before.len() >= 5 && keyword(text, before.len() - 4, b"pcall")
}

fn require_at(text: &[u8], i: usize) -> Option<Require> {
    if !keyword(text, i, b"require") {
        return None;
    }
    // `%s*`, newlines too: the argument may open the next line, and the
    // comments between are already blank.
    let after = text::sub(text, (i + 7) as i64, text.len() as i64);
    let after = &after[strip_spaces(after)..];
    let (call, tail) = match after.first() {
        Some(b'(') => (true, &after[1..]),
        Some(b',') if protected(text, i) => (true, &after[1..]),
        _ => (false, after),
    };
    let tail = &tail[strip_spaces(tail)..];
    match tail.first() {
        // `require("states." .. name)`: the literal is only a prefix. Without
        // parentheses `require "v" .. "-dev"` concatenates the call's result.
        Some(&q @ (b'"' | b'\'')) => {
            let name = required_name(&tail[1..])?;
            let after = &tail[(text::scan_quoted(tail, 1, q, None) - 1).min(tail.len())..];
            if call && after[strip_spaces(after)..].starts_with(b"..") {
                Some(Require::Dynamic)
            } else {
                Some(Require::Module(name))
            }
        }
        // `require(name)`, and `require(... .. ".x")` relative to the vararg.
        Some(&c)
            if call
                && (is_alpha_(c) || c == b'(' || tail.starts_with(b"..."))
                && !matches!(at(text, i - 1), Some(b'.' | b':')) =>
        {
            Some(Require::Dynamic)
        }
        _ => None,
    }
}

fn skip_ws(t: &[u8], p: usize) -> usize {
    p + t[p.min(t.len())..]
        .iter()
        .take_while(|&&c| is_space(c))
        .count()
}

/// An identifier at 0-based `p`: its end, or `None`.
fn ident_end(t: &[u8], p: usize) -> Option<usize> {
    if !t.get(p).is_some_and(|&c| is_alpha_(c)) {
        return None;
    }
    Some(p + t[p..].iter().take_while(|&&c| is_word(c)).count())
}

/// `{ "a.b", 'c' }` at 0-based `p`: the module names of a constructor that
/// holds only string literals; anything else is not a literal list.
fn literal_modules(t: &[u8], p: usize) -> Option<Vec<Vec<u8>>> {
    if t.get(p) != Some(&b'{') {
        return None;
    }
    let mut out = Vec::new();
    let mut p = skip_ws(t, p + 1);
    while t.get(p) != Some(&b'}') {
        let quote = *t.get(p).filter(|&&c| c == b'"' || c == b'\'')?;
        let start = p + 1;
        let len = t[start..]
            .iter()
            .take_while(|&&c| c != quote && c != b'\n')
            .count();
        let module = required_name(&t[start..start + len]).filter(|m| m.len() == len)?;
        if t.get(start + len) != Some(&quote) {
            return None;
        }
        out.push(module);
        p = skip_ws(t, start + len + 1);
        if matches!(t.get(p), Some(b',' | b';')) {
            p = skip_ws(t, p + 1);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The literal list bound by the last `local name = {` before 0-based `end`.
fn local_literal_modules(t: &[u8], name: &[u8], end: usize) -> Option<Vec<Vec<u8>>> {
    (0..end).rev().find_map(|j| {
        if !keyword(t, j + 1, b"local") {
            return None;
        }
        let p = skip_ws(t, j + 5);
        let e = ident_end(t, p)?;
        if &t[p..e] != name {
            return None;
        }
        let p = skip_ws(t, e);
        (t.get(p) == Some(&b'=')).then(|| literal_modules(t, skip_ws(t, p + 1)))?
    })
}

/// Whether the block opened just before 0-based `from` is still open at `to`.
fn block_open(t: &[u8], holes: &[(usize, usize)], from: usize, to: usize) -> bool {
    let mut depth = 1usize;
    for j in from + 1..=to {
        if in_hole(holes, j) {
            continue;
        }
        if ["function", "if", "do", "repeat"]
            .iter()
            .any(|w| keyword(t, j, w.as_bytes()))
        {
            depth += 1;
        } else if keyword(t, j, b"end") || keyword(t, j, b"until") {
            depth -= 1;
            if depth == 0 {
                return false;
            }
        }
    }
    true
}

/// `require(name)` (or `pcall(require, name)`) at 1-based `i` where `name`
/// is the value of the nearest open `for _, name in ipairs(list) do` over a
/// literal list (inline, or a `local list = {...}`): the listed modules,
/// which are static after all.
fn loop_literal_modules(t: &[u8], holes: &[(usize, usize)], i: usize) -> Option<Vec<Vec<u8>>> {
    let p = skip_ws(t, i - 1 + 7);
    let p = skip_ws(t, p + matches!(t.get(p), Some(b'(' | b',')) as usize);
    let name_end = ident_end(t, p)?;
    let name = &t[p..name_end];
    if t.get(skip_ws(t, name_end)) != Some(&b')') {
        return None;
    }
    let f = (0..i - 1)
        .rev()
        .find(|&j| !in_hole(holes, j + 1) && keyword(t, j + 1, b"for"))?;
    let q = skip_ws(t, f + 3);
    let q = skip_ws(t, ident_end(t, q)?);
    if t.get(q) != Some(&b',') {
        return None;
    }
    let q = skip_ws(t, q + 1);
    let e = ident_end(t, q)?;
    if &t[q..e] != name {
        return None;
    }
    let q = skip_ws(t, e);
    if !keyword(t, q + 1, b"in") {
        return None;
    }
    let q = skip_ws(t, q + 2);
    let e = ident_end(t, q)?;
    if !matches!(&t[q..e], b"ipairs" | b"pairs") {
        return None;
    }
    let q = skip_ws(t, e);
    if t.get(q) != Some(&b'(') {
        return None;
    }
    let q = skip_ws(t, q + 1);
    let (modules, q) = match ident_end(t, q) {
        Some(e) => (local_literal_modules(t, &t[q..e], f)?, e),
        None => {
            let close = q + t[q..].iter().position(|&c| c == b'}')?;
            (literal_modules(t, q)?, close + 1)
        }
    };
    let q = skip_ws(t, q);
    if t.get(q) != Some(&b')') {
        return None;
    }
    let q = skip_ws(t, q + 1);
    (keyword(t, q + 1, b"do") && block_open(t, holes, q + 2, i - 1)).then_some(modules)
}

/// 1-based line of position `i`.
fn line_of(text: &[u8], i: usize) -> i64 {
    text[..i - 1].iter().filter(|&&c| c == b'\n').count() as i64 + 1
}

/// Table-constructor depth after the byte `c`.
fn track_brace(braces: usize, c: Option<u8>) -> usize {
    match c {
        Some(b'{') => braces + 1,
        Some(b'}') => braces.saturating_sub(1),
        _ => braces,
    }
}

/// Opens or closes a block for the keyword; `true` marks a function body.
fn track_block(blocks: &mut Vec<bool>, kw: Option<&str>) {
    match kw {
        Some("function") => blocks.push(true),
        Some("if" | "do" | "repeat") => blocks.push(false),
        Some("end" | "until") => {
            blocks.pop();
        }
        _ => {}
    }
}

/// The stderr line for a file's unresolved `require(name)` calls, if any.
fn dynamic_require_warning(file: &str, lines: &[Value]) -> Option<String> {
    let at: Vec<String> = lines
        .iter()
        .filter_map(|l| match l {
            Value::Integer(n) => Some(format!("{file}:{n}")),
            _ => None,
        })
        .collect();
    (!at.is_empty()).then(|| format!("Unresolved dynamic require: {}", at.join(", ")))
}

/// `read_module(source)`: requires, definitions and the bases named by a
/// top-level `setmetatable`, `Base:extend()` or middleclass `class(n, Base)`.
/// A definition is nested inside a function body or a table constructor,
/// as crapper counts entries; a top-level `if`/`do` block does not nest.
/// `require(name)` cannot be resolved statically; its line is reported,
/// unless `name` iterates a literal module list.
pub fn read_module(source: &[u8]) -> Value {
    let (text, holes) = mask(source);
    let mut requires = Vec::new();
    let mut defns = Vec::new();
    let mut bases: Vec<Value> = Vec::new();
    let mut bindings: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut base_names: Vec<Vec<u8>> = Vec::new();
    let mut dynamic_requires = Vec::new();
    // Open blocks, innermost last: `true` for a function body.
    let mut blocks: Vec<bool> = Vec::new();
    let mut braces: usize = 0;
    let mut pos = 1;
    while let Some((a, b, binding, module)) = find_binding(&text, pos) {
        if !in_hole(&holes, a) {
            bindings.push((binding, module));
        }
        pos = b + 1;
    }
    for i in 1..=text.len() {
        if in_hole(&holes, i) {
            continue;
        }
        match require_at(&text, i) {
            Some(Require::Module(m)) => requires.push(Value::String(text::text(&m))),
            Some(Require::Dynamic) => match loop_literal_modules(&text, &holes, i) {
                Some(modules) => {
                    requires.extend(modules.iter().map(|m| Value::String(text::text(m))))
                }
                None => dynamic_requires.push(Value::Integer(line_of(&text, i))),
            },
            None => {}
        }
        let c = at(&text, i);
        braces = track_brace(braces, c);
        let prev = at(&text, i.wrapping_sub(1));
        if ident(c) && !ident(prev) && prev != Some(b'.') && prev != Some(b':') {
            let mut kw_found = None;
            for word in ["function", "local", "if", "do", "repeat", "end", "until"] {
                if keyword(&text, i, word.as_bytes()) {
                    kw_found = Some(word);
                    break;
                }
            }
            let statement = statement_before(&text, i);
            if kw_found == Some("local") || !ends_with_local(&statement) {
                let (n, loc) = declaration(text::sub(&text, i as i64, text.len() as i64), kw_found);
                if let Some(n) = n {
                    defns.push(record(vec![
                        ("name", Value::String(text::text(&n))),
                        ("local", Value::Bool(loc)),
                        ("nested", Value::Bool(braces > 0 || blocks.contains(&true))),
                    ]));
                }
            }
            if blocks.is_empty() {
                let rest = text::sub(&text, i as i64, text.len() as i64);
                if keyword(&text, i, b"setmetatable") {
                    base_names.extend(match_setmetatable(rest).and_then(match_base));
                } else {
                    base_names
                        .extend(match_extend(rest).or_else(|| middleclass_base(rest, &bindings)));
                }
            }
            track_block(&mut blocks, kw_found);
        }
    }
    for b in &base_names {
        if let Some((_, module)) = bindings.iter().find(|(name, _)| name == b) {
            let v = Value::String(text::text(module));
            if !bases.contains(&v) {
                bases.push(v);
            }
        }
    }
    record(vec![
        ("requires", Value::Vector(requires)),
        ("defns", Value::Vector(defns)),
        ("bases", Value::Vector(bases)),
        ("dynamic-requires", Value::Vector(dynamic_requires)),
    ])
}

/// `text:sub(i):match('^setmetatable%s*%(%s*(.*)')`, past the line end: a
/// metatable constructor may span several lines.
fn match_setmetatable(rest: &[u8]) -> Option<&[u8]> {
    let s = rest.strip_prefix(b"setmetatable")?;
    let s = &s[strip_spaces(s)..];
    let s = s.strip_prefix(b"(")?;
    Some(&s[strip_spaces(s)..])
}

/// `^([%a_][%w_]*)%s*:%s*extend%s*[%(\"'{]`, or `subclass` for `extend`:
/// classic's (and 30log's) `Base:extend()` derives a class from `Base`, as
/// middleclass's `Base:subclass("Name")` does.
fn match_extend(rest: &[u8]) -> Option<Vec<u8>> {
    let len = ident_end(rest, 0)?;
    let s = &rest[len..];
    let s = &s[strip_spaces(s)..];
    let s = s.strip_prefix(b":")?;
    let s = &s[strip_spaces(s)..];
    let s = s
        .strip_prefix(b"extend")
        .or_else(|| s.strip_prefix(b"subclass"))?;
    let s = &s[strip_spaces(s)..];
    matches!(s.first(), Some(b'(' | b'"' | b'\'' | b'{')).then(|| rest[..len].to_vec())
}

/// `^([%a_][%w_]*)%s*%(%s*(["'])[^\n]-%2%s*,%s*([%a_][%w_]*)%s*%)` where the
/// callee is bound to `require "middleclass"` (or `"lib.middleclass"`):
/// middleclass's `class("Lemon", Fruit)` derives `Lemon` from `Fruit`.
fn middleclass_base(rest: &[u8], bindings: &[(Vec<u8>, Vec<u8>)]) -> Option<Vec<u8>> {
    let len = ident_end(rest, 0)?;
    let (_, module) = bindings.iter().find(|(name, _)| *name == rest[..len])?;
    if module.rsplit(|&c| c == b'.').next() != Some(b"middleclass") {
        return None;
    }
    let s = &rest[len..];
    let s = &s[strip_spaces(s)..];
    let s = s.strip_prefix(b"(")?;
    let s = &s[strip_spaces(s)..];
    let quote = *s.first().filter(|&&c| c == b'"' || c == b'\'')?;
    let close = text::scan_quoted(s, 1, quote, None) - 1;
    if close > s.len() || s[..close].contains(&b'\n') {
        return None;
    }
    let s = &s[close..];
    let s = &s[strip_spaces(s)..];
    let s = s.strip_prefix(b",")?;
    let s = &s[strip_spaces(s)..];
    let base = ident_end(s, 0)?;
    let after = &s[base..];
    (after[strip_spaces(after)..].first() == Some(&b')')).then(|| s[..base].to_vec())
}

/// `%b{}` opening `s`: one past its closing brace. Quoted strings are
/// skipped; comments and long strings are already blank.
fn constructor_end(s: &[u8]) -> Option<usize> {
    if s.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut k = 0;
    while k < s.len() {
        let c = s[k];
        match c {
            b'"' | b'\'' => {
                k = text::scan_quoted(s, k + 1, c, None) - 1;
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(k + 1);
                }
            }
            _ => {}
        }
        k += 1;
    }
    None
}

/// `%b{}` opening `s` searched at its own depth for
/// `[{,;]%s*__index%s*=%s*([%a_][%w_]*)%s*[,;}]`: the `__index` field among
/// the metatable's other fields (`__call`, `__tostring`), on any line.
/// Quoted strings are skipped; comments and long strings are already blank.
fn index_field(s: &[u8]) -> Option<Vec<u8>> {
    let mut depth = 0usize;
    let mut k = 0;
    while k < s.len() {
        let c = s[k];
        match c {
            b'"' | b'\'' => {
                k = text::scan_quoted(s, k + 1, c, None) - 1;
                continue;
            }
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return None;
                }
            }
            _ => {}
        }
        if depth == 1 && matches!(c, b'{' | b',' | b';') {
            let f = &s[k + 1..];
            let f = &f[strip_spaces(f)..];
            if let Some(f) = f.strip_prefix(b"__index") {
                let f = &f[strip_spaces(f)..];
                if let Some(f) = f.strip_prefix(b"=") {
                    let f = &f[strip_spaces(f)..];
                    if let Some(len) = ident_end(f, 0) {
                        let after = &f[len..];
                        let after = &after[strip_spaces(after)..];
                        if matches!(after.first(), Some(b',' | b';' | b'}')) {
                            return Some(f[..len].to_vec());
                        }
                    }
                }
            }
        }
        k += 1;
    }
    None
}

/// The base-name alternatives after the first `setmetatable` argument.
fn match_base(tail: &[u8]) -> Option<Vec<u8>> {
    // `^%b{}%s*,%s*(.*)` (the new class's own fields may be filled in) or
    // `^[%a_][%w_%.]*%s*,%s*(.*)`
    let rest: Option<&[u8]> = if tail.first() == Some(&b'{') {
        let s = &tail[constructor_end(tail)?..];
        let s = &s[strip_spaces(s)..];
        s.strip_prefix(b",").map(|s| &s[strip_spaces(s)..])
    } else if tail.first().is_some_and(|&c| is_alpha_(c)) {
        let len = tail
            .iter()
            .take_while(|&&c| is_word(c) || c == b'.')
            .count();
        let s = &tail[len..];
        let s = &s[strip_spaces(s)..];
        s.strip_prefix(b",").map(|s| &s[strip_spaces(s)..])
    } else {
        None
    };
    let rest = rest?;
    // `{ __index = Base, ... }` (see `index_field`) or `^([%a_][%w_]*)%s*%)`
    if rest.first() == Some(&b'{') {
        index_field(rest)
    } else if rest.first().is_some_and(|&c| is_alpha_(c)) {
        let len = rest.iter().take_while(|&&c| is_word(c)).count();
        let base = rest[..len].to_vec();
        let s = &rest[len..];
        let s = &s[strip_spaces(s)..];
        if s.first() == Some(&b')') {
            return Some(base);
        }
        None
    } else {
        None
    }
}

/// `module_relative(path, root)`.
fn module_relative(path: &str, root: &str) -> String {
    let mut rel = scan::relative_path(path, root);
    for prefix in ["src/lua/", "src/", "lua/"] {
        if let Some(stripped) = rel.strip_prefix(prefix) {
            rel = stripped.to_owned();
        }
    }
    module_path(&rel)
}

/// The module a `?.lua;?/init.lua` searcher finds at the relative path `rel`.
fn module_path(rel: &str) -> String {
    let rel = rel.strip_suffix(".lua").unwrap_or(rel);
    let rel = if rel == "init" {
        ""
    } else {
        rel.strip_suffix("/init").unwrap_or(rel)
    };
    rel.replace('/', ".")
}

pub struct LuaScanner;

pub static SCANNER: LuaScanner = LuaScanner;

impl Scanner for LuaScanner {
    fn accepts(&self, relative: &str, opts: &Value) -> bool {
        if !relative.ends_with(".lua") || relative.ends_with("_spec.lua") {
            return false;
        }
        let excluded: Vec<&str> = seq(get(opts, "exclude"))
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        let mut rest = relative;
        while let Some((dir, tail)) = rest.split_once('/') {
            if dir.starts_with('.') || SKIP_DIRS.contains(&dir) || excluded.contains(&dir) {
                return false;
            }
            rest = tail;
        }
        true
    }

    fn accepts_directory(&self, relative: &str, opts: &Value) -> bool {
        self.accepts(&format!("{relative}/__module.lua"), opts)
    }

    fn scan(
        &self,
        root: &str,
        opts: &Value,
        files: &[SourceFile],
        cwd: &str,
    ) -> Result<Value, String> {
        let prefix = get(opts, "prefix")
            .cloned()
            .unwrap_or(Value::String("app".into()));
        let prefix = match &prefix {
            Value::String(s) => s.clone(),
            _ => String::new(),
        };
        let ns_prefix = get(opts, "ns-prefix")
            .and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| prefix.clone());
        let mut sorted: Vec<&SourceFile> = files
            .iter()
            .filter(|f| self.accepts(&scan::relative_path(&f.path, root), opts))
            .collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let mut metas = Vec::new();
        let mut by_rel: Vec<(String, Value)> = Vec::new();
        let mut by_ns: Vec<(String, Value)> = Vec::new();
        // The unstripped path from the scan or project root: a LÖVE game
        // requires from its own root (`require "src.entities.player"`).
        let mut by_path: Vec<(String, Value)> = Vec::new();
        for f in &sorted {
            let rel = module_relative(&f.path, root);
            let ns = if rel == ns_prefix || rel.starts_with(&format!("{ns_prefix}.")) {
                rel.clone()
            } else {
                scan::ns_join(&ns_prefix, &rel)
            };
            let cid = scan::id_of(&ns, Some(&Value::String(prefix.clone())));
            metas.push((f, rel.clone(), ns.clone(), cid.clone()));
            for base in [root, cwd] {
                let path = module_path(&scan::relative_path(&f.path, base));
                if !path.is_empty() {
                    by_path.push((path, cid.clone()));
                }
            }
            if !rel.is_empty() {
                by_rel.push((rel, cid.clone()));
            }
            if !ns.is_empty() {
                by_ns.push((ns, cid));
            }
        }
        // A required name, then the package `a` that `a.init` spells out.
        let resolve = |name: &str| {
            let package = name.strip_suffix(".init").unwrap_or(name);
            [name, package].into_iter().find_map(|n| {
                [&by_rel, &by_ns, &by_path]
                    .into_iter()
                    .find_map(|index| index.iter().find(|(r, _)| r == n))
                    .map(|(_, cid)| cid.clone())
            })
        };
        let mut classes = Vec::new();
        let mut edges: Vec<Value> = Vec::new();
        let mut foreigns: Vec<Value> = Vec::new();
        for (f, _, ns, cid) in &metas {
            let surface = read_module(&f.source);
            let file = scan::relative_path(&f.path, cwd);
            if let Some(warning) =
                dynamic_require_warning(&file, seq(get(&surface, "dynamic-requires")))
            {
                eprintln!("{warning}");
            }
            let mut ops = Vec::new();
            for d in seq(get(&surface, "defns")) {
                if !flag(d, "local") && !flag(d, "nested") {
                    ops.push(record(vec![
                        ("name", get(d, "name").cloned().unwrap_or(Value::Nil)),
                        ("text", get(d, "name").cloned().unwrap_or(Value::Nil)),
                    ]));
                }
            }
            classes.push(record(vec![
                ("id", cid.clone()),
                ("ns", Value::String(ns.clone())),
                ("name", Value::String(scan::module_name(cid))),
                ("lang", Value::keyword("lua")),
                ("file", Value::String(scan::relative_path(&f.path, cwd))),
                ("ops", Value::Vector(ops)),
            ]));
            let edge = |edges: &mut Vec<Value>, to: Value, kind: &str| {
                if *cid != to {
                    let e = record(vec![
                        ("from", cid.clone()),
                        ("to", to),
                        ("kind", Value::keyword(kind)),
                    ]);
                    if !edges.contains(&e) {
                        edges.push(e);
                    }
                }
            };
            for n in seq(get(&surface, "bases")) {
                let name = match n {
                    Value::String(s) => s.clone(),
                    _ => continue,
                };
                if let Some(target) = resolve(&name) {
                    edge(&mut edges, target, "inheritance");
                }
            }
            for n in seq(get(&surface, "requires")) {
                let name = match n {
                    Value::String(s) => s.clone(),
                    _ => continue,
                };
                let target = match resolve(&name) {
                    Some(target) => target,
                    None => {
                        let foreign = Value::keyword(&name);
                        if !foreigns.contains(&foreign) {
                            foreigns.push(foreign.clone());
                        }
                        foreign
                    }
                };
                edge(&mut edges, target, "dependency");
            }
        }
        for cid in foreigns {
            classes.push(scan::foreign_class(&cid));
        }
        Ok(record(vec![
            ("classes", Value::Vector(classes)),
            ("edges", Value::Vector(edges)),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names `scan` turns into `:ops`: neither local nor nested.
    fn ops(source: &str) -> Vec<String> {
        let surface = read_module(source.as_bytes());
        seq(get(&surface, "defns"))
            .iter()
            .filter(|d| !flag(d, "local") && !flag(d, "nested"))
            .filter_map(|d| match get(d, "name") {
                Some(Value::String(n)) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn functions_in_a_table_constructor_are_not_operations() {
        let source = "local M = {}\nM.t = { f = function() end }\nfunction M.a() end\nreturn M\n";
        assert_eq!(vec!["M.a"], ops(source));
    }

    #[test]
    fn functions_in_top_level_blocks_are_operations_like_crapper() {
        let source = "local M = {}\n\
                      if jit then\n  function M.fast() end\nelse\n  M.slow = function() end\nend\n\
                      do function M.d() end end\n\
                      function M.outer()\n  function M.inner() end\nend\n\
                      return M\n";
        assert_eq!(vec!["M.fast", "M.slow", "M.d", "M.outer"], ops(source));
    }

    #[test]
    fn a_warning_names_each_dynamic_require_by_file_and_line() {
        assert_eq!(None, dynamic_require_warning("a.lua", &[]));
        assert_eq!(
            Some("Unresolved dynamic require: a.lua:3, a.lua:9".to_owned()),
            dynamic_require_warning("a.lua", &[Value::Integer(3), Value::Integer(9)])
        );
    }

    fn strings(values: &[Value]) -> Vec<&str> {
        values
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_require_over_a_literal_module_list_resolves_each_module() {
        let source = "local function load()\n\
                      \x20 local mods = {\n    \"a.b\",\n    'c', -- note\n  }\n\
                      \x20 for _, m in ipairs(mods) do\n    reg(require(m))\n  end\n\
                      \x20 for _, n in pairs({ \"d\"; \"e.f\" }) do require(n) end\n\
                      end\n";
        let surface = read_module(source.as_bytes());
        assert_eq!(
            vec!["a.b", "c", "d", "e.f"],
            strings(seq(get(&surface, "requires")))
        );
        assert!(seq(get(&surface, "dynamic-requires")).is_empty());
    }

    #[test]
    fn a_require_outside_a_literal_list_loop_stays_dynamic() {
        let source = "local mods = { \"a\", x }\n\
                      for _, m in ipairs(mods) do require(m) end\n\
                      local ok = { \"b\" }\n\
                      for _, m in ipairs(ok) do end\n\
                      require(m)\n\
                      for i, m in ipairs(ok) do require(i) end\n\
                      for _, m in ipairs(other) do require(m) end\n";
        let surface = read_module(source.as_bytes());
        assert!(seq(get(&surface, "requires")).is_empty());
        assert_eq!(
            &[
                Value::Integer(2),
                Value::Integer(5),
                Value::Integer(6),
                Value::Integer(7)
            ][..],
            seq(get(&surface, "dynamic-requires"))
        );
    }

    fn requires(source: &str) -> Vec<String> {
        let surface = read_module(source.as_bytes());
        strings(seq(get(&surface, "requires")))
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_slash_or_hyphen_module_name_is_the_dotted_module() {
        let source = "local lume = require \"lib/lume\"\n\
                      local menu = require('src/states/menu')\n\
                      local state = require(\"game-state\")\n\
                      -- require(\"lib/no\")\n\
                      local s = \"require('lib/no')\"\n";
        assert_eq!(
            vec!["lib.lume", "src.states.menu", "game-state"],
            requires(source)
        );
    }

    #[test]
    fn a_protected_require_names_its_module() {
        let source = "local ok, jit = pcall(require, \"jit\")\n\
                      local has, m = pcall ( require , 'a.b' )\n\
                      local ok2, n = pcall(require, name)\n\
                      register(require, \"no\")\n\
                      -- pcall(require, \"no\")\n\
                      local s = 'pcall(require, \"no\")'\n\
                      for _, p in ipairs({ \"plug.a\" }) do pcall(require, p) end\n";
        let surface = read_module(source.as_bytes());
        assert_eq!(vec!["jit", "a.b", "plug.a"], requires(source));
        assert_eq!(
            &[Value::Integer(3)][..],
            seq(get(&surface, "dynamic-requires"))
        );
    }

    #[test]
    fn a_require_argument_may_start_on_the_next_line() {
        let source = "local a = require(\n  \"a.b\"\n)\n\
                      local c = require\n  'c'\n\
                      local d = require(\n  -- the module\n  \"d\")\n\
                      local r = require\n\
                      return r\n";
        assert_eq!(vec!["a.b", "c", "d"], requires(source));
    }

    /// The `(from, to)` dependency edges of a scan over in-memory files.
    fn dependencies(root: &str, cwd: &str, files: &[(&str, &str)]) -> Vec<(String, String)> {
        let files: Vec<SourceFile> = files
            .iter()
            .map(|(path, source)| SourceFile {
                path: format!("{cwd}/{path}"),
                source: source.as_bytes().to_vec(),
            })
            .collect();
        let opts = edn::parse("{:prefix \"game\"}").unwrap();
        let graph = SCANNER.scan(root, &opts, &files, cwd).unwrap();
        seq(get(&graph, "edges"))
            .iter()
            .map(|e| {
                let end = |k| edn::record::name(get(e, k).unwrap());
                (end("from"), end("to"))
            })
            .collect()
    }

    fn pairs(edges: &[(&str, &str)]) -> Vec<(String, String)> {
        edges
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn a_love_require_names_the_module_from_the_game_root() {
        let files = [
            (
                "main.lua",
                "local Player = require(\"src.entities.player\")\n\
                 local menu = require \"src/states/menu\"\n\
                 local lume = require(\"lib.lume\")\n",
            ),
            ("lib/lume.lua", "return {}\n"),
            ("src/entities/player.lua", "return {}\n"),
            (
                "src/states/menu.lua",
                "local Player = require(\"src.entities.player\")\n",
            ),
        ];
        assert_eq!(
            pairs(&[
                ("main", "entities.player"),
                ("main", "states.menu"),
                ("main", "lib.lume"),
                ("states.menu", "entities.player"),
            ]),
            dependencies("/g", "/g", &files)
        );
        assert_eq!(
            pairs(&[("states.menu", "entities.player")]),
            dependencies("/g/src", "/g", &files[2..])
        );
    }

    #[test]
    fn a_require_of_an_init_module_names_its_package() {
        let files = [
            ("app/main.lua", "local hump = require(\"hump.init\")\n"),
            ("app/hump/init.lua", "return {}\n"),
        ];
        assert_eq!(
            pairs(&[("main", "hump")]),
            dependencies("/g/app", "/g", &files)
        );
    }

    fn bases(source: &str) -> Vec<String> {
        let surface = read_module(source.as_bytes());
        strings(seq(get(&surface, "bases")))
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_class_extended_from_a_required_class_names_its_base() {
        let source = "local Object = require(\"lib.classic\")\n\
                      local Entity = require \"src.entities.entity\"\n\
                      local Other = require(\"other\")\n\
                      local Player = Entity:extend()\n\
                      local Boss = Object : extend \"Boss\"\n\
                      local helper = Other:extend_with(1)\n\
                      local s = \"Other:extend()\"\n\
                      -- Other:extend()\n\
                      function Player:spawn() return Other:extend() end\n\
                      return Player\n";
        assert_eq!(vec!["src.entities.entity", "lib.classic"], bases(source));
    }

    #[test]
    fn a_middleclass_subclass_names_its_base() {
        let source = "local class = require(\"lib.middleclass\")\n\
                      local Fruit = require(\"fruit\")\n\
                      local Bird = require(\"bird\")\n\
                      local Other = require(\"other\")\n\
                      local Lemon = class(\"Lemon\", Fruit)\n\
                      local Duck = Bird:subclass('Duck')\n\
                      local x = print(\"x\", Other)\n\
                      local Plain = class(\"Plain\")\n";
        assert_eq!(vec!["fruit", "bird"], bases(source));
    }

    #[test]
    fn a_metatable_over_a_populated_table_names_its_base() {
        let source = "local Entity = require(\"entity\")\n\
                      local Mixin = require(\"mixin\")\n\
                      local Other = require(\"other\")\n\
                      local Player = setmetatable({ speed = 10, tag = \"}\" }, { __index = Entity })\n\
                      local Mob = setmetatable({\n  hp = { max = 3 },\n}, Mixin)\n\
                      local T = setmetatable({ Other }, nil)\n";
        assert_eq!(vec!["entity", "mixin"], bases(source));
    }

    #[test]
    fn a_metatable_index_among_other_fields_names_its_base() {
        let source = "local Base = require(\"base\")\n\
                      local Mixin = require(\"mixin\")\n\
                      local Other = require(\"other\")\n\
                      local D = {}\n\
                      setmetatable(D, { __index = Base, __call = function(cls, ...) return cls.new(...) end })\n\
                      local E = setmetatable({}, {\n\
                      \x20 __tostring = function() return \"}\" end,\n\
                      \x20 __index = Mixin,\n\
                      })\n\
                      local F = setmetatable({}, { __index = function(t, k) return Other[k] end })\n\
                      local G = setmetatable({}, { __newindex = Other, name = \"x, __index = Other\" })\n\
                      local H = setmetatable({}, { meta = { __index = Other } })\n";
        assert_eq!(vec!["base", "mixin"], bases(source));
    }

    #[test]
    fn a_require_of_a_concatenated_name_is_reported_dynamic() {
        let source = "local function load(name)\n  return require(\"states.\" .. name)\nend\n\
                      local c = require('conf' .. suffix)\n\
                      local v = require \"version\" .. \"-dev\"\n\
                      local m = require(\"states.menu\"):new()\n\
                      local B = require(\"base\" .. variant)\n\
                      local D = B:extend()\n";
        let surface = read_module(source.as_bytes());
        assert_eq!(vec!["version", "states.menu"], requires(source));
        assert_eq!(
            &[Value::Integer(2), Value::Integer(4), Value::Integer(7)][..],
            seq(get(&surface, "dynamic-requires"))
        );
        assert!(bases(source).is_empty());
    }

    #[test]
    fn a_require_relative_to_the_vararg_is_reported_dynamic() {
        let source = "local x = require(... .. \".x\")\n\
                      local y = require((...):gsub(\"%.init$\", \"\") .. \".y\")\n\
                      local z = require()\n";
        let surface = read_module(source.as_bytes());
        assert!(seq(get(&surface, "requires")).is_empty());
        assert_eq!(
            &[Value::Integer(1), Value::Integer(2)][..],
            seq(get(&surface, "dynamic-requires"))
        );
    }

    #[test]
    fn dynamic_requires_are_reported_with_their_lines() {
        let source = "local a = require \"x.y\"\n\
                      local name = 'z'\n\
                      local b = require(name)\n\
                      local c = pkg.require(other)\n\
                      -- require(nope)\n\
                      local d = require([[long]])\n";
        let surface = read_module(source.as_bytes());
        assert_eq!(
            &[Value::String("x.y".into())][..],
            seq(get(&surface, "requires"))
        );
        assert_eq!(
            &[Value::Integer(3)][..],
            seq(get(&surface, "dynamic-requires"))
        );
    }
}
