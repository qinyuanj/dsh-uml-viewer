//! EDN codec: the external data contract (IR, policy, mail, `.metrics`).
//!
//! Semantics follow `love/uml/edn.lua`, extended by #135 with sets, tagged
//! values and `#_` discard:
//! - nil, booleans, strings (`\n \r \t \b \f \" \\` and `\uXXXX`, surrogate
//!   pairs combined), keywords, symbols, vectors, maps, sets, tagged values;
//!   `;` comments and commas are whitespace.
//! - Numbers are signed decimal integers or floats with an optional exponent.
//!   Ratios, `N`/`M` suffixes, hex, NaN and infinities are rejected. Integral
//!   values outside `[-(2^53-1), 2^53-1]` are rejected, never rounded.
//!   Integers and floats are kept apart so `68.0` re-encodes as `68.0`, but
//!   compare by numeric value like the Lua codec (`68 == 68.0`).
//! - Duplicate map keys or set elements are errors. Map and set equality
//!   ignores order.
//! - [`encode`] writes on one line, map keys and set elements sorted by their
//!   encoded bytes, so output is byte-stable regardless of insertion order.
//!   Comments and discarded forms are not preserved.
//!
//! Lists `( )` and character literals are unsupported, as in the Lua codec.

pub mod pair_order;
pub mod record;

use std::fmt;

/// Largest integral magnitude a double represents exactly (Lua parity).
pub const MAX_EXACT_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Keyword(String),
    Symbol(String),
    Vector(Vec<Value>),
    /// Entries in reading/insertion order; [`encode`] sorts them.
    Map(Vec<(Value, Value)>),
    /// Elements in reading/insertion order; [`encode`] sorts them.
    Set(Vec<Value>),
    Tagged(String, Box<Value>),
}

impl Value {
    pub fn keyword(name: &str) -> Value {
        Value::Keyword(name.to_owned())
    }

    /// Map lookup by structural key; `None` for missing keys and non-maps.
    pub fn get(&self, key: &Value) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Replace the value under `key`, or append the entry. No-op on non-maps.
    pub fn assoc(&mut self, key: Value, value: Value) {
        if let Value::Map(entries) = self {
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key, value)),
            }
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        use Value::*;
        match (self, other) {
            (Nil, Nil) => true,
            (Bool(a), Bool(b)) => a == b,
            (Integer(a), Integer(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Integer(a), Float(b)) | (Float(b), Integer(a)) => *a as f64 == *b,
            (String(a), String(b)) | (Keyword(a), Keyword(b)) | (Symbol(a), Symbol(b)) => a == b,
            (Vector(a), Vector(b)) => a == b,
            (Map(a), Map(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v == v2))
            }
            (Set(a), Set(b)) => a.len() == b.len() && a.iter().all(|x| b.contains(x)),
            (Tagged(t, a), Tagged(u, b)) => t == u && a == b,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// 1-based byte position for parse errors, 0 for encode errors.
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.offset == 0 {
            write!(f, "EDN: {}", self.message)
        } else {
            write!(f, "EDN byte {}: {}", self.offset, self.message)
        }
    }
}

impl std::error::Error for Error {}

const RANGE_MESSAGE: &str =
    "EDN number outside supported exact integer range [-9007199254740991, 9007199254740991]";

pub fn parse(text: &str) -> Result<Value, Error> {
    let mut parser = Parser {
        text: text.as_bytes(),
        p: 0,
    };
    let value = parser.read()?;
    parser.skip()?;
    if parser.p < parser.text.len() {
        return parser.fail("trailing input");
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a [u8],
    p: usize,
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c | b',')
}

fn is_delimiter(c: u8) -> bool {
    is_space(c) || matches!(c, b'[' | b']' | b'{' | b'}' | b'"' | b';')
}

fn symbol_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || b"_*+!/?<>=.$%&-".contains(&c)
}

fn symbol_rest(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"_*+!/?<>=.$%&:-".contains(&c)
}

fn is_symbol(token: &[u8]) -> bool {
    !token.is_empty() && symbol_start(token[0]) && token[1..].iter().all(|&c| symbol_rest(c))
}

impl Parser<'_> {
    fn fail<T>(&self, message: &str) -> Result<T, Error> {
        Err(Error {
            offset: self.p + 1,
            message: message.to_owned(),
        })
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.p).copied()
    }

    fn skip(&mut self) -> Result<(), Error> {
        while let Some(c) = self.peek() {
            if is_space(c) {
                self.p += 1;
            } else if c == b';' {
                self.p = match self.text[self.p..].iter().position(|&b| b == b'\n') {
                    Some(offset) => self.p + offset + 1,
                    None => self.text.len(),
                };
            } else if c == b'#' && self.text.get(self.p + 1) == Some(&b'_') {
                self.p += 2;
                self.read()?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn read(&mut self) -> Result<Value, Error> {
        self.skip()?;
        match self.peek() {
            Some(b'"') => self.read_string(),
            Some(b'[') => {
                self.p += 1;
                Ok(Value::Vector(self.read_items(b']')?))
            }
            Some(b'{') => {
                self.p += 1;
                self.read_map()
            }
            Some(b'#') => self.read_dispatch(),
            _ => self.read_token(),
        }
    }

    fn read_items(&mut self, close: u8) -> Result<Vec<Value>, Error> {
        let mut items = Vec::new();
        loop {
            self.skip()?;
            match self.peek() {
                None => return self.fail("unterminated collection"),
                Some(c) if c == close => {
                    self.p += 1;
                    return Ok(items);
                }
                _ => items.push(self.read()?),
            }
        }
    }

    fn read_map(&mut self) -> Result<Value, Error> {
        let mut entries: Vec<(Value, Value)> = Vec::new();
        loop {
            self.skip()?;
            match self.peek() {
                None => return self.fail("unterminated collection"),
                Some(b'}') => {
                    self.p += 1;
                    return Ok(Value::Map(entries));
                }
                _ => {}
            }
            let key = self.read()?;
            self.skip()?;
            if self.peek() == Some(b'}') {
                return self.fail("map requires key/value pairs");
            }
            let value = self.read()?;
            if entries.iter().any(|(k, _)| *k == key) {
                return self.fail("duplicate map key");
            }
            entries.push((key, value));
        }
    }

    fn read_dispatch(&mut self) -> Result<Value, Error> {
        match self.text.get(self.p + 1) {
            Some(b'{') => {
                self.p += 2;
                let items = self.read_items(b'}')?;
                for (i, item) in items.iter().enumerate() {
                    if items[..i].contains(item) {
                        return self.fail("duplicate set element");
                    }
                }
                Ok(Value::Set(items))
            }
            Some(&c) if c.is_ascii_alphabetic() => {
                self.p += 1;
                let start = self.p;
                while self.peek().is_some_and(|c| !is_delimiter(c)) {
                    self.p += 1;
                }
                let tag = &self.text[start..self.p];
                if !is_symbol(tag) {
                    return self.fail("invalid tag");
                }
                let tag = String::from_utf8(tag.to_vec()).unwrap();
                self.skip()?;
                if self.peek().is_none() {
                    return self.fail("tag requires a value");
                }
                Ok(Value::Tagged(tag, Box::new(self.read()?)))
            }
            _ => self.fail("unsupported dispatch"),
        }
    }

    fn read_string(&mut self) -> Result<Value, Error> {
        self.p += 1;
        let mut out: Vec<u8> = Vec::new();
        while let Some(c) = self.peek() {
            self.p += 1;
            match c {
                b'"' => return Ok(Value::String(String::from_utf8(out).unwrap())),
                b'\\' => {
                    let escape = self.peek();
                    self.p += 1;
                    match escape {
                        Some(b'n') => out.push(b'\n'),
                        Some(b'r') => out.push(b'\r'),
                        Some(b't') => out.push(b'\t'),
                        Some(b'b') => out.push(0x08),
                        Some(b'f') => out.push(0x0c),
                        Some(b'"') => out.push(b'"'),
                        Some(b'\\') => out.push(b'\\'),
                        Some(b'u') => {
                            let ch = self.read_unicode_escape()?;
                            let mut buffer = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                        }
                        _ => return self.fail("invalid string escape"),
                    }
                }
                _ => out.push(c),
            }
        }
        self.fail("unterminated string")
    }

    fn hex4(&self, at: usize) -> Option<u32> {
        let digits = self.text.get(at..at + 4)?;
        if !digits.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    }

    fn read_unicode_escape(&mut self) -> Result<char, Error> {
        let Some(mut code) = self.hex4(self.p) else {
            return self.fail("invalid Unicode escape");
        };
        self.p += 4;
        if (0xD800..=0xDBFF).contains(&code) {
            if self.text.get(self.p..self.p + 2) != Some(b"\\u") {
                return self.fail("missing low surrogate");
            }
            match self.hex4(self.p + 2) {
                Some(low) if (0xDC00..=0xDFFF).contains(&low) => {
                    code = 0x10000 + (code - 0xD800) * 1024 + (low - 0xDC00);
                    self.p += 6;
                }
                _ => return self.fail("invalid low surrogate"),
            }
        } else if (0xDC00..=0xDFFF).contains(&code) {
            return self.fail("unexpected low surrogate");
        }
        Ok(char::from_u32(code).expect("surrogates excluded above"))
    }

    fn read_token(&mut self) -> Result<Value, Error> {
        let start = self.p;
        while self.peek().is_some_and(|c| !is_delimiter(c)) {
            self.p += 1;
        }
        let token = &self.text[start..self.p];
        let text = std::str::from_utf8(token).unwrap_or("");
        match text {
            "" => return self.fail("expected value"),
            "nil" => return Ok(Value::Nil),
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            _ => {}
        }
        if token[0] == b':' && token.len() > 1 {
            return Ok(Value::Keyword(text[1..].to_owned()));
        }
        let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
        if unsigned.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return self.number(text);
        }
        if is_symbol(token) {
            return Ok(Value::Symbol(text.to_owned()));
        }
        self.fail(&format!("unsupported token {text}"))
    }

    fn number(&self, token: &str) -> Result<Value, Error> {
        let unsupported = || self.fail(&format!("unsupported EDN number {token}"));
        let (mantissa, exponent) = match token.find(['e', 'E']) {
            Some(i) => (&token[..i], Some(&token[i + 1..])),
            None => (token, None),
        };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if let Some(exponent) = exponent
            && !digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent))
        {
            return unsupported();
        }
        let body = mantissa.strip_prefix(['+', '-']).unwrap_or(mantissa);
        let (whole, fraction) = match body.split_once('.') {
            Some((whole, fraction)) => (whole, Some(fraction)),
            None => (body, None),
        };
        if !digits(whole) || fraction.is_some_and(|f| !f.is_empty() && !digits(f)) {
            return unsupported();
        }
        if exponent.is_none() && fraction.is_none() {
            return match token.parse::<i64>() {
                Ok(n) if n.abs() <= MAX_EXACT_INTEGER => Ok(Value::Integer(n)),
                _ => self.fail(RANGE_MESSAGE),
            };
        }
        let Ok(number) = token.parse::<f64>() else {
            return unsupported();
        };
        if !number.is_finite() {
            return self.fail("non-finite EDN number");
        }
        if number.fract() == 0.0 && number.abs() > MAX_EXACT_INTEGER as f64 {
            return self.fail(RANGE_MESSAGE);
        }
        Ok(Value::Float(number))
    }
}

/// Encodes on a single line with sorted map keys and set elements.
pub fn encode(value: &Value) -> Result<String, Error> {
    let mut out = String::new();
    write_value(value, &mut out)?;
    Ok(out)
}

fn encode_error<T>(message: &str) -> Result<T, Error> {
    Err(Error {
        offset: 0,
        message: message.to_owned(),
    })
}

fn write_value(value: &Value, out: &mut String) -> Result<(), Error> {
    match value {
        Value::Nil => out.push_str("nil"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Integer(n) => {
            if n.abs() > MAX_EXACT_INTEGER {
                return encode_error(RANGE_MESSAGE);
            }
            out.push_str(&n.to_string());
        }
        Value::Float(x) => {
            if !x.is_finite() {
                return encode_error("non-finite EDN number");
            }
            if x.fract() == 0.0 && x.abs() > MAX_EXACT_INTEGER as f64 {
                return encode_error(RANGE_MESSAGE);
            }
            // Shortest round-trip form; Debug always keeps `.` or `e`, so the
            // value reads back as a float.
            out.push_str(&format!("{x:?}"));
        }
        Value::String(s) => write_string(s, out),
        Value::Keyword(name) => {
            out.push(':');
            out.push_str(name);
        }
        Value::Symbol(name) => out.push_str(name),
        Value::Vector(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        Value::Map(entries) => {
            let mut encoded = entries
                .iter()
                .map(|(k, v)| Ok((encode(k)?, encode(v)?)))
                .collect::<Result<Vec<_>, Error>>()?;
            encoded.sort();
            out.push('{');
            for (i, (k, v)) in encoded.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(k);
                out.push(' ');
                out.push_str(v);
            }
            out.push('}');
        }
        Value::Set(items) => {
            let mut encoded = items
                .iter()
                .map(encode)
                .collect::<Result<Vec<_>, Error>>()?;
            encoded.sort();
            out.push_str("#{");
            out.push_str(&encoded.join(" "));
            out.push('}');
        }
        Value::Tagged(tag, inner) => {
            out.push('#');
            out.push_str(tag);
            out.push(' ');
            write_value(inner, out)?;
        }
    }
    Ok(())
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
