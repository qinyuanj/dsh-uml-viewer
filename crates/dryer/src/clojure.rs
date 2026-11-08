//! Clojure source, read and normalized the way dry4clj does it: dryer's
//! `clojure_read.py`, ported. Clojure has no tree-sitter grammar here; forms
//! come from this reader so reader conditionals, discards, metadata, tagged
//! literals and reader errors behave exactly like the original.

use crate::normalize::Normalized;
use quality_core::number::decimal_digit;

const WHITESPACE: &[char] = &[' ', '\t', '\n', '\r', ','];
const TERMINATORS: &[char] = &[
    '(', ')', '[', ']', '{', '}', '"', ';', '\'', '@', '^', '`', '~', '\\', ',', '#', ':',
];

/// A form in the source, with the line it starts on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Form {
    Sym(String, usize),
    Kw(String, usize),
    Lit(usize),
    Coll(Coll),
    Splice(Vec<Form>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Coll {
    pub kind: &'static str,
    pub line: usize,
    pub items: Vec<Form>,
    pub pairs: Vec<(Form, Form)>,
}

impl Form {
    pub fn line(&self) -> usize {
        match self {
            Form::Sym(_, line) | Form::Kw(_, line) | Form::Lit(line) => *line,
            Form::Coll(coll) => coll.line,
            // A spliced reader conditional never reaches a line query in the
            // original either; it is spliced into its parent first.
            Form::Splice(items) => items.iter().map(Form::line).next().unwrap_or(0),
        }
    }
    fn symbol_name(&self) -> Option<&str> {
        match self {
            Form::Sym(name, _) => Some(name),
            _ => None,
        }
    }
    fn keyword_name(&self) -> Option<&str> {
        match self {
            Form::Kw(name, _) => Some(name),
            _ => None,
        }
    }
    fn items(&self) -> Option<&[Form]> {
        match self {
            Form::Coll(coll) if coll.kind != "map" => Some(&coll.items),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReadError {
    pub line: usize,
    pub message: String,
}

type Read = Result<Option<Form>, ReadError>;

/// `_NUMBER` from `clojure_read.py`: five anchored alternatives, each tried
/// with the backtracking Python's regex applies before `[MN]?$`.
mod number {
    use super::decimal_digit;

    fn digits(chars: &[char], at: usize) -> Vec<usize> {
        let mut end = at;
        let mut found = Vec::new();
        while chars.get(end).is_some_and(|c| decimal_digit(*c).is_some()) {
            end += 1;
            found.push(end);
        }
        found.reverse();
        found
    }
    fn hex(chars: &[char], at: usize) -> Vec<usize> {
        let mut end = at;
        let mut found = Vec::new();
        while chars.get(end).is_some_and(|c| c.is_ascii_hexdigit()) {
            end += 1;
            found.push(end);
        }
        found.reverse();
        found
    }
    fn exponent(chars: &[char], at: usize) -> Vec<usize> {
        if !chars.get(at).is_some_and(|c| *c == 'e' || *c == 'E') {
            return Vec::new();
        }
        let sign = usize::from(chars.get(at + 1).is_some_and(|c| *c == '+' || *c == '-'));
        digits(chars, at + 1 + sign)
    }
    /// Every position each alternative can end at, longest first.
    fn ends(chars: &[char]) -> Vec<usize> {
        let mut found = Vec::new();
        // \d+/\d+
        for first in digits(chars, 0) {
            if chars.get(first) == Some(&'/') {
                found.extend(digits(chars, first + 1));
            }
        }
        // \d+\.\d+(?:[eE][+-]?\d+)?
        for first in digits(chars, 0) {
            if chars.get(first) == Some(&'.') {
                for second in digits(chars, first + 1) {
                    found.extend(exponent(chars, second));
                    found.push(second);
                }
            }
        }
        // \.\d+(?:[eE][+-]?\d+)?
        if chars.first() == Some(&'.') {
            for second in digits(chars, 1) {
                found.extend(exponent(chars, second));
                found.push(second);
            }
        }
        // \d+(?:[eE][+-]?\d+)?
        for first in digits(chars, 0) {
            found.extend(exponent(chars, first));
            found.push(first);
        }
        // 0[xX][0-9A-Fa-f]+
        if chars.first() == Some(&'0') && chars.get(1).is_some_and(|c| *c == 'x' || *c == 'X') {
            found.extend(hex(chars, 2));
        }
        found
    }
    pub fn matches(token: &str) -> bool {
        let chars: Vec<char> = token.chars().collect();
        let body = if chars.first().is_some_and(|c| *c == '+' || *c == '-') {
            &chars[1..]
        } else {
            &chars[..]
        };
        ends(body).into_iter().any(|end| {
            let rest = &body[end..];
            rest.is_empty() || (rest.len() == 1 && matches!(rest[0], 'M' | 'N'))
        })
    }
}

struct Reader<'a> {
    text: &'a [char],
    i: usize,
    line: usize,
}

impl<'a> Reader<'a> {
    fn peek(&self) -> Option<char> {
        self.text.get(self.i).copied()
    }
    fn eof(&self) -> bool {
        self.i >= self.text.len()
    }
    fn get(&mut self) -> Option<char> {
        let char = self.peek()?;
        self.i += 1;
        if char == '\n' {
            self.line += 1;
        }
        Some(char)
    }
    fn error<T>(&self, line: usize, message: &str) -> Result<T, ReadError> {
        Err(ReadError {
            line,
            message: message.to_string(),
        })
    }
    fn skip_ws(&mut self) {
        while !self.eof() {
            let char = self.peek().unwrap();
            if WHITESPACE.contains(&char) {
                self.get();
                continue;
            }
            if char == ';' {
                while !self.eof() && self.peek() != Some('\n') {
                    self.get();
                }
                continue;
            }
            break;
        }
    }
    fn read_token(&mut self) -> Result<String, ReadError> {
        let start = self.i;
        let line = self.line;
        while !self.eof() {
            let char = self.peek().unwrap();
            if WHITESPACE.contains(&char) || TERMINATORS.contains(&char) {
                break;
            }
            self.get();
        }
        if self.i == start {
            return self.error(line, "expected a symbol");
        }
        Ok(self.text[start..self.i].iter().collect())
    }
    fn read_string(&mut self) -> Result<usize, ReadError> {
        let line = self.line;
        self.get();
        while !self.eof() {
            let char = self.get().unwrap();
            if char == '\\' {
                if self.eof() {
                    break;
                }
                self.get();
                continue;
            }
            if char == '"' {
                return Ok(line);
            }
        }
        self.error(line, "unterminated string")
    }
    fn read_char(&mut self) -> Result<Form, ReadError> {
        let line = self.line;
        self.get();
        if self.eof() {
            return self.error(line, "incomplete character");
        }
        self.get();
        while !self.eof() {
            let char = self.peek().unwrap();
            if WHITESPACE.contains(&char) || TERMINATORS.contains(&char) {
                break;
            }
            self.get();
        }
        Ok(Form::Lit(line))
    }
    fn read_keyword(&mut self) -> Result<Form, ReadError> {
        let line = self.line;
        self.get();
        if self.peek() == Some(':') {
            self.get();
        }
        if self.eof()
            || self
                .peek()
                .is_some_and(|c| WHITESPACE.contains(&c) || TERMINATORS.contains(&c))
        {
            return Ok(Form::Kw(String::new(), line));
        }
        Ok(Form::Kw(self.read_token()?, line))
    }
    fn read_collection(&mut self, end: char, kind: &'static str) -> Result<Form, ReadError> {
        let line = self.line;
        self.get();
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.eof() {
                return self.error(line, &format!("unterminated {kind}"));
            }
            if self.peek() == Some(end) {
                self.get();
                break;
            }
            match self.read_form()? {
                None => continue,
                Some(Form::Splice(spliced)) => items.extend(spliced),
                Some(form) => items.push(form),
            }
        }
        if kind == "map" {
            if items.len() % 2 == 1 {
                return self.error(line, "map literal has an odd number of forms");
            }
            let mut pairs = Vec::new();
            let mut items = items.into_iter();
            while let (Some(key), Some(value)) = (items.next(), items.next()) {
                pairs.push((key, value));
            }
            return Ok(Form::Coll(Coll {
                kind,
                line,
                items: Vec::new(),
                pairs,
            }));
        }
        Ok(Form::Coll(Coll {
            kind,
            line,
            items,
            pairs: Vec::new(),
        }))
    }
    fn wrap(&mut self, name: &'static str, line: usize) -> Result<Form, ReadError> {
        let inner = self.read_form()?;
        let mut items = vec![Form::Sym(name.to_string(), line)];
        if let Some(inner) = inner
            && !matches!(inner, Form::Splice(_))
        {
            items.push(inner);
        }
        Ok(Form::Coll(Coll {
            kind: "list",
            line,
            items,
            pairs: Vec::new(),
        }))
    }
    fn read_prefixed(&mut self) -> Result<Form, ReadError> {
        let line = self.line;
        let name = match self.get() {
            Some('\'') => "quote",
            Some('@') => "deref",
            Some('`') => "syntax-quote",
            _ => unreachable!("read_prefixed is only called for a prefix"),
        };
        self.wrap(name, line)
    }
    fn read_unquote(&mut self) -> Result<Form, ReadError> {
        let line = self.line;
        self.get();
        let name = if self.peek() == Some('@') {
            self.get();
            "unquote-splicing"
        } else {
            "unquote"
        };
        self.wrap(name, line)
    }
    fn read_meta(&mut self) -> Read {
        self.get();
        self.read_form()?;
        self.read_form()
    }
    fn read_atom(&mut self) -> Result<Form, ReadError> {
        let line = self.line;
        let token = self.read_token()?;
        if number::matches(&token) {
            return Ok(Form::Lit(line));
        }
        Ok(Form::Sym(token, line))
    }
    fn read_dispatch(&mut self) -> Read {
        let line = self.line;
        self.get();
        let Some(char) = self.peek() else {
            return self.error(line, "incomplete dispatch");
        };
        match char {
            '_' => {
                self.get();
                self.read_form()?;
                Ok(None)
            }
            '{' => self.read_collection('}', "set").map(Some),
            '(' => {
                let inner = self.read_collection(')', "list")?;
                let Form::Coll(inner) = inner else {
                    unreachable!("a collection read is a collection")
                };
                let mut items = vec![Form::Sym("fn*".to_string(), line)];
                items.extend(inner.items);
                Ok(Some(Form::Coll(Coll {
                    kind: "list",
                    line,
                    items,
                    pairs: Vec::new(),
                })))
            }
            '"' => {
                self.read_string()?;
                Ok(Some(Form::Lit(line)))
            }
            '?' => {
                self.get();
                let splicing = self.peek() == Some('@');
                if splicing {
                    self.get();
                }
                self.read_cond(splicing)
            }
            ':' => {
                self.get();
                if self.peek() == Some(':') {
                    self.get();
                } else if self
                    .peek()
                    .is_some_and(|c| c != '{' && !WHITESPACE.contains(&c))
                {
                    self.read_token()?;
                }
                self.read_form()
            }
            _ => {
                self.read_token()?;
                self.read_form()?;
                Ok(Some(Form::Lit(line)))
            }
        }
    }
    fn read_cond(&mut self, splicing: bool) -> Read {
        let form = self.read_form()?;
        let items = match &form {
            Some(Form::Coll(coll)) if coll.kind == "list" => &coll.items,
            _ => {
                return self.error(self.line, "reader conditional body must be a list");
            }
        };
        let mut index = 0;
        let mut chosen = None;
        while index + 1 < items.len() {
            if matches!(items[index].keyword_name(), Some("clj" | "default")) {
                chosen = Some(items[index + 1].clone());
                break;
            }
            index += 2;
        }
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        if !splicing {
            return Ok(Some(chosen));
        }
        match chosen {
            Form::Coll(coll) if matches!(coll.kind, "list" | "vector") => {
                Ok(Some(Form::Splice(coll.items)))
            }
            _ => self.error(self.line, "splicing reader conditional needs a collection"),
        }
    }
    fn read_form(&mut self) -> Read {
        self.skip_ws();
        if self.eof() {
            return self.error(self.line, "unexpected end of file");
        }
        let char = self.peek().unwrap();
        match char {
            '(' => return self.read_collection(')', "list").map(Some),
            '[' => return self.read_collection(']', "vector").map(Some),
            '{' => return self.read_collection('}', "map").map(Some),
            _ => {}
        }
        match char {
            '"' => self.read_string().map(|line| Some(Form::Lit(line))),
            '\\' => self.read_char().map(Some),
            ':' => self.read_keyword().map(Some),
            '\'' | '@' | '`' => self.read_prefixed().map(Some),
            '~' => self.read_unquote().map(Some),
            '^' => self.read_meta(),
            '#' => self.read_dispatch(),
            _ => self.read_atom().map(Some),
        }
    }
}

/// Top-level forms, plus a warning when a later form could not be read.
pub(crate) fn read_source(text: &str) -> (Vec<Form>, Option<String>) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let chars: Vec<char> = text.chars().collect();
    let mut reader = Reader {
        text: &chars,
        i: 0,
        line: 1,
    };
    if text.starts_with("#!") {
        while !matches!(reader.peek(), None | Some('\n')) {
            reader.get();
        }
    }
    let mut forms = Vec::new();
    loop {
        reader.skip_ws();
        if reader.eof() {
            return (forms, None);
        }
        match reader.read_form() {
            Err(error) => return (forms, Some(format!("{}: {}", error.line, error.message))),
            Ok(None | Some(Form::Splice(_))) => continue,
            Ok(Some(form)) => forms.push(form),
        }
    }
}

fn children(form: &Form) -> Vec<&Form> {
    match form {
        Form::Coll(coll) if coll.kind == "map" => coll
            .pairs
            .iter()
            .flat_map(|(key, value)| [key, value])
            .collect(),
        Form::Coll(coll) => coll.items.iter().collect(),
        _ => Vec::new(),
    }
}

/// The last line any part of the form occupies.
pub(crate) fn max_line(form: &Form) -> usize {
    children(form)
        .into_iter()
        .fold(form.line(), |best, child| best.max(max_line(child)))
}

fn keyword(name: &str) -> Normalized {
    Normalized::Keyword(name.to_string())
}

/// dry4clj's `normalize-form`: collection heads keep their spelling, other
/// symbols, keywords and literals become markers.
pub(crate) fn normalize(form: &Form, head: bool) -> Normalized {
    match form {
        Form::Coll(coll) => match coll.kind {
            "list" => {
                let mut out = vec![keyword("list")];
                match coll.items.split_first() {
                    None => out.push(keyword("literal")),
                    Some((first, rest)) => {
                        out.push(normalize(first, true));
                        out.extend(rest.iter().map(|item| normalize(item, false)));
                    }
                }
                Normalized::Vector(out)
            }
            "vector" | "set" => Normalized::Vector(
                std::iter::once(keyword(coll.kind))
                    .chain(coll.items.iter().map(|item| normalize(item, false)))
                    .collect(),
            ),
            "map" => Normalized::Vector(
                std::iter::once(keyword("map"))
                    .chain(coll.pairs.iter().map(|(key, value)| {
                        Normalized::Vector(vec![normalize(key, false), normalize(value, false)])
                    }))
                    .collect(),
            ),
            _ => Normalized::Vector(vec![keyword("literal")]),
        },
        Form::Sym(name, _) if head => {
            Normalized::Vector(vec![keyword("symbol"), Normalized::Name(name.clone())])
        }
        Form::Sym(_, _) => keyword("symbol"),
        Form::Kw(_, _) => keyword("keyword"),
        _ => keyword("literal"),
    }
}

/// A non-empty top-level list whose head is not `ns`.
pub(crate) fn is_candidate_form(form: &Form) -> bool {
    let Some(items) = form.items() else {
        return false;
    };
    if items.is_empty() || !matches!(form, Form::Coll(coll) if coll.kind == "list") {
        return false;
    }
    items[0].symbol_name() != Some("ns")
}
