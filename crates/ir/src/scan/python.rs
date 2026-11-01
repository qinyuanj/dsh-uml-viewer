//! Port of `love/uml/python_language/graph_python.lua`: the JVM Python
//! scanner semantics over caller-owned canonical source snapshots.

use edn::Value;

use crate::scan::{self, Scanner, SourceFile};
use crate::text::{self, at, is_alpha_, is_space, is_word};
use crate::value::get_truthy as get;
use edn::record::{record, seq};

fn ident(c: Option<u8>) -> bool {
    c.is_some_and(is_word)
}

fn in_hole(holes: &[(usize, usize)], i: usize) -> bool {
    holes.iter().any(|&(a, b)| i >= a && i < b)
}

fn trim(s: &[u8]) -> &[u8] {
    text::trim(s)
}

/// `flags(s, i)`: string-prefix flags for a quote at 1-based `i`;
/// `(raw, f)` or `None` when `i` is no quote. A `t` (template, PEP 750)
/// string interpolates like an `f` one.
fn flags(s: &[u8], i: usize) -> Option<(bool, bool)> {
    let q = at(s, i)?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let mut raw = false;
    let mut f = false;
    let mut j = i as i64 - 1;
    while j > 0
        && matches!(at(s, j as usize), Some(c) if matches!(c, b'r' | b'R' | b'u' | b'U' | b'b' | b'B' | b'f' | b'F' | b't' | b'T'))
    {
        let c = at(s, j as usize).unwrap().to_ascii_lowercase();
        raw = raw || c == b'r';
        f = f || c == b'f' || c == b't';
        j -= 1;
    }
    if j == 0 || !ident(at(s, j as usize)) {
        return Some((raw, f));
    }
    // Outside a string every quote opens one. A word glued to it that is
    // not all prefix letters is a name or keyword before a plain string:
    // `return"x"`, `x or"y"`, `if"z" in w`.
    Some((false, false))
}

/// `skip_string(s, i)`: one past the closing quote(s), or `#s + 1`.
fn skip_string(s: &[u8], i: usize) -> Option<usize> {
    let (raw, f) = flags(s, i)?;
    let q = at(s, i).unwrap();
    let len = if at(s, i + 1) == Some(q) && at(s, i + 2) == Some(q) {
        3
    } else {
        1
    };
    let mut j = i + len;
    let mut depth = 0i64;
    while j <= s.len() {
        let c = at(s, j).unwrap();
        let n = at(s, j + 1);
        if c == b'\\' && j < s.len() && (!raw || n == Some(q)) {
            j += 2;
        } else if f && (c == b'{' || c == b'}') {
            if n == Some(c) {
                // `{{`/`}}` are literal braces.
                j += 2;
            } else if c == b'{' {
                depth += 1;
                j += 1;
            } else if depth > 0 {
                depth -= 1;
                j += 1;
            } else {
                j += 1;
            }
        } else if depth > 0 && (c == b'"' || c == b'\'') {
            j = skip_string(s, j).unwrap_or(j + 1);
        } else if depth == 0 && (1..=len).all(|k| at(s, j + k - 1) == Some(q)) {
            return Some(j + len);
        } else {
            j += 1;
        }
    }
    Some(s.len() + 1)
}

/// `mask(s)`: `#` comments blanked; strings kept with holes.
fn mask(s: &[u8]) -> (Vec<u8>, Vec<(usize, usize)>) {
    let mut out = Vec::new();
    let mut holes = Vec::new();
    let mut i = 1;
    while i <= s.len() {
        if at(s, i) == Some(b'#') {
            let j = text::find_plain(s, b"\n", i).unwrap_or(s.len() + 1);
            out.extend(std::iter::repeat_n(b' ', j - i));
            i = j;
        } else if let Some(j) = skip_string(s, i) {
            out.extend_from_slice(&s[i - 1..j - 1]);
            holes.push((i, j));
            i = j;
        } else {
            out.push(at(s, i).unwrap());
            i += 1;
        }
    }
    (out, holes)
}

/// Length of the backslash line continuation at 1-based `i`: `\` then `\n`
/// (2) or `\r\n` (3); 0 when there is none.
fn continuation(s: &[u8], i: usize) -> usize {
    match (at(s, i), at(s, i + 1), at(s, i + 2)) {
        (Some(b'\\'), Some(b'\n'), _) => 2,
        (Some(b'\\'), Some(b'\r'), Some(b'\n')) => 3,
        _ => 0,
    }
}

/// `statement_end(s, i, import)`: position of the statement-ending newline
/// (`#s + 1` at end of input). An import holds no `;`, so its first `;`
/// ends it too (`import a; import b`, `from a import f; f("(")`), before
/// any bracket after it can carry the scan past the line.
fn statement_end(s: &[u8], mut i: usize, import: bool) -> usize {
    let (mut p, mut b, mut c) = (0i64, 0i64, 0i64);
    while i <= s.len() {
        let x = at(s, i).unwrap();
        if continuation(s, i) > 0 {
            i += continuation(s, i);
        } else if (x == b'\n' && p == 0 && b == 0 && c == 0) || (import && x == b';') {
            return i;
        } else {
            match x {
                b'(' => p += 1,
                b')' => p = (p - 1).max(0),
                b'[' => b += 1,
                b']' => b = (b - 1).max(0),
                b'{' => c += 1,
                b'}' => c = (c - 1).max(0),
                _ => {}
            }
            i += 1;
        }
    }
    s.len() + 1
}

/// `^([%a_][%w_%.]*)%s+as%s+([%a_][%w_]*)$` or `^([%a_][%w_%.]*)$`.
fn parse_part(part: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let dotted = |s: &[u8]| -> Option<usize> {
        if !s.first().is_some_and(|&c| is_alpha_(c)) {
            return None;
        }
        Some(s.iter().take_while(|&&c| is_word(c) || c == b'.').count())
    };
    let plain = |s: &[u8]| -> Option<usize> {
        if !s.first().is_some_and(|&c| is_alpha_(c)) {
            return None;
        }
        Some(s.iter().take_while(|&&c| is_word(c)).count())
    };
    if let Some(len) = dotted(part) {
        let rest = &part[len..];
        let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
        if spaces > 0
            && let Some(after) = rest[spaces..].strip_prefix(b"as")
        {
            let spaces2 = after.iter().take_while(|&&c| is_space(c)).count();
            if spaces2 > 0 {
                let rest2 = &after[spaces2..];
                if let Some(blen) = plain(rest2)
                    && blen == rest2.len()
                {
                    return Some((part[..len].to_vec(), Some(rest2[..blen].to_vec())));
                }
            }
        }
        if len == part.len() {
            return Some((part[..len].to_vec(), None));
        }
    }
    None
}

/// `parts(s)`: comma-split import names with `as` bindings.
fn parts(s: &[u8]) -> Vec<Value> {
    let mut out = Vec::new();
    let cleaned: Vec<u8> = s
        .iter()
        .map(|&c| if c == b'(' || c == b')' { b' ' } else { c })
        .collect();
    for part in cleaned.split(|&c| c == b',') {
        let part = trim(part);
        if part.is_empty() {
            continue;
        }
        if let Some((name, bind)) = parse_part(part) {
            // `alias`: the bind is an explicit `as`, not the first segment.
            let alias = bind.is_some();
            let bind = bind.unwrap_or_else(|| {
                let first = name.iter().take_while(|&&c| c != b'.').count();
                name[..first].to_vec()
            });
            out.push(record(vec![
                ("name", Value::String(text::text(&name))),
                ("bind", Value::String(text::text(&bind))),
                ("alias", Value::Bool(alias)),
            ]));
        }
    }
    out
}

fn strip_spaces(s: &[u8]) -> &[u8] {
    &s[s.iter().take_while(|&&c| is_space(c)).count()..]
}

/// `s:gsub('\\\r?\n',' '):gsub('%s+',' ')`: each run of whitespace and
/// backslash continuations becomes one space.
fn collapse(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 1;
    while i <= s.len() {
        let skip = continuation(s, i);
        if skip > 0 || is_space(s[i - 1]) {
            if out.last() != Some(&b' ') {
                out.push(b' ');
            }
        } else {
            out.push(s[i - 1]);
        }
        i += skip.max(1);
    }
    out
}

/// `parse_import(s)`: the statement text without its trailing newline.
fn parse_import(s: &[u8]) -> Option<Value> {
    let collapsed = collapse(s);
    let s = trim(&collapsed);
    if s.starts_with(b"from ") {
        // `^from%s+([^ ]*)%s+import%s+(.+)$` — whitespace already collapsed.
        let rest = &s[5..];
        let path_end = rest.iter().position(|&c| c == b' ')?;
        let path = &rest[..path_end];
        let rest = strip_spaces(&rest[path_end..]);
        let rest = rest.strip_prefix(b"import")?;
        let names = strip_spaces(rest);
        if names.is_empty() {
            return None;
        }
        let dots = path.iter().take_while(|&&c| c == b'.').count();
        let module = &path[dots..];
        let valid = module.is_empty()
            || (module.first().is_some_and(|&c| is_alpha_(c))
                && module.iter().all(|&c| is_word(c) || c == b'.'));
        if valid {
            return Some(record(vec![
                ("kind", Value::keyword("from")),
                ("dots", Value::Integer(dots as i64)),
                (
                    "module",
                    if module.is_empty() {
                        Value::Nil
                    } else {
                        Value::String(text::text(module))
                    },
                ),
                ("names", Value::Vector(parts(names))),
            ]));
        }
        return None;
    }
    if s.starts_with(b"import ") {
        return Some(record(vec![
            ("kind", Value::keyword("import")),
            ("modules", Value::Vector(parts(&s[7..]))),
        ]));
    }
    None
}

/// `read_module(source)`: imports and top-level defs.
pub fn read_module(source: &[u8]) -> Value {
    // A UTF-8 BOM is not text: line 1 starts after it.
    let source = source.strip_prefix(b"\xef\xbb\xbf").unwrap_or(source);
    let (text, holes) = mask(source);
    let mut imports = Vec::new();
    let mut defs = Vec::new();
    let mut i = 1;
    while i <= text.len() {
        // `text:sub(i):match('^(%a+)')`
        let word_len = text[i - 1..]
            .iter()
            .take_while(|&&c| c.is_ascii_alphabetic())
            .count();
        let word = &text[i - 1..i - 1 + word_len];
        if (word == b"from" || word == b"import")
            && !ident(at(&text, i.wrapping_sub(1)))
            && !ident(at(&text, i + word_len))
            && !in_hole(&holes, i)
            && statement_is_clean(&text, i)
        {
            let finish = statement_end(&text, i, true);
            if let Some(imp) = parse_import(&text[i - 1..finish - 1]) {
                imports.push(imp);
            }
            i = finish;
        } else {
            i += 1;
        }
    }
    let mut i = 1;
    while i <= text.len() {
        let line_len = text[i - 1..]
            .iter()
            .position(|&c| c == b'\n')
            .unwrap_or(text.len() - i + 1);
        let line = &text[i - 1..i - 1 + line_len];
        let name = match_def_name(line, b"async", true)
            .or_else(|| match_def_name(line, b"def", false))
            .or_else(|| match_def_name(line, b"class", false));
        if let Some(name) = name
            && name.first() != Some(&b'_')
            && !in_hole(&holes, i)
        {
            // A `\` continuation is whitespace: `class X(A, \` then `B):`
            // keeps `B`.
            let end = statement_end(&text, i, false);
            let header = collapse(&text[i - 1..end - 1]);
            if line.starts_with(b"class") {
                let body = match_class_bases(&header);
                let mut bases = Vec::new();
                if let Some(body) = &body {
                    for part in top_level_parts(body) {
                        if let Some(base) = base_name(trim(part)) {
                            bases.push(Value::String(text::text(base)));
                        }
                    }
                }
                defs.push(record(vec![
                    ("name", Value::String(text::text(&name))),
                    ("kind", Value::keyword("class")),
                    (
                        "bases",
                        body.map(|_| Value::Vector(bases)).unwrap_or(Value::Nil),
                    ),
                ]));
            } else {
                defs.push(record(vec![
                    ("name", Value::String(text::text(&name))),
                    ("kind", Value::keyword("function")),
                    (
                        "return",
                        match_return(&header)
                            .map(Value::String)
                            .unwrap_or(Value::Nil),
                    ),
                ]));
            }
        }
        i += line_len + 1;
    }
    record(vec![
        ("imports", Value::Vector(imports)),
        ("defs", Value::Vector(defs)),
    ])
}

/// The statement-before check: `([^\n;:]*)$` then `^%s*$`. A `:` ends a
/// compound header, so `try: import x` and `if TYPE_CHECKING: from a import
/// B` count; no expression may hold `import`/`from` after a `:`.
fn statement_is_clean(text: &[u8], i: usize) -> bool {
    let before = &text[..i - 1];
    let start = before
        .iter()
        .rposition(|&c| c == b'\n' || c == b';' || c == b':')
        .map_or(0, |p| p + 1);
    before[start..].iter().all(|&c| is_space(c))
}

/// `^async[ \t]+def[ \t]+(name)` (async_def) or `^def`/`^class` + name.
fn match_def_name(line: &[u8], word: &[u8], async_def: bool) -> Option<Vec<u8>> {
    let rest = line.strip_prefix(word)?;
    let spaces = rest
        .iter()
        .take_while(|&&c| c == b' ' || c == b'\t')
        .count();
    if spaces == 0 {
        return None;
    }
    let rest = &rest[spaces..];
    let rest = if async_def {
        let rest = rest.strip_prefix(b"def")?;
        let spaces = rest
            .iter()
            .take_while(|&&c| c == b' ' || c == b'\t')
            .count();
        if spaces == 0 {
            return None;
        }
        &rest[spaces..]
    } else {
        rest
    };
    if !rest.first().is_some_and(|&c| is_alpha_(c)) {
        return None;
    }
    let len = rest.iter().take_while(|&&c| is_word(c)).count();
    Some(rest[..len].to_vec())
}

/// `%((.-)%)%s*:` on the collapsed header, the `)` matched by depth:
/// `class A(B, metaclass=make())` keeps `B`.
fn match_class_bases(header: &[u8]) -> Option<Vec<u8>> {
    let open = header.iter().position(|&c| c == b'(')?;
    let mut depth = 0i64;
    let close = header[open..]
        .iter()
        .position(|&c| {
            match c {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            depth == 0
        })
        .map(|p| open + p)?;
    let rest = strip_spaces(&header[close + 1..]);
    if rest.first() == Some(&b':') {
        Some(header[open + 1..close].to_vec())
    } else {
        None
    }
}

/// The bases body split at depth-0 commas: `Mapping[str, int], Base` is
/// two parts.
fn top_level_parts(body: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i64, 0);
    for (k, &c) in body.iter().enumerate() {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&body[start..k]);
                start = k + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

/// A base part's `^[%a_][%w_%.]*$` name. A subscript is dropped
/// (`Repository[User]` is `Repository`, `Protocol[T]` is `Protocol`), but
/// `Generic[T]` only declares type parameters: no base. Of the keywords only
/// `metaclass=ABCMeta` names one, the marker of an abstract class.
fn base_name(part: &[u8]) -> Option<&[u8]> {
    let len = dotted_len(part)?;
    let (name, rest) = part.split_at(len);
    let rest = strip_spaces(rest);
    // Only a leading `name =` is a keyword; an `=` in a subscript is not
    // (`User[Literal["a=b"]]`, `User[Annotated[int, Field(gt=0)]]`).
    if let Some(value) = rest.strip_prefix(b"=") {
        // `^metaclass%s*=%s*([%a_][%w_%.]*)$` naming `ABCMeta`.
        let value = trim(value);
        let abc = dotted_len(value) == Some(value.len()) && name_tail(value) == b"ABCMeta";
        return (name == b"metaclass" && abc).then_some(value);
    }
    if rest.is_empty() {
        return Some(name);
    }
    let subscript = rest.first() == Some(&b'[') && rest.last() == Some(&b']');
    (subscript && name_tail(name) != b"Generic").then_some(name)
}

/// `^[%a_][%w_%.]*` length.
fn dotted_len(s: &[u8]) -> Option<usize> {
    s.first()
        .is_some_and(|&c| is_alpha_(c))
        .then(|| s.iter().take_while(|&&c| is_word(c) || c == b'.').count())
}

/// The last dotted segment: `typing.Generic` is `Generic`.
fn name_tail(name: &[u8]) -> &[u8] {
    name.rsplit(|&c| c == b'.').next().unwrap_or(name)
}

/// `%)%s*%->%s*([%a_][%w_]*)%s*:` on the collapsed header.
fn match_return(header: &[u8]) -> Option<String> {
    let mut search = 0;
    while let Some(off) = header[search.min(header.len())..]
        .iter()
        .position(|&c| c == b')')
    {
        let p = search + off;
        let rest = strip_spaces(&header[p + 1..]);
        if let Some(rest) = rest.strip_prefix(b"->") {
            let rest = strip_spaces(rest);
            if rest.first().is_some_and(|&c| is_alpha_(c)) {
                let len = rest.iter().take_while(|&&c| is_word(c)).count();
                let after = strip_spaces(&rest[len..]);
                if after.first() == Some(&b':') {
                    return Some(text::text(&rest[..len]));
                }
            }
        }
        search = p + 1;
    }
    None
}

const SKIP_DIRS: &[&str] = &[
    "__pycache__",
    "venv",
    "site-packages",
    "dist",
    "build",
    "node_modules",
    "tests",
    "__pypackages__",
];

/// `accepts_directory(relative)`.
fn accepts_directory(relative: &str) -> bool {
    for dir in relative.split('/') {
        if dir.is_empty() {
            continue;
        }
        if dir.starts_with('.') || SKIP_DIRS.contains(&dir) {
            return false;
        }
    }
    true
}

/// `accepts(relative)`.
fn accepts(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or("");
    if !name.ends_with(".py")
        || name == "conftest.py"
        || is_test_file(name, "test_", ".py")
        || is_test_file(name, "", "_test.py")
        || is_test_file(name, "", "_tests.py")
    {
        return false;
    }
    let dir = relative.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    accepts_directory(dir)
}

/// `^test_.+%.py$` / `^.+_test%.py$` / `^.+_tests%.py$` shape checks.
fn is_test_file(name: &str, prefix: &str, suffix: &str) -> bool {
    name.len() > prefix.len() + suffix.len() && name.starts_with(prefix) && name.ends_with(suffix)
}

fn parent(s: &str) -> Option<&str> {
    s.rsplit_once('.').map(|(p, _)| p)
}

fn join(a: &str, b: &str) -> String {
    if b.is_empty() {
        return a.to_owned();
    }
    if a.is_empty() {
        return b.to_owned();
    }
    format!("{a}.{b}")
}

/// `marker(s)`: Protocol/ABC/ABCMeta as the last dotted segment.
fn marker(s: &str) -> bool {
    let n = s.rsplit('.').next().unwrap_or(s);
    n == "Protocol" || n == "ABC" || n == "ABCMeta"
}

pub struct PythonScanner;

pub static SCANNER: PythonScanner = PythonScanner;

/// A resolved import hit.
enum Hit {
    Id {
        id: Value,
        bind: Option<String>,
        path: Option<String>,
    },
    /// A name bound with no dependency: a package on the path of `import
    /// a.b.c` (`a`, `a.b`).
    Bind {
        bind: String,
        id: Value,
    },
    Foreign(String),
}

impl Scanner for PythonScanner {
    fn accepts(&self, relative: &str, _opts: &Value) -> bool {
        accepts(relative)
    }

    fn accepts_directory(&self, relative: &str, _opts: &Value) -> bool {
        accepts_directory(relative)
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
        let mut sorted: Vec<&SourceFile> = files
            .iter()
            .filter(|f| accepts(&scan::relative_path(&f.path, root)))
            .collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        struct Meta {
            file_path: String,
            source: Vec<u8>,
            ns: String,
            id: Value,
            init: bool,
        }
        let mut metas = Vec::new();
        let mut by_ns: Vec<(String, Value)> = Vec::new();
        let mut by_rel: Vec<(String, Value)> = Vec::new();
        for f in &sorted {
            let mut rel = scan::relative_path(&f.path, root);
            rel = rel.strip_suffix(".py").unwrap_or(&rel).to_owned();
            if rel == "__init__" {
                rel = String::new();
            } else if let Some(stripped) = rel.strip_suffix("/__init__") {
                rel = stripped.to_owned();
            }
            let rel = rel.replace('/', ".");
            let ns = if rel == ns_prefix || rel.starts_with(&format!("{ns_prefix}.")) {
                rel.clone()
            } else {
                scan::ns_join(&ns_prefix, &rel)
            };
            let id = scan::id_of(&ns, Some(&Value::String(prefix.clone())));
            let init = f.path.ends_with("/__init__.py");
            by_ns.push((ns.clone(), id.clone()));
            if !rel.is_empty() {
                by_rel.push((rel, id.clone()));
            }
            metas.push(Meta {
                file_path: f.path.clone(),
                source: f.source.clone(),
                ns,
                id,
                init,
            });
        }
        let exact = |s: &str| -> Option<Value> {
            if s.is_empty() {
                return None;
            }
            by_ns
                .iter()
                .find(|(n, _)| n == s)
                .or_else(|| by_rel.iter().find(|(n, _)| n == s))
                .map(|(_, id)| id.clone())
        };
        let mut classes = Vec::new();
        let mut edges: Vec<Value> = Vec::new();
        let mut foreigns: Vec<Value> = Vec::new();
        let project: std::collections::HashSet<String> = metas
            .iter()
            .map(|m| match &m.id {
                Value::Keyword(n) => n.clone(),
                _ => String::new(),
            })
            .collect();
        for m in &metas {
            let surface = read_module(&m.source);
            let mut hits: Vec<Hit> = Vec::new();
            let package = if m.init {
                m.ns.clone()
            } else {
                parent(&m.ns).unwrap_or("").to_owned()
            };
            for imp in seq(get(&surface, "imports")) {
                if get(imp, "kind") == Some(&Value::keyword("import")) {
                    for part in seq(get(imp, "modules")) {
                        let n = match get(part, "name") {
                            Some(Value::String(s)) => s.clone(),
                            _ => continue,
                        };
                        let bind = match get(part, "bind") {
                            Some(Value::String(s)) => Some(s.clone()),
                            _ => None,
                        };
                        // `import a.b.c` binds `a`, not `a.b.c`, and loads
                        // `a.b` too: each package on the path names its own
                        // `X` (`a.X`, `a.b.X`); `a.b.c.X` resolves by path.
                        // `import a.b as a` binds `a.b` alone.
                        let packages: Vec<String> = if get(part, "alias").is_some() {
                            Vec::new()
                        } else {
                            n.match_indices('.')
                                .map(|(k, _)| n[..k].to_owned())
                                .collect()
                        };
                        match exact(&n) {
                            Some(id) => hits.push(Hit::Id {
                                id,
                                bind: if packages.is_empty() { bind } else { None },
                                path: Some(n),
                            }),
                            None => hits.push(Hit::Foreign(n)),
                        }
                        for package in packages {
                            if let Some(id) = exact(&package) {
                                hits.push(Hit::Bind { bind: package, id });
                            }
                        }
                    }
                } else {
                    let dots = match get(imp, "dots") {
                        Some(Value::Integer(d)) => *d as usize,
                        _ => 0,
                    };
                    let module = match get(imp, "module") {
                        Some(Value::String(s)) => Some(s.clone()),
                        _ => None,
                    };
                    let mut rootmod = module.clone();
                    if dots > 0 {
                        let mut rm = Some(package.clone());
                        let mut left = dots - 1;
                        while left > 0 {
                            match rm.as_deref() {
                                Some("") | None => {
                                    rm = None;
                                    break;
                                }
                                Some(current) => {
                                    rm = Some(parent(current).unwrap_or("").to_owned());
                                }
                            }
                            left -= 1;
                        }
                        rootmod = rm.map(|rm| join(&rm, module.as_deref().unwrap_or("")));
                    }
                    if let Some(rootmod) = rootmod {
                        let relative_only = dots > 0 && module.as_deref().is_none_or(str::is_empty);
                        let rootid = exact(&rootmod);
                        let mut found = false;
                        for part in seq(get(imp, "names")) {
                            let name = match get(part, "name") {
                                Some(Value::String(s)) => s.clone(),
                                _ => continue,
                            };
                            let bind = match get(part, "bind") {
                                Some(Value::String(s)) => Some(s.clone()),
                                _ => None,
                            };
                            // `from . import db` names the package's own `db`, like
                            // `from app import db` does.
                            let id = exact(&join(&rootmod, &name)).or_else(|| rootid.clone());
                            if let Some(id) = id {
                                hits.push(Hit::Id {
                                    id,
                                    bind,
                                    path: None,
                                });
                                found = true;
                            }
                        }
                        if !found {
                            if let Some(rootid) = rootid {
                                hits.push(Hit::Id {
                                    id: rootid,
                                    bind: None,
                                    path: None,
                                });
                            } else if !rootmod.is_empty() && !relative_only {
                                // A package with no `__init__.py` is no module
                                // and no foreign library either.
                                hits.push(Hit::Foreign(rootmod));
                            }
                        }
                    }
                }
            }
            let mut bindings: Vec<(String, Value)> = Vec::new();
            let mut paths: Vec<(String, Value)> = Vec::new();
            let mut requires: Vec<Value> = Vec::new();
            let mut external: Vec<Value> = Vec::new();
            for h in &hits {
                match h {
                    Hit::Id { id, bind, path } => {
                        if let Some(bind) = bind {
                            bindings.retain(|(k, _)| k != bind);
                            bindings.push((bind.clone(), id.clone()));
                        }
                        if let Some(path) = path {
                            paths.retain(|(k, _)| k != path);
                            paths.push((path.clone(), id.clone()));
                        }
                        if !requires.contains(id) {
                            requires.push(id.clone());
                        }
                    }
                    Hit::Bind { bind, id } => {
                        bindings.retain(|(k, _)| k != bind);
                        bindings.push((bind.clone(), id.clone()));
                    }
                    Hit::Foreign(name) => {
                        let foreign = Value::keyword(name);
                        if !external.contains(&foreign) {
                            external.push(foreign.clone());
                        }
                        if !foreigns.contains(&foreign) {
                            foreigns.push(foreign);
                        }
                    }
                }
            }
            let target = |expr: &str| -> Option<Value> {
                if let Some((_, id)) = bindings.iter().find(|(k, _)| k == expr) {
                    return Some(id.clone());
                }
                if let Some((_, id)) = paths.iter().find(|(k, _)| k == expr) {
                    return Some(id.clone());
                }
                let mut p = parent(expr);
                while let Some(current) = p {
                    if let Some((_, id)) = paths.iter().find(|(k, _)| k == current) {
                        return Some(id.clone());
                    }
                    if let Some((_, id)) = bindings.iter().find(|(k, _)| k == current) {
                        return Some(id.clone());
                    }
                    p = parent(current);
                }
                None
            };
            let mut inherits: Vec<Value> = Vec::new();
            let mut impls: Vec<Value> = Vec::new();
            let mut ops: Vec<Value> = Vec::new();
            let mut class_count = 0;
            let mut interface = true;
            for d in seq(get(&surface, "defs")) {
                let n = get(d, "name").cloned().unwrap_or(Value::Nil);
                ops.push(record(vec![("name", n.clone()), ("text", n)]));
                if get(d, "kind") == Some(&Value::keyword("function")) {
                    interface = false;
                } else {
                    class_count += 1;
                    let bases = seq(get(d, "bases"));
                    if bases.is_empty() {
                        interface = false;
                    }
                    for base in bases {
                        if let Value::String(base) = base
                            && !marker(base)
                        {
                            interface = false;
                        }
                    }
                }
                for base in seq(get(d, "bases")) {
                    if let Value::String(base) = base
                        && let Some(id) = target(base)
                    {
                        let group = if marker(base) {
                            &mut impls
                        } else {
                            &mut inherits
                        };
                        if !group.contains(&id) {
                            group.push(id);
                        }
                    }
                }
            }
            for d in seq(get(&surface, "defs")) {
                if let Some(Value::String(ret)) = get(d, "return")
                    && let Some((_, id)) = bindings.iter().find(|(k, _)| k == ret)
                    && !impls.contains(id)
                {
                    impls.push(id.clone());
                }
            }
            let mut c = record(vec![
                ("id", m.id.clone()),
                ("ns", Value::String(m.ns.clone())),
                ("name", Value::String(scan::module_name(&m.id))),
                ("lang", Value::keyword("python")),
                (
                    "file",
                    Value::String(scan::relative_path(&m.file_path, cwd)),
                ),
            ]);
            if !ops.is_empty() {
                edn::record::put(&mut c, "ops", Value::Vector(ops));
            }
            if class_count > 0 && interface {
                edn::record::put(&mut c, "stereotype", Value::keyword("interface"));
            }
            classes.push(c);
            for (group, kind) in [
                (&impls, "implements"),
                (&inherits, "inheritance"),
                (&requires, "dependency"),
                (&external, "dependency"),
            ] {
                for to in group {
                    if m.id != *to {
                        let e = record(vec![
                            ("from", m.id.clone()),
                            ("to", to.clone()),
                            ("kind", Value::keyword(kind)),
                        ]);
                        if !edges.contains(&e) {
                            edges.push(e);
                        }
                    }
                }
            }
        }
        for id in foreigns {
            let name = match &id {
                Value::Keyword(n) => n.clone(),
                _ => String::new(),
            };
            if !project.contains(&name) {
                classes.push(scan::foreign_class(&id));
            }
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
    use crate::value::id_name;

    /// Scans `files` (paths under `src/`, prefix `app`): each interface as
    /// `id <<interface>>`, then each edge as `from -> to kind`.
    fn graph(files: &[(&str, &str)]) -> Vec<String> {
        let files: Vec<SourceFile> = files
            .iter()
            .map(|(path, source)| SourceFile {
                path: format!("/p/src/{path}"),
                source: source.as_bytes().to_vec(),
            })
            .collect();
        let opts = record(vec![("prefix", Value::String("app".into()))]);
        let g = SCANNER.scan("/p/src", &opts, &files, "/p").unwrap();
        let name = |m: &Value, k: &str| id_name(get(m, k).unwrap());
        let interfaces = seq(get(&g, "classes"))
            .iter()
            .filter(|c| get(c, "stereotype").is_some())
            .map(|c| format!("{} <<interface>>", name(c, "id")));
        let edges = seq(get(&g, "edges")).iter().map(|e| {
            format!(
                "{} -> {} {}",
                name(e, "from"),
                name(e, "to"),
                name(e, "kind")
            )
        });
        interfaces.chain(edges).collect()
    }

    fn defs(source: &str) -> Vec<String> {
        seq(get(&read_module(source.as_bytes()), "defs"))
            .iter()
            .filter_map(|d| match get(d, "name") {
                Some(Value::String(n)) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_subscripted_base_is_inherited() {
        let files = [
            ("app/base.py", "class Repository:\n    pass\n"),
            (
                "app/kw.py",
                "from app.base import Repository\n\
                 class Kw(Repository, metaclass=make_meta()):\n    pass\n",
            ),
            ("app/models.py", "class User:\n    pass\n"),
            (
                "app/qualified.py",
                "import app.base\nclass Ints(app.base.Repository[int]):\n    pass\n",
            ),
            (
                "app/repo.py",
                "from app.base import Repository\nfrom app.models import User\n\
                 class UserRepo(Repository[User]):\n    pass\n",
            ),
            (
                "app/wrapped.py",
                "from app.base import Repository\n\
                 class Wrapped(\n    Repository[\n        dict[str, int],\n    ],\n):\n    pass\n",
            ),
        ];
        assert_eq!(
            vec![
                "kw -> base inheritance",
                "kw -> base dependency",
                "qualified -> base inheritance",
                "qualified -> base dependency",
                "repo -> base inheritance",
                "repo -> base dependency",
                "repo -> models dependency",
                "wrapped -> base inheritance",
                "wrapped -> base dependency",
            ],
            graph(&files)
        );
    }

    #[test]
    fn a_generic_protocol_or_abc_metaclass_marks_an_interface() {
        let files = [
            (
                "app/box.py",
                "from typing import Generic\nclass Box(Generic[T]):\n    pass\n",
            ),
            (
                "app/port.py",
                "from abc import ABCMeta\nclass Port(metaclass=ABCMeta):\n    pass\n",
            ),
            (
                "app/reader.py",
                "from typing import Protocol\nclass Reader(Protocol[T]):\n    def read(self) -> T: ...\n",
            ),
            (
                "app/store.py",
                "from abc import ABC\nfrom typing import Generic\n\
                 class Store(ABC, Generic[K, V]):\n    pass\n",
            ),
        ];
        assert_eq!(
            vec![
                "port <<interface>>",
                "reader <<interface>>",
                "store <<interface>>",
                "box -> typing dependency",
                "port -> abc dependency",
                "reader -> typing dependency",
                "store -> abc dependency",
                "store -> typing dependency",
            ],
            graph(&files)
        );
    }

    #[test]
    fn a_relative_import_of_a_package_name_depends_on_the_package() {
        let files = [
            ("app/__init__.py", "db = object()\n"),
            ("app/models.py", "from . import db\n"),
            ("app/sub/__init__.py", ""),
            ("app/sub/deep.py", "from .. import db\n"),
            ("app/sub/sibling.py", "from . import deep\n"),
            ("app/views.py", "from . import *\n"),
        ];
        assert_eq!(
            vec![
                "models -> app dependency",
                "sub.deep -> app dependency",
                "sub.sibling -> sub.deep dependency",
                "views -> app dependency",
            ],
            graph(&files)
        );
    }

    #[test]
    fn a_dotted_import_binds_its_top_package() {
        let files = [
            ("app/__init__.py", "class Entity:\n    pass\n"),
            ("app/models.py", "class User:\n    pass\n"),
            (
                "app/views.py",
                "import app.models\nclass Page(app.Entity):\n    pass\n\
                 class Form(app.models.User):\n    pass\n",
            ),
        ];
        assert_eq!(
            vec![
                "views -> app inheritance",
                "views -> models inheritance",
                "views -> models dependency",
            ],
            graph(&files)
        );
    }

    #[test]
    fn an_import_after_a_compound_statement_colon_counts() {
        let files = [
            (
                "app/compat.py",
                "try: import app.util\nexcept ImportError: import app.models\n",
            ),
            (
                "app/hints.py",
                "from typing import TYPE_CHECKING\n\
                 if TYPE_CHECKING: from app.models import User\n",
            ),
            ("app/models.py", ""),
            (
                "app/plain.py",
                "\"\"\"Note: import app.util\n\"\"\"\n\
                 d = {\"k\": \"import app.util\"}\nf = lambda: 0\n\
                 s = 'x: import app.models'  # y: import app.util\n\
                 raise ValueError() from None\n",
            ),
            ("app/util.py", ""),
        ];
        assert_eq!(
            vec![
                "compat -> util dependency",
                "compat -> models dependency",
                "hints -> models dependency",
                "hints -> typing dependency",
            ],
            graph(&files)
        );
    }

    #[test]
    fn a_semicolon_ends_an_import_statement() {
        let files = [
            ("app/both.py", "import app.util; import app.models\n"),
            (
                "app/later.py",
                "from app.util import helper; x = \"(\"\nimport app.models\n",
            ),
            ("app/models.py", ""),
            ("app/util.py", ""),
        ];
        assert_eq!(
            vec![
                "both -> util dependency",
                "both -> models dependency",
                "later -> util dependency",
                "later -> models dependency",
            ],
            graph(&files)
        );
    }

    #[test]
    fn an_import_scan_stops_at_its_semicolon() {
        // Each `(` after a `;` stays open to the end of input: an import scan
        // that passes the `;` rescans the rest of the file once per line.
        let source = "import app.util; s = \"(\"\n".repeat(8000);
        let started = std::time::Instant::now();
        let surface = read_module(source.as_bytes());
        let elapsed = started.elapsed();
        assert_eq!(8000, seq(get(&surface, "imports")).len());
        // Instrumentation and shared mutation workers measure host load.
        let instrumented = cfg!(coverage)
            || std::env::var_os("CARGO_TARGET_DIR").is_some_and(|dir| {
                std::path::Path::new(&dir)
                    .components()
                    .any(|c| c.as_os_str() == "mutation-workers")
            });
        if !instrumented {
            assert!(
                elapsed < std::time::Duration::from_secs(3),
                "8000 lines took {elapsed:?}"
            );
        }
    }

    #[test]
    fn crlf_continuations_and_a_utf8_bom_keep_their_imports() {
        let files = [
            ("app/bom.py", "\u{feff}import app.models\n"),
            ("app/crlf.py", "from app.models \\\r\n    import User\r\n"),
            ("app/models.py", "class User:\r\n    pass\r\n"),
        ];
        assert_eq!(
            vec!["bom -> models dependency", "crlf -> models dependency"],
            graph(&files)
        );
        assert_eq!(vec!["Bom"], defs("\u{feff}class Bom:\n    pass\n"));
    }

    #[test]
    fn a_template_string_or_one_glued_to_a_keyword_is_no_import() {
        let files = [
            ("app/__init__.py", "class Entity:\n    pass\n"),
            (
                "app/glued.py",
                "def f(x):\n    if\"a: import app.util, b\" in x: return\"Note: import app.util, x\"\n\
                 \x20   y = 1 if x else\"Else: import app.util, z\"\n\
                 \x20   return x or\"Or: import app.util, y\"\n",
            ),
            (
                "app/hint.py",
                "msg = t\"Hint: import app.util, then retry\"\n\
                 raw = rt\"Raw: import app.util, x\"\nup = T\"Up: {x:>4} import app.util, x\"\n\
                 semi = t\"a; import app.util, b\"\n",
            ),
            ("app/models.py", ""),
            (
                "app/usage.py",
                "USAGE = t\"\"\"\nExample: import requests\n\"\"\"\nimport app.models\n",
            ),
            ("app/util.py", ""),
        ];
        assert_eq!(vec!["usage -> models dependency"], graph(&files));
    }

    #[test]
    fn an_explicit_alias_keeps_its_module() {
        let files = [
            ("app/__init__.py", "class Entity:\n    pass\n"),
            ("app/models.py", "class User:\n    pass\n"),
            (
                "app/x.py",
                "import app.models as app\nclass X(app.User):\n    pass\n\
                 def make() -> app:\n    pass\n",
            ),
        ];
        assert_eq!(
            vec![
                "x -> models implements",
                "x -> models inheritance",
                "x -> models dependency",
            ],
            graph(&files)
        );
    }

    #[test]
    fn a_dotted_import_binds_each_package_on_its_path() {
        let files = [
            ("app/__init__.py", "class Entity:\n    pass\n"),
            ("app/a/__init__.py", "class Y:\n    pass\n"),
            ("app/a/b.py", "class W:\n    pass\n"),
            (
                "app/x.py",
                "import app.a.b\nclass E(app.Entity):\n    pass\n\
                 class X(app.a.Y):\n    pass\nclass Z(app.a.b.W):\n    pass\n",
            ),
        ];
        assert_eq!(
            vec![
                "x -> app inheritance",
                "x -> a inheritance",
                "x -> a.b inheritance",
                "x -> a.b dependency",
            ],
            graph(&files)
        );
    }

    #[test]
    fn a_keyword_in_a_subscript_or_a_continued_header_keeps_its_bases() {
        let files = [
            ("app/__init__.py", "class Entity:\n    pass\n"),
            (
                "app/ann.py",
                "from app.models import User\n\
                 class A(User[Annotated[int, Field(gt=0)]]):\n    pass\n",
            ),
            (
                "app/cont.py",
                "from app import Entity\nfrom app.models import User\n\
                 class X(Entity, \\\n        User):\n    pass\n",
            ),
            (
                "app/crlf.py",
                "from app import Entity\r\nfrom app.models import User\r\n\
                 class X(Entity, \\\r\n        User):\r\n    pass\r\n",
            ),
            (
                "app/lit.py",
                "from app.models import User\nclass A(User[Literal[\"a=b\"]]):\n    pass\n",
            ),
            ("app/models.py", "class User:\n    pass\n"),
            (
                "app/port.py",
                "import abc\nclass Port(metaclass = abc.ABCMeta):\n    pass\n",
            ),
        ];
        assert_eq!(
            vec![
                "port <<interface>>",
                "ann -> models inheritance",
                "ann -> models dependency",
                "cont -> app inheritance",
                "cont -> models inheritance",
                "cont -> app dependency",
                "cont -> models dependency",
                "crlf -> app inheritance",
                "crlf -> models inheritance",
                "crlf -> app dependency",
                "crlf -> models dependency",
                "lit -> models inheritance",
                "lit -> models dependency",
                "port -> abc dependency",
            ],
            graph(&files)
        );
    }
}
