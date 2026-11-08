//! Jump to source (#141): the LanguageSource contract of `love/uml/source.lua`
//! with the four scanner languages' declaration patterns, and the source
//! window presentation of `application/source_view.lua` (tab display map,
//! token colors, byte-exact selections). Window-free; filesystem access is
//! supplied by the caller through [`Files`].
//!
//! Positions are 0-based byte offsets (the Lua ones were 1-based).

use crate::theme::{self, RowMarker};

/// What a class card asks to open: a module (no name) or one member.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ident {
    pub lang: Option<String>,
    pub file: Option<String>,
    pub ns: Option<String>,
    pub name: Option<String>,
}

/// Filesystem boundary: an optional project root for relative files.
pub struct Files<'a> {
    pub root: Option<String>,
    pub read: &'a dyn Fn(&str) -> Option<String>,
    pub is_file: &'a dyn Fn(&str) -> bool,
}

/// An opened source: the whole file and, for a member, its 1-based line.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub file: String,
    pub body: String,
    pub line: Option<usize>,
    pub lang: Option<String>,
    pub title: String,
}

// ---------------------------------------------------------------------------
// The Lua pattern subset the declaration patterns use: literals, `%s %w %a`,
// `%x` escapes, sets with ranges/negation, and the `+ * ?` quantifiers.

#[derive(Debug, Clone)]
enum Class {
    Lit(u8),
    Space,
    Alnum,
    Alpha,
    Set { negate: bool, items: Vec<SetItem> },
}

#[derive(Debug, Clone)]
enum SetItem {
    Range(u8, u8),
    Class(Class),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Quant {
    One,
    Plus,
    Star,
    Optional,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

impl Class {
    fn matches(&self, b: u8) -> bool {
        match self {
            Class::Lit(c) => *c == b,
            Class::Space => is_space(b),
            Class::Alnum => b.is_ascii_alphanumeric(),
            Class::Alpha => b.is_ascii_alphabetic(),
            Class::Set { negate, items } => {
                let hit = items.iter().any(|item| match item {
                    SetItem::Range(lo, hi) => (*lo..=*hi).contains(&b),
                    SetItem::Class(c) => c.matches(b),
                });
                hit != *negate
            }
        }
    }
}

fn escape_class(c: u8) -> Class {
    match c {
        b's' => Class::Space,
        b'w' => Class::Alnum,
        b'a' => Class::Alpha,
        other => Class::Lit(other),
    }
}

fn compile(pattern: &[u8]) -> Vec<(Class, Quant)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < pattern.len() {
        let class = match pattern[i] {
            b'%' if i + 1 < pattern.len() => {
                i += 2;
                escape_class(pattern[i - 1])
            }
            b'[' => {
                i += 1;
                let negate = pattern.get(i) == Some(&b'^');
                if negate {
                    i += 1;
                }
                let mut items = Vec::new();
                let mut first = true;
                while i < pattern.len() && (first || pattern[i] != b']') {
                    first = false;
                    if pattern[i] == b'%' && i + 1 < pattern.len() {
                        items.push(SetItem::Class(escape_class(pattern[i + 1])));
                        i += 2;
                    } else if i + 2 < pattern.len()
                        && pattern[i + 1] == b'-'
                        && pattern[i + 2] != b']'
                    {
                        items.push(SetItem::Range(pattern[i], pattern[i + 2]));
                        i += 3;
                    } else {
                        items.push(SetItem::Class(Class::Lit(pattern[i])));
                        i += 1;
                    }
                }
                i += 1; // the closing ]
                Class::Set { negate, items }
            }
            c => {
                i += 1;
                Class::Lit(c)
            }
        };
        let quant = match pattern.get(i) {
            Some(b'+') => Quant::Plus,
            Some(b'*') => Quant::Star,
            Some(b'?') => Quant::Optional,
            _ => Quant::One,
        };
        if quant != Quant::One {
            i += 1;
        }
        out.push((class, quant));
    }
    out
}

/// End of a match of `items` anchored at `pos` (greedy, backtracking).
fn match_here(items: &[(Class, Quant)], text: &[u8], pos: usize) -> Option<usize> {
    let Some(((class, quant), rest)) = items.split_first() else {
        return Some(pos);
    };
    let fits = |p: usize| p < text.len() && class.matches(text[p]);
    match quant {
        Quant::One => {
            if fits(pos) {
                match_here(rest, text, pos + 1)
            } else {
                None
            }
        }
        Quant::Optional => {
            if fits(pos)
                && let Some(end) = match_here(rest, text, pos + 1)
            {
                return Some(end);
            }
            match_here(rest, text, pos)
        }
        Quant::Plus | Quant::Star => {
            let mut n = 0;
            while fits(pos + n) {
                n += 1;
            }
            let least = usize::from(*quant == Quant::Plus);
            (least..=n)
                .rev()
                .find_map(|k| match_here(rest, text, pos + k))
        }
    }
}

/// `string.find(text, pattern, from)`: first match at or after `from`, as
/// (start, exclusive end).
fn find(text: &[u8], items: &[(Class, Quant)], from: usize) -> Option<(usize, usize)> {
    (from..=text.len()).find_map(|start| match_here(items, text, start).map(|end| (start, end)))
}

fn is_word(b: Option<u8>) -> bool {
    b.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Escape every non-alphanumeric byte (`gsub('([^%w])','%%%1')`).
fn quote(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push('%');
                // Non-ASCII bytes stay bytes: rebuild them below.
                out.push(b as char);
            }
        }
    }
    out
}

/// Earliest declaration start of `name` by pattern-group priority.
pub fn pattern_start(text: &str, name: &str, groups: &[Vec<String>]) -> Option<usize> {
    let bytes = text.as_bytes();
    let quoted: Vec<u8> = {
        // `quote` stores each escaped byte as a char < 256; map back to bytes.
        quote(name).chars().map(|c| c as u32 as u8).collect()
    };
    for group in groups {
        let mut best: Option<usize> = None;
        for fmt in group {
            let mut pattern = Vec::new();
            for &b in fmt.as_bytes() {
                if b == b'@' {
                    pattern.extend_from_slice(&quoted);
                } else {
                    pattern.push(b);
                }
            }
            let items = compile(&pattern);
            let mut from = 0;
            while from <= bytes.len() {
                let Some((first, end)) = find(bytes, &items, from) else {
                    break;
                };
                // Java regex \b compares both sides, including non-word names.
                let last = end.checked_sub(1).map(|i| bytes[i]);
                if is_word(last) != is_word(bytes.get(end).copied()) {
                    if best.is_none_or(|b| first < b) {
                        best = Some(first);
                    }
                    break;
                }
                from = first + 1;
            }
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

pub fn line_number(text: &str, start: usize) -> usize {
    text.as_bytes()[..start]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

fn trim_end(s: &str) -> String {
    s.trim_end_matches(|c: char| c.is_ascii() && is_space(c as u8))
        .to_string()
}

pub fn declaration_line(text: &str, start: usize) -> String {
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    trim_end(&text[start..end])
}

/// A file-backed language: declaration pattern groups by priority.
#[derive(Debug, Clone)]
pub struct FileSource {
    groups: Vec<Vec<String>>,
    /// Rust: a declaration may start at its attribute line.
    attribute: bool,
}

fn absolute(path: &str) -> bool {
    path.starts_with('/')
}

impl FileSource {
    pub fn locate(&self, ident: &Ident, files: &Files) -> Option<String> {
        let path = ident.file.as_deref().filter(|p| !p.is_empty())?;
        let mut path = path.to_owned();
        if let Some(root) = &files.root
            && !absolute(&path)
        {
            path = format!("{}/{path}", root.trim_end_matches('/'));
        }
        (files.is_file)(&path).then_some(path)
    }

    pub fn extract(&self, text: &str, ident: &Ident) -> Option<String> {
        let name = ident.name.as_deref()?;
        let first = pattern_start(text, name, &self.groups)?;
        if self.attribute {
            let name_at = text[first..].find(name).map_or(first, |i| first + i);
            let end = text[name_at..]
                .find('\n')
                .map_or(text.len(), |i| name_at + i);
            return Some(trim_end(&text[first..end]));
        }
        Some(declaration_line(text, first))
    }

    pub fn start_line(&self, text: &str, ident: &Ident) -> Option<usize> {
        let name = ident.name.as_deref()?;
        pattern_start(text, name, &self.groups).map(|first| line_number(text, first))
    }

    pub fn title(&self, ident: &Ident) -> String {
        let base = ident
            .file
            .clone()
            .or_else(|| ident.ns.clone())
            .unwrap_or_default();
        match &ident.name {
            Some(name) => format!("{base}/{name}"),
            None => base,
        }
    }
}

fn groups(items: &[&[&str]]) -> Vec<Vec<String>> {
    items
        .iter()
        .map(|g| g.iter().map(|s| s.to_string()).collect())
        .collect()
}

fn rust_source() -> FileSource {
    let public = ["pub%s+", "pub%([^)]*%)%s+"];
    let attribute = "#%[%s*tauri%s*::%s*command[^%]]*%]%s*";
    let kinds = ["struct", "enum", "trait", "type", "const", "static"];
    let mut attributed = vec![
        format!("{attribute}fn%s+@"),
        format!("{attribute}async%s+fn%s+@"),
    ];
    let (mut async_public, mut fn_public, mut types_public) = (vec![], vec![], vec![]);
    for prefix in public {
        attributed.push(format!("{attribute}{prefix}fn%s+@"));
        attributed.push(format!("{attribute}{prefix}async%s+fn%s+@"));
        async_public.push(format!("{prefix}async%s+fn%s+@"));
        fn_public.push(format!("{prefix}fn%s+@"));
        for kind in kinds {
            types_public.push(format!("{prefix}{kind}%s+@"));
        }
    }
    let types = kinds.iter().map(|k| format!("{k}%s+@")).collect();
    FileSource {
        groups: vec![
            attributed,
            async_public,
            fn_public,
            vec!["async%s+fn%s+@".into()],
            vec!["fn%s+@".into()],
            types_public,
            types,
        ],
        attribute: true,
    }
}

/// The registered language adapters (`main/source.lua`).
pub fn lookup(lang: &str) -> Option<FileSource> {
    Some(match lang {
        "lua" => FileSource {
            groups: groups(&[
                &["function%s+@:[A-Za-z_][%w_]*", "function%s+@"],
                &[
                    "local%s+function%s+@:[A-Za-z_][%w_]*",
                    "local%s+function%s+@",
                ],
                &["@%s*=%s*function"],
                &["local%s+@%s*=%s*function"],
            ]),
            attribute: false,
        },
        "python" => FileSource {
            groups: groups(&[&["async%s+def%s+@"], &["def%s+@"], &["class%s+@"]]),
            attribute: false,
        },
        "typescript" => FileSource {
            groups: groups(&[
                &["export%s+default%s+async%s+function%s+@"],
                &["export%s+async%s+function%s+@"],
                &["export%s+default%s+function%s+@"],
                &["export%s+function%s+@"],
                &["export%s+default%s+class%s+@"],
                &["export%s+class%s+@"],
                &["export%s+const%s+@"],
                &["export%s+let%s+@"],
                &["export%s+var%s+@"],
                &["export%s+enum%s+@"],
                &["async%s+function%s+@"],
                &["function%s+@"],
                &["class%s+@"],
                &["const%s+@", "let%s+@", "var%s+@"],
            ]),
            attribute: false,
        },
        "rust" => rust_source(),
        _ => return None,
    })
}

/// Open a module (no name: the whole file, no line) or a member (its
/// declaration line). None when the language, file or member is unknown.
pub fn member_source(ident: &Ident, files: &Files) -> Option<Opened> {
    let lang = ident.lang.as_deref()?;
    let language = lookup(lang)?;
    let path = language.locate(ident, files)?;
    let body = (files.read)(&path)?;
    let named = ident.name.as_deref().is_some_and(|n| !n.is_empty());
    if named && language.extract(&body, ident).is_none() {
        return None;
    }
    let line = if named {
        language.start_line(&body, ident)
    } else {
        None
    };
    // The title names the file as the project does: relative to its root.
    let shown = files
        .root
        .as_deref()
        .and_then(|r| std::path::Path::new(&path).strip_prefix(r).ok())
        .map_or_else(|| path.clone(), |p| p.to_string_lossy().into_owned());
    let title = if named {
        format!("{shown}:{}", line.unwrap_or(1))
    } else {
        shown
    };
    Some(Opened {
        file: path,
        body,
        line,
        lang: Some(lang.to_string()),
        title,
    })
}

// ---------------------------------------------------------------------------
// Presentation.

/// One numbered source row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub number: usize,
    pub text: String,
    pub marker: Option<RowMarker>,
}

/// Split a body into rows (CR trimmed); a trailing newline leaves an empty
/// last row, as the LOVE window shows.
pub fn rows(body: &str, line: Option<usize>) -> Vec<Row> {
    body.split('\n')
        .enumerate()
        .map(|(i, text)| Row {
            number: i + 1,
            text: text.strip_suffix('\r').unwrap_or(text).to_string(),
            marker: (Some(i + 1) == line).then(theme::row_marker),
        })
        .collect()
}

fn char_size(b: u8) -> usize {
    if b < 128 {
        1
    } else if b < 224 {
        2
    } else if b < 240 {
        3
    } else {
        4
    }
}

/// Tab-expanded display text with byte maps both ways. Selections stay in
/// source bytes; only painting and hit testing use display bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    pub text: String,
    /// Display byte → source byte (length = display length + 1).
    pub to_source: Vec<usize>,
    /// Source byte → display byte at char boundaries (length = source + 1).
    pub to_display: Vec<usize>,
}

pub fn display(text: &str) -> Display {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut to_source = Vec::with_capacity(text.len() + 1);
    let mut to_display = vec![0; text.len() + 1];
    let mut i = 0;
    while i < bytes.len() {
        let n = char_size(bytes[i]).min(bytes.len() - i);
        to_display[i] = out.len();
        let painted = if bytes[i] == b'\t' {
            "  "
        } else {
            &text[i..i + n]
        };
        for _ in 0..painted.len() {
            to_source.push(i);
        }
        out.push_str(painted);
        i += n;
    }
    to_source.push(i);
    to_display[i] = out.len();
    Display {
        text: out,
        to_source,
        to_display,
    }
}

/// A colored run of one display line.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    /// `keyword`, `string`, `comment` or `ink`.
    pub kind: &'static str,
}

const KEYWORDS: &str = "and break do else elseif end false for function if in local nil not or repeat return then true until while class def elif except finally from import lambda None pass raise try with yield async await as const export extends interface let new public private static this throw var void fn impl match mod mut pub self struct trait use enum where loop move ref unsafe";

pub fn tokens(text: &str, lang: &str) -> Vec<Run> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut emit = |from: usize, to: usize, kind: &'static str| {
        out.push(Run {
            text: String::from_utf8_lossy(&bytes[from..to]).into_owned(),
            kind,
        })
    };
    while i < bytes.len() {
        let rest = &bytes[i..];
        let ch = bytes[i];
        let comment = (lang == "lua" && rest.starts_with(b"--"))
            || (lang == "python" && ch == b'#')
            || ((lang == "rust" || lang == "typescript") && rest.starts_with(b"//"));
        if comment {
            emit(i, bytes.len(), "comment");
            i = bytes.len();
        } else if matches!(ch, b'"' | b'\'' | b'`') {
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b'\\' {
                    j += 2;
                } else if bytes[j] == ch {
                    j += 1;
                    break;
                } else {
                    j += 1;
                }
            }
            let end = j.min(bytes.len());
            emit(i, end, "string");
            i = end;
        } else if ch.is_ascii_alphabetic() || ch == b'_' || ch >= 128 {
            let mut j = i;
            while j < bytes.len() {
                let b = bytes[j];
                if !(b.is_ascii_alphanumeric() || b == b'_') && b < 128 {
                    break;
                }
                j += char_size(b);
            }
            let j = j.min(bytes.len());
            let word = &text[i..j];
            let kind = if KEYWORDS.split(' ').any(|k| k == word) {
                "keyword"
            } else {
                "ink"
            };
            emit(i, j, kind);
            i = j;
        } else {
            emit(i, i + 1, "ink");
            i += 1;
        }
    }
    out
}

/// A caret: 0-based row and source byte column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

pub fn ordered(a: Pos, b: Pos) -> (Pos, Pos) {
    if a.row > b.row || (a.row == b.row && a.col > b.col) {
        (b, a)
    } else {
        (a, b)
    }
}

/// The selected source text between two carets, either direction.
pub fn selected(rows: &[String], a: Option<Pos>, b: Option<Pos>) -> String {
    let (Some(a), Some(b)) = (a, b) else {
        return String::new();
    };
    let (a, b) = ordered(a, b);
    let mut out = Vec::new();
    for (i, text) in rows
        .iter()
        .enumerate()
        .take(b.row.min(rows.len().saturating_sub(1)) + 1)
        .skip(a.row)
    {
        let from = if i == a.row { a.col.min(text.len()) } else { 0 };
        let to = if i == b.row {
            b.col.min(text.len())
        } else {
            text.len()
        };
        out.push(text.get(from..to.max(from)).unwrap_or_default());
    }
    out.join("\n")
}
