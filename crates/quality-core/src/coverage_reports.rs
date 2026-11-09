//! Native report formats used by Go, Java and Cloverage.
use crate::coverage::{
    Lines, Records, SuffixIndex, candidates, coverage_percent, normalize_path, python_repr,
    python_strip, report_texts, splitlines,
};
use crate::functions::Function;
use crate::language::Language;
use crate::paths::posix;
use regex::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// crapper `suffix_match`: `suffix`'s path segments end `path`'s.
pub fn suffix_match(path: &str, suffix: &str) -> bool {
    let segments = |p: &str| crate::coverage::segments(p);
    let a = segments(path);
    let b = segments(suffix);
    !b.is_empty() && a.len() >= b.len() && a[a.len() - b.len()..] == b
}
/// Profile source → (start line, end line, statements, executions).
pub(crate) type GoProfile = Vec<(String, Vec<(usize, usize, u64, u64)>)>;

/// crapper `_profile_segments`: the first profile file any candidate name of
/// `path` matches, exactly or by a path suffix either way.
pub(crate) fn go_segments<'p>(
    profile: &'p GoProfile,
    path: &str,
) -> Option<&'p [(usize, usize, u64, u64)]> {
    for candidate in candidates(path) {
        if let Some((_, segments)) = profile.iter().find(|(key, _)| {
            normalize_path(key) == candidate
                || suffix_match(key, &candidate)
                || suffix_match(&candidate, key)
        }) {
            return Some(segments);
        }
    }
    None
}

pub(crate) const GO_REPORTS: &[&str] = &[
    "target/coverage/coverage.out",
    "target/coverage/go/coverage.out",
    "coverage.out",
    "*/target/coverage/go/coverage.out",
    "*/*/target/coverage/go/coverage.out",
];
pub(crate) const JACOCO_REPORTS: &[&str] = &[
    "target/site/jacoco/jacoco.xml",
    "*/target/site/jacoco/jacoco.xml",
    "*/*/target/site/jacoco/jacoco.xml",
];

/// JaCoCo XML with its DOCTYPE removed, as crapper and mutator parse it.
pub(crate) fn jacoco_document(text: &str) -> String {
    Regex::new(r"(?i)<!DOCTYPE[^>]*>")
        .unwrap()
        .replace_all(text, "")
        .into_owned()
}

/// crapper `_merge_forms`: Cloverage's per-line form counts for every HTML
/// page under `target/coverage`, keyed by the page path without `.html`.
pub(crate) fn cloverage_forms(root: &Path) -> Records {
    let mut forms = Records::default();
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
        forms.insert(key, lines);
    }
    forms
}
pub(super) struct Reports {
    go: GoProfile,
    java: Vec<(String, String, usize, u64, u64)>,
    forms: SuffixIndex,
}

impl Default for Reports {
    fn default() -> Self {
        Reports {
            go: GoProfile::new(),
            java: Vec::new(),
            forms: SuffixIndex::new(&Records::default()),
        }
    }
}

impl Reports {
    /// crapper `_SuffixIndex.get` for Cloverage HTML: an exact report key
    /// wins, then the earliest record ending with a candidate.
    pub fn forms_percent(&self, path: &str) -> Option<&Lines> {
        self.forms.get(path)
    }
    pub fn percent(&self, f: &Function) -> Option<f64> {
        match f.language {
            Language::Go => {
                let segments = go_segments(&self.go, &f.path)?;
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
                Some(coverage_percent(covered, total))
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
    /// crapper `parse_jacoco_index`: unparseable XML raises for the run.
    fn add_java(&mut self, text: &str) -> Result<(), String> {
        let clean = jacoco_document(text);
        let doc = roxmltree::Document::parse(&clean)
            .map_err(|error| format!("invalid JaCoCo XML: {error}"))?;
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
        Ok(())
    }
    pub fn load(root: &Path) -> Result<Self, String> {
        let mut reports = Self::default();
        for text in report_texts(root, GO_REPORTS) {
            parse_go_profile(&text, &mut reports.go)?;
        }
        for text in report_texts(root, JACOCO_REPORTS) {
            reports.add_java(&text)?;
        }
        reports.forms = SuffixIndex::new(&cloverage_forms(root));
        Ok(reports)
    }
}

/// crapper `parse_go_profile`, merged into `profile` as `_merge_lists` does:
/// an unusable segment raises, so the run fails instead of publishing
/// unmeasured classes.
pub(crate) fn parse_go_profile(text: &str, profile: &mut GoProfile) -> Result<(), String> {
    for (index, raw) in splitlines(text).into_iter().enumerate() {
        let number = index + 1;
        let line = python_strip(raw);
        if line.is_empty() || line.starts_with("mode:") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let invalid = || {
            format!(
                "invalid coverage segment on line {number}: {}",
                python_repr(raw)
            )
        };
        if fields.len() != 3 || !fields[0].contains(':') {
            return Err(invalid());
        }
        let Some((file, range)) = fields[0].split_once(':') else {
            return Err(invalid());
        };
        let Some((start, end)) = range.split_once(',') else {
            return Err(invalid());
        };
        let parse_line = |s: &str| s.split('.').next().and_then(|n| n.parse::<usize>().ok());
        let (Some(start), Some(end), Ok(statements), Ok(hits)) = (
            parse_line(start),
            parse_line(end),
            fields[1].parse(),
            fields[2].parse(),
        ) else {
            return Err(invalid());
        };
        let position = profile
            .iter()
            .position(|(key, _)| key == file)
            .unwrap_or_else(|| {
                profile.push((file.into(), Vec::new()));
                profile.len() - 1
            });
        profile[position].1.push((start, end, statements, hits));
    }
    Ok(())
}
