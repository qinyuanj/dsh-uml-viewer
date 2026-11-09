//! The per-line coverage mutator reads for Go, Java and Clojure.
//!
//! crapper scores a function from these reports; mutator asks which lines a
//! test hit (mutator `coverage.py`). The readers are crapper's own: the Go
//! statement profile and Cloverage form counts load exactly as `load_bundle`
//! loads them, including its failure on a malformed Go profile or JaCoCo
//! report. Java's line index is mutator's `_jacoco_index`, which skips an
//! unreadable report instead.

use std::collections::BTreeSet;
use std::path::Path;

use crate::coverage::{LCOV_REPORTS, Records, parse_lcov, report_texts};
use crate::coverage_reports::{
    GO_REPORTS, GoProfile, JACOCO_REPORTS, cloverage_forms, go_segments, jacoco_document,
    parse_go_profile,
};

/// The reports `load_bundle` reads under one directory, in its order.
#[derive(Default)]
pub struct NativeReports {
    go: GoProfile,
    forms: Records,
    lcov: Records,
}

impl NativeReports {
    /// crapper `load_bundle(root)`: the Go profiles, the Cloverage pages and
    /// the LCOV reports. A malformed Go profile or JaCoCo report is the error
    /// crapper raises.
    pub fn load(root: &Path) -> Result<Self, String> {
        let mut reports = NativeReports::default();
        for text in report_texts(root, GO_REPORTS) {
            parse_go_profile(&text, &mut reports.go)?;
        }
        for text in report_texts(root, JACOCO_REPORTS) {
            roxmltree::Document::parse(&jacoco_document(&text))
                .map_err(|error| format!("invalid JaCoCo XML: {error}"))?;
        }
        reports.forms = cloverage_forms(root);
        for text in report_texts(root, LCOV_REPORTS) {
            reports.lcov.update(parse_lcov(&text));
        }
        Ok(reports)
    }

    /// Reports another directory produced, after these, as one glob over
    /// both would list them.
    pub fn extend(&mut self, other: NativeReports) {
        for (file, segments) in other.go {
            match self.go.iter_mut().find(|(key, _)| *key == file) {
                Some((_, known)) => known.extend(segments),
                None => self.go.push((file, segments)),
            }
        }
        self.forms.update(other.forms);
        self.lcov.update(other.lcov);
    }

    /// mutator `_go_lines`: every line of a segment with a hit, or None when
    /// no profile names the file (crapper `_profile_segments`).
    pub fn go_lines(&self, path: &str) -> Option<BTreeSet<usize>> {
        if self.go.is_empty() {
            return None;
        }
        let segments = go_segments(&self.go, path)?;
        Some(
            segments
                .iter()
                .filter(|(_, _, _, hits)| *hits > 0)
                .flat_map(|(start, end, _, _)| *start..=*end)
                .collect(),
        )
    }

    /// Cloverage's per-line form counts, keyed by report path.
    pub fn forms(&self) -> &Records {
        &self.forms
    }

    /// The merged LCOV records, keyed by report path.
    pub fn lcov(&self) -> &Records {
        &self.lcov
    }
}

/// Python `int(text or "0")` for the plain decimal attributes JaCoCo writes.
fn attribute(value: Option<&str>) -> Option<i64> {
    let text = value.filter(|text| !text.is_empty()).unwrap_or("0");
    text.trim().parse().ok()
}

/// mutator `_jacoco_index`: `package/File.java` → the lines with a covered
/// instruction or branch, over every JaCoCo report under `root`. A report
/// that does not parse is skipped.
pub fn jacoco_lines(root: &Path) -> Vec<(String, BTreeSet<usize>)> {
    let mut found: Vec<(String, BTreeSet<usize>)> = Vec::new();
    for text in report_texts(root, JACOCO_REPORTS) {
        let clean = jacoco_document(&text);
        let Ok(document) = roxmltree::Document::parse(&clean) else {
            continue;
        };
        for package in document.descendants().filter(|n| n.has_tag_name("package")) {
            let name = package.attribute("name").unwrap_or("").trim_matches('/');
            for source in package.children().filter(|n| n.has_tag_name("sourcefile")) {
                let file = source.attribute("name").unwrap_or("");
                let key = if name.is_empty() {
                    file.to_string()
                } else {
                    format!("{name}/{file}")
                };
                let mut covered = BTreeSet::new();
                for line in source.children().filter(|n| n.has_tag_name("line")) {
                    let (Some(number), Some(instructions), Some(branches)) = (
                        attribute(line.attribute("nr")),
                        attribute(line.attribute("ci")),
                        attribute(line.attribute("cb")),
                    ) else {
                        continue;
                    };
                    if instructions > 0 || branches > 0 {
                        covered.insert(number.max(0) as usize);
                    }
                }
                match found.iter_mut().find(|(known, _)| *known == key) {
                    Some((_, lines)) => *lines = covered,
                    None => found.push((key, covered)),
                }
            }
        }
    }
    found
}
