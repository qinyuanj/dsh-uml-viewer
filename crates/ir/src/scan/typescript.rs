//! Port of `love/uml/typescript_language/graph_typescript.lua`: a regex/string
//! scanner over caller-owned paths. Scanner limitations are preserved.

use std::collections::{HashMap, HashSet};

use edn::Value;

use crate::scan::{self, Scanner, SourceFile};
use crate::text::{self, at, is_space, is_ts_ident};
use crate::value::get_truthy as get;
use edn::record::{flag, record, seq};

fn ident(c: Option<u8>) -> bool {
    c.is_some_and(is_ts_ident)
}

fn is_id_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}

/// Captures `ID` = `[A-Za-z_$][%w_$]*` at the start of `s`.
fn capture_id(s: &[u8]) -> Option<usize> {
    if !s.first().is_some_and(|&c| is_id_start(c)) {
        return None;
    }
    Some(s.iter().take_while(|&&c| is_ts_ident(c)).count())
}

fn strip_spaces(s: &[u8]) -> &[u8] {
    &s[s.iter().take_while(|&&c| is_space(c)).count()..]
}

fn trim(s: &[u8]) -> &[u8] {
    text::trim(s)
}

/// `part:match('%f[%w_$]as%s+(ID)%s*$')`: a trailing `as name`.
fn match_as_name(part: &[u8]) -> Option<Vec<u8>> {
    // Frontier before 'as': walk candidate splits from the left.
    let mut search = 0;
    while let Some(off) = find_subslice(&part[search.min(part.len())..], b"as") {
        let p = search + off;
        let before_ok = p == 0 || !is_ts_ident(part[p - 1]);
        let after = &part[p + 2..];
        let spaces = after.iter().take_while(|&&c| is_space(c)).count();
        if before_ok && spaces > 0 {
            let rest = &after[spaces..];
            if let Some(len) = capture_id(rest)
                && trim(&rest[len..]).is_empty()
            {
                return Some(rest[..len].to_vec());
            }
        }
        search = p + 2;
    }
    None
}

/// `brace_names(stmt)`: names inside the first `{...}`, comma-separated.
fn brace_names(stmt: &[u8]) -> Vec<Value> {
    let mut names = Vec::new();
    let body: &[u8] = match stmt.iter().position(|&c| c == b'{') {
        Some(open) => {
            let rest = &stmt[open + 1..];
            let close = rest.iter().position(|&c| c == b'}').unwrap_or(rest.len());
            &rest[..close]
        }
        None => b"",
    };
    for part in body.split(|&c| c == b',') {
        let part = trim(part);
        if part.is_empty() {
            continue; // gmatch('[^,]+') yields no empty parts
        }
        let part = match part.strip_prefix(b"type") {
            Some(rest) if rest.first().is_some_and(|&c| is_space(c)) => strip_spaces(rest),
            _ => part,
        };
        let n = match_as_name(part).or_else(|| capture_id(part).map(|len| part[..len].to_vec()));
        if let Some(n) = n {
            names.push(Value::String(text::text(&n)));
        }
    }
    names
}

const REGEX_KEYWORDS: &[&[u8]] = &[
    b"return", b"throw", b"case", b"void", b"typeof", b"delete", b"await", b"yield", b"else",
    b"do", b"in", b"of",
];

/// The last non-space position at or before 1-based `j`; 0 when none.
fn skip_space_back(s: &[u8], mut j: i64) -> i64 {
    while j > 0 && at(s, j as usize).is_some_and(is_space) {
        j -= 1;
    }
    j
}

/// `regex_end(s, i)`: whether the `/` at 1-based `i` opens a regex literal;
/// returns one past the closing flags.
fn regex_end(s: &[u8], i: usize) -> Option<usize> {
    let mut j = skip_space_back(s, i as i64 - 1);
    let mut c = at(s, j.max(0) as usize);
    // `++` or `--` is postfix after an operand, so the `/` divides; any
    // other is prefix and may precede a regex.
    if j > 1 && matches!(c, Some(b'+' | b'-')) && at(s, j as usize - 1) == c {
        let operand = skip_space_back(s, j - 2);
        if text::sub(s, operand + 1, j - 2)
            .iter()
            .any(|&c| c == b'\n' || c == b'\r')
        {
            // A line terminator before the update makes it prefix (ASI).
            j = 0;
        } else {
            j = operand;
        }
        c = at(s, j.max(0) as usize);
    }
    if j > 0 && (ident(c) || c == Some(b')') || c == Some(b']')) {
        let mut k = j;
        while k > 0 && ident(at(s, k as usize)) {
            k -= 1;
        }
        let word = text::sub(s, k + 1, j);
        if !REGEX_KEYWORDS.contains(&word) {
            return None;
        }
    }
    let mut inside = false;
    let mut j = i + 1;
    while j <= s.len() {
        let c = at(s, j).unwrap();
        if c == b'\\' {
            j += 2;
        } else if c == b'\n' {
            return None;
        } else if c == b'/' && !inside {
            j += 1;
            while at(s, j).is_some_and(|c| c.is_ascii_alphabetic()) {
                j += 1;
            }
            return Some(j);
        } else {
            if c == b'[' && !inside {
                inside = true;
            } else if c == b']' && inside {
                inside = false;
            }
            j += 1;
        }
    }
    None
}

fn in_hole(holes: &[(usize, usize)], i: usize) -> bool {
    holes.iter().any(|&(a, b)| i >= a && i < b)
}

/// `quoted_end(s, i, quote)`: one past the `'` or `"` string opening at
/// 1-based `i`. The string ends with its line, as in JavaScript, so the
/// apostrophe of JSX text (`<p>Don't</p>`) opens no string past it; a `\`
/// before LF, CR or CR LF continues it onto the next line.
fn quoted_end(s: &[u8], i: usize, quote: u8) -> usize {
    let mut j = i + 1;
    while let Some(c) = at(s, j) {
        if c == b'\\' {
            let crlf = at(s, j + 1) == Some(b'\r') && at(s, j + 2) == Some(b'\n');
            j += if crlf { 3 } else { 2 };
        } else if c == quote {
            return j + 1;
        } else if c == b'\n' || c == b'\r' {
            return j;
        } else {
            j += 1;
        }
    }
    s.len() + 1
}

/// `template_end(s, i)`: one past the template text that the `` ` `` or the
/// interpolation-closing `}` at 1-based `i` starts, and whether a `${`
/// (rather than the closing `` ` ``) ends it.
fn template_end(s: &[u8], i: usize) -> (usize, bool) {
    let mut j = i + 1;
    while let Some(c) = at(s, j) {
        if c == b'\\' {
            j += 2;
        } else if c == b'`' {
            return (j + 1, false);
        } else if c == b'$' && at(s, j + 1) == Some(b'{') {
            return (j + 2, true);
        } else {
            j += 1;
        }
    }
    (s.len() + 1, false)
}

/// `mask(s)`: comments blanked; regexes, quoted strings and template text
/// kept as holes. A template's `${...}` is code, up to the `}` that closes
/// it at its own brace depth.
fn mask(s: &[u8]) -> (Vec<u8>, Vec<(usize, usize)>) {
    let mut out = Vec::new();
    let mut holes = Vec::new();
    // The brace depth of the code, and that of each open `${`.
    let (mut depth, mut interpolations) = (0usize, Vec::new());
    let mut i = 1;
    while i <= s.len() {
        let c = at(s, i);
        let nxt = at(s, i + 1);
        if text::line_comment(c, nxt) {
            i = text::mask_line_comment(s, &mut out, i, None);
            continue;
        }
        if text::block_comment(c, nxt) {
            i = text::mask_block_comment(s, &mut out, i, None);
            continue;
        }
        let mut finish = None;
        if c == Some(b'/') && nxt.is_some() && nxt != Some(b'*') {
            finish = regex_end(s, i);
        } else if matches!(c, Some(b'"') | Some(b'\'')) {
            finish = Some(quoted_end(s, i, c.unwrap()));
        } else if c == Some(b'`') || (c == Some(b'}') && interpolations.last() == Some(&depth)) {
            if c == Some(b'}') {
                interpolations.pop();
            }
            let (end, interpolation) = template_end(s, i);
            if interpolation {
                interpolations.push(depth);
            }
            finish = Some(end);
        }
        if let Some(finish) = finish {
            out.extend_from_slice(&s[i - 1..finish - 1]);
            holes.push((i, finish));
            i = finish;
        } else {
            match c {
                Some(b'{') => depth += 1,
                Some(b'}') => depth = depth.saturating_sub(1),
                _ => {}
            }
            out.push(c.unwrap());
            i += 1;
        }
    }
    (out, holes)
}

/// `statement_end(s, start, header)`: inclusive end position. A header
/// closes its braces too: a destructured parameter `({ a }: T)` must not
/// keep the body's `{` from ending it.
fn statement_end(s: &[u8], start: usize, header: bool) -> usize {
    let (mut paren, mut brace, mut bracket) = (0i64, 0i64, 0i64);
    let mut i = start;
    while i <= s.len() {
        let c = at(s, i).unwrap();
        if c == b'(' {
            paren += 1;
        } else if c == b')' {
            paren = (paren - 1).max(0);
        } else if c == b'[' {
            bracket += 1;
        } else if c == b']' {
            bracket = (bracket - 1).max(0);
        } else if c == b'{' {
            if header && paren == 0 && bracket == 0 && brace == 0 {
                return i - 1;
            }
            brace += 1;
        } else if c == b'}' {
            brace = (brace - 1).max(0);
        } else if paren == 0 && bracket == 0 && brace == 0 {
            if header && (c == b'=' || c == b';') {
                return i - 1;
            } else if c == b';' {
                return i;
            }
        }
        i += 1;
    }
    s.len()
}

/// The `["']([^"']+)["']` tail of a specifier: the first quote of either kind
/// closes the capture, and the capture holds neither kind.
fn spec_capture(rest: &[u8]) -> Option<Vec<u8>> {
    let end = rest.iter().position(|&c| c == b'"' || c == b'\'')?;
    if end == 0 {
        return None;
    }
    Some(rest[..end].to_vec())
}

/// `from_spec(stmt)`: `from%s*["']([^"']+)["']` anywhere.
fn from_spec(stmt: &[u8]) -> Option<Vec<u8>> {
    let mut search = 0;
    while let Some(off) = find_subslice(&stmt[search.min(stmt.len())..], b"from") {
        let p = search + off + 4;
        let after = strip_spaces(&stmt[p..]);
        if let Some(&quote) = after.first()
            && (quote == b'"' || quote == b'\'')
            && let Some(spec) = spec_capture(&after[1..])
        {
            return Some(spec);
        }
        search = search + off + 1;
    }
    None
}

/// `return_type(header)`: the `: Name` after the parameter list.
fn return_type(header: &[u8]) -> Option<Vec<u8>> {
    let open = header.iter().position(|&c| c == b'(')?;
    let mut depth = 1;
    for i in open + 1..header.len() {
        let c = header[i];
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                let rest = strip_spaces(&header[i + 1..]);
                let rest = rest.strip_prefix(b":")?;
                let rest = strip_spaces(rest);
                let len = capture_id(rest)?;
                if trim(&rest[len..]).is_empty() {
                    return Some(rest[..len].to_vec());
                }
                return None;
            }
        }
    }
    None
}

/// `^export%s+(type%s+)?` then the `{...}` or `%*(%s+as%s+ID)?` clause, then
/// `%s*from%s*["']([^"']+)["']`: only the `from` that ends the clause. A
/// statement without `;` runs on, and `"loaded from './x'"` later is no
/// specifier.
fn export_spec(stmt: &[u8]) -> Option<Vec<u8>> {
    let rest = strip_prefix_word(stmt, b"export")?;
    let rest = strip_prefix_word(rest, b"type").unwrap_or(rest);
    let rest = if rest.first() == Some(&b'{') {
        let close = rest.iter().position(|&c| c == b'}')?;
        &rest[close + 1..]
    } else {
        let rest = rest.strip_prefix(b"*")?;
        match strip_prefix_word(strip_spaces(rest), b"as") {
            Some(name) => &name[capture_id(name)?..],
            None => rest,
        }
    };
    let rest = strip_spaces(strip_spaces(rest).strip_prefix(b"from")?);
    let quote = *rest.first()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    spec_capture(&rest[1..])
}

/// `^=%s*(async%s+)?%b()%s*:%s*(ID)%s*=>`: the declared return type of an
/// arrow function initializer.
fn arrow_return_type(init: &[u8]) -> Option<Vec<u8>> {
    let rest = strip_spaces(init.strip_prefix(b"=")?);
    let rest = strip_prefix_word(rest, b"async").unwrap_or(rest);
    if rest.first() != Some(&b'(') {
        return None;
    }
    return_type(&rest[..find_subslice(rest, b"=>")?])
}

/// The `extends` and `implements` names of a class or interface declaration,
/// read up to its body's `{`. Only names at depth 0 of `<>` and `()` count
/// (`<T extends B>` is a constraint, `mixin(A, B)` a call; `=>` closes
/// nothing), each by its first dotted segment (`ns.B` names the namespace
/// import `ns`). The header would end at a type parameter default's `=`.
fn heritage(stmt: &[u8]) -> (Vec<Value>, Vec<Value>) {
    let (mut extends, mut implements) = (Vec::new(), Vec::new());
    let mut clause: Option<&mut Vec<Value>> = None;
    let mut expect = false;
    let mut depth = 0i64;
    let mut i = 0;
    while i < stmt.len() {
        let c = stmt[i];
        if c == b'{' && depth == 0 {
            break;
        } else if c == b'<' || c == b'(' {
            depth += 1;
        } else if c == b')' || (c == b'>' && i > 0 && stmt[i - 1] != b'=') {
            depth = (depth - 1).max(0);
        } else if c == b',' && depth == 0 {
            expect = clause.is_some();
        } else if is_id_start(c) && (i == 0 || !is_ts_ident(stmt[i - 1])) {
            let len = capture_id(&stmt[i..]).unwrap();
            let word = &stmt[i..i + len];
            if depth == 0 && word == b"extends" {
                (clause, expect) = (Some(&mut extends), true);
            } else if depth == 0 && word == b"implements" {
                (clause, expect) = (Some(&mut implements), true);
            } else if depth == 0
                && expect
                && let Some(names) = clause.as_deref_mut()
            {
                names.push(Value::String(text::text(word)));
                expect = false;
            }
            i += len;
            continue;
        }
        i += 1;
    }
    (extends, implements)
}

/// An import ends at its first string outside `{...}`, the specifier:
/// without `;` the statement would run on to the next line's `from "..."`.
/// A string inside the braces is a name (`{ "kebab-name" as k }`).
fn through_first_string(stmt: &[u8]) -> &[u8] {
    let mut brace = 0i64;
    let Some(open) = stmt.iter().position(|&c| {
        if c == b'{' {
            brace += 1;
        } else if c == b'}' {
            brace = (brace - 1).max(0);
        }
        brace == 0 && (c == b'"' || c == b'\'')
    }) else {
        return stmt;
    };
    match stmt[open + 1..].iter().position(|&c| c == stmt[open]) {
        Some(close) => &stmt[..open + close + 2],
        None => stmt,
    }
}

fn starts_with_word(s: &[u8], word: &[u8]) -> bool {
    s.starts_with(word) && !s.get(word.len()).is_some_and(|&c| is_ts_ident(c))
}

/// `export_fact(stmt, header)`. The `declare` and `abstract` modifiers and
/// the `const` of `const enum` leave the declaration visible; a class or an
/// interface also records its heritage names.
fn export_fact(stmt: &[u8], header: &[u8]) -> Option<Value> {
    let spec = || {
        export_spec(stmt)
            .map(|s| Value::String(text::text(&s)))
            .unwrap_or(Value::Nil)
    };
    // `^export%s+type%s*{` or `^export%s+(type%s+)?%*`
    if match_export_type_star(stmt) {
        return Some(record(vec![("spec", spec())]));
    }
    if match_prefix_brace(stmt, b"export") {
        return Some(record(vec![
            ("spec", spec()),
            ("ops", Value::Vector(brace_names(stmt))),
        ]));
    }
    // decl = stmt minus `^export%s+` minus `^declare%s+`; tail = decl minus
    // `^default%s+` minus `^abstract%s+`, and minus the `const` of `const enum`
    let decl = strip_prefix_word(stmt, b"export").unwrap_or(stmt);
    let decl = strip_prefix_word(decl, b"declare").unwrap_or(decl);
    let mut tail = strip_prefix_word(decl, b"default").unwrap_or(decl);
    tail = strip_prefix_word(tail, b"abstract").unwrap_or(tail);
    if let Some(rest) = strip_prefix_word(tail, b"const")
        && starts_with_word(rest, b"enum")
    {
        tail = rest;
    }
    if let Some(rest) = strip_prefix_word(tail, b"async") {
        if !starts_with_word(rest, b"function") {
            return None;
        }
        tail = rest;
    }
    let mut n: Option<Vec<u8>> = None;
    let mut typ: Option<Vec<u8>> = None;
    let (mut extends, mut implements) = (Vec::new(), Vec::new());
    if starts_with_word(tail, b"function") {
        // `^function%s*%*?%s*(ID)`: `function*` needs no space.
        let rest = strip_spaces(&tail[8..]);
        let rest = rest.strip_prefix(b"*").map(strip_spaces).unwrap_or(rest);
        n = capture_id(rest).map(|len| rest[..len].to_vec());
        typ = return_type(header);
    } else if starts_with_word(tail, b"class") {
        let rest = strip_spaces(&tail[5..]);
        if tail[5..].iter().take_while(|&&c| is_space(c)).count() > 0 {
            // `class extends Base` is anonymous.
            n = capture_id(rest)
                .map(|len| rest[..len].to_vec())
                .filter(|n| n != b"extends" && n != b"implements");
        }
        (extends, implements) = heritage(stmt);
    } else if starts_with_word(tail, b"interface") || match_type_alias(tail) {
        let mut entries = vec![("interface", Value::Bool(true))];
        if starts_with_word(tail, b"interface") {
            let (extends, _) = heritage(stmt);
            if !extends.is_empty() {
                entries.push(("extends", Value::Vector(extends)));
            }
        }
        return Some(record(entries));
    } else if starts_with_word(tail, b"enum") {
        let rest = strip_spaces(&tail[4..]);
        if tail[4..].iter().take_while(|&&c| is_space(c)).count() > 0 {
            n = capture_id(rest).map(|len| rest[..len].to_vec());
        }
    } else if starts_with_word(decl, b"const")
        || starts_with_word(decl, b"let")
        || starts_with_word(decl, b"var")
    {
        // `^%a+%s+(ID)` on the tail
        let letters = tail
            .iter()
            .take_while(|&&c| c.is_ascii_alphabetic())
            .count();
        let rest = &tail[letters..];
        let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
        if letters > 0 && spaces > 0 {
            let rest = &rest[spaces..];
            n = capture_id(rest).map(|len| rest[..len].to_vec());
        }
        typ = ["const", "let", "var"]
            .iter()
            .find_map(|k| match_const_typed(header, k.as_bytes()))
            .or_else(|| arrow_return_type(stmt.get(header.len()..)?));
    }
    if n.is_none() && extends.is_empty() && implements.is_empty() {
        return None;
    }
    let mut entries = Vec::new();
    if let Some(n) = n {
        entries.push(("ops", Value::Vector(vec![Value::String(text::text(&n))])));
    }
    entries.push(("value", Value::Bool(true)));
    if let Some(typ) = typ {
        entries.push(("type", Value::String(text::text(&typ))));
    }
    for (key, names) in [("extends", extends), ("implements", implements)] {
        if !names.is_empty() {
            entries.push((key, Value::Vector(names)));
        }
    }
    Some(record(entries))
}

/// `^type%s+ID`: a type alias; `type {` and `type *` are re-exports.
fn match_type_alias(tail: &[u8]) -> bool {
    strip_prefix_word(tail, b"type").is_some_and(|rest| capture_id(rest).is_some())
}

/// `^export%s+type%s*{` or `^export%s+(type%s+)?%*`.
fn match_export_type_star(stmt: &[u8]) -> bool {
    let Some(rest) = strip_prefix_word(stmt, b"export") else {
        return false;
    };
    if rest.first() == Some(&b'*') {
        return true; // `%s*%*`: the strip consumed the required spaces
    }
    let Some(rest) = strip_prefix_word(rest, b"type") else {
        return false;
    };
    // `type%s*{` or `type%s+%*` — strip_prefix_word required spaces already.
    let rest = strip_spaces(rest);
    matches!(rest.first(), Some(b'{') | Some(b'*'))
}

/// `^export%s+{`: export, spaces, then `{`.
fn match_prefix_brace(stmt: &[u8], word: &[u8]) -> bool {
    match strip_prefix_word(stmt, word) {
        Some(rest) => strip_spaces(rest).first() == Some(&b'{'),
        None => false,
    }
}

/// `^word%s+` — strips the word and the following whitespace run.
fn strip_prefix_word<'a>(s: &'a [u8], word: &[u8]) -> Option<&'a [u8]> {
    let rest = s.strip_prefix(word)?;
    let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    Some(&rest[spaces..])
}

/// `%f[%w_$]<kw>%s+ID%s*:%s*(ID)%s*$` on the header.
fn match_const_typed(header: &[u8], keyword: &[u8]) -> Option<Vec<u8>> {
    let mut search = 0;
    while let Some(off) = find_subslice(&header[search.min(header.len())..], keyword) {
        let p = search + off;
        let before_ok = p == 0 || !is_ts_ident(header[p - 1]);
        let rest = &header[p + keyword.len()..];
        let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
        if before_ok && spaces > 0 {
            let rest = &rest[spaces..];
            if let Some(len) = capture_id(rest) {
                let rest = strip_spaces(&rest[len..]);
                if let Some(rest) = rest.strip_prefix(b":") {
                    let rest = strip_spaces(rest);
                    if let Some(tlen) = capture_id(rest)
                        && trim(&rest[tlen..]).is_empty()
                    {
                        return Some(rest[..tlen].to_vec());
                    }
                }
            }
        }
        search = p + keyword.len();
    }
    None
}

fn find_subslice(s: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || s.len() < needle.len() {
        return None;
    }
    s.windows(needle.len()).position(|w| w == needle)
}

/// `^%s*word%f[^%w_$]` anchored at 1-based `start`; returns (a, b) inclusive.
fn find_anchored(text: &[u8], word: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut p = start;
    while at(text, p).is_some_and(is_space) {
        p += 1;
    }
    if text::sub(text, p as i64, p as i64 + word.len() as i64 - 1) != word {
        return None;
    }
    if at(text, p + word.len()).is_some_and(is_ts_ident) {
        return None;
    }
    Some((start, p + word.len() - 1))
}

/// `read_module(source)`: imports, exports and `invoke("...")` commands.
pub fn read_module(source: &[u8]) -> Value {
    let (text, holes) = mask(source);
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut invokes: Vec<Value> = Vec::new();
    for word in ["import", "export"] {
        let mut start = 1usize;
        while start <= text.len() {
            if let Some((a, b)) = find_anchored(&text, word.as_bytes(), start) {
                if !in_hole(&holes, a) {
                    let at_pos = text::find_plain(&text, word.as_bytes(), a).unwrap();
                    let stmt = text::sub(
                        &text,
                        at_pos as i64,
                        statement_end(&text, at_pos, false) as i64,
                    );
                    let header = text::sub(
                        &text,
                        at_pos as i64,
                        statement_end(&text, at_pos, true) as i64,
                    );
                    if word == "import" {
                        let stmt = through_first_string(stmt);
                        let spec = from_spec(stmt).or_else(|| match_bare_import(stmt));
                        if let Some(spec) = spec {
                            let mut names = brace_names(stmt);
                            let tail = strip_prefix_word(stmt, b"import")
                                .map(|t| strip_prefix_word(t, b"type").unwrap_or(t))
                                .unwrap_or(stmt);
                            if let Some(len) = capture_id(tail) {
                                let n = &tail[..len];
                                let rest = &tail[len..];
                                let rest = strip_spaces(rest);
                                let comma = rest.first() == Some(&b',');
                                let from_after = {
                                    // `^ID%s+from%f[^%w_$]` on the whole tail
                                    let after = strip_spaces(&tail[len..]);
                                    after.starts_with(b"from")
                                        && !after.get(4).is_some_and(|&c| is_ts_ident(c))
                                        && tail[len..].iter().take_while(|&&c| is_space(c)).count()
                                            > 0
                                };
                                if n != b"type" && (comma || from_after) {
                                    names.push(Value::String(text::text(n)));
                                }
                            }
                            if let Some(n) = match_star_as(stmt) {
                                names.push(Value::String(text::text(&n)));
                            }
                            imports.push(record(vec![
                                ("spec", Value::String(text::text(&spec))),
                                ("names", Value::Vector(names)),
                            ]));
                        }
                    } else if let Some(fact) = export_fact(stmt, header) {
                        exports.push(fact);
                    }
                }
                start = b + 1;
            }
            match text::find_plain(&text, b"\n", start) {
                Some(newline) => start = newline + 1,
                None => break,
            }
        }
    }
    let mut i = 1;
    while let Some(at_pos) = text::find_plain(&text, b"invoke", i) {
        i = at_pos + 1;
        if ident(at(&text, at_pos.wrapping_sub(1)))
            || ident(at(&text, at_pos + 6))
            || in_hole(&holes, at_pos)
        {
            continue;
        }
        let mut j = at_pos + 6;
        while at(&text, j).is_some_and(is_space) {
            j += 1;
        }
        if at(&text, j) == Some(b'<') {
            let mut depth = 1;
            j += 1;
            while j <= text.len() && depth > 0 {
                match at(&text, j).unwrap() {
                    b'<' => depth += 1,
                    b'>' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
        }
        if at(&text, j) == Some(b'(') {
            j += 1;
            while at(&text, j).is_some_and(is_space) {
                j += 1;
            }
            let quote = at(&text, j);
            if quote == Some(b'"') || quote == Some(b'\'') {
                let quote = quote.unwrap();
                let mut stop = j + 1;
                while stop <= text.len() {
                    let c = at(&text, stop).unwrap();
                    if c == b'\\' {
                        stop += 2;
                    } else if c == quote {
                        break;
                    } else {
                        stop += 1;
                    }
                }
                let end = stop.min(text.len() + 1) - 1;
                let v = Value::String(text::text(text::sub(&text, (j + 1) as i64, end as i64)));
                if !invokes.contains(&v) {
                    invokes.push(v);
                }
            }
        }
    }
    record(vec![
        ("imports", Value::Vector(imports)),
        ("exports", Value::Vector(exports)),
        ("invokes", Value::Vector(invokes)),
        ("calls", Value::Vector(call_specs(&text, &holes))),
    ])
}

/// `import("...")` and `require("...")` with a literal specifier, in source
/// order; `import x = require("...")` is a require. Not a member call
/// (`loader.require`; a spread `...require` is a call), not a concatenation
/// (`"./a/" + b`) and not a template with a `${}`. Comments are blanked and
/// a call inside a string or regex starts in a hole.
fn call_specs(text: &[u8], holes: &[(usize, usize)]) -> Vec<Value> {
    let mut found: Vec<(usize, Value)> = Vec::new();
    for word in [&b"import"[..], b"require"] {
        let mut i = 1;
        while let Some(at_pos) = text::find_plain(text, word, i) {
            i = at_pos + 1;
            let before = at(text, at_pos.wrapping_sub(1));
            let member = before == Some(b'.') && at(text, at_pos.wrapping_sub(2)) != Some(b'.');
            if ident(before)
                || member
                || ident(at(text, at_pos + word.len()))
                || in_hole(holes, at_pos)
            {
                continue;
            }
            let mut j = at_pos + word.len();
            while at(text, j).is_some_and(is_space) {
                j += 1;
            }
            if at(text, j) != Some(b'(') {
                continue;
            }
            j += 1;
            while at(text, j).is_some_and(is_space) {
                j += 1;
            }
            let quote = at(text, j);
            if !matches!(quote, Some(b'"') | Some(b'\'') | Some(b'`')) {
                continue;
            }
            let Ok(hole) = holes.binary_search_by_key(&j, |&(a, _)| a) else {
                continue;
            };
            let finish = holes[hole].1;
            let spec = text::sub(text, (j + 1) as i64, finish as i64 - 2);
            if finish < j + 2
                || at(text, finish - 1) != quote
                || spec.is_empty()
                || (quote == Some(b'`') && find_subslice(spec, b"${").is_some())
            {
                continue;
            }
            let mut k = finish;
            while at(text, k).is_some_and(is_space) {
                k += 1;
            }
            if matches!(at(text, k), Some(b')') | Some(b',')) {
                found.push((at_pos, Value::String(text::text(spec))));
            }
        }
    }
    found.sort_by_key(|&(at_pos, _)| at_pos);
    let mut specs: Vec<Value> = Vec::new();
    for (_, spec) in found {
        if !specs.contains(&spec) {
            specs.push(spec);
        }
    }
    specs
}

/// `^import%s*["']([^"']+)["']` — a side-effect import.
fn match_bare_import(stmt: &[u8]) -> Option<Vec<u8>> {
    let rest = stmt.strip_prefix(b"import")?;
    let rest = strip_spaces(rest);
    let quote = *rest.first()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    spec_capture(&rest[1..])
}

/// `%*%s+as%s+(ID)` anywhere in the statement.
fn match_star_as(stmt: &[u8]) -> Option<Vec<u8>> {
    let mut search = 0;
    while let Some(off) = find_subslice(&stmt[search.min(stmt.len())..], b"*") {
        let p = search + off;
        let rest = strip_spaces(&stmt[p + 1..]);
        if let Some(rest) = rest.strip_prefix(b"as") {
            let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
            if spaces > 0 {
                let rest = &rest[spaces..];
                if let Some(len) = capture_id(rest) {
                    return Some(rest[..len].to_vec());
                }
            }
        }
        search = p + 1;
    }
    None
}

/// The 12 `%.[cm]?[jt]sx?$` suffixes, longest first.
const TS_EXTENSIONS: &[&str] = &[
    ".cjsx", ".ctsx", ".mjsx", ".mtsx", ".cjs", ".cts", ".mjs", ".mts", ".jsx", ".tsx", ".js",
    ".ts",
];

/// `accepts(relative)` — the full path is passed, like the Lua scanner.
fn accepts(path: &str) -> bool {
    if !TS_EXTENSIONS.iter().any(|ext| path.ends_with(ext)) {
        return false;
    }
    if [".d.ts", ".d.mts", ".d.cts"]
        .iter()
        .any(|declaration| path.ends_with(declaration))
    {
        return false;
    }
    // `%.test%.[cm]?[jt]sx?$` and `%.spec%.[cm]?[jt]sx?$`
    for marker in [".test", ".spec"] {
        for ext in TS_EXTENSIONS {
            if path.ends_with(&format!("{marker}{ext}")) {
                return false;
            }
        }
    }
    let slashed = format!("/{path}");
    !(slashed.contains("/node_modules/")
        || slashed.contains("/dist/")
        || slashed.contains("/target/"))
}

/// `normalize(path)`: resolve `.` and `..` segments.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        if p.is_empty() {
            continue;
        }
        if p == ".." {
            parts.pop();
        } else if p != "." {
            parts.push(p);
        }
    }
    let mut out = String::new();
    // A Windows UNC root (`//server/share`) keeps both of its slashes.
    if path.starts_with("//") {
        out.push('/');
    }
    if path.starts_with('/') {
        out.push('/');
    }
    out.push_str(&parts.join("/"));
    out
}

/// `module_relative(path, root)`.
fn module_relative(path: &str, root: &str) -> String {
    let rel = scan::relative_path(path, root);
    // `gsub('%.[cm]?[jt]sx?$','')`
    let rel = strip_ts_extension(&rel);
    let rel = if rel == "index" {
        String::new()
    } else {
        rel.strip_suffix("/index")
            .map(|s| format!("{s}/"))
            .unwrap_or(rel)
    };
    let rel = rel.replace("/.", ".");
    let rel = rel.strip_suffix('/').unwrap_or(&rel).to_owned();
    rel.replace('/', ".")
}

fn strip_ts_extension(rel: &str) -> String {
    for ext in TS_EXTENSIONS {
        if let Some(stripped) = rel.strip_suffix(ext) {
            return stripped.to_owned();
        }
    }
    rel.to_owned()
}

const EXTS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];

/// An ESM specifier names the emitted file: `./a.js` is the source `a.ts`.
const ESM_SOURCES: &[(&str, &[&str])] = &[
    (".js", &[".ts", ".tsx"]),
    (".jsx", &[".tsx"]),
    (".mjs", &[".mts"]),
    (".cjs", &[".cts"]),
];

/// `metadata_paths(root)`: the tsconfig (or jsconfig) of the source root,
/// then of its parent.
fn metadata_paths(root: &str) -> Vec<String> {
    let mut dirs = vec![root];
    if let Some((parent, _)) = root.rsplit_once('/') {
        dirs.push(parent);
    }
    dirs.iter()
        .flat_map(|dir| {
            [
                format!("{dir}/tsconfig.json"),
                format!("{dir}/jsconfig.json"),
            ]
        })
        .collect()
}

/// `compilerOptions.paths` and `baseUrl` of the project tsconfig, made
/// absolute: `paths` substitutions sit under `baseUrl` when it is set, else
/// beside the tsconfig.
#[derive(Default)]
struct Aliases {
    base: Option<String>,
    paths: Vec<(String, Vec<String>)>,
}

impl Aliases {
    fn read(path: &str, source: &[u8]) -> Option<Aliases> {
        let options = jsonc(source)?
            .get(&Value::String("compilerOptions".into()))?
            .clone();
        let option = |key: &str| options.get(&Value::String(key.into()));
        let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let base = match option("baseUrl") {
            Some(Value::String(url)) => Some(normalize(&format!("{dir}/{url}"))),
            _ => None,
        };
        let under = base.clone().unwrap_or_else(|| dir.to_owned());
        let mut paths = Vec::new();
        if let Some(Value::Map(entries)) = option("paths") {
            for (key, subs) in entries {
                if let Value::String(key) = key {
                    let subs = seq(Some(subs))
                        .iter()
                        .filter_map(|s| match s {
                            Value::String(s) => Some(format!("{under}/{s}")),
                            _ => None,
                        })
                        .collect();
                    paths.push((key.clone(), subs));
                }
            }
        }
        Some(Aliases { base, paths })
    }

    /// The paths a bare specifier may name, in TypeScript's order: the exact
    /// `paths` key, else the `*` key with the longest prefix, each
    /// substitution in turn; then the specifier under `baseUrl`.
    fn candidates(&self, spec: &str) -> Vec<String> {
        let exact = self
            .paths
            .iter()
            .find(|(key, _)| key == spec)
            .map(|(_, subs)| (subs, ""));
        let wildcard = || {
            self.paths
                .iter()
                .filter_map(|(key, subs)| {
                    let (prefix, suffix) = key.split_once('*')?;
                    let star = spec.strip_prefix(prefix)?.strip_suffix(suffix)?;
                    Some((prefix.len(), subs, star))
                })
                .max_by_key(|&(prefix, ..)| prefix)
                .map(|(_, subs, star)| (subs, star))
        };
        let mut out: Vec<String> = match exact.or_else(wildcard) {
            Some((subs, star)) => subs
                .iter()
                .map(|s| normalize(&s.replacen('*', star, 1)))
                .collect(),
            None => Vec::new(),
        };
        if let Some(base) = &self.base {
            out.push(normalize(&format!("{base}/{spec}")));
        }
        out
    }
}

/// A tsconfig is JSONC: with comments blanked by `mask` and the `:` outside
/// strings dropped it reads as EDN, where commas (trailing ones too) are
/// whitespace. An editor's UTF-8 BOM leads no value.
fn jsonc(source: &[u8]) -> Option<Value> {
    let source = source.strip_prefix(b"\xef\xbb\xbf").unwrap_or(source);
    let (mut text, holes) = mask(source);
    for (i, c) in text.iter_mut().enumerate() {
        if *c == b':' && !in_hole(&holes, i + 1) {
            *c = b' ';
        }
    }
    edn::parse(std::str::from_utf8(&text).ok()?).ok()
}

pub struct TypeScriptScanner;

pub static SCANNER: TypeScriptScanner = TypeScriptScanner;

impl Scanner for TypeScriptScanner {
    fn accepts(&self, relative: &str, _opts: &Value) -> bool {
        accepts(relative)
    }

    fn metadata_paths(&self, root: &str) -> Vec<String> {
        metadata_paths(root)
    }

    fn scan(
        &self,
        root: &str,
        opts: &Value,
        files: &[SourceFile],
        cwd: &str,
    ) -> Result<Value, String> {
        let prefix = match get(opts, "prefix") {
            Some(Value::String(s)) => s.clone(),
            _ => "app".to_owned(),
        };
        let ns_prefix = match get(opts, "ns-prefix") {
            Some(Value::String(s)) => s.clone(),
            _ => prefix.clone(),
        };
        let mut sorted: Vec<&SourceFile> = files.iter().filter(|f| accepts(&f.path)).collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let mut by_path: HashMap<String, Value> = HashMap::new();
        let mut project: HashSet<String> = HashSet::new();
        for f in &sorted {
            let cid = scan::id_of(
                &scan::ns_join(&ns_prefix, &module_relative(&f.path, root)),
                Some(&Value::String(prefix.clone())),
            );
            by_path.insert(f.path.clone(), cid.clone());
            project.insert(crate::value::encode_key(&cid));
        }
        let aliases = metadata_paths(root)
            .iter()
            .filter(|path| scan::relative_path(path, cwd) != **path)
            .find_map(|path| files.iter().find(|f| f.path == *path))
            .and_then(|f| Aliases::read(&f.path, &f.source))
            .unwrap_or_default();
        let lookup = |base: &str| -> Option<Value> {
            if let Some(cid) = by_path.get(base) {
                return Some(cid.clone());
            }
            for (emitted, sources) in ESM_SOURCES {
                if let Some(stem) = base.strip_suffix(emitted) {
                    for ext in *sources {
                        if let Some(cid) = by_path.get(&format!("{stem}{ext}")) {
                            return Some(cid.clone());
                        }
                    }
                }
            }
            for ext in EXTS {
                if let Some(cid) = by_path.get(&format!("{base}{ext}")) {
                    return Some(cid.clone());
                }
            }
            for ext in EXTS {
                if let Some(cid) = by_path.get(&format!("{base}/index{ext}")) {
                    return Some(cid.clone());
                }
            }
            None
        };
        let resolve = |file: &str, spec: &Value| -> Option<Value> {
            let spec = match spec {
                Value::String(s) => s.clone(),
                _ => return None,
            };
            if !spec.starts_with('.') {
                return aliases
                    .candidates(&spec)
                    .iter()
                    .find_map(|base| lookup(base));
            }
            let dir = match file.rsplit_once('/') {
                Some((d, _)) => d,
                None => "",
            };
            lookup(&normalize(&format!("{dir}/{spec}")))
        };
        let mut classes = Vec::new();
        let mut edges: Vec<Value> = Vec::new();
        let mut foreigns: Vec<Value> = Vec::new();
        let mut invokes_map: Vec<(Value, Value)> = Vec::new();
        for f in &sorted {
            let surface = read_module(&f.source);
            let cid = by_path[&f.path].clone();
            let mut ops: Vec<Value> = Vec::new();
            let mut bindings: Vec<(String, Value)> = Vec::new();
            let mut specs: Vec<Value> = Vec::new();
            let mut impls: Vec<Value> = Vec::new();
            let mut inherits: Vec<Value> = Vec::new();
            let mut interface = false;
            let mut value = false;
            for imp in seq(get(&surface, "imports")) {
                let spec = get(imp, "spec").cloned().unwrap_or(Value::Nil);
                if !specs.contains(&spec) {
                    specs.push(spec.clone());
                }
                if let Some(to) = resolve(&f.path, &spec) {
                    for n in seq(get(imp, "names")) {
                        if let Value::String(n) = n {
                            bindings.push((n.clone(), to.clone()));
                        }
                    }
                }
            }
            for exp in seq(get(&surface, "exports")) {
                for n in seq(get(exp, "ops")) {
                    if let Value::String(n) = n {
                        let op = record(vec![
                            ("name", Value::String(n.clone())),
                            ("text", Value::String(n.clone())),
                        ]);
                        if !ops.contains(&op) {
                            ops.push(op);
                        }
                    }
                }
                if let Some(spec) = get(exp, "spec")
                    && !specs.contains(spec)
                {
                    specs.push(spec.clone());
                }
                if let Some(Value::String(typ)) = get(exp, "type")
                    && let Some((_, to)) = bindings.iter().find(|(n, _)| n == typ)
                    && !impls.contains(to)
                {
                    impls.push(to.clone());
                }
                for (key, group) in [("implements", &mut impls), ("extends", &mut inherits)] {
                    for n in seq(get(exp, key)) {
                        if let Value::String(n) = n
                            && let Some((_, to)) = bindings.iter().find(|(b, _)| b == n)
                            && !group.contains(to)
                        {
                            group.push(to.clone());
                        }
                    }
                }
                interface = interface || flag(exp, "interface");
                value = value || flag(exp, "value");
            }
            for spec in seq(get(&surface, "calls")) {
                if !specs.contains(spec) {
                    specs.push(spec.clone());
                }
            }
            let mut c = record(vec![
                ("id", cid.clone()),
                ("name", Value::String(scan::module_name(&cid))),
                (
                    "ns",
                    Value::String(scan::ns_join(&ns_prefix, &module_relative(&f.path, root))),
                ),
                ("lang", Value::keyword("typescript")),
                ("file", Value::String(scan::relative_path(&f.path, cwd))),
            ]);
            if !ops.is_empty() {
                edn::record::put(&mut c, "ops", Value::Vector(ops));
            }
            if interface && !value {
                edn::record::put(&mut c, "stereotype", Value::keyword("interface"));
            }
            classes.push(c);
            let mut requires: Vec<Value> = Vec::new();
            let mut external: Vec<Value> = Vec::new();
            const ASSETS: &[&str] = &[
                "css", "scss", "sass", "less", "svg", "png", "jpg", "jpeg", "gif", "webp", "html",
                "md", "json",
            ];
            for spec in &specs {
                let spec_text = match spec {
                    Value::String(s) => s.clone(),
                    _ => continue,
                };
                let ext = spec_text.rsplit('.').next().unwrap_or("");
                if ASSETS.contains(&ext) {
                    continue;
                }
                if let Some(to) = resolve(&f.path, spec) {
                    if !requires.contains(&to) {
                        requires.push(to);
                    }
                } else if !spec_text.starts_with('.') {
                    let foreign = Value::keyword(
                        &spec_text
                            .strip_prefix('@')
                            .map(|s| format!("at.{s}"))
                            .unwrap_or(spec_text.clone())
                            .replace([':', '/'], "."),
                    );
                    if !external.contains(&foreign) {
                        external.push(foreign.clone());
                    }
                    if !foreigns.contains(&foreign) {
                        foreigns.push(foreign);
                    }
                }
            }
            for (group, kind) in [
                (&requires, "dependency"),
                (&external, "dependency"),
                (&impls, "implements"),
                (&inherits, "inheritance"),
            ] {
                for to in group {
                    if cid != *to {
                        let e = record(vec![
                            ("from", cid.clone()),
                            ("to", to.clone()),
                            ("kind", Value::keyword(kind)),
                        ]);
                        if !edges.contains(&e) {
                            edges.push(e);
                        }
                    }
                }
            }
            let inv = seq(get(&surface, "invokes"));
            if !inv.is_empty() {
                invokes_map.push((cid.clone(), Value::Vector(inv.to_vec())));
            }
        }
        for cid in foreigns {
            if !project.contains(&crate::value::encode_key(&cid)) {
                classes.push(scan::foreign_class(&cid));
            }
        }
        Ok(record(vec![
            ("classes", Value::Vector(classes)),
            ("edges", Value::Vector(edges)),
            ("invokes", Value::Map(invokes_map)),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::id_name;

    const V: &str = "export const v = 1;\n";

    fn scan_files(root: &str, cwd: &str, files: &[(&str, &str)]) -> Value {
        let files: Vec<SourceFile> = files
            .iter()
            .map(|(path, source)| SourceFile {
                path: path.to_string(),
                source: source.as_bytes().to_vec(),
            })
            .collect();
        let opts = record(vec![("prefix", Value::String("p".into()))]);
        SCANNER.scan(root, &opts, &files, cwd).unwrap()
    }

    /// Scans project `/p` with source root `/p/src`; paths are
    /// project-relative.
    fn scan(files: &[(&str, &str)]) -> Value {
        let files: Vec<(String, &str)> = files
            .iter()
            .map(|(path, source)| (format!("/p/{path}"), *source))
            .collect();
        let files: Vec<(&str, &str)> = files.iter().map(|(p, s)| (p.as_str(), *s)).collect();
        scan_files("/p/src", "/p", &files)
    }

    /// `to kind` of each edge out of `from`, in scan order.
    fn edges_from(scan: &Value, from: &str) -> Vec<String> {
        seq(get(scan, "edges"))
            .iter()
            .filter(|e| id_name(get(e, "from").unwrap()) == from)
            .map(|e| {
                let to = id_name(get(e, "to").unwrap());
                format!("{to} {}", id_name(get(e, "kind").unwrap()))
            })
            .collect()
    }

    fn edges(files: &[(&str, &str)], from: &str) -> Vec<String> {
        edges_from(&scan(files), from)
    }

    fn class(files: &[(&str, &str)], id: &str) -> Value {
        seq(get(&scan(files), "classes"))
            .iter()
            .find(|c| id_name(get(c, "id").unwrap()) == id)
            .unwrap()
            .clone()
    }

    fn ops(files: &[(&str, &str)], id: &str) -> Vec<String> {
        seq(get(&class(files, id), "ops"))
            .iter()
            .map(|op| id_name(get(op, "name").unwrap()))
            .collect()
    }

    #[test]
    fn an_import_or_require_call_with_a_literal_names_a_dependency() {
        let main = "export async function load() { return import(\"./a\"); }\n\
                    import('./b').then((m) => m.v);\n\
                    const c = require(\"./c\");\n\
                    import d = require(\"./d\");\n\
                    module.exports = { ...require(`./e`) };\n\
                    const l = require(\"./locales/\" + lang);\n\
                    const t = import(`./locales/${lang}`);\n\
                    // require(\"./x\")\n\
                    /* import(\"./x\") */\n\
                    const s = \"import('./x')\";\n\
                    const u = `require(\"./x\")`;\n\
                    const re = /require\\(\"\\.\\/x\"\\)/;\n\
                    loader.require(\"./x\");\n";
        let files = [
            ("src/a.ts", V),
            ("src/b.ts", V),
            ("src/c.ts", V),
            ("src/d.ts", V),
            ("src/e.ts", V),
            ("src/x.ts", V),
            ("src/locales/index.ts", V),
            ("src/main.ts", main),
        ];
        assert_eq!(
            vec![
                "a dependency",
                "b dependency",
                "c dependency",
                "d dependency",
                "e dependency"
            ],
            edges(&files, "main")
        );
    }

    #[test]
    fn an_esm_specifier_names_its_typescript_source() {
        let files = [
            ("src/a.ts", V),
            ("src/b.tsx", V),
            ("src/c.tsx", V),
            ("src/m.mts", V),
            ("src/n.cts", V),
            ("src/plain.js", V),
            (
                "src/main.ts",
                "import { a } from \"./a.js\";\n\
                 export { b } from \"./b.js\";\n\
                 import c from \"./c.jsx\";\n\
                 import \"./m.mjs\";\n\
                 import \"./n.cjs\";\n\
                 import \"./plain.js\";\n",
            ),
        ];
        assert_eq!(
            vec![
                "a dependency",
                "c dependency",
                "m dependency",
                "n dependency",
                "plain dependency",
                "b dependency"
            ],
            edges(&files, "main")
        );
    }

    #[test]
    fn an_import_without_a_semicolon_ends_at_its_specifier() {
        let files = [
            ("src/side.ts", V),
            ("src/a.ts", V),
            ("src/b.ts", V),
            ("src/c.ts", V),
            (
                "src/main.ts",
                "import \"./side\"\n\
                 import x from \"./a\"\n\
                 import type { Fs } from \"./b\"\n\
                 import { \"kebab-name\" as kebab } from \"./c\"\n\
                 export const v: Fs = x\n",
            ),
        ];
        assert_eq!(
            vec![
                "side dependency",
                "a dependency",
                "b dependency",
                "c dependency",
                "b implements"
            ],
            edges(&files, "main")
        );
    }

    #[test]
    fn a_from_in_a_later_string_names_no_module() {
        let files = [
            ("src/cache.ts", V),
            (
                "src/main.ts",
                "const a = 1\nexport { a }\nconst msg = \"loaded from './cache'\"\n",
            ),
        ];
        assert!(edges(&files, "main").is_empty());
        assert_eq!(vec!["a"], ops(&files, "main"));
    }

    #[test]
    fn a_type_only_star_export_depends_on_its_module() {
        let files = [
            ("src/a.ts", V),
            ("src/b.ts", V),
            (
                "src/main.ts",
                "export type * from \"./a\";\nexport type * as B from \"./b\";\n",
            ),
        ];
        assert_eq!(vec!["a dependency", "b dependency"], edges(&files, "main"));
    }

    #[test]
    fn a_modified_declaration_names_its_operation() {
        let files = [(
            "src/main.ts",
            "export abstract class Shape {}\n\
             export default abstract class Base {}\n\
             export const enum Color { Red }\n\
             export function* gen() {}\n\
             export async function* agen() {}\n\
             export declare function declared(): void;\n\
             export declare const dc: number;\n",
        )];
        assert_eq!(
            vec!["Shape", "Base", "Color", "gen", "agen", "declared", "dc"],
            ops(&files, "main")
        );
    }

    #[test]
    fn a_function_returning_an_imported_interface_implements_it() {
        let files = [
            ("src/fs.ts", "export interface Fs { read(): string; }\n"),
            (
                "src/destructured.ts",
                "import type { Fs } from \"./fs\";\n\
                 export function make({ root }: { root: string }): Fs { return root as Fs; }\n",
            ),
            (
                "src/arrow.ts",
                "import type { Fs } from \"./fs\";\n\
                 export const make = ({ root }: Opts): Fs => root as Fs;\n",
            ),
        ];
        let scan = scan(&files);
        assert_eq!(
            vec!["fs dependency", "fs implements"],
            edges_from(&scan, "destructured")
        );
        assert_eq!(
            vec!["fs dependency", "fs implements"],
            edges_from(&scan, "arrow")
        );
    }

    #[test]
    fn a_heritage_clause_names_its_base_and_interfaces() {
        let files = [
            ("src/fs.ts", "export interface Fs { read(): string; }\n"),
            (
                "src/closer.ts",
                "export interface Closer { close(): void; }\n",
            ),
            ("src/base.ts", "export class Base<T> {}\n"),
            (
                "src/node.ts",
                "import type { Fs } from \"./fs\";\n\
                 import * as closer from \"./closer\";\n\
                 import { Base } from \"./base\";\n\
                 export class NodeFs<T> extends Base<T> implements Fs, closer.Closer {\n  \
                 read() { return \"\"; }\n  close() {}\n}\n",
            ),
            (
                "src/anon.ts",
                "import { Base } from \"./base\";\nexport default class extends Base<string> {}\n",
            ),
            (
                "src/rich.ts",
                "import type { Fs } from \"./fs\";\nexport interface RichFs extends Fs { size(): number; }\n",
            ),
            (
                "src/boxed.ts",
                "import { Base } from \"./base\";\nexport class Boxed<T = string> extends Base<T> {}\n",
            ),
            (
                "src/mixed.ts",
                "import { Base } from \"./base\";\nimport type { Fs } from \"./fs\";\n\
                 export class Mixed extends mixin(Fs, Base) {}\n",
            ),
        ];
        let scan = scan(&files);
        assert_eq!(
            vec![
                "fs dependency",
                "closer dependency",
                "base dependency",
                "fs implements",
                "closer implements",
                "base inheritance"
            ],
            edges_from(&scan, "node")
        );
        assert_eq!(
            vec!["base dependency", "base inheritance"],
            edges_from(&scan, "anon")
        );
        assert_eq!(
            vec!["fs dependency", "fs inheritance"],
            edges_from(&scan, "rich")
        );
        assert_eq!(
            vec!["base dependency", "base inheritance"],
            edges_from(&scan, "boxed"),
            "a type parameter default does not end the header"
        );
        assert_eq!(
            vec!["base dependency", "fs dependency"],
            edges_from(&scan, "mixed"),
            "a mixin call's arguments are not bases"
        );
        assert_eq!(vec!["NodeFs"], ops(&files, "node"));
        assert!(ops(&files, "anon").is_empty());
    }

    #[test]
    fn an_apostrophe_in_jsx_text_hides_only_its_line() {
        let files = [
            ("src/a.ts", V),
            (
                "src/page.tsx",
                "export function Intro() {\n  return <p>Don't panic</p>;\n}\n\
                 export function Outro() {\n  return <p>Bye</p>;\n}\n\
                 export { a } from \"./a\";\n",
            ),
        ];
        assert_eq!(vec!["a dependency"], edges(&files, "page"));
        assert_eq!(vec!["Intro", "Outro", "a"], ops(&files, "page"));
    }

    #[test]
    fn a_line_continuation_keeps_a_string_open() {
        // An odd run of `\` before LF or CR LF continues the string; an even
        // run is escaped backslashes, and the JSX apostrophe ends its line.
        let main = "const lf = \"first \\\nimport { fake } from './fake';\";\n\
                    const crlf = 'first \\\r\nimport { fake } from \"./fake\";';\r\n\
                    const odd = \"a \\\\\\\nimport { fake } from './fake';\";\n\
                    import { real } from './real';\n";
        let page = "export const Tip = () => (\n  <p>\n    Don't type C:\\\\\n    \
                    {lazy(() => import('./real'))}\n  </p>\n);\n";
        let files = [
            ("src/fake.ts", V),
            ("src/real.ts", V),
            ("src/main.ts", main),
            ("src/page.tsx", page),
        ];
        let scan = scan(&files);
        assert_eq!(vec!["real dependency"], edges_from(&scan, "main"));
        assert_eq!(vec!["real dependency"], edges_from(&scan, "page"));
    }

    #[test]
    fn a_call_in_a_template_interpolation_names_a_dependency() {
        // The `${...}` expression is code; the template text around it is not.
        let main = "const a = `${require(\"./real\")} and require(\"./fake\")`;\n\
                    const b = `outer ${`inner ${import('./nested')}`} import('./fake')`;\n\
                    const c = `${ {k: require('./obj')}.k } require(\"./fake\")`;\n\
                    const d = `${ \"}\" + '}' /* } */ } require(\"./fake\")`;\n\
                    const e = `${name}\nimport { fake } from './fake';\n`;\n";
        let files = [
            ("src/fake.ts", V),
            ("src/real.ts", V),
            ("src/nested.ts", V),
            ("src/obj.ts", V),
            ("src/main.ts", main),
        ];
        assert_eq!(
            vec!["real dependency", "nested dependency", "obj dependency"],
            edges(&files, "main")
        );
    }

    #[test]
    fn a_slash_after_a_postfix_update_divides() {
        // `i++ /` and `i-- /` divide; a unary `+` or a prefix `++` still
        // precedes a regex, whose `` ` `` opens no template text.
        let inc = "export const f = (i: number, n: number, m: number) => \
                   `${i++ / n}/${m} require(\"./fake\")`;\n\
                   export const g = () => require(\"./real\");\n";
        let dec = "export const f = (i: number, n: number, m: number) => \
                   `${i-- / n}/${m} require(\"./fake\")`;\n\
                   export const g = () => require(\"./real\");\n";
        let unary = "export const f = (s: string) => s + +/`/.test(s);\n\
                     export const g = () => require(\"./real\");\n";
        let prefix = "export const f = (n: number) => n + ++/`/.lastIndex;\n\
                      export const g = () => require(\"./real\");\n";
        let newline = "let n = 0; n\n++/`/.lastIndex;\n\
                       export const g = () => require(\"./real\");\n";
        let files = [
            ("src/fake.ts", V),
            ("src/real.ts", V),
            ("src/inc.ts", inc),
            ("src/dec.ts", dec),
            ("src/unary.ts", unary),
            ("src/prefix.ts", prefix),
            ("src/newline.ts", newline),
        ];
        let scan = scan(&files);
        assert_eq!(vec!["real dependency"], edges_from(&scan, "inc"));
        assert_eq!(vec!["real dependency"], edges_from(&scan, "dec"));
        assert_eq!(vec!["real dependency"], edges_from(&scan, "unary"));
        assert_eq!(vec!["real dependency"], edges_from(&scan, "prefix"));
        assert_eq!(vec!["real dependency"], edges_from(&scan, "newline"));
    }

    #[test]
    fn a_module_of_type_aliases_is_an_interface() {
        let files = [
            (
                "src/types.ts",
                "export type Props = { a: number };\nexport type Id<T> = T;\n",
            ),
            (
                "src/mixed.ts",
                "export type Props = { a: number };\nexport const v = 1;\n",
            ),
        ];
        assert_eq!(
            Some(&Value::keyword("interface")),
            get(&class(&files, "types"), "stereotype")
        );
        assert_eq!(None, get(&class(&files, "mixed"), "stereotype"));
    }

    #[test]
    fn a_module_declaration_file_is_not_a_source() {
        assert!(!accepts("src/env.d.ts"));
        assert!(!accepts("src/env.d.mts"));
        assert!(!accepts("src/env.d.cts"));
        assert!(accepts("src/env.mts"));
    }

    #[test]
    fn tsconfig_paths_resolve_an_alias_to_its_module() {
        let tsconfig = "{\n  // JSONC: comments and trailing commas\n  \
                        \"compilerOptions\": {\n    /* \"paths\": {} */\n    \
                        \"paths\": {\n      \"@/*\": [\"./src/*\"],\n      \
                        \"@/components/*\": [\"src/ui/*\"],\n      \
                        \"~lib/*\": [\"missing/*\", \"src/lib/*\",],\n      \
                        \"config\": [\"src/settings.ts\"],\n    },\n  },\n}\n";
        let files = [
            ("tsconfig.json", tsconfig),
            ("src/lib/util.ts", V),
            ("src/ui/Button.tsx", V),
            ("src/settings.ts", V),
            (
                "src/page.ts",
                "import { util } from \"@/lib/util\";\n\
                 import { Button } from \"@/components/Button\";\n\
                 import { util as u2 } from \"~lib/util\";\n\
                 import config from \"config\";\n\
                 import React from \"react\";\n",
            ),
        ];
        assert_eq!(
            vec![
                "lib.util dependency",
                "ui.Button dependency",
                "settings dependency",
                "react dependency"
            ],
            edges(&files, "page")
        );
    }

    #[test]
    fn a_jsconfig_base_url_resolves_a_bare_specifier_under_it() {
        // `baseUrl: "."` above the `src` root: the specifier's foreign id
        // (`src.components.Button`) is not the class id. An editor's BOM
        // leads the file.
        let files = [
            (
                "jsconfig.json",
                "\u{feff}{ \"compilerOptions\": { \"baseUrl\": \".\" } }\n",
            ),
            ("src/components/Button.tsx", V),
            (
                "src/page.ts",
                "import { Button } from \"src/components/Button\";\n\
                 import React from \"react\";\n",
            ),
        ];
        assert_eq!(
            vec!["components.Button dependency", "react dependency"],
            edges(&files, "page")
        );
    }

    #[test]
    fn a_tsconfig_outside_the_project_is_ignored() {
        let files = [
            (
                "/tsconfig.json",
                "{ \"compilerOptions\": { \"paths\": { \"lib\": [\"p/a.ts\"] } } }",
            ),
            ("/p/a.ts", V),
            ("/p/main.ts", "import { v } from \"lib\";\n"),
        ];
        let scan = scan_files("/p", "/p", &files);
        assert_eq!(vec!["lib dependency"], edges_from(&scan, "main"));
    }

    /// Windows canonical roots (`files::portable`): a drive path and a UNC
    /// share keep resolving relative and aliased imports.
    #[test]
    fn drive_and_unc_roots_resolve_relative_and_aliased_imports() {
        for project in ["C:/p", "//server/share/p"] {
            let files = [
                (
                    format!("{project}/tsconfig.json"),
                    "{ \"compilerOptions\": { \"paths\": { \"@/*\": [\"./src/*\"] } } }",
                ),
                (format!("{project}/src/lib/util.ts"), V),
                (format!("{project}/src/a.ts"), V),
                (
                    format!("{project}/src/page.ts"),
                    "import { util } from \"@/lib/util\";\nimport { v } from \"./a\";\n",
                ),
            ];
            let files: Vec<(&str, &str)> = files.iter().map(|(p, s)| (p.as_str(), *s)).collect();
            let scan = scan_files(&format!("{project}/src"), project, &files);
            assert_eq!(
                vec!["lib.util dependency", "a dependency"],
                edges_from(&scan, "page"),
                "{project}"
            );
        }
    }
}
