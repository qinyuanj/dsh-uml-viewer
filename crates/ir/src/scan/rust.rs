//! Port of `love/uml/rust_language/graph_rust.lua`: canonical file snapshots
//! are supplied by the IO adapter. Scanner limitations are preserved.

use std::collections::{HashMap, HashSet};

use edn::Value;

use crate::scan::{self, Scanner, SourceFile};
use crate::text::{self, at, is_space};
use crate::value::get_truthy as get;
use edn::record::{record, seq};

fn is_rust_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_rust_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn in_hole(holes: &[(usize, usize)], i: usize) -> bool {
    holes.iter().any(|&(a, b)| i >= a && i < b)
}

/// `mask(source)`: comments blanked; strings and char literals kept as holes.
/// Block comments nest, and a raw string ends only at its closing quote and
/// `#`s, whatever quotes or backslashes it holds.
fn mask(source: &[u8]) -> (Vec<u8>, Vec<(usize, usize)>) {
    let mut out = Vec::new();
    let mut holes = Vec::new();
    let mut i = 1;
    while i <= source.len() {
        let c = at(source, i);
        let n = at(source, i + 1);
        if text::line_comment(c, n) {
            i = text::mask_line_comment(source, &mut out, i, None);
            continue;
        }
        if text::block_comment(c, n) {
            i = mask_nested_comment(source, &mut out, i);
            continue;
        }
        let mut finish = None;
        if let Some(raw) = raw_string(source, i) {
            finish = Some(raw);
        } else if c == Some(b'"') {
            finish = Some(text::scan_quoted(source, i, b'"', None));
        } else if c == Some(b'\'')
            && n.is_some()
            && n != Some(b'\\')
            && at(source, i + 2) == Some(b'\'')
        {
            finish = Some(i + 3);
        } else if c == Some(b'\'') && n == Some(b'\\') {
            finish = Some(text::find_plain(source, b"'", i + 3).unwrap_or(source.len()) + 1);
        }
        if let Some(finish) = finish {
            holes.push((i, finish));
            out.extend_from_slice(&source[i - 1..finish - 1]);
            i = finish;
        } else {
            out.push(c.unwrap());
            i += 1;
        }
    }
    (out, holes)
}

/// The block comment at `i` blanked through the `*/` that closes it, nested
/// comments included, newlines kept: one past it, or past the end.
fn mask_nested_comment(source: &[u8], out: &mut Vec<u8>, i: usize) -> usize {
    let mut depth = 0usize;
    let mut j = i;
    while j <= source.len() {
        let rest = &source[j - 1..];
        if rest.starts_with(b"/*") {
            depth += 1;
            j += 2;
        } else if rest.starts_with(b"*/") {
            depth -= 1;
            j += 2;
            if depth == 0 {
                break;
            }
        } else {
            j += 1;
        }
    }
    let finish = j.min(source.len() + 1);
    out.extend(
        source[i - 1..finish - 1]
            .iter()
            .map(|&c| if c == b'\n' { c } else { b' ' }),
    );
    finish
}

/// The raw string at `i` (`r"…"`, `br#"…"#`, `cr##"…"##`): one past the
/// quote and `#`s that close it, or past the end when nothing does.
fn raw_string(source: &[u8], i: usize) -> Option<usize> {
    if at(source, i - 1).is_some_and(is_rust_ident) {
        return None;
    }
    let mut j = i;
    if matches!(at(source, j), Some(b'b' | b'c')) {
        j += 1;
    }
    if at(source, j) != Some(b'r') {
        return None;
    }
    let hashes = source[j..].iter().take_while(|&&c| c == b'#').count();
    j += 1 + hashes;
    if at(source, j) != Some(b'"') {
        return None;
    }
    let close = [&b"\""[..], &vec![b'#'; hashes]].concat();
    Some(text::find_plain(source, &close, j + 1).map_or(source.len() + 1, |p| p + close.len()))
}

/// `paths(path)`: strip `{...}` groups and a trailing `as` rename, then split
/// on `::` into trimmed non-empty segments.
fn paths(path: &[u8]) -> Vec<Vec<u8>> {
    // gsub('{[^\n]*', ''): from each '{' to before the next newline.
    let mut stripped = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        if path[i] == b'{' {
            while i < path.len() && path[i] != b'\n' {
                i += 1;
            }
        } else {
            stripped.push(path[i]);
            i += 1;
        }
    }
    // gsub(' as [^\n]*$', '')
    let mut end = stripped.len();
    if let Some(pos) = find_last_as(&stripped) {
        end = pos;
    }
    let stripped = &stripped[..end];
    let mut xs = Vec::new();
    let mut rest = stripped;
    loop {
        match find_subslice(rest, b"::") {
            Some(p) => {
                let seg = text::trim(&rest[..p]);
                if !seg.is_empty() {
                    xs.push(seg.to_vec());
                }
                rest = &rest[p + 2..];
            }
            None => {
                let seg = text::trim(rest);
                if !seg.is_empty() {
                    xs.push(seg.to_vec());
                }
                break;
            }
        }
    }
    xs
}

/// One path a `use` tree imports: its segments and the name it binds (an
/// `as` alias, else its last segment).
type Import = (Vec<Vec<u8>>, Vec<u8>);

/// `imports(tree)`: the paths of a `use` tree with its groups expanded, so
/// `a::{b, c::{self, d as e}, *}` imports `a::b`, `a::c`, `a::c::d` (bound
/// as `e`) and `a::*`.
fn imports(tree: &[u8]) -> Vec<Import> {
    let mut out = Vec::new();
    expand_use(tree, &[], &mut out);
    out
}

fn expand_use(tree: &[u8], prefix: &[Vec<u8>], out: &mut Vec<Import>) {
    for item in split_top_level(tree) {
        let item = text::trim(item);
        if item.is_empty() {
            continue;
        }
        if let Some(open) = item.iter().position(|&c| c == b'{') {
            let close = closing_brace(item, open);
            let mut head = prefix.to_vec();
            head.extend(paths(&item[..open]));
            expand_use(&item[open + 1..close], &head, out);
            continue;
        }
        let (path, alias) = split_alias(item);
        let segs = if path == b"self" && !prefix.is_empty() {
            prefix.to_vec()
        } else {
            [prefix.to_vec(), paths(path)].concat()
        };
        if let Some(last) = segs.last() {
            let name = alias.unwrap_or(last).to_vec();
            out.push((segs, name));
        }
    }
}

/// The items of a use group: `s` split at commas outside nested braces.
fn split_top_level(s: &[u8]) -> Vec<&[u8]> {
    let mut items = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, &c) in s.iter().enumerate() {
        match c {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                items.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&s[start..]);
    items
}

/// The index of the `}` closing the `{` at `open`; the end when unbalanced.
fn closing_brace(s: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, &c) in s.iter().enumerate().skip(open) {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    s.len()
}

/// A use leaf split at its `as` rename: `x::Y as Z` is `x::Y`, alias `Z`.
fn split_alias(item: &[u8]) -> (&[u8], Option<&[u8]>) {
    let word_as = |p: usize| {
        item[p..].starts_with(b"as")
            && p > 0
            && is_space(item[p - 1])
            && item.get(p + 2).is_some_and(|&c| is_space(c))
    };
    match (0..item.len()).rev().find(|&p| word_as(p)) {
        Some(p) => (text::trim(&item[..p]), Some(text::trim(&item[p + 2..]))),
        None => (item, None),
    }
}

/// The ` as [^\n]*$` strip: last ` as ` whose remainder has no newline.
fn find_last_as(s: &[u8]) -> Option<usize> {
    let mut found = None;
    let mut start = 0;
    while let Some(off) = find_subslice(&s[start.min(s.len())..], b" as ") {
        let pos = start + off;
        if !s[pos..].contains(&b'\n') {
            found = Some(pos);
        }
        start = pos + 1;
    }
    found
}

fn find_subslice(s: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || s.len() < needle.len() {
        return None;
    }
    s.windows(needle.len()).position(|w| w == needle)
}

/// `%b()` at the start of `s`: length of the balanced `(...)` span.
fn balanced_parens(s: &[u8]) -> Option<usize> {
    if s.first() != Some(&b'(') {
        return None;
    }
    let mut depth = 0;
    for (i, &c) in s.iter().enumerate() {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// `^%s*pub%b()%s+` then `^%s*pub%s+` then (for `command`) `^%s*async%s+`.
fn strip_pub(s: &[u8], also_async: bool) -> &[u8] {
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    if let Some(rest) = s[spaces..].strip_prefix(b"pub")
        && let Some(len) = balanced_parens(rest)
    {
        let after = &rest[len..];
        let trailing = after.iter().take_while(|&&c| is_space(c)).count();
        if trailing > 0 {
            return strip_pub_simple(&after[trailing..], also_async);
        }
    }
    strip_pub_simple(s, also_async)
}

fn strip_pub_simple(s: &[u8], also_async: bool) -> &[u8] {
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    if let Some(rest) = s[spaces..].strip_prefix(b"pub") {
        let trailing = rest.iter().take_while(|&&c| is_space(c)).count();
        if trailing > 0 {
            let after = &rest[trailing..];
            if !also_async {
                return after;
            }
            return strip_async(after);
        }
    }
    if also_async {
        return strip_async(s);
    }
    s
}

fn strip_async(s: &[u8]) -> &[u8] {
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    if let Some(rest) = s[spaces..].strip_prefix(b"async") {
        let trailing = rest.iter().take_while(|&&c| is_space(c)).count();
        if trailing > 0 {
            return &rest[trailing..];
        }
    }
    s
}

/// `^#%[%s*tauri%s*::%s*command%f[^%w_][^%]]*%](.*)$`: returns the capture.
fn match_command_attribute(s: &[u8]) -> Option<&[u8]> {
    let s = s.strip_prefix(b"#[")?;
    let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
    let s = s.strip_prefix(b"tauri")?;
    let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
    let s = s.strip_prefix(b"::")?;
    let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
    let s = s.strip_prefix(b"command")?;
    if s.first().is_some_and(|&c| crate::text::is_word(c)) {
        return None;
    }
    let close = s.iter().position(|&c| c == b']')?;
    Some(&s[close + 1..])
}

/// `command(text, at)`: a `#[tauri::command]` attribute directly precedes the
/// `fn` at `at` (within the 240-byte lookback), modulo pub/async.
fn command(text: &[u8], at: usize) -> bool {
    let start = at.saturating_sub(240).max(1);
    let prefix = text::sub(text, start as i64, at as i64 - 1);
    let mut search = 0;
    while let Some(off) = find_subslice(&prefix[search..], b"#[") {
        let p = search + off; // 0-based position of '#['
        if let Some(rest) = match_command_attribute(&prefix[p..]) {
            let rest = strip_pub(rest, true);
            if rest.iter().all(|&c| is_space(c)) {
                return true;
            }
        }
        search = p + 2;
    }
    false
}

/// Positions of `word` with `%f[%w_]` before and `%f[^%w_]` after (1-based).
fn word_positions(text: &[u8], word: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut search = 0;
    while let Some(off) = find_subslice(&text[search.min(text.len())..], word) {
        let p = search + off; // 0-based
        let before_ok = p == 0 || !crate::text::is_word(text[p - 1]);
        let after = p + word.len();
        let after_ok = after >= text.len() || !crate::text::is_word(text[after]);
        if before_ok && after_ok {
            out.push(p + 1);
        }
        search = after.max(p + 1);
    }
    out
}

/// `read_module(source)`.
pub fn read_module(source: &[u8]) -> Value {
    let (text, holes) = mask(source);
    let blocks = inline_mods(&text, &holes);
    let mut uses: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut mods: Vec<Vec<u8>> = Vec::new();
    let mut impls: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut commands: Vec<Vec<u8>> = Vec::new();
    let mut ops: Vec<Vec<u8>> = Vec::new();
    let mut publics: HashSet<Vec<u8>> = HashSet::new();
    let mut use_end = 0usize;
    let mut mod_end = 0usize;
    for i in 1..=text.len() {
        if i == 1 || at(&text, i - 1) == Some(b'\n') {
            let rest = &text[i - 1..];
            let body = strip_pub(rest, false);
            let u = match_use(body);
            let m = match_mod(body);
            if let Some(capture) = u
                && i > use_end
            {
                use_end = i + find_subslice(rest, b";").unwrap_or(rest.len() - 1);
                let depth = blocks.iter().filter(|b| b.holds(i)).count();
                if !in_hole(&holes, i)
                    && let Some(path) = out_of_inline_mods(text::trim(capture), depth)
                {
                    uses.push((path, i));
                }
            }
            if let Some(capture) = m
                && i > mod_end
            {
                mod_end = i + find_subslice(rest, b";").unwrap_or(rest.len() - 1);
                if !in_hole(&holes, i) && capture != b"tests" {
                    mods.push(capture.to_vec());
                }
            }
        }
    }
    for at_pos in word_positions(&text, b"pub") {
        if !in_hole(&holes, at_pos)
            && let Some(name) = match_pub_fn(&text[at_pos - 1 + 3..])
        {
            publics.insert(name);
        }
    }
    let mut functions: Vec<Vec<u8>> = Vec::new();
    for at_pos in word_positions(&text, b"fn") {
        if !in_hole(&holes, at_pos)
            && let Some(name) = match_fn_name(&text[at_pos - 1..])
        {
            functions.push(name.clone());
            if command(&text, at_pos) && !commands.contains(&name) {
                commands.push(name);
            }
        }
    }
    for n in &functions {
        if publics.contains(n) {
            if !ops.contains(n) {
                ops.push(n.clone());
            }
        } else {
            for cmd in &commands {
                if cmd == n && !ops.contains(n) {
                    ops.push(n.clone());
                }
            }
        }
    }
    for at_pos in word_positions(&text, b"impl") {
        if !in_hole(&holes, at_pos) {
            let rest = &text[at_pos - 1 + 4..];
            let mut tr = match_trait_for(rest);
            if starts_with_spaces_then(rest, b"<") {
                let generic: &[u8] = match_generic(rest).unwrap_or(&[]);
                let offset = find_subslice(rest, b"<").unwrap();
                let mut search = 0;
                while let Some(off) = find_subslice(&generic[search.min(generic.len())..], b">") {
                    let close = search + off; // 0-based in generic
                    if let Some(candidate) = match_trait_for(&rest[offset + close + 2..]) {
                        tr = Some(candidate);
                    }
                    search = close + 1;
                }
            }
            if let Some(tr) = tr
                && find_subslice(&tr, b":::").is_none()
                && tr.last() != Some(&b':')
            {
                impls.push((tr, at_pos));
            }
        }
    }
    // A `use` binds and an `impl` names in the scope at its position.
    let located = |xs: &[(Vec<u8>, usize)]| {
        let at = |at: &usize| Value::Integer(i64::try_from(*at).unwrap_or(i64::MAX));
        xs.iter()
            .map(|(x, i)| record(vec![("path", Value::String(text::text(x))), ("at", at(i))]))
            .collect()
    };
    record(vec![
        ("uses", Value::Vector(located(&uses))),
        (
            "mods",
            Value::Vector(mods.iter().map(|m| Value::String(text::text(m))).collect()),
        ),
        ("impls", Value::Vector(located(&impls))),
        (
            "commands",
            Value::Vector(
                commands
                    .iter()
                    .map(|c| Value::String(text::text(c)))
                    .collect(),
            ),
        ),
        (
            "ops",
            Value::Vector(ops.iter().map(|o| Value::String(text::text(o))).collect()),
        ),
    ])
}

/// `^%s*use%s+([^;]+);` — the capture may span lines.
fn match_use(body: &[u8]) -> Option<&[u8]> {
    let s = &body[body.iter().take_while(|&&c| is_space(c)).count()..];
    let s = s.strip_prefix(b"use")?;
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    let s = &s[spaces..];
    let end = s.iter().position(|&c| c == b';')?;
    if end == 0 {
        return None;
    }
    Some(&s[..end])
}

/// `^%s*mod%s+([A-Za-z_][A-Za-z0-9_]*)%s*;`.
fn match_mod(body: &[u8]) -> Option<&[u8]> {
    let s = &body[body.iter().take_while(|&&c| is_space(c)).count()..];
    let s = s.strip_prefix(b"mod")?;
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    let s = &s[spaces..];
    if !s.first().is_some_and(|&c| is_rust_ident_start(c)) {
        return None;
    }
    let len = s.iter().take_while(|&&c| is_rust_ident(c)).count();
    let name = &s[..len];
    let s = &s[len..];
    let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
    s.strip_prefix(b";")?;
    Some(name)
}

/// `mod name {` at the start of `s`: the name and the offset of its `{`.
fn match_inline_mod(s: &[u8]) -> Option<(Vec<u8>, usize)> {
    let rest = s.strip_prefix(b"mod")?;
    let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
    let name = &rest[spaces..];
    if spaces == 0 || !name.first().is_some_and(|&c| is_rust_ident_start(c)) {
        return None;
    }
    let len = name.iter().take_while(|&&c| is_rust_ident(c)).count();
    let after = &name[len..];
    let gap = after.iter().take_while(|&&c| is_space(c)).count();
    (after.get(gap) == Some(&b'{')).then(|| (name[..len].to_vec(), s.len() - after.len() + gap))
}

/// An inline `mod name { … }`: the 1-based positions of its `mod` keyword
/// and of the braces around its body.
struct Block {
    at: usize,
    name: Vec<u8>,
    open: usize,
    close: usize,
}

impl Block {
    fn holds(&self, i: usize) -> bool {
        i > self.open && i < self.close
    }
}

/// Each inline `mod name { … }` outside literals, wherever it starts:
/// `#[cfg(test)] mod tests { … }` on one line counts.
fn inline_mods(text: &[u8], holes: &[(usize, usize)]) -> Vec<Block> {
    let mut blocks = Vec::new();
    for at in word_positions(text, b"mod") {
        if !in_hole(holes, at)
            && let Some((name, off)) = match_inline_mod(&text[at - 1..])
        {
            let open = at + off;
            let close = closing(text, holes, open);
            blocks.push(Block {
                at,
                name,
                open,
                close,
            });
        }
    }
    blocks
}

/// The 1-based position of the `}` closing the `{` at `open`, braces in
/// literals aside; one past the end when unbalanced.
fn closing(text: &[u8], holes: &[(usize, usize)], open: usize) -> usize {
    // Holes are in source order: step over each one as it comes.
    let mut hole = holes.partition_point(|&(_, b)| b <= open);
    let mut depth = 0usize;
    let mut j = open;
    while j <= text.len() {
        if let Some(&(a, b)) = holes.get(hole)
            && j >= a
        {
            j = b;
            hole += 1;
            continue;
        }
        match text[j - 1] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
        j += 1;
    }
    text.len() + 1
}

/// The inline mod whose body most closely holds `i`, if any.
fn scope(blocks: &[Block], i: usize) -> Option<&Block> {
    blocks.iter().filter(|b| b.holds(i)).max_by_key(|b| b.open)
}

/// Whether a name declared at `from` is in scope at `at`: declared at file
/// level, or in an inline mod that holds `at`. A mod nested there sees it
/// as its `use super::*` would show it; a sibling or the file outside does
/// not.
fn reaches(blocks: &[Block], from: usize, at: usize) -> bool {
    scope(blocks, from).is_none_or(|b| b.holds(at))
}

/// A `use` path written `depth` inline mods deep, restated from the file:
/// each `super` climbs one inline mod, and a path that stays inside the
/// file names nothing outside it.
fn out_of_inline_mods(path: &[u8], depth: usize) -> Option<Vec<u8>> {
    if depth == 0 {
        return Some(path.to_vec());
    }
    let mut rest = path;
    let mut climbed = 0;
    while climbed < depth
        && let Some(r) = rest.strip_prefix(b"super::")
    {
        rest = r;
        climbed += 1;
    }
    if climbed == depth && rest.starts_with(b"super::") {
        Some(rest.to_vec())
    } else if climbed == depth {
        Some([b"self::", rest].concat())
    } else if climbed == 0 && !rest.starts_with(b"self::") {
        Some(path.to_vec())
    } else {
        None
    }
}

/// `rest:gsub('^(%s+)async%s+','%1')` then `^%s+fn%s+([A-Za-z_][A-Za-z0-9_]*)`
/// for the `pub` scan.
fn match_pub_fn(s: &[u8]) -> Option<Vec<u8>> {
    let lead = s.iter().take_while(|&&c| is_space(c)).count();
    if lead == 0 {
        return None;
    }
    let mut rest = &s[lead..];
    if let Some(after) = rest.strip_prefix(b"async") {
        let trailing = after.iter().take_while(|&&c| is_space(c)).count();
        if trailing > 0 {
            rest = &after[trailing..];
        }
    }
    let rest = rest.strip_prefix(b"fn")?;
    let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    let rest = &rest[spaces..];
    if !rest.first().is_some_and(|&c| is_rust_ident_start(c)) {
        return None;
    }
    let len = rest.iter().take_while(|&&c| is_rust_ident(c)).count();
    Some(rest[..len].to_vec())
}

/// `^fn%s+([A-Za-z_][A-Za-z0-9_]*)` — text starting at the `fn` keyword.
fn match_fn_name(s: &[u8]) -> Option<Vec<u8>> {
    let s = s.strip_prefix(b"fn")?;
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    let s = &s[spaces..];
    if !s.first().is_some_and(|&c| is_rust_ident_start(c)) {
        return None;
    }
    let len = s.iter().take_while(|&&c| is_rust_ident(c)).count();
    Some(s[..len].to_vec())
}

/// `^%s+([A-Za-z_][A-Za-z0-9_:]*)%s+for%s+`.
fn match_trait_for(s: &[u8]) -> Option<Vec<u8>> {
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    let s = &s[spaces..];
    if !s.first().is_some_and(|&c| is_rust_ident_start(c)) {
        return None;
    }
    let len = s
        .iter()
        .take_while(|&&c| is_rust_ident(c) || c == b':')
        .count();
    let name = &s[..len];
    let s = &s[len..];
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    let s = &s[spaces..];
    let s = s.strip_prefix(b"for")?;
    if s.first().is_some_and(|&c| is_space(c)) {
        Some(name.to_vec())
    } else {
        None
    }
}

fn starts_with_spaces_then(s: &[u8], lit: &[u8]) -> bool {
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    s[spaces..].starts_with(lit)
}

/// `^%s*<([^;{]*)` — the generic parameter text after `<`.
fn match_generic(s: &[u8]) -> Option<&[u8]> {
    let spaces = s.iter().take_while(|&&c| is_space(c)).count();
    let s = &s[spaces..];
    let s = s.strip_prefix(b"<")?;
    let end = s
        .iter()
        .position(|&c| c == b';' || c == b'{')
        .unwrap_or(s.len());
    Some(&s[..end])
}

fn parent(path: &str) -> Option<String> {
    // `^(.*)/[^/]+$`: a slash with a non-empty final segment.
    match path.rsplit_once('/') {
        Some((p, last)) if !last.is_empty() => Some(p.to_owned()),
        _ => None,
    }
}

pub struct RustScanner;

pub static SCANNER: RustScanner = RustScanner;

/// `metadata_paths(root)`: the explicitly named Cargo.toml files.
fn metadata_paths(root: &str) -> Vec<String> {
    let mut out = vec![format!("{root}/Cargo.toml")];
    if let Some(p) = parent(root) {
        out.push(format!("{p}/Cargo.toml"));
    }
    out
}

/// The first present metadata file of `root`.
fn manifest<'a>(root: &str, by_path: &HashMap<String, &'a SourceFile>) -> Option<&'a [u8]> {
    metadata_paths(root)
        .iter()
        .find_map(|path| by_path.get(path).map(|f| f.source.as_slice()))
}

/// A Cargo name as Rust code spells it: `-` becomes `_`.
fn crate_ident(name: &[u8]) -> Vec<u8> {
    name.iter()
        .map(|&c| if c == b'-' { b'_' } else { c })
        .collect()
}

/// `lib_name(root, by_path)`: the `[lib] name = "..."` of the first present
/// metadata file, else its `[package] name` when `root` has a `lib.rs`.
fn lib_name(root: &str, by_path: &HashMap<String, &SourceFile>) -> Option<Vec<u8>> {
    let cargo = manifest(root, by_path)?;
    table_name(cargo, b"[lib]")
        .or_else(|| {
            by_path
                .contains_key(&format!("{root}/lib.rs"))
                .then(|| table_name(cargo, b"[package]"))
                .flatten()
        })
        .map(|n| crate_ident(&n))
}

/// The `name = "..."` key of the manifest table headed `header`.
fn table_name(cargo: &[u8], header: &[u8]) -> Option<Vec<u8>> {
    let b = find_subslice(cargo, header).map(|a| a + header.len())?;
    let finish = find_subslice(&cargo[b..], b"\n[")
        .map(|p| b + p)
        .unwrap_or(cargo.len());
    let body = &cargo[b..finish];
    // `^name%s*=%s*"([^"]+)"` or `\nname%s*=%s*"([^"]+)"`
    let mut search = 0;
    while search <= body.len() {
        let at_line_start = search == 0 || body[search - 1] == b'\n';
        if at_line_start && body[search..].starts_with(b"name") {
            let s = &body[search + 4..];
            let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
            if let Some(s) = s.strip_prefix(b"=") {
                let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
                if let Some(s) = s.strip_prefix(b"\"")
                    && let Some(end) = s.iter().position(|&c| c == b'"')
                {
                    return Some(s[..end].to_vec());
                }
            }
        }
        match body[search..].iter().position(|&c| c == b'\n') {
            Some(off) => search += off + 1,
            None => break,
        }
    }
    None
}

/// `dependencies(root, by_path)`: the crates the first present metadata file
/// declares under `[dependencies]` or `[target.<cfg>.dependencies]`, as code
/// names them. Dev and build dependencies never reach the scanned sources.
fn dependencies(root: &str, by_path: &HashMap<String, &SourceFile>) -> Vec<Vec<u8>> {
    let Some(cargo) = manifest(root, by_path) else {
        return Vec::new();
    };
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut listing = false;
    let mut value = ManifestValue::default();
    for line in cargo.split(|&c| c == b'\n') {
        let line = text::trim(line);
        let header = !value.continuing() && line.starts_with(b"[");
        let name = if header {
            value = ManifestValue::default();
            let header = line.split(|&c| c == b'#').next().unwrap_or_default();
            let table = dependency_table(text::trim(header));
            listing = table.is_some_and(|t| t.is_empty());
            table.filter(|t| !t.is_empty())
        } else if listing && !value.continuing() {
            let key = line
                .iter()
                .take_while(|&&c| is_rust_ident(c) || c == b'-')
                .count();
            let after = text::trim(&line[key..]);
            (key > 0 && (after.starts_with(b"=") || after.starts_with(b"."))).then(|| &line[..key])
        } else {
            None
        };
        if let Some(name) = name.map(crate_ident)
            && !out.contains(&name)
        {
            out.push(name);
        }
        if listing && !header {
            scan_manifest_value(line, &mut value);
        }
    }
    out
}

/// The rest of a manifest table header after its `dependencies` key: empty
/// for a dependency list, the crate name for one dependency's own table
/// (`[dependencies.name]`), `None` for any other table.
fn dependency_table(header: &[u8]) -> Option<&[u8]> {
    let h = header.strip_prefix(b"[")?.strip_suffix(b"]")?;
    let rest = match h.strip_prefix(b"dependencies") {
        Some(rest) => rest,
        None => {
            let t = h.strip_prefix(b"target.")?;
            let p = find_subslice(t, b".dependencies")?;
            &t[p + b".dependencies".len()..]
        }
    };
    if rest.is_empty() {
        Some(rest)
    } else {
        rest.strip_prefix(b".")
    }
}

#[derive(Clone, Copy)]
enum ManifestString {
    Basic,
    Literal,
    MultilineBasic,
    MultilineLiteral,
}

/// Enough TOML lexical state to tell a new dependency key from a
/// continuation of the preceding inline table, array or multiline string.
#[derive(Default)]
struct ManifestValue {
    depth: usize,
    string: Option<ManifestString>,
}

impl ManifestValue {
    fn continuing(&self) -> bool {
        self.depth > 0 || self.string.is_some()
    }
}

fn scan_manifest_value(line: &[u8], value: &mut ManifestValue) {
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        if let Some(string) = value.string {
            match string {
                ManifestString::Basic => match rest[0] {
                    b'\\' => i = (i + 2).min(line.len()),
                    b'"' => {
                        value.string = None;
                        i += 1;
                    }
                    _ => i += 1,
                },
                ManifestString::Literal => {
                    if rest[0] == b'\'' {
                        value.string = None;
                    }
                    i += 1;
                }
                ManifestString::MultilineBasic => {
                    if rest.starts_with(b"\"\"\"") {
                        value.string = None;
                        i += 3;
                    } else if rest[0] == b'\\' {
                        i = (i + 2).min(line.len());
                    } else {
                        i += 1;
                    }
                }
                ManifestString::MultilineLiteral => {
                    if rest.starts_with(b"'''") {
                        value.string = None;
                        i += 3;
                    } else {
                        i += 1;
                    }
                }
            }
            continue;
        }
        if rest.starts_with(b"\"\"\"") {
            value.string = Some(ManifestString::MultilineBasic);
            i += 3;
            continue;
        }
        if rest.starts_with(b"'''") {
            value.string = Some(ManifestString::MultilineLiteral);
            i += 3;
            continue;
        }
        match rest[0] {
            b'#' => break,
            b'"' => {
                value.string = Some(ManifestString::Basic);
                i += 1;
            }
            b'\'' => {
                value.string = Some(ManifestString::Literal);
                i += 1;
            }
            b'{' | b'[' | b'(' => {
                value.depth += 1;
                i += 1;
            }
            b'}' | b']' | b')' => {
                value.depth = value.depth.saturating_sub(1);
                i += 1;
            }
            _ => i += 1,
        }
    }
    if matches!(
        value.string,
        Some(ManifestString::Basic | ManifestString::Literal)
    ) {
        // Ordinary TOML strings cannot continue on the next line. Recover at
        // the physical line boundary rather than hiding later dependencies.
        value.string = None;
    }
}

/// Each path `text` heads with `word` (`word::…`, or the global `::word::…`)
/// outside string and char literals: its 1-based position and its segments
/// up to the first non-identifier (`word::a::B::<T>` is `word::a::B`).
/// `word` as a later segment (`a::word::…`) or the tail of an identifier
/// does not count.
fn inline_paths(text: &[u8], holes: &[(usize, usize)], word: &[u8]) -> Vec<(usize, Vec<Vec<u8>>)> {
    let mut needle = word.to_vec();
    needle.extend_from_slice(b"::");
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(off) = find_subslice(&text[pos..], &needle) {
        let a = pos + off; // 0-based match start
        pos = a + needle.len();
        let lead = if text[..a].ends_with(b"::") { a - 2 } else { a };
        let before = lead.checked_sub(1).map(|b| text[b]);
        // `v.dep::<T>()` is a method call; `0..dep::MAX` is a range.
        let method = text[..lead]
            .iter()
            .rposition(|&c| !is_space(c))
            .is_some_and(|b| text[b] == b'.' && (b == 0 || text[b - 1] != b'.'));
        if before.is_some_and(|c| is_rust_ident(c) || c == b':') || method || in_hole(holes, a + 1)
        {
            continue;
        }
        let mut segs = vec![word.to_vec()];
        let mut rest = &text[pos..];
        loop {
            let n = rest.iter().take_while(|&&c| is_rust_ident(c)).count();
            if n == 0 {
                break;
            }
            segs.push(rest[..n].to_vec());
            match rest[n..].strip_prefix(b"::") {
                Some(r) => rest = r,
                None => break,
            }
        }
        out.push((a + 1, segs));
    }
    out
}

/// What a path in a module names.
enum Target {
    /// A module of the scanned crate.
    Module(Value),
    /// An item of another crate, as a qualified stub id (`egui.Context`).
    Foreign(Value),
}

/// The qualified stub id of a path into another crate: its leading
/// identifier segments joined with `.` (`egui::Context` is `egui.Context`;
/// a glob ends it).
fn qualified(segs: &[Vec<u8>]) -> Option<Value> {
    let idents: Vec<String> = segs
        .iter()
        .take_while(|s| {
            s.first().is_some_and(|&c| is_rust_ident_start(c))
                && s.iter().all(|&c| is_rust_ident(c))
        })
        .map(|s| text::text(s))
        .collect();
    (!idents.is_empty()).then(|| Value::keyword(&idents.join(".")))
}

/// The 1-based source position of a located `use` or `impl`.
fn position(site: &Value) -> usize {
    match get(site, "at") {
        Some(Value::Integer(i)) => usize::try_from(*i).unwrap_or(usize::MAX),
        _ => 0,
    }
}

/// The source text of a located `use` or `impl`.
fn site_path(site: &Value) -> Option<&str> {
    match get(site, "path") {
        Some(Value::String(path)) => Some(path),
        _ => None,
    }
}

/// The path a visible `use` binds `name` to at `at`. Imports in an inline
/// module do not leak into its parent or siblings.
fn binding(surface: &Value, blocks: &[Block], name: &[u8], at: usize) -> Option<Vec<Vec<u8>>> {
    seq(get(surface, "uses"))
        .iter()
        .rev()
        .filter(|site| reaches(blocks, position(site), at))
        .flat_map(|site| {
            site_path(site)
                .map(|path| imports(path.as_bytes()))
                .unwrap_or_default()
        })
        .find_map(|(segs, bound)| (bound == name).then_some(segs))
}

fn push_new(xs: &mut Vec<Value>, x: Value) {
    if !xs.contains(&x) {
        xs.push(x);
    }
}

/// `module_relative(path, root)`.
fn module_relative(path: &str, root: &str) -> String {
    let rel = scan::relative_path(path, root);
    if rel == "lib.rs" || rel == "mod.rs" {
        return String::new();
    }
    let rel = rel.strip_suffix("/mod.rs").unwrap_or(&rel);
    let rel = rel.strip_suffix(".rs").unwrap_or(rel);
    rel.replace('/', ".")
}

/// `resolve_mod(path, name, by_path)`.
fn resolve_mod(path: &str, name: &[u8], by_path: &HashMap<String, &SourceFile>) -> Option<String> {
    let base = scan::file_base(path)?;
    let mut dir = parent(path)?;
    if base != "lib.rs" && base != "main.rs" && base != "mod.rs" {
        let stem = base.strip_suffix(".rs").unwrap_or(&base);
        dir = format!("{dir}/{stem}");
    }
    let name = text::text(name);
    [format!("{dir}/{name}.rs"), format!("{dir}/{name}/mod.rs")]
        .into_iter()
        .find(|p| by_path.contains_key(p))
}

impl Scanner for RustScanner {
    fn accepts(&self, relative: &str, _opts: &Value) -> bool {
        relative.ends_with(".rs") || relative.ends_with("/Cargo.toml") || relative == "Cargo.toml"
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
            _ => "rust".to_owned(),
        };
        let ns_prefix = match get(opts, "ns-prefix") {
            Some(Value::String(s)) => s.clone(),
            _ => prefix.clone(),
        };
        let by_path: HashMap<String, &SourceFile> =
            files.iter().map(|f| (f.path.clone(), f)).collect();
        let mut surfaces: HashMap<String, Value> = HashMap::new();
        let surface = |path: &str,
                       surfaces: &mut HashMap<String, Value>,
                       by_path: &HashMap<String, &SourceFile>| {
            surfaces
                .entry(path.to_owned())
                .or_insert_with(|| read_module(&by_path[path].source))
                .clone()
        };
        let mut queue: Vec<(String, String)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut reachable: Vec<String> = Vec::new();
        let mut module_roots: HashMap<String, String> = HashMap::new();
        for n in ["lib.rs", "main.rs"] {
            let p = format!("{root}/{n}");
            if by_path.contains_key(&p) {
                queue.push((p.clone(), p));
            }
        }
        let mut at = 0;
        while at < queue.len() {
            let (path, crate_root) = queue[at].clone();
            at += 1;
            if seen.insert(path.clone()) {
                module_roots.insert(path.clone(), crate_root.clone());
                reachable.push(path.clone());
                for n in seq(get(&surface(&path, &mut surfaces, &by_path), "mods")) {
                    if let Value::String(n) = n
                        && let Some(child) = resolve_mod(&path, n.as_bytes(), &by_path)
                        && !seen.contains(&child)
                    {
                        queue.push((child, crate_root.clone()));
                    }
                }
            }
        }
        reachable.sort();
        let mut metas = Vec::new();
        let mut by_rel: Vec<(String, Value)> = Vec::new();
        let mut by_file: HashMap<&str, usize> = HashMap::new();
        let mut lib_id: Option<Value> = None;
        let name = lib_name(root, &by_path);
        let deps = dependencies(root, &by_path);
        for path in &reachable {
            let rel = module_relative(path, root);
            let ns = scan::ns_join(&ns_prefix, &rel);
            let id = scan::id_of(&ns, Some(&Value::String(prefix.clone())));
            by_file.insert(path.as_str(), metas.len());
            if !by_rel.iter().any(|(r, _)| *r == rel) {
                by_rel.push((rel.clone(), id.clone()));
            }
            if rel.is_empty() && lib_id.is_none() {
                lib_id = Some(id.clone());
            }
            metas.push((path.clone(), rel, ns, id));
        }
        // The deepest module on a crate-relative path: `a.b.C.f` names `a.b`.
        let suffix = |s: &str| -> Option<Value> {
            let mut s = s.trim_start_matches('.').trim_end_matches('.');
            while !s.is_empty() {
                if let Some((_, id)) = by_rel.iter().find(|(r, _)| r == s) {
                    return Some(id.clone());
                }
                s = s.rsplit_once('.').map_or("", |(parent, _)| parent);
            }
            None
        };
        let root_ids: HashMap<String, Value> = metas
            .iter()
            .filter(|m| module_roots.get(&m.0) == Some(&m.0))
            .map(|m| (m.0.clone(), m.3.clone()))
            .collect();
        let main_rs = format!("{root}/main.rs");
        let resolve = |segs: &[Vec<u8>], m: &(String, String, String, Value)| -> Option<Value> {
            let head = segs.first()?.clone();
            let tail: Vec<String> = segs[1..].iter().map(|s| text::text(s)).collect();
            let tail = tail.join(".");
            let crate_root = module_roots
                .get(&m.0)
                .and_then(|path| root_ids.get(path))
                .cloned();
            // `main.rs` is a crate root like `lib.rs`: `self` names the root.
            let relative = if m.0 == main_rs { "" } else { m.1.as_str() };
            match head.as_slice() {
                b"crate" => suffix(&tail).or(crate_root),
                b"self" => {
                    let found = suffix(&scan::ns_join(relative, &tail));
                    if relative.is_empty() {
                        found.or(crate_root)
                    } else {
                        found
                    }
                }
                b"super" => {
                    let mut base = relative.to_owned();
                    let mut next = 1;
                    loop {
                        if base.is_empty() {
                            return None;
                        }
                        base = base
                            .rsplit_once('.')
                            .map(|(p, _)| p)
                            .unwrap_or("")
                            .to_owned();
                        if segs.get(next).is_some_and(|s| s == b"super") {
                            next += 1;
                        } else {
                            break;
                        }
                    }
                    let tail: Vec<String> = segs[next..].iter().map(|s| text::text(s)).collect();
                    let found = suffix(&scan::ns_join(&base, &tail.join(".")));
                    if base.is_empty() {
                        found.or(crate_root)
                    } else {
                        found
                    }
                }
                _ => {
                    if Some(&head) == name.as_ref() {
                        suffix(&tail).or_else(|| lib_id.clone())
                    } else {
                        None
                    }
                }
            }
        };
        // A head naming a module declared in this file is a uniform path
        // (`mod a; use a::B;`); any other head is a crate when `external`.
        let target = |segs: &[Vec<u8>],
                      m: &(String, String, String, Value),
                      local: &[Value],
                      external: bool|
         -> Option<Target> {
            let head = segs.first()?;
            if matches!(head.as_slice(), b"crate" | b"self" | b"super")
                || Some(head) == name.as_ref()
            {
                resolve(segs, m).map(Target::Module)
            } else if local.contains(&Value::String(text::text(head))) {
                resolve(&[&[b"self".to_vec()], segs].concat(), m).map(Target::Module)
            } else if external {
                qualified(segs).map(Target::Foreign)
            } else {
                None
            }
        };
        let mut classes = Vec::new();
        let mut edges: Vec<Value> = Vec::new();
        let mut foreigns: Vec<Value> = Vec::new();
        let mut commands_map: Vec<(Value, Value)> = Vec::new();
        let project_ids: HashSet<String> = metas
            .iter()
            .map(|m| crate::value::encode_key(&m.3))
            .collect();
        // A path headed by a listed dependency names that crate, never a
        // module of this one; an id it shares with a module is dropped
        // rather than joined to the module.
        let collides = |id: &Value| {
            project_ids.contains(&crate::value::encode_key(id))
                && edn::record::local_name(id)
                    .and_then(|path| path.split('.').next())
                    .is_some_and(|head| deps.iter().any(|d| d.as_slice() == head.as_bytes()))
        };
        for m in &metas {
            let s = surface(&m.0, &mut surfaces, &by_path);
            let ops: Vec<Value> = seq(get(&s, "ops"))
                .iter()
                .map(|n| record(vec![("name", n.clone()), ("text", n.clone())]))
                .collect();
            let mut c = record(vec![
                ("id", m.3.clone()),
                ("ns", Value::String(m.2.clone())),
                (
                    "name",
                    Value::String(scan::file_base(&m.0).unwrap_or_default()),
                ),
                ("lang", Value::keyword("rust")),
                ("file", Value::String(scan::relative_path(&m.0, cwd))),
            ]);
            if let Some(name) = &name {
                edn::record::put(&mut c, "scan-crate", Value::String(text::text(name)));
            }
            if !ops.is_empty() {
                edn::record::put(&mut c, "ops", Value::Vector(ops));
            }
            classes.push(c);
            let mut requires: Vec<Value> = Vec::new();
            let mut externs: Vec<Value> = Vec::new();
            let mut impls: Vec<Value> = Vec::new();
            for n in seq(get(&s, "mods")) {
                if let Value::String(n) = n
                    && let Some(path) = resolve_mod(&m.0, n.as_bytes(), &by_path)
                {
                    let id = metas[by_file[path.as_str()]].3.clone();
                    if !requires.contains(&id) {
                        requires.push(id);
                    }
                }
            }
            let local = seq(get(&s, "mods")).to_vec();
            for u in seq(get(&s, "uses")) {
                let Some(path) = site_path(u) else {
                    continue;
                };
                for (segs, _) in imports(path.as_bytes()) {
                    match target(&segs, m, &local, true) {
                        Some(Target::Module(id)) => push_new(&mut requires, id),
                        Some(Target::Foreign(id)) if !collides(&id) => {
                            push_new(&mut foreigns, id.clone());
                            push_new(&mut externs, id);
                        }
                        _ => {}
                    }
                }
            }
            // `crate::a::f()` names a module of this crate without a `use`;
            // inside an inline mod, `self` and `super` read as a `use` would.
            let (text_masked, holes) = mask(&by_path[&m.0].source);
            let blocks = inline_mods(&text_masked, &holes);
            let heads = [b"crate".to_vec(), b"self".to_vec(), b"super".to_vec()];
            for head in heads.iter().chain(&name) {
                for (at, segs) in inline_paths(&text_masked, &holes, head) {
                    let depth = blocks.iter().filter(|b| b.holds(at)).count();
                    // The trailing `::` lets a bare `super` (`super::*`) climb.
                    let path = [segs.join(&b"::"[..]), b"::".to_vec()].concat();
                    if let Some(path) = out_of_inline_mods(&path, depth)
                        && let Some(id) = resolve(&paths(&path), m)
                    {
                        push_new(&mut requires, id);
                    }
                }
            }
            // A dependency is in scope without a `use`: `dep::item` names it,
            // unless a module declared here or a `use` binding the same name
            // (`use crate::journal as log;`) shadows it.
            for dep in &deps {
                if Some(dep) == name.as_ref() {
                    continue;
                }
                for (at, segs) in inline_paths(&text_masked, &holes, dep) {
                    let module = local.contains(&Value::String(text::text(dep)))
                        || blocks
                            .iter()
                            .any(|b| b.name == *dep && reaches(&blocks, b.at, at));
                    let imported =
                        binding(&s, &blocks, dep, at).is_some_and(|b| b.first() != Some(dep));
                    if module || imported {
                        continue;
                    }
                    if let Some(id) = qualified(&segs)
                        && !collides(&id)
                    {
                        push_new(&mut foreigns, id.clone());
                        push_new(&mut externs, id);
                    }
                }
            }
            for site in seq(get(&s, "impls")) {
                let Some(tr) = site_path(site) else {
                    continue;
                };
                let at = position(site);
                // `impl a::T` reads `a` through a `use` visible at the impl;
                // an unbound head is another crate only when listed.
                let segs = paths(tr.as_bytes());
                let bound = segs
                    .first()
                    .and_then(|head| binding(&s, &blocks, head, at))
                    .map(|b| [b, segs[1..].to_vec()].concat());
                let listed = segs.first().is_some_and(|head| deps.contains(head));
                let found = bound
                    .and_then(|b| target(&b, m, &local, true))
                    .or_else(|| target(&segs, m, &local, listed))
                    .or_else(|| {
                        let first = text::text(segs.first()?);
                        by_rel
                            .iter()
                            .find(|(r, _)| *r == first)
                            .map(|(_, id)| Target::Module(id.clone()))
                    });
                match found {
                    Some(Target::Module(id)) => push_new(&mut impls, id),
                    Some(Target::Foreign(id)) if !collides(&id) => {
                        push_new(&mut foreigns, id.clone());
                        push_new(&mut impls, id);
                    }
                    _ => {}
                }
            }
            for (group, kind) in [
                (&requires, "dependency"),
                (&externs, "dependency"),
                (&impls, "implements"),
            ] {
                for to in group {
                    if m.3 != *to {
                        let e = record(vec![
                            ("from", m.3.clone()),
                            ("to", to.clone()),
                            ("kind", Value::keyword(kind)),
                        ]);
                        if !edges.contains(&e) {
                            edges.push(e);
                        }
                    }
                }
            }
            let cmds = seq(get(&s, "commands"));
            if !cmds.is_empty() {
                commands_map.push((m.3.clone(), Value::Vector(cmds.to_vec())));
            }
        }
        // Stubs name a path into the crate (`egui.Context`); merging maps one
        // onto the deepest scanned module on that path.
        for id in foreigns {
            if !project_ids.contains(&crate::value::encode_key(&id)) {
                let mut c = scan::foreign_class(&id);
                edn::record::put(&mut c, "qualified", Value::Bool(true));
                if let Some(krate) =
                    edn::record::local_name(&id).and_then(|path| path.split('.').next())
                {
                    edn::record::put(&mut c, "scan-crate", Value::String(krate.to_owned()));
                }
                classes.push(c);
            }
        }
        Ok(record(vec![
            ("classes", Value::Vector(classes)),
            ("edges", Value::Vector(edges)),
            ("commands", Value::Map(commands_map)),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uses(source: &str) -> Vec<String> {
        seq(get(&read_module(source.as_bytes()), "uses"))
            .iter()
            .filter_map(|u| site_path(u).map(str::to_owned))
            .collect()
    }

    #[test]
    fn super_inside_an_inline_mod_names_the_enclosing_file() {
        let source = "use super::sibling;\n\
                      pub fn f() {}\n\
                      #[cfg(test)]\nmod tests {\n    use super::*;\n    \
                      mod deeper {\n        use super::super::x::Y;\n    }\n}\n\
                      use crate::after;\n";
        assert_eq!(
            vec!["super::sibling", "self::*", "self::x::Y", "crate::after"],
            uses(source)
        );
    }

    fn file(path: &str, source: &str) -> SourceFile {
        SourceFile {
            path: path.to_owned(),
            source: source.as_bytes().to_vec(),
        }
    }

    /// The edges of a scan of `files` rooted at `/p/src`: `from -> to` for a
    /// dependency, `from -|> to` for an implementation.
    fn edges(files: &[SourceFile]) -> Vec<String> {
        let opts = record(vec![("prefix", Value::String("app".to_owned()))]);
        let g = SCANNER.scan("/p/src", &opts, files, "/p").unwrap();
        seq(get(&g, "edges"))
            .iter()
            .map(|e| {
                let id = |k| crate::value::encode_key(get(e, k).unwrap());
                let arrow = match get(e, "kind") {
                    Some(k) if *k == Value::keyword("implements") => "-|>",
                    _ => "->",
                };
                format!("{} {arrow} {}", id("from"), id("to"))
            })
            .collect()
    }

    #[test]
    fn a_use_tree_imports_each_path_of_its_groups() {
        let tree = "crate::{\n    a,\n    b::{self, C as D, e::*},\n    f::G,\n}";
        let got: Vec<(String, String)> = imports(tree.as_bytes())
            .into_iter()
            .map(|(segs, name)| {
                let segs: Vec<String> = segs.iter().map(|s| text::text(s)).collect();
                (segs.join("::"), text::text(&name))
            })
            .collect();
        let want = [
            ("crate::a", "a"),
            ("crate::b", "b"),
            ("crate::b::C", "D"),
            ("crate::b::e::*", "*"),
            ("crate::f::G", "G"),
        ];
        assert_eq!(
            want.map(|(p, n)| (p.to_owned(), n.to_owned())).to_vec(),
            got
        );
    }

    #[test]
    fn a_grouped_use_depends_on_every_module_it_names() {
        let main = "mod a;\nmod b;\nmod t;\n\
                    use crate::{a, b::Thing};\nuse self::t::{Shape as Form};\n\
                    struct S;\nimpl Form for S {}\nfn main() {}\n";
        let files = [
            file("/p/src/main.rs", main),
            file("/p/src/a.rs", ""),
            file("/p/src/b.rs", "pub struct Thing;\n"),
            file("/p/src/t.rs", "pub trait Shape {}\n"),
        ];
        assert_eq!(
            vec![":main -> :a", ":main -> :b", ":main -> :t", ":main -|> :t"],
            edges(&files)
        );
    }

    #[test]
    fn super_inside_a_test_mod_names_the_file_not_its_parent() {
        let test_mod = |body: &str| {
            format!(
                "pub fn f() {{}}\n#[cfg(test)]\nmod tests {{\n    use super::*;\n    \
                 fn t() {{ {body} }}\n}}\n"
            )
        };
        let files = [
            file("/p/src/lib.rs", "mod a;\n"),
            file("/p/src/a.rs", "mod b;\nmod c;\npub fn g() {}\n"),
            file("/p/src/a/b.rs", &test_mod("super::f();")),
            file("/p/src/a/c.rs", &test_mod("super::super::g();")),
        ];
        // `super` inside `tests` is the file; only `c`'s file-level
        // `super::g` (written `super::super::g` there) reaches the parent.
        assert_eq!(
            vec![":a -> :a.b", ":a -> :a.c", ":a.c -> :a", ":app -> :a"],
            edges(&files)
        );
    }

    #[test]
    fn the_manifest_lists_the_dependencies_code_can_name() {
        let cargo = "[package]\nname = \"app\"\n\n\
                     [dependencies]\n\
                     edn = { path = \"../edn\" }\n\
                     serde-json = \"1\"\n\
                     eframe = { version = \"0.37\", features = [\n    \"wgpu\",\n] }\n\
                     tokio.workspace = true # inherited\n\n\
                     [dependencies.regex]\nversion = \"1\"\n\n\
                     [target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n\n\
                     [dev-dependencies]\nproptest = \"1\"\n\n\
                     [build-dependencies]\ncc = \"1\"\n\n\
                     [workspace.dependencies]\nanyhow = \"1\"\n";
        let files = [file("/p/Cargo.toml", cargo)];
        let by_path: HashMap<String, &SourceFile> =
            files.iter().map(|f| (f.path.clone(), f)).collect();
        let names: Vec<String> = dependencies("/p/src", &by_path)
            .iter()
            .map(|n| text::text(n))
            .collect();
        assert_eq!(
            vec!["edn", "serde_json", "eframe", "tokio", "regex", "libc"],
            names
        );
    }

    #[test]
    fn a_path_head_is_code_not_a_later_segment_or_a_literal() {
        let heads = |source: &str| -> Vec<String> {
            let (text, holes) = mask(source.as_bytes());
            inline_paths(&text, &holes, b"dep")
                .into_iter()
                .map(|(_, segs)| {
                    let segs: Vec<String> = segs.iter().map(|s| text::text(s)).collect();
                    segs.join("::")
                })
                .collect()
        };
        assert_eq!(vec!["dep::run"], heads("fn f() { dep::run(); }"));
        assert_eq!(vec!["dep::run"], heads("fn f() { ::dep::run(); }"));
        assert_eq!(
            vec!["dep::a::Value"],
            heads("fn f() -> dep::a::Value::<T> {}")
        );
        assert!(heads("fn f() { other::dep::run(); }").is_empty());
        assert!(heads("fn f() { mydep::run(); }").is_empty());
        assert!(heads("fn f() { let s = \"dep::run\"; }").is_empty());
        assert!(heads("// dep::run\nfn f() {}").is_empty());
    }

    #[test]
    fn a_dependency_named_inline_is_a_dependency_without_a_use() {
        let cargo = "[package]\nname = \"app\"\n\n[dependencies]\nviewer = \"1\"\nir = \"1\"\nlog = \"1\"\n";
        let main = "mod log;\n\
                    fn main() {\n    viewer::run();\n    let _ = \"ir::run\";\n    log::note();\n}\n";
        let files = [
            file("/p/Cargo.toml", cargo),
            file("/p/src/main.rs", main),
            file("/p/src/log.rs", "pub fn note() {}\n"),
        ];
        // A stub names the path into the other crate; merging maps it to the
        // deepest module scanned there.
        assert_eq!(vec![":main -> :log", ":main -> :viewer.run"], edges(&files));
    }

    #[test]
    fn a_use_binding_a_dependency_name_shadows_the_dependency() {
        let cargo = "[package]\nname = \"app\"\n\n[dependencies]\nlog = \"1\"\n";
        let main = "mod journal;\nuse crate::journal as log;\n\
                    fn main() {\n    log::note();\n}\n";
        let files = [
            file("/p/Cargo.toml", cargo),
            file("/p/src/main.rs", main),
            file("/p/src/journal.rs", "pub fn note() {}\n"),
        ];
        assert_eq!(vec![":main -> :journal"], edges(&files));
    }

    #[test]
    fn the_package_names_its_lib_when_the_manifest_has_no_lib_table() {
        let cargo = "[package]\nname = \"book-shelf\"\n";
        let files = [
            file("/p/Cargo.toml", cargo),
            file("/p/src/lib.rs", "pub mod shelf;\n"),
            file("/p/src/shelf.rs", "pub fn add() {}\n"),
            file(
                "/p/src/main.rs",
                "use book_shelf::shelf;\nfn main() { shelf::add(); }\n",
            ),
        ];
        assert_eq!(vec![":app -> :shelf", ":main -> :shelf"], edges(&files));
    }

    #[test]
    fn raw_strings_and_nested_comments_hide_inline_paths() {
        let source = r####"pub fn f() {
    let a = r#"say "hi crate::x::g"#;
    let b = br##"raw "# still raw crate::x::g"##;
    let p = r"C:\";
    let s = "crate::x::g";
}
/* outer /* inner */ crate::x::g() */
"####;
        let files = [
            file("/p/src/lib.rs", "mod x;\nmod y;\n"),
            file("/p/src/x.rs", ""),
            file("/p/src/y.rs", source),
        ];
        assert_eq!(vec![":app -> :x", ":app -> :y"], edges(&files));
    }

    #[test]
    fn every_leading_super_climbs_one_module() {
        let files = [
            file("/p/src/lib.rs", "mod a;\n"),
            file("/p/src/a.rs", "mod b;\nmod x;\n"),
            file("/p/src/a/x.rs", "pub fn g() {}\n"),
            file("/p/src/a/b.rs", "mod c;\n"),
            file("/p/src/a/b/c.rs", "fn f() { super::super::x::g(); }\n"),
        ];
        let got = edges(&files);
        assert!(got.contains(&":a.b.c -> :a.x".to_owned()), "{got:?}");
        assert!(!got.contains(&":a.b.c -> :a.b".to_owned()), "{got:?}");
    }

    #[test]
    fn an_inner_use_does_not_rebind_an_outer_impl() {
        let files = [
            file("/p/src/lib.rs", "mod x;\n"),
            file(
                "/p/src/x.rs",
                "pub trait Q {}\nstruct S;\nimpl Q for S {}\n\
                 #[cfg(test)] mod tests { use mockall::Q; }\n",
            ),
        ];
        let got = edges(&files);
        assert!(!got.iter().any(|e| e.contains("-|>")), "{got:?}");
    }

    #[test]
    fn a_foreign_stub_does_not_join_an_internal_crate_prefix() {
        let files = [
            file(
                "/p/Cargo.toml",
                "[package]\nname = \"app\"\n[dependencies]\nfoo = \"1\"\n",
            ),
            file("/p/src/lib.rs", "mod foo;\nmod x;\n"),
            file("/p/src/foo.rs", ""),
            file("/p/src/x.rs", "fn f() { foo::bar::f(); }\n"),
        ];
        let opts = record(vec![("prefix", Value::String("app".to_owned()))]);
        let scan = SCANNER.scan("/p/src", &opts, &files, "/p").unwrap();
        let merged = crate::graph::merge_scans(&[scan]).unwrap();
        let got = seq(get(&merged, "edges"));
        assert!(got.iter().any(|e| {
            get(e, "from") == Some(&Value::keyword("x"))
                && get(e, "to") == Some(&Value::keyword("foo.bar.f"))
        }));
        assert!(!got.iter().any(|e| {
            get(e, "from") == Some(&Value::keyword("x"))
                && get(e, "to") == Some(&Value::keyword("foo"))
        }));
        assert!(
            seq(get(&merged, "classes"))
                .iter()
                .all(|c| edn::record::get(c, "scan-crate").is_none())
        );
    }

    #[test]
    fn inline_modules_shadow_dependencies_and_count_after_attributes() {
        let cargo = "[package]\nname = \"app\"\n[dependencies]\nlog = \"1\"\n";
        let files = [
            file("/p/Cargo.toml", cargo),
            file(
                "/p/src/main.rs",
                "mod log { pub fn note() {} }\nfn main() { log::note(); }\n",
            ),
        ];
        assert!(edges(&files).is_empty());

        let files = [
            file("/p/src/lib.rs", "mod p;\n"),
            file("/p/src/p.rs", "mod a;\n"),
            file(
                "/p/src/p/a.rs",
                "pub fn h() {}\n#[cfg(test)] mod tests { fn t() { super::h(); } }\n",
            ),
        ];
        assert_eq!(vec![":app -> :p", ":p -> :p.a"], edges(&files));
    }

    #[test]
    fn a_method_turbofish_is_not_a_crate_path() {
        let files = [
            file(
                "/p/Cargo.toml",
                "[package]\nname = \"app\"\n[dependencies]\nlog = \"1\"\n",
            ),
            file(
                "/p/src/main.rs",
                "fn main() { let x = v.log::<u8>(); log::info::<u8>(\"x\"); }\n",
            ),
        ];
        assert_eq!(vec![":main -> :log.info"], edges(&files));
    }

    #[test]
    fn a_range_before_a_path_is_not_a_method_call() {
        let files = [
            file(
                "/p/Cargo.toml",
                "[package]\nname = \"app\"\n[dependencies]\nlog = \"1\"\n",
            ),
            file("/p/src/lib.rs", "mod x;\nmod y;\n"),
            file("/p/src/x.rs", "pub const N: u8 = 1;\n"),
            file(
                "/p/src/y.rs",
                "fn f() { for _ in 0..crate::x::N {} let _ = 0..log::MAX; let _ = 0.. log::MIN; }\n",
            ),
        ];
        assert_eq!(
            vec![
                ":app -> :x",
                ":app -> :y",
                ":y -> :x",
                ":y -> :log.MAX",
                ":y -> :log.MIN"
            ],
            edges(&files)
        );
    }

    #[test]
    fn a_dependency_path_never_lands_on_a_module_with_its_id() {
        let crate_files = |cargo: &str, x: &str| {
            [
                file("/p/Cargo.toml", cargo),
                file("/p/src/lib.rs", "mod foo;\nmod x;\n"),
                file("/p/src/foo.rs", "pub mod bar;\n"),
                file("/p/src/foo/bar.rs", "pub fn g() {}\n"),
                file("/p/src/x.rs", x),
            ]
        };
        let listed = "[package]\nname = \"app\"\n[dependencies]\nfoo = \"1\"\n";
        let x = "use foo::bar;\nstruct S;\nimpl foo::bar for S {}\n\
                 fn f() { foo::bar(); foo::bar::g(); }\n";
        assert_eq!(
            vec![
                ":foo -> :foo.bar",
                ":app -> :foo",
                ":app -> :x",
                ":x -> :foo.bar.g"
            ],
            edges(&crate_files(listed, x))
        );
        // An unlisted head still reads as a crate-relative path.
        let unlisted = "[package]\nname = \"app\"\n";
        assert_eq!(
            vec![
                ":foo -> :foo.bar",
                ":app -> :foo",
                ":app -> :x",
                ":x -> :foo.bar"
            ],
            edges(&crate_files(unlisted, "use foo::bar;\n"))
        );
    }

    #[test]
    fn root_items_resolve_in_lib_and_binary_crates() {
        let lib = [
            file("/p/src/lib.rs", "mod a;\npub fn run() {}\n"),
            file("/p/src/a.rs", "fn f() { crate::run(); }\n"),
        ];
        assert_eq!(vec![":a -> :app", ":app -> :a"], edges(&lib));
        let bin = [
            file("/p/src/main.rs", "mod a;\npub fn run() {}\n"),
            file("/p/src/a.rs", "fn f() { super::run(); crate::run(); }\n"),
        ];
        assert_eq!(vec![":a -> :main", ":main -> :a"], edges(&bin));
    }

    #[test]
    fn dependency_values_may_continue_on_later_lines() {
        let files = [
            file(
                "/p/Cargo.toml",
                "[dependencies]\nfoo = { version = \"1\",\n  default-features = false }\n\
                 bar = { version = \"1\", features = [\"}\",\n  \"x\"] }\n",
            ),
            file(
                "/p/src/main.rs",
                "fn main() { default_features::x(); foo::y(); bar::z(); }\n",
            ),
        ];
        assert_eq!(vec![":main -> :foo.y", ":main -> :bar.z"], edges(&files));
    }
}
