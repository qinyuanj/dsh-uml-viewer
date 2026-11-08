//! Native report formats used by Go, Java and Cloverage.
use crate::coverage::{Lines, Records, candidates, coverage_percent, normalize_path, report_texts};
use crate::functions::Function;
use crate::language::Language;
use crate::paths::posix;
use regex::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn suffix_match(path: &str, suffix: &str) -> bool {
    let segments = |p: &str| crate::coverage::segments(p);
    let a = segments(path);
    let b = segments(suffix);
    !b.is_empty() && a.len() >= b.len() && a[a.len() - b.len()..] == b
}
#[derive(Default)]
pub(super) struct Reports {
    go: Vec<(String, Vec<(usize, usize, u64, u64)>)>,
    java: Vec<(String, String, usize, u64, u64)>,
    pub forms: Records,
}
impl Reports {
    pub fn forms_percent(&self, path: &str) -> Option<&Lines> {
        let candidates = candidates(path);
        self.forms
            .iter()
            .find(|(key, _)| {
                candidates
                    .iter()
                    .any(|candidate| suffix_match(key, candidate))
            })
            .map(|(_, lines)| lines)
    }
    pub fn percent(&self, f: &Function) -> Option<f64> {
        match f.language {
            Language::Go => {
                for candidate in candidates(&f.path) {
                    if let Some((_, segments)) = self.go.iter().find(|(key, _)| {
                        normalize_path(key) == candidate
                            || suffix_match(key, &candidate)
                            || suffix_match(&candidate, key)
                    }) {
                        let (mut covered, mut total) = (0, 0);
                        for &(start, end, statements, hits) in segments {
                            if end < f.start_line || start > f.end_line {
                                continue;
                            }
                            total += statements;
                            if hits > 0 {
                                covered += statements;
                            }
                        }
                        return Some(coverage_percent(covered, total));
                    }
                }
                None
            }
            Language::Java => self
                .java
                .iter()
                .filter(|(class, name, _, _, _)| {
                    f.jacoco_class.as_deref().map_or_else(
                        || class.replace('$', ".") == f.namespace,
                        |key| class == key,
                    ) && name == &f.name
                })
                .min_by_key(|(_, _, line, _, _)| line.abs_diff(f.start_line))
                .map(|(_, _, _, missed, covered)| coverage_percent(*covered, missed + covered)),
            _ => None,
        }
    }
    fn add_go(&mut self, text: &str) {
        for raw in text.lines() {
            let fields: Vec<&str> = raw.split_whitespace().collect();
            if fields.is_empty() || raw.trim().starts_with("mode:") {
                continue;
            }
            if fields.len() != 3 {
                continue;
            }
            let Some((file, range)) = fields[0].split_once(':') else {
                continue;
            };
            let Some((start, end)) = range.split_once(',') else {
                continue;
            };
            let parse_line = |s: &str| s.split('.').next()?.parse::<usize>().ok();
            let (Some(start), Some(end), Ok(statements), Ok(hits)) = (
                parse_line(start),
                parse_line(end),
                fields[1].parse(),
                fields[2].parse(),
            ) else {
                continue;
            };
            let index = self
                .go
                .iter()
                .position(|(key, _)| key == file)
                .unwrap_or_else(|| {
                    self.go.push((file.into(), Vec::new()));
                    self.go.len() - 1
                });
            self.go[index].1.push((start, end, statements, hits));
        }
    }
    fn add_java(&mut self, text: &str) {
        let clean = Regex::new(r"(?i)<!DOCTYPE[^>]*>")
            .unwrap()
            .replace_all(text, "");
        let Ok(doc) = roxmltree::Document::parse(&clean) else {
            return;
        };
        for class in doc.descendants().filter(|n| n.has_tag_name("class")) {
            let name = class.attribute("name").unwrap_or("").replace('/', ".");
            for method in class.children().filter(|n| n.has_tag_name("method")) {
                let Some(counter) = method.children().find(|n| {
                    n.has_tag_name("counter") && n.attribute("type") == Some("INSTRUCTION")
                }) else {
                    continue;
                };
                let line = method.attribute("line").unwrap_or("0").parse().unwrap_or(0);
                let missed = counter
                    .attribute("missed")
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0);
                let covered = counter
                    .attribute("covered")
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0);
                self.java.push((
                    name.clone(),
                    method.attribute("name").unwrap_or("").into(),
                    line,
                    missed,
                    covered,
                ));
            }
        }
    }
    pub fn load(root: &Path) -> Self {
        let mut reports = Self::default();
        for text in report_texts(
            root,
            &[
                "target/coverage/coverage.out",
                "target/coverage/go/coverage.out",
                "coverage.out",
                "*/target/coverage/go/coverage.out",
                "*/*/target/coverage/go/coverage.out",
            ],
        ) {
            reports.add_go(&text);
        }
        for text in report_texts(
            root,
            &[
                "target/site/jacoco/jacoco.xml",
                "*/target/site/jacoco/jacoco.xml",
                "*/*/target/site/jacoco/jacoco.xml",
            ],
        ) {
            reports.add_java(&text);
        }
        let coverage = root.join("target/coverage");
        let span =
            Regex::new(r#"<span[^>]*title="(\d+) out of (\d+) forms covered"[^>]*>\s*(\d+)&nbsp;"#)
                .unwrap();
        fn htmls(root: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(root) else {
                return;
            };
            let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    htmls(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "html") {
                    out.push(path);
                }
            }
        }
        let mut paths = Vec::new();
        htmls(&coverage, &mut paths);
        for path in paths {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if !text.contains("forms covered") {
                continue;
            }
            let mut lines = Lines::new();
            for cap in span.captures_iter(&text) {
                let (covered, total, line) = (
                    cap[1].parse::<u64>().unwrap_or(0),
                    cap[2].parse::<u64>().unwrap_or(0),
                    cap[3].parse::<u64>().unwrap_or(0),
                );
                if total > 0 {
                    lines.insert(line, (covered, total));
                }
            }
            let key = posix(&path.strip_prefix(&coverage).unwrap().with_extension(""));
            reports.forms.insert(key, lines);
        }
        reports
    }
}
