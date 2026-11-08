//! The writer for `.metrics` snapshots: one place decides how strings and
//! numbers are spelled and in which order a row's keys appear. The format
//! is the Python tools' own, not `crates/edn`'s encoder (sorted keys, one
//! line): strings escape only `\` and `"`, and a decimal is rounded to a
//! fixed number of places with trailing zeros dropped (`68.0`, `3.1`).

/// Python `round(value, places)`: correctly rounded, ties to even.
pub fn round(value: f64, places: usize) -> f64 {
    format!("{value:.places$}").parse().unwrap_or(value)
}

/// An EDN string literal.
pub fn string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `f"{value:.{places}f}"` without trailing zeros, keeping one decimal.
pub fn decimal(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    let mut text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    };
    if !text.contains('.') {
        text.push_str(".0");
    }
    text
}

/// One snapshot row: keys in insertion order, `{:a 1, :b 2}` or
/// `{:a 1 :b 2}` by the separator a format uses.
pub struct Row {
    separator: &'static str,
    fields: Vec<String>,
}

impl Row {
    pub fn new(separator: &'static str) -> Row {
        Row {
            separator,
            fields: Vec::new(),
        }
    }

    fn field(mut self, key: &str, value: String) -> Row {
        self.fields.push(format!(":{key} {value}"));
        self
    }

    pub fn string(self, key: &str, value: &str) -> Row {
        self.field(key, string(value))
    }

    pub fn integer(self, key: &str, value: impl std::fmt::Display) -> Row {
        self.field(key, value.to_string())
    }

    /// A decimal rounded to `places`, or `nil` when absent.
    pub fn decimal(self, key: &str, value: Option<f64>, places: usize) -> Row {
        let text = value.map_or_else(|| "nil".to_string(), |value| decimal(value, places));
        self.field(key, text)
    }

    pub fn render(&self) -> String {
        format!("{{{}}}", self.fields.join(self.separator))
    }
}

/// `{:<key> [\n  row\n  row\n]}\n`, or `{:<key> []}\n` without rows.
pub fn vector_map(key: &str, rows: &[Row]) -> String {
    if rows.is_empty() {
        return format!("{{:{key} []}}\n");
    }
    let body: Vec<String> = rows
        .iter()
        .map(|row| format!("  {}", row.render()))
        .collect();
    format!("{{:{key} [\n{}\n]}}\n", body.join("\n"))
}
