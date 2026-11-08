//! The CRAP formula, the sorted entries, the text report and the
//! `.metrics/crap.edn` snapshot.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use quality_core::coverage::CoverageBundle;
use quality_core::functions::Function;
use quality_core::language::Language;
use quality_core::snapshot::{self, Row};

/// One row of the report and of the snapshot.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub namespace: String,
    pub complexity: u32,
    pub coverage: Option<f64>,
    pub crap: Option<f64>,
    pub path: String,
    pub language: Language,
}

/// CRAP = CC² × (1 − coverage)³ + CC; no score without coverage.
pub fn crap_score(complexity: u32, coverage_pct: Option<f64>) -> Option<f64> {
    let coverage = coverage_pct?;
    let cc = f64::from(complexity);
    let uncovered = 1.0 - (coverage / 100.0);
    Some((cc * cc * uncovered * uncovered * uncovered) + cc)
}

const PLACES: usize = 4;

pub fn make_entry(function: &Function, coverage: Option<f64>) -> Entry {
    let coverage = coverage.map(|value| snapshot::round(value, PLACES));
    Entry {
        name: function.name.clone(),
        namespace: function.namespace.clone(),
        complexity: function.complexity,
        coverage,
        crap: crap_score(function.complexity, coverage).map(|value| snapshot::round(value, PLACES)),
        path: function.path.clone(),
        language: function.language,
    }
}

/// Worst CRAP first; unscored rows after scored rows, by complexity.
pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        let rank = |entry: &Entry| match entry.crap {
            Some(crap) => (0, -crap),
            None => (1, -f64::from(entry.complexity)),
        };
        let (a_rank, a_score) = rank(a);
        let (b_rank, b_score) = rank(b);
        a_rank
            .cmp(&b_rank)
            .then(a_score.partial_cmp(&b_score).unwrap_or(Ordering::Equal))
            .then_with(|| a.namespace.cmp(&b.namespace))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
}

/// Entries for every function in `files` (project-relative to `root`).
pub fn analyze(
    files: &[PathBuf],
    root: &Path,
    bundle: Option<&CoverageBundle>,
) -> std::io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for file in files {
        let Some(language) = quality_core::language::language_of(file) else {
            continue;
        };
        let source = quality_core::grammar::read_source(file)?;
        let relative = quality_core::paths::relative_posix(file, root)
            .unwrap_or_else(|| quality_core::paths::posix(file));
        for function in
            quality_core::functions::functions_in_file(language, &source, &relative, root)
        {
            let coverage = bundle.and_then(|bundle| bundle.percent_for(&function));
            entries.push(make_entry(&function, coverage));
        }
    }
    sort_entries(&mut entries);
    Ok(entries)
}

/// The snapshot uml-viewer joins on `:namespace` and `:name`.
pub fn render_edn(entries: &[Entry]) -> String {
    let rows: Vec<Row> = entries
        .iter()
        .map(|entry| {
            Row::new(", ")
                .string("name", &entry.name)
                .string("namespace", &entry.namespace)
                .integer("complexity", entry.complexity)
                .decimal("coverage", entry.coverage, PLACES)
                .decimal("crap", entry.crap, PLACES)
        })
        .collect();
    snapshot::vector_map("entries", &rows)
}

pub fn metrics_path(root: &Path) -> PathBuf {
    root.join(".metrics").join("crap.edn")
}

fn pad(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

/// The text report, worst first.
pub fn format_report(entries: &[Entry]) -> String {
    let widest = |key: fn(&Entry) -> usize| entries.iter().map(key).max().unwrap_or(0);
    let mut name_width = 30;
    let mut namespace_width = 35;
    if !entries.is_empty() {
        name_width = name_width.max(widest(|e| e.name.chars().count()).min(48));
        namespace_width = namespace_width.max(widest(|e| e.namespace.chars().count()).min(72));
    }
    let header = format!(
        "{} {} {:>4} {:>7} {:>8}",
        pad("Function", name_width),
        pad("Namespace", namespace_width),
        "CC",
        "Cov%",
        "CRAP"
    );
    let separator = "-".repeat(header.chars().count());
    let mut languages: Vec<&str> = entries.iter().map(|entry| entry.language.name()).collect();
    languages.sort();
    languages.dedup();
    let mut lines = vec!["CRAP Report".to_string(), "===========".to_string()];
    if !languages.is_empty() {
        lines.push(format!("Languages: {}", languages.join(", ")));
        lines.push(String::new());
    }
    lines.push(header);
    lines.push(separator);
    for entry in entries {
        let coverage = entry
            .coverage
            .map_or_else(|| "  N/A ".to_string(), |value| format!("{value:5.1}%"));
        let crap = entry
            .crap
            .map_or_else(|| "     N/A".to_string(), |value| format!("{value:8.1}"));
        lines.push(format!(
            "{} {} {:4} {coverage} {crap}",
            pad(&entry.name, name_width),
            pad(&entry.namespace, namespace_width),
            entry.complexity
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}
