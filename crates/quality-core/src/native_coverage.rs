use std::collections::BTreeSet;
use std::path::Path;

use crate::coverage::{LCOV_REPORTS, Records, parse_lcov};
use crate::coverage_reports::{
    GO_REPORTS, GoProfile, JACOCO_REPORTS, go_segments, jacoco_document, parse_go_profile,
    tree_cloverage_forms,
};
use crate::report_files::{ReportTree, tree_report_texts};

#[derive(Default)]
pub struct NativeReports {
    go: GoProfile,
    forms: Records,
    lcov: Records,
}

impl NativeReports {
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::load_tree(ReportTree::plain(root))
    }

    pub fn load_tree(tree: ReportTree<'_>) -> Result<Self, String> {
        let mut reports = NativeReports::default();
        for text in tree_report_texts(tree, GO_REPORTS) {
            parse_go_profile(&text, &mut reports.go)?;
        }
        for text in tree_report_texts(tree, JACOCO_REPORTS) {
            roxmltree::Document::parse(&jacoco_document(&text))
                .map_err(|error| format!("invalid JaCoCo XML: {error}"))?;
        }
        reports.forms = tree_cloverage_forms(tree);
        for text in tree_report_texts(tree, LCOV_REPORTS) {
            reports.lcov.update(parse_lcov(&text));
        }
        Ok(reports)
    }

    pub fn go_report(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("Go coverage report unavailable: {error}"))?;
        let mut reports = Self::default();
        parse_go_profile(&text, &mut reports.go)?;
        Ok(reports)
    }

    pub fn rebase(&mut self, from: &Path, to: &Path) {
        let from = format!("{}/", crate::paths::posix(from).trim_end_matches('/'));
        let to = crate::paths::posix(to);
        let key = |source: &str| {
            let normalized = source.replace('\\', "/");
            normalized
                .strip_prefix(&from)
                .map(|relative| format!("{}/{relative}", to.trim_end_matches('/')))
                .unwrap_or_else(|| source.to_string())
        };
        for (source, _) in &mut self.go {
            *source = key(source);
        }
        let mut lcov = Records::default();
        for (source, lines) in self.lcov.iter() {
            lcov.insert(key(source), lines.clone());
        }
        self.lcov = lcov;
    }

    pub fn invalidate(&mut self, language: crate::language::Language) {
        match language {
            crate::language::Language::Go => self.go.clear(),
            crate::language::Language::Clojure => {
                self.forms = Records::default();
                self.lcov = Records::default();
            }
            _ => {}
        }
    }

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

    pub fn go_lines_exact(&self, path: &str) -> Option<BTreeSet<usize>> {
        let (_, segments) = self.go.iter().find(|(key, _)| {
            crate::coverage::normalize_path(key) == crate::coverage::normalize_path(path)
        })?;
        Some(
            segments
                .iter()
                .filter(|(_, _, _, hits)| *hits > 0)
                .flat_map(|(start, end, _, _)| *start..=*end)
                .collect(),
        )
    }

    pub fn forms(&self) -> &Records {
        &self.forms
    }

    pub fn lcov(&self) -> &Records {
        &self.lcov
    }
}

fn attribute(value: Option<&str>) -> Option<i64> {
    let text = value.filter(|text| !text.is_empty()).unwrap_or("0");
    text.trim().parse().ok()
}

pub fn jacoco_lines(root: &Path) -> Vec<(String, BTreeSet<usize>)> {
    jacoco_lines_tree(ReportTree::plain(root))
}

pub fn jacoco_lines_tree(tree: ReportTree<'_>) -> Vec<(String, BTreeSet<usize>)> {
    jacoco_texts(tree_report_texts(tree, JACOCO_REPORTS))
}

pub fn jacoco_report(path: &Path) -> Vec<(String, BTreeSet<usize>)> {
    jacoco_texts(std::fs::read_to_string(path))
}

fn jacoco_texts(texts: impl IntoIterator<Item = String>) -> Vec<(String, BTreeSet<usize>)> {
    let mut found: Vec<(String, BTreeSet<usize>)> = Vec::new();
    for text in texts {
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
