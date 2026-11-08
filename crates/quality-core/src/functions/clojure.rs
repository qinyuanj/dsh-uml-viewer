//! Crapper's custom Clojure reader, preserving the original scanner semantics.
use super::Function;
use crate::language::Language;
use std::path::Path;

fn string_end(chars: &[char], start: usize) -> usize {
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            i = (i + 2).min(chars.len());
        } else if chars[i] == '"' {
            return i + 1;
        } else {
            i += 1;
        }
    }
    i
}
fn clean(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => {
                let end = string_end(&chars, i);
                out.push('"');
                for (j, ch) in chars.iter().enumerate().take(end).skip(i + 1) {
                    out.push(if *ch == '\n' || (j + 1 == end && *ch == '"') {
                        *ch
                    } else {
                        ' '
                    });
                }
                i = end;
            }
            ';' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}
fn top_forms(chars: &[char], start: usize) -> i64 {
    let (mut depth, mut forms, mut in_form) = (1, 0, false);
    for ch in &chars[start..] {
        if ")}]".contains(*ch) {
            depth -= 1;
            if depth == 0 {
                break;
            }
        } else if ch.is_whitespace() {
            in_form = depth != 1;
        } else {
            if !in_form {
                forms += 1;
            }
            in_form = true;
            if "({[".contains(*ch) {
                depth += 1;
            }
        }
    }
    forms
}
fn complexity(source: &str) -> u32 {
    let chars: Vec<char> = clean(source).chars().collect();
    let mut count = 1i64;
    for i in 0..chars.len() {
        if chars[i] != '(' {
            continue;
        }
        let end = (i + 1..chars.len())
            .find(|j| chars[*j].is_whitespace() || chars[*j] == ')')
            .unwrap_or(chars.len());
        if end == chars.len() {
            continue;
        }
        let head: String = chars[i + 1..end].iter().collect();
        match head.as_str() {
            "if-not" | "if-let" | "if-some" | "when-not" | "when-let" | "when-some"
            | "when-first" | "if" | "when" | "and" | "or" | "loop" | "catch" => count += 1,
            "cond" | "condp" | "case" | "cond->" | "cond->>" | "some->" | "some->>" => {
                let leading = match head.as_str() {
                    "condp" => 3,
                    "cond" => 1,
                    _ => 2,
                };
                let remaining = top_forms(&chars, i + 1) - leading;
                count += if head == "some->" || head == "some->>" {
                    remaining
                } else {
                    remaining.div_euclid(2) + i64::from(head == "case" && remaining % 2 != 0)
                };
            }
            _ => {}
        }
    }
    count.max(0) as u32
}
fn namespace(source: &str, path: &str, root: &Path) -> String {
    fn symbol(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || "*+!_?.-/".contains(ch)
    }
    for head in ["ns", "in-ns", "in-ns-quote"] {
        for (i, _) in source.match_indices('(') {
            let rest = source[i + 1..].trim_start();
            let target = if head == "ns" { "ns" } else { "in-ns" };
            let Some(rest) = rest
                .strip_prefix(target)
                .filter(|s| s.starts_with(char::is_whitespace))
            else {
                continue;
            };
            let rest = rest.trim_start();
            let rest = match head {
                "in-ns" => {
                    let Some(r) = rest.strip_prefix('\'') else {
                        continue;
                    };
                    r
                }
                "in-ns-quote" => {
                    let Some(r) = rest
                        .strip_prefix("(quote")
                        .filter(|s| s.starts_with(char::is_whitespace))
                    else {
                        continue;
                    };
                    r.trim_start()
                }
                _ => rest,
            };
            let name: String = rest.chars().take_while(|ch| symbol(*ch)).collect();
            if name.is_empty() {
                continue;
            }
            let tail = &rest[name.len()..];
            if head == "in-ns" && !tail.trim_start().starts_with(')') {
                continue;
            }
            if head == "in-ns-quote" && !tail.starts_with(')') {
                continue;
            }
            return name;
        }
    }
    let root = root.to_string_lossy().replace('\\', "/");
    let mut relative = path.to_string();
    let prefix = format!("{}/", root.trim_end_matches('/'));
    if root != "." && relative.starts_with(&prefix) {
        relative = relative[prefix.len()..].to_string();
    }
    // crapper `strip_src`: a leading `src/`, else everything after the
    // first `/src/`.
    if let Some(rest) = relative.strip_prefix("src/") {
        relative = rest.to_string();
    } else if let Some((_, rest)) = relative.split_once("/src/") {
        relative = rest.to_string();
    }
    for ext in [".clj", ".cljc", ".cljs", ".bb"] {
        if let Some(stem) = relative.strip_suffix(ext) {
            relative = stem.to_string();
            break;
        }
    }
    relative.replace('/', ".").replace('_', "-")
}
pub fn functions(source: &str, path: &str, root: &Path) -> Vec<Function> {
    let chars: Vec<char> = source.chars().collect();
    let ns = namespace(source, path, root);
    let (mut i, mut line, mut depth, mut start, mut first_line) = (0, 1, 0usize, 0, 1);
    let mut found = Vec::new();
    while i < chars.len() {
        match chars[i] {
            ';' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                if i < chars.len() {
                    line += 1;
                    i += 1;
                }
                continue;
            }
            '"' => {
                let end = string_end(&chars, i);
                line += chars[i..end].iter().filter(|ch| **ch == '\n').count();
                i = end;
                continue;
            }
            '\\' => {
                i = (i + 2).min(chars.len());
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !"()[]{}\";,".contains(chars[i])
                {
                    i += 1;
                }
                continue;
            }
            '(' => {
                if depth == 0 {
                    start = i;
                    first_line = line;
                }
                depth += 1;
            }
            ')' => {
                if depth == 1 {
                    let form: String = chars[start..=i].iter().collect();
                    let rest = form[1..].trim_start();
                    let rest = rest
                        .strip_prefix("defn-")
                        .or_else(|| rest.strip_prefix("defn"));
                    if let Some(rest) = rest.filter(|s| s.starts_with(char::is_whitespace)) {
                        let name: String = rest
                            .trim_start()
                            .chars()
                            .take_while(|ch| !ch.is_whitespace() && !"()[]{}\"".contains(*ch))
                            .collect();
                        if !name.is_empty() {
                            found.push(Function::new(
                                name,
                                ns.clone(),
                                complexity(&form),
                                first_line,
                                line,
                                path.into(),
                                Language::Clojure,
                            ));
                        }
                    }
                }
                depth = depth.saturating_sub(1);
            }
            '\n' => line += 1,
            _ => {}
        }
        i += 1;
    }
    found
}
