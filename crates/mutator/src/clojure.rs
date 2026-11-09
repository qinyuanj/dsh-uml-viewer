//! Clojure mutation sites: the original's clj-mutate rules over a custom reader.
//!
//! The pinned `sites.py` walks the tree-sitter `clojure` grammar (sogaiu, as
//! bundled by tree-sitter-language-pack 1.21.0). This port does not link that
//! grammar: a hand-written reader rebuilds the parts of its tree the rules
//! observe. Those are the node kinds and spans of `sym_lit` (metadata
//! included), `bool_lit` and `num_lit`, which collection a node sits in
//! directly, and every enclosing node kind. It follows the grammar's token
//! rules (a number prefix wins at its own longest match, `true`/`false`/`nil`
//! only at exact length, `#_` is a gap whose form is still walked) and its
//! error recovery. Collections still open at end of file flatten into one
//! `ERROR` node, a stray closer is an `ERROR` of its own, and a closer that
//! belongs to an enclosing collection closes the inner ones as if their
//! closer were present.

/// One syntax node: its grammar kind and UTF-8 byte span.
#[derive(Debug)]
struct Node {
    kind: &'static str,
    start: usize,
    end: usize,
    children: Vec<Node>,
}

impl Node {
    fn leaf(kind: &'static str, start: usize, end: usize) -> Self {
        Node {
            kind,
            start,
            end,
            children: Vec::new(),
        }
    }
}

/// `WHITESPACE_CHAR`: Java whitespace and the comma.
fn whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{c}'
            | '\n'
            | '\r'
            | '\t'
            | ','
            | ' '
            | '\u{b}'
            | '\u{1c}'..='\u{1f}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200a}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// Whitespace without the comma, which the token classes list separately.
fn blank(ch: char) -> bool {
    ch != ',' && whitespace(ch)
}

/// `SYMBOL_HEAD`.
fn symbol_head(ch: char) -> bool {
    !blank(ch) && !"/()[]{}\"@~^;`\\,:#'0123456789".contains(ch)
}

/// `SYMBOL_BODY`.
fn symbol_body(ch: char) -> bool {
    symbol_head(ch) || ":#'0123456789".contains(ch)
}

/// `SYMBOL_NAMESPACED_NAME`'s characters.
fn symbol_name(ch: char) -> bool {
    symbol_head(ch) || "/:#'0123456789".contains(ch)
}

/// `KEYWORD_HEAD`.
fn keyword_head(ch: char) -> bool {
    !blank(ch) && !"()[]{}\"@~^;`\\,:/".contains(ch)
}

fn digits(bytes: &[u8], at: usize) -> usize {
    bytes[at..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count()
}

/// `NUMBER`'s longest match at the start of `text`, sign included.
fn number(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let sign = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let lead = digits(bytes, sign);
    if lead == 0 {
        return None;
    }
    let at = |index: usize| bytes.get(index).copied();
    let mut best = sign + lead;
    // INTEGER: digits, then M or N.
    if matches!(at(sign + lead), Some(b'M' | b'N')) {
        best = best.max(sign + lead + 1);
    }
    // DOUBLE: digits, an optional fraction, an optional exponent, then M.
    let mut end = sign + lead;
    if at(end) == Some(b'.') {
        end += 1 + digits(bytes, end + 1);
    }
    if matches!(at(end), Some(b'e' | b'E')) {
        let signed = usize::from(matches!(at(end + 1), Some(b'+' | b'-')));
        let exponent = digits(bytes, end + 1 + signed);
        if exponent > 0 {
            end += 1 + signed + exponent;
        }
    }
    if at(end) == Some(b'M') {
        end += 1;
    }
    best = best.max(end);
    // RATIO: digits / digits.
    if at(sign + lead) == Some(b'/') {
        let denominator = digits(bytes, sign + lead + 1);
        if denominator > 0 {
            best = best.max(sign + lead + 1 + denominator);
        }
    }
    // RADIX: digits r alphanumerics.
    if matches!(at(sign + lead), Some(b'r' | b'R')) {
        let body = bytes[sign + lead + 1..]
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        if body > 0 {
            best = best.max(sign + lead + 1 + body);
        }
    }
    if at(sign) == Some(b'0') {
        // HEX: 0x digits, then N.
        if matches!(at(sign + 1), Some(b'x' | b'X')) {
            let hex = bytes[sign + 2..]
                .iter()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            if hex > 0 {
                let end = sign + 2 + hex;
                best = best.max(end + usize::from(at(end) == Some(b'N')));
            }
        }
        // OCTAL: 0 octal digits, then N.
        let octal = bytes[sign + 1..]
            .iter()
            .take_while(|b| (b'0'..=b'7').contains(b))
            .count();
        if octal > 0 {
            let end = sign + 1 + octal;
            best = best.max(end + usize::from(at(end) == Some(b'N')));
        }
    }
    Some(best)
}

/// `CHARACTER` after its backslash: the longest of an octal, named, unicode
/// or single character.
fn character(text: &str) -> Option<usize> {
    let first = text.chars().next()?;
    let mut best = first.len_utf8();
    let bytes = text.as_bytes();
    if first == 'o' {
        let count = digits(bytes, 1).min(3);
        best = best.max(1 + count);
    }
    for name in ["backspace", "formfeed", "newline", "return", "space", "tab"] {
        if text.starts_with(name) {
            best = best.max(name.len());
        }
    }
    if first == 'u' && bytes.len() >= 5 && bytes[1..5].iter().all(u8::is_ascii_hexdigit) {
        best = best.max(5);
    }
    Some(best)
}

/// What a form produced: complete nodes, or the flattened contents of a
/// collection still open at end of file.
enum Parsed {
    Done(Vec<Node>),
    Open(Vec<Node>),
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

const META_TARGETS: &[&str] = &[
    "sym_lit",
    "list_lit",
    "map_lit",
    "vec_lit",
    "set_lit",
    "anon_fn_lit",
    "read_cond_lit",
    "splicing_read_cond_lit",
    "ns_map_lit",
    "var_quoting_lit",
    "evaling_lit",
    "tagged_or_ctor_lit",
    "derefing_lit",
    "quoting_lit",
    "syn_quoting_lit",
    "unquote_splicing_lit",
    "unquoting_lit",
];

impl<'a> Reader<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek().filter(|ch| whitespace(*ch)) {
            self.at += ch.len_utf8();
        }
    }
    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> usize {
        let start = self.at;
        while let Some(ch) = self.peek().filter(|ch| keep(*ch)) {
            self.at += ch.len_utf8();
        }
        self.at - start
    }

    /// `_gap`: whitespace, a comment or a discarded form. None when the next
    /// character starts a form or a closer.
    fn gap(&mut self, closers: &[char]) -> Option<Parsed> {
        self.skip_whitespace();
        let rest = self.rest();
        if rest.starts_with(';') || rest.starts_with("#!") {
            let start = self.at;
            self.at += rest.find('\n').map_or(rest.len(), |at| at + 1);
            return Some(Parsed::Done(vec![Node::leaf("comment", start, self.at)]));
        }
        if rest.starts_with("#_") {
            let start = self.at;
            self.at += 2;
            let (children, open) = self.prefixed(closers);
            let node = Node {
                kind: "dis_expr",
                start,
                end: self.at,
                children,
            };
            return Some(if open {
                Parsed::Open(flatten(node))
            } else {
                Parsed::Done(vec![node])
            });
        }
        None
    }

    /// Gaps, then one form: the body of every prefix reader macro. The flag
    /// is true when a collection inside was still open at end of file.
    fn prefixed(&mut self, closers: &[char]) -> (Vec<Node>, bool) {
        let mut children = Vec::new();
        loop {
            match self.gap(closers) {
                Some(Parsed::Done(nodes)) => children.extend(nodes),
                Some(Parsed::Open(nodes)) => {
                    children.extend(nodes);
                    return (children, true);
                }
                None => break,
            }
        }
        match self.peek() {
            // The form is missing: the grammar inserts an empty one.
            None => (children, false),
            Some(ch) if ")]}".contains(ch) => (children, false),
            Some(_) => match self.form(closers) {
                Parsed::Done(nodes) => {
                    children.extend(nodes);
                    (children, false)
                }
                Parsed::Open(nodes) => {
                    children.extend(nodes);
                    (children, true)
                }
            },
        }
    }

    /// The items of a collection up to `close`. `closers` are the closers of
    /// the enclosing collections.
    fn items(&mut self, close: char, closers: &[char]) -> Parsed {
        let mut inner = closers.to_vec();
        inner.push(close);
        let mut children = Vec::new();
        loop {
            match self.gap(&inner) {
                Some(Parsed::Done(nodes)) => {
                    children.extend(nodes);
                    continue;
                }
                Some(Parsed::Open(nodes)) => {
                    children.extend(nodes);
                    return Parsed::Open(children);
                }
                None => {}
            }
            let Some(ch) = self.peek() else {
                return Parsed::Open(children);
            };
            if ch == close {
                self.at += 1;
                return Parsed::Done(children);
            }
            if ")]}".contains(ch) {
                if closers.contains(&ch) {
                    // An enclosing collection's closer: this one is closed as if
                    // its own closer were present.
                    return Parsed::Done(children);
                }
                children.push(Node::leaf("ERROR", self.at, self.at + 1));
                self.at += 1;
                continue;
            }
            match self.form(&inner) {
                Parsed::Done(nodes) => children.extend(nodes),
                Parsed::Open(nodes) => {
                    children.extend(nodes);
                    return Parsed::Open(children);
                }
            }
        }
    }

    /// A collection whose opening marker of `marker` bytes starts at `start`.
    fn collection(
        &mut self,
        kind: &'static str,
        start: usize,
        marker: usize,
        close: char,
        closers: &[char],
    ) -> Parsed {
        self.at = start + marker;
        match self.items(close, closers) {
            Parsed::Done(children) => Parsed::Done(vec![Node {
                kind,
                start,
                end: self.at,
                children,
            }]),
            Parsed::Open(children) => Parsed::Open(children),
        }
    }

    /// A prefix reader macro: its marker, gaps and one form.
    fn prefix(&mut self, kind: &'static str, marker: usize, closers: &[char]) -> Parsed {
        let start = self.at;
        self.at += marker;
        let (children, open) = self.prefixed(closers);
        let node = Node {
            kind,
            start,
            end: self.at,
            children,
        };
        if open {
            Parsed::Open(flatten(node))
        } else {
            Parsed::Done(vec![node])
        }
    }

    /// `ns_map_lit`: `#`, a keyword or `::` prefix, gaps, then a map.
    fn namespaced_map(&mut self, closers: &[char]) -> Parsed {
        let start = self.at;
        self.at += 1;
        let mut children = vec![self.keyword()];
        loop {
            match self.gap(closers) {
                Some(Parsed::Done(nodes)) => children.extend(nodes),
                Some(Parsed::Open(nodes)) => {
                    children.extend(nodes);
                    return Parsed::Open(children);
                }
                None => break,
            }
        }
        if self.peek() != Some('{') {
            return Parsed::Done(vec![Node {
                kind: "ERROR",
                start,
                end: self.at,
                children,
            }]);
        }
        let open = self.at;
        match self.collection("map_lit", open, 1, '}', closers) {
            Parsed::Done(map) => {
                // The map's items are the namespaced map's own children.
                for node in map {
                    children.extend(node.children);
                }
                Parsed::Done(vec![Node {
                    kind: "ns_map_lit",
                    start,
                    end: self.at,
                    children,
                }])
            }
            Parsed::Open(items) => {
                children.extend(items);
                Parsed::Open(children)
            }
        }
    }

    /// `kwd_lit` at a `:`; `::` and a bare `::` mark are kept whole.
    fn keyword(&mut self) -> Node {
        let start = self.at;
        if self.rest().starts_with("::") {
            self.at += 2;
        } else if self.rest().starts_with(':') {
            self.at += 1;
        }
        if self.peek() == Some('/') {
            self.at += 1;
            self.take_while(|ch| keyword_head(ch) || ":'/".contains(ch));
        } else if self.take_while(keyword_head) > 0 {
            self.take_while(|ch| keyword_head(ch) || ":'".contains(ch));
            let rest = self.rest();
            if rest.starts_with('/')
                && rest[1..]
                    .chars()
                    .next()
                    .is_some_and(|ch| keyword_head(ch) || ":'/".contains(ch))
            {
                self.at += 1;
                self.take_while(|ch| keyword_head(ch) || ":'/".contains(ch));
            }
        }
        Node::leaf("kwd_lit", start, self.at)
    }

    /// `str_lit` (or the regex body after `#`); unterminated is an error to
    /// end of file.
    fn string(&mut self, kind: &'static str, start: usize) -> Node {
        let mut chars = self.rest().char_indices();
        chars.next();
        while let Some((offset, ch)) = chars.next() {
            match ch {
                '"' => {
                    self.at += offset + 1;
                    return Node::leaf(kind, start, self.at);
                }
                '\\' => {
                    chars.next();
                }
                _ => {}
            }
        }
        self.at = self.text.len();
        Node::leaf("ERROR", start, self.at)
    }

    /// Symbols, booleans and nil, which share one token at a form's start.
    fn symbol(&mut self) -> Vec<Node> {
        let start = self.at;
        if self.peek() == Some('/') {
            self.at += 1;
            return vec![Node::leaf("sym_lit", start, self.at)];
        }
        self.at += self.peek().map_or(0, char::len_utf8);
        self.take_while(symbol_body);
        let word = &self.text[start..self.at];
        match word {
            "true" | "false" => return vec![Node::leaf("bool_lit", start, self.at)],
            "nil" => return vec![Node::leaf("nil_lit", start, self.at)],
            _ => {}
        }
        if self.peek() == Some('/') {
            let delimiter = self.at;
            self.at += 1;
            if self.take_while(symbol_name) == 0 {
                // A namespace without a name: the delimiter is an error.
                return vec![
                    Node::leaf("sym_lit", start, delimiter),
                    Node::leaf("ERROR", delimiter, self.at),
                ];
            }
        }
        vec![Node::leaf("sym_lit", start, self.at)]
    }

    /// One form, metadata included.
    fn form(&mut self, closers: &[char]) -> Parsed {
        let rest = self.rest();
        if rest.starts_with('^') || rest.starts_with("#^") {
            return self.with_metadata(closers);
        }
        let start = self.at;
        let Some(first) = self.peek() else {
            return Parsed::Done(Vec::new());
        };
        if let Some(length) = number(rest) {
            self.at += length;
            return Parsed::Done(vec![Node::leaf("num_lit", start, self.at)]);
        }
        let done = |node: Node| Parsed::Done(vec![node]);
        match first {
            '(' => self.collection("list_lit", start, 1, ')', closers),
            '[' => self.collection("vec_lit", start, 1, ']', closers),
            '{' => self.collection("map_lit", start, 1, '}', closers),
            '"' => done(self.string("str_lit", start)),
            ':' => done(self.keyword()),
            '\\' => {
                let length = character(&rest[1..]).unwrap_or(0);
                self.at += 1 + length;
                done(Node::leaf(
                    if length == 0 { "ERROR" } else { "char_lit" },
                    start,
                    self.at,
                ))
            }
            '\'' => self.prefix("quoting_lit", 1, closers),
            '`' => self.prefix("syn_quoting_lit", 1, closers),
            '~' if rest.starts_with("~@") => self.prefix("unquote_splicing_lit", 2, closers),
            '~' => self.prefix("unquoting_lit", 1, closers),
            '@' => self.prefix("derefing_lit", 1, closers),
            '#' => self.dispatch(closers),
            _ => Parsed::Done(self.symbol()),
        }
    }

    /// The `#` dispatch reader macros.
    fn dispatch(&mut self, closers: &[char]) -> Parsed {
        let start = self.at;
        let rest = self.rest();
        let second = rest[1..].chars().next();
        match second {
            Some('{') => self.collection("set_lit", start, 2, '}', closers),
            Some('(') => self.collection("anon_fn_lit", start, 2, ')', closers),
            Some('"') => {
                self.at += 1;
                Parsed::Done(vec![self.string("regex_lit", start)])
            }
            Some('?') => {
                let (kind, marker) = if rest.starts_with("#?@") {
                    ("splicing_read_cond_lit", 3)
                } else {
                    ("read_cond_lit", 2)
                };
                self.at += marker;
                // Whitespace only, never a comment or a discarded form.
                self.skip_whitespace();
                if self.peek() != Some('(') {
                    return Parsed::Done(vec![Node::leaf("ERROR", start, self.at)]);
                }
                let open = self.at;
                match self.collection(kind, open, 1, ')', closers) {
                    Parsed::Done(mut nodes) => {
                        for node in &mut nodes {
                            node.start = start;
                        }
                        Parsed::Done(nodes)
                    }
                    open => open,
                }
            }
            Some('\'') => self.prefix("var_quoting_lit", 2, closers),
            Some('#') => self.prefix("sym_val_lit", 2, closers),
            Some('=') => self.prefix("evaling_lit", 2, closers),
            Some(':') => self.namespaced_map(closers),
            _ => self.prefix("tagged_or_ctor_lit", 1, closers),
        }
    }

    /// `^` / `#^` metadata, each with its trailing gaps, attached to the
    /// form it precedes when that form takes metadata.
    fn with_metadata(&mut self, closers: &[char]) -> Parsed {
        let start = self.at;
        let mut metadata = Vec::new();
        loop {
            let rest = self.rest();
            let (kind, marker) = if rest.starts_with('^') {
                ("meta_lit", 1)
            } else if rest.starts_with("#^") {
                ("old_meta_lit", 2)
            } else {
                break;
            };
            match self.prefix(kind, marker, closers) {
                Parsed::Done(nodes) => metadata.extend(nodes),
                Parsed::Open(nodes) => {
                    metadata.extend(nodes);
                    return Parsed::Open(metadata);
                }
            }
            loop {
                match self.gap(closers) {
                    Some(Parsed::Done(nodes)) => metadata.extend(nodes),
                    Some(Parsed::Open(nodes)) => {
                        metadata.extend(nodes);
                        return Parsed::Open(metadata);
                    }
                    None => break,
                }
            }
        }
        let gaps_end = self.at;
        let target = match self.peek() {
            Some(ch) if !")]}".contains(ch) => self.form(closers),
            _ => Parsed::Done(Vec::new()),
        };
        match target {
            Parsed::Done(mut nodes) => {
                if nodes.len() == 1 && META_TARGETS.contains(&nodes[0].kind) {
                    let mut node = nodes.pop().unwrap();
                    metadata.append(&mut node.children);
                    node.children = metadata;
                    node.start = start;
                    return Parsed::Done(vec![node]);
                }
                // Metadata on a form that takes none: a symbol holding only
                // the metadata, then the form on its own.
                let mut found = vec![Node {
                    kind: "sym_lit",
                    start,
                    end: gaps_end,
                    children: metadata,
                }];
                found.extend(nodes);
                Parsed::Done(found)
            }
            Parsed::Open(nodes) => {
                metadata.extend(nodes);
                Parsed::Open(metadata)
            }
        }
    }
}

/// A node's contents inline, as the grammar's error recovery keeps them.
fn flatten(node: Node) -> Vec<Node> {
    node.children
}

/// The whole file: top-level forms and gaps. Contents still open at end of
/// file become one `ERROR` node.
fn read(text: &str) -> Node {
    let mut reader = Reader { text, at: 0 };
    let mut children = Vec::new();
    loop {
        match reader.gap(&[]) {
            Some(Parsed::Done(nodes)) => {
                children.extend(nodes);
                continue;
            }
            Some(Parsed::Open(nodes)) => {
                children.push(error(nodes, text.len()));
                break;
            }
            None => {}
        }
        let Some(ch) = reader.peek() else {
            break;
        };
        if ")]}".contains(ch) {
            children.push(Node::leaf("ERROR", reader.at, reader.at + 1));
            reader.at += 1;
            continue;
        }
        let start = reader.at;
        match reader.form(&[]) {
            Parsed::Done(nodes) => children.extend(nodes),
            Parsed::Open(nodes) => {
                let mut node = error(nodes, text.len());
                node.start = start;
                children.push(node);
                break;
            }
        }
    }
    Node {
        kind: "source",
        start: 0,
        end: text.len(),
        children,
    }
}

fn error(children: Vec<Node>, end: usize) -> Node {
    Node {
        kind: "ERROR",
        start: children.first().map_or(end, |node| node.start),
        end,
        children,
    }
}

/// `_CLOJURE_HEAD`: clj-mutate's head symbol swaps and their categories.
fn head_swap(symbol: &str) -> Option<(&'static str, &'static str)> {
    Some(match symbol {
        "+" => ("-", "arithmetic"),
        "-" => ("+", "arithmetic"),
        "*" => ("/", "arithmetic"),
        "inc" => ("dec", "arithmetic"),
        "dec" => ("inc", "arithmetic"),
        ">" => (">=", "comparison"),
        ">=" => (">", "comparison"),
        "<" => ("<=", "comparison"),
        "<=" => ("<", "comparison"),
        "=" => ("not=", "equality"),
        "not=" => ("=", "equality"),
        "if" => ("if-not", "conditional"),
        "if-not" => ("if", "conditional"),
        "when" => ("when-not", "conditional"),
        "when-not" => ("when", "conditional"),
        "and" => ("or", "logical"),
        "or" => ("and", "logical"),
        "double" => ("int", "coercion"),
        "int" => ("double", "coercion"),
        "first" => ("second", "seq"),
        "second" => ("first", "seq"),
        "filter" => ("remove", "seq"),
        "remove" => ("filter", "seq"),
        "take" => ("drop", "seq"),
        "drop" => ("take", "seq"),
        "rest" => ("next", "seq"),
        "next" => ("rest", "seq"),
        "every?" => ("some", "seq"),
        "some" => ("every?", "seq"),
        "min" => ("max", "numeric"),
        "max" => ("min", "numeric"),
        "pos?" => ("neg?", "predicate"),
        "neg?" => ("pos?", "predicate"),
        "even?" => ("odd?", "predicate"),
        "odd?" => ("even?", "predicate"),
        "nil?" => ("some?", "predicate"),
        "some?" => ("nil?", "predicate"),
        _ => return None,
    })
}

/// A raw site: line, byte span, original, mutant and category.
pub type Raw = (usize, usize, usize, String, String, &'static str);

/// `_clojure_head`: the first symbol directly in a list.
fn head<'n>(list: &'n Node, text: &'n str) -> Option<&'n str> {
    list.children
        .iter()
        .find(|child| child.kind == "sym_lit")
        .map(|child| &text[child.start..child.end])
}

/// `_clojure_skipped` along the path from the root: comments, quotes,
/// strings and characters, and the bodies of `(comment …)` and `(quote …)`.
fn skipped(node: &Node, text: &str) -> bool {
    ["comment", "quot", "string", "str_lit", "char_lit"]
        .iter()
        .any(|part| node.kind.contains(part))
        || (node.kind == "list_lit" && matches!(head(node, text), Some("comment" | "quote")))
}

fn visit(
    node: &Node,
    parent: Option<&Node>,
    hidden: bool,
    text: &str,
    lines: &[usize],
    found: &mut Vec<Raw>,
) {
    let hidden = hidden || skipped(node, text);
    if !hidden {
        let token = &text[node.start..node.end];
        let line = lines.partition_point(|&newline| newline < node.start) + 1;
        let mut add = |mutant: &str, category: &'static str| {
            if mutant != token {
                found.push((
                    line,
                    node.start,
                    node.end,
                    token.to_string(),
                    mutant.to_string(),
                    category,
                ));
            }
        };
        match node.kind {
            "sym_lit" => {
                let is_head = parent.is_some_and(|list| {
                    list.kind == "list_lit"
                        && list
                            .children
                            .iter()
                            .find(|child| child.kind == "sym_lit")
                            .is_some_and(|first| std::ptr::eq(first, node))
                });
                if is_head && let Some((mutant, category)) = head_swap(token) {
                    add(mutant, category);
                }
            }
            "bool_lit" => add(if token == "true" { "false" } else { "true" }, "boolean"),
            "num_lit" if matches!(token, "0" | "1") => {
                add(if token == "0" { "1" } else { "0" }, "constant")
            }
            _ => {}
        }
    }
    for child in &node.children {
        visit(child, Some(node), hidden, text, lines, found);
    }
}

/// The original's Clojure `discover_raw`, sorted by line and span.
pub fn raw_sites(text: &str) -> Vec<Raw> {
    let tree = read(text);
    let lines: Vec<usize> = text
        .bytes()
        .enumerate()
        .filter(|(_, byte)| *byte == b'\n')
        .map(|(at, _)| at)
        .collect();
    let mut found = Vec::new();
    visit(&tree, None, false, text, &lines, &mut found);
    found.sort_by_key(|site| (site.0, site.1, site.2));
    found
}

/// `is_private` for Clojure: the declaration's header starts `(defn-`.
pub fn private(header: &str) -> bool {
    header.starts_with("(defn-")
}
