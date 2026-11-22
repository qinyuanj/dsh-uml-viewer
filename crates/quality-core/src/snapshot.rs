pub fn round(value: f64, places: usize) -> f64 {
    format!("{value:.places$}").parse().unwrap_or(value)
}

pub fn string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

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

    pub fn decimal(self, key: &str, value: Option<f64>, places: usize) -> Row {
        let text = value.map_or_else(|| "nil".to_string(), |value| decimal(value, places));
        self.field(key, text)
    }

    pub fn render(&self) -> String {
        format!("{{{}}}", self.fields.join(self.separator))
    }
}

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
