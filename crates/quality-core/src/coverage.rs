//! Coverage reports and the coverage of each function's line range.
//!
//! LCOV line hits serve Rust (and later TypeScript, Python, Lua). Reports
//! already on disk are matched to sources by path suffix; a report a custom
//! coverage command just wrote is validated strictly and matched exactly.
//! A file with no coverage data scores N/A rather than 0%.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::functions::Function;
use crate::language::Language;
use crate::paths::{normcase, posix, resolve};

/// Line → (covered lines, lines): one `DA` record per line.
pub type Lines = HashMap<u64, (u64, u64)>;

/// Report records in report order; a repeated key keeps its first position
/// and takes the later value, as a Python dict does.
#[derive(Clone, Debug, Default)]
pub struct Records {
    order: Vec<String>,
    values: HashMap<String, Lines>,
}

impl Records {
    pub fn insert(&mut self, key: String, lines: Lines) {
        if !self.values.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.values.insert(key, lines);
    }

    pub fn update(&mut self, other: Records) {
        let Records { order, mut values } = other;
        for key in order {
            let lines = values.remove(&key).unwrap_or_default();
            self.insert(key, lines);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Lines)> {
        self.order.iter().map(|key| (key, &self.values[key]))
    }
}

/// Percent-decoding as `urllib.parse.unquote` does (invalid UTF-8 replaced).
fn unquote(text: &str) -> String {
    if !text.contains('%') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |at: usize| bytes.get(at).and_then(|b| (*b as char).to_digit(16));
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (hex(index + 1), hex(index + 2))
        {
            out.push((high * 16 + low) as u8);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A report path in comparable form: decoded, `/` separators, no `file:`
/// or leading `./`, no doubled slashes.
pub fn normalize_path(path: &str) -> String {
    let mut text = unquote(path).replace('\\', "/");
    if let Some(rest) = text.strip_prefix("file:") {
        text = rest.to_string();
    }
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_string();
    }
    while text.contains("//") {
        text = text.replace("//", "/");
    }
    text
}

pub(super) fn segments(path: &str) -> Vec<String> {
    normalize_path(path)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

/// The names one source path may appear under in a report.
pub(super) fn candidates(source_path: &str) -> Vec<String> {
    let relative = normalize_path(source_path);
    let absolute = if source_path.is_empty() {
        relative.clone()
    } else {
        normalize_path(&resolve(Path::new(source_path)).to_string_lossy())
    };
    let no_src = relative
        .strip_prefix("src/")
        .unwrap_or(&relative)
        .to_string();
    let absolute_no_src = absolute.replace("/src/", "/");
    let mut found: Vec<String> = Vec::new();
    for value in [relative, absolute, no_src, absolute_no_src] {
        if !value.is_empty() && !found.contains(&value) {
            found.push(value);
        }
    }
    found
}

/// `100 · covered / total`, 0 when there is nothing to count.
pub fn coverage_percent(covered: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    100.0 * covered as f64 / total as f64
}

/// Coverage of lines `start..=end`; None without data for the file.
pub fn percent_for_range(lines: Option<&Lines>, start: usize, end: usize) -> Option<f64> {
    let lines = lines?;
    let (mut covered, mut total) = (0, 0);
    for line in start..=end {
        if let Some((hit, count)) = lines.get(&(line as u64)) {
            covered += hit;
            total += count;
        }
    }
    Some(coverage_percent(covered, total))
}

/// `str.splitlines()`: every line boundary Python recognizes.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let boundary = matches!(
            c,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        lines.push(&text[start..at]);
        let mut next = at + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|(_, n)| *n == '\n') {
            chars.next();
            next += 1;
        }
        start = next;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn python_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

fn digits(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Permissive LCOV: `SF` opens a record, `DA:line,hits` lines fill it,
/// anything else is ignored.
pub fn parse_lcov(text: &str) -> Records {
    let mut out = Records::default();
    let mut current: Option<String> = None;
    for raw in splitlines(text) {
        let line = python_strip(raw);
        if let Some(source) = line.strip_prefix("SF:") {
            out.insert(source.to_string(), Lines::new());
            current = Some(source.to_string());
        } else if line == "end_of_record" {
            current = None;
        } else if let Some(source) = &current
            && let Some(rest) = line.strip_prefix("DA:")
        {
            // `DA:(\d+),(\d+)` matched at the start of the line.
            let (number, rest) = rest.split_at(
                rest.find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(rest.len()),
            );
            let Some(rest) = rest.strip_prefix(',') else {
                continue;
            };
            let hits = &rest[..rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len())];
            if let (Some(number), Some(hits)) = (digits(number), digits(hits)) {
                let lines = out.values.get_mut(source).unwrap();
                lines.insert(number, (u64::from(hits > 0), 1));
            }
        }
    }
    out
}

/// Validate the LCOV a coverage command just wrote, resolving `SF` against
/// `source_base`. Keys are the resolved source paths.
pub fn read_fresh_lcov(report: &Path, source_base: &Path) -> Result<Records, String> {
    let bytes = fs::read(report).map_err(|error| io_message(&error, report))?;
    let text = String::from_utf8(bytes).map_err(|error| {
        format!(
            "'utf-8' codec can't decode byte 0x{:02x} in position {}: invalid start byte",
            error.as_bytes()[error.utf8_error().valid_up_to()],
            error.utf8_error().valid_up_to()
        )
    })?;
    let ancillary = ["TN", "FN", "FNDA", "FNF", "FNH", "BRDA", "BRF", "BRH"];
    let mut out = Records::default();
    let mut sources: Vec<String> = Vec::new();
    let mut source: Option<String> = None;
    let mut lines = Lines::new();
    let mut counts: HashMap<String, u64> = HashMap::new();
    for (index, raw) in splitlines(&text).into_iter().enumerate() {
        let number = index + 1;
        let line = python_strip(raw);
        if line.is_empty() {
            continue;
        }
        let (tag, value) = line.split_once(':').unwrap_or((line, ""));
        if tag == "SF" {
            if source.is_some() || value.is_empty() || value.contains('\0') {
                return Err(format!("invalid SF on line {number}"));
            }
            let mut path = PathBuf::from(value.replace('\\', "/"));
            if !path.is_absolute() {
                path = source_base.join(path);
            }
            let path = resolve(&path);
            if !path.is_file() {
                return Err(format!("unusable LCOV source: {value}"));
            }
            fs::File::open(&path).map_err(|error| io_message(&error, &path))?;
            let identity = normcase(&path);
            if sources.contains(&identity) {
                return Err(format!("duplicate LCOV source: {value}"));
            }
            sources.push(identity);
            source = Some(path.to_string_lossy().into_owned());
            lines = Lines::new();
            counts.clear();
        } else if line == "end_of_record" {
            let hit: u64 = lines.values().map(|entry| entry.0).sum();
            let consistent =
                counts.get("LF") == Some(&(lines.len() as u64)) && counts.get("LH") == Some(&hit);
            let Some(done) = source.take().filter(|_| consistent) else {
                return Err(format!("missing or inconsistent LF/LH on line {number}"));
            };
            out.insert(done, std::mem::take(&mut lines));
        } else if tag == "DA" {
            let parsed = fresh_da(value);
            let (Some(_), Some((line_number, hits))) = (&source, parsed) else {
                return Err(format!("invalid DA on line {number}"));
            };
            if lines.contains_key(&line_number) {
                return Err(format!("duplicate DA on line {number}"));
            }
            lines.insert(line_number, (u64::from(hits > 0), 1));
        } else if tag == "LF" || tag == "LH" {
            if source.is_none() || counts.contains_key(tag) {
                return Err(format!("invalid {tag} on line {number}"));
            }
            let Some(count) = digits(value) else {
                return Err(format!("invalid {tag} on line {number}"));
            };
            counts.insert(tag.to_string(), count);
        } else if !ancillary.contains(&tag) {
            return Err(format!("invalid LCOV record on line {number}: {line}"));
        }
    }
    if source.is_some() || out.is_empty() {
        return Err("LCOV requires at least one complete source record".to_string());
    }
    Ok(out)
}

/// `([1-9]\d*),(\d+)(?:,[0-9a-fA-F]{32})?` as a full match.
fn fresh_da(value: &str) -> Option<(u64, u64)> {
    let mut fields = value.split(',');
    let line = fields.next()?;
    let hits = fields.next()?;
    if let Some(checksum) = fields.next()
        && (checksum.len() != 32 || !checksum.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return None;
    }
    if fields.next().is_some() || line.starts_with('0') {
        return None;
    }
    Some((digits(line)?, digits(hits)?))
}

/// An OSError as Python prints it.
fn io_message(error: &std::io::Error, path: &Path) -> String {
    let path = path.display();
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            format!("[Errno 2] No such file or directory: '{path}'")
        }
        std::io::ErrorKind::PermissionDenied => format!("[Errno 13] Permission denied: '{path}'"),
        _ => format!("{error}: '{path}'"),
    }
}

/// Legacy exact-then-suffix matching: the first report record, in report
/// order, whose path ends with any candidate name of the source.
struct SuffixIndex {
    normalized: HashMap<String, usize>,
    lines: Vec<Lines>,
    suffixes: HashMap<Vec<String>, usize>,
}

impl SuffixIndex {
    fn new(records: &Records) -> SuffixIndex {
        let mut index = SuffixIndex {
            normalized: HashMap::new(),
            lines: Vec::new(),
            suffixes: HashMap::new(),
        };
        let mut keys: Vec<String> = Vec::new();
        for (key, lines) in records.iter() {
            let key = normalize_path(key);
            match index.normalized.get(&key) {
                Some(&position) => index.lines[position] = lines.clone(),
                None => {
                    index.normalized.insert(key.clone(), index.lines.len());
                    index.lines.push(lines.clone());
                    keys.push(key);
                }
            }
        }
        for (position, key) in keys.iter().enumerate() {
            let parts = segments(key);
            for length in 1..=parts.len() {
                let suffix = parts[parts.len() - length..].to_vec();
                index.suffixes.entry(suffix).or_insert(position);
            }
        }
        index
    }

    fn get(&self, source_path: &str) -> Option<&Lines> {
        if self.lines.is_empty() {
            return None;
        }
        let candidates = candidates(source_path);
        for candidate in &candidates {
            if let Some(&position) = self.normalized.get(candidate) {
                return Some(&self.lines[position]);
            }
        }
        candidates
            .iter()
            .map(|candidate| segments(candidate))
            .filter(|key| !key.is_empty())
            .filter_map(|key| self.suffixes.get(&key).copied())
            .min()
            .map(|position| &self.lines[position])
    }
}

/// Coverage for every analysed function.
pub struct CoverageBundle {
    languages: crate::coverage_reports::Reports,
    suffix: SuffixIndex,
    fresh: Option<(HashMap<String, Lines>, PathBuf)>,
}

impl CoverageBundle {
    /// Reports already on disk, matched by path suffix.
    pub fn from_reports(lcov: &Records) -> CoverageBundle {
        CoverageBundle {
            languages: crate::coverage_reports::Reports::default(),
            suffix: SuffixIndex::new(lcov),
            fresh: None,
        }
    }

    /// A validated fresh report, matched by resolved path under `root`.
    pub fn fresh(records: &Records, root: &Path) -> CoverageBundle {
        let mut exact = HashMap::new();
        for (key, lines) in records.iter() {
            exact
                .entry(normcase(Path::new(key)))
                .or_insert_with(|| lines.clone());
        }
        CoverageBundle {
            languages: crate::coverage_reports::Reports::default(),
            suffix: SuffixIndex::new(&Records::default()),
            fresh: Some((exact, root.to_path_buf())),
        }
    }

    /// The percentage of `function`'s lines covered, or None.
    ///
    /// Java (JaCoCo) and Go (statement profile) have their own report formats
    /// and never fall back to LCOV, so a custom coverage command's fresh
    /// report leaves them unmeasured. Every other language prefers
    /// Cloverage's form counts and only then reads LCOV, as crapper's
    /// `percent_for` does.
    pub fn percent_for(&self, function: &Function) -> Option<f64> {
        if matches!(function.language, Language::Java | Language::Go) {
            return self.languages.percent(function);
        }
        let lines = self
            .languages
            .forms_percent(&function.path)
            .or_else(|| match &self.fresh {
                Some((exact, root)) => exact.get(&normcase(&resolve(&root.join(&function.path)))),
                None => self.suffix.get(&function.path),
            });
        percent_for_range(lines, function.start_line, function.end_line)
    }
}

const LCOV_REPORTS: &[&str] = &[
    "target/coverage/lcov.info",
    "coverage/lcov.info",
    "target/coverage/typescript/lcov.info",
    "target/coverage/rust/lcov.info",
    "target/coverage/python/lcov.info",
    "target/coverage/lua/lcov.info",
    "target/coverage/**/lcov.info",
    "coverage/**/lcov.info",
];

/// `root.glob(pattern)` for patterns of literal segments with at most one
/// `**`; directory entries are visited in name order.
fn glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    if pattern.starts_with("*/") || pattern.starts_with("*/*/") {
        let depth = usize::from(pattern.starts_with("*/*/")) + 1;
        let rest = pattern.splitn(depth + 1, '/').last().unwrap();
        let mut found = Vec::new();
        fn children_at(root: &Path, depth: usize, found: &mut Vec<PathBuf>) {
            if depth == 0 {
                found.push(root.to_path_buf());
                return;
            }
            let Ok(entries) = fs::read_dir(root) else {
                return;
            };
            let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            entries.sort();
            for entry in entries {
                if entry.is_dir() {
                    children_at(&entry, depth - 1, found);
                }
            }
        }
        children_at(root, depth, &mut found);
        return found
            .into_iter()
            .flat_map(|directory| glob(&directory, rest))
            .collect();
    }
    let Some((prefix, rest)) = pattern.split_once("/**/") else {
        let path = root.join(pattern);
        return if path.exists() {
            vec![path]
        } else {
            Vec::new()
        };
    };
    let mut found = Vec::new();
    fn descend(directory: &Path, rest: &str, found: &mut Vec<PathBuf>) {
        let candidate = directory.join(rest);
        if candidate.exists() {
            found.push(candidate);
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        dirs.sort();
        for dir in dirs {
            descend(&dir, rest, found);
        }
    }
    let base = root.join(prefix);
    if base.is_dir() {
        descend(&base, rest, &mut found);
    }
    found
}

/// The text of each report once, in pattern order.
pub(super) fn report_texts(root: &Path, patterns: &[&str]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut texts = Vec::new();
    for path in patterns.iter().flat_map(|pattern| glob(root, pattern)) {
        let key = posix(&resolve(&path));
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if !path.is_file() {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if !text.is_empty() {
                texts.push(text);
            }
        }
    }
    texts
}

/// Every LCOV report on disk under `root`, merged; a later record for a
/// source wins.
pub fn load_bundle(root: &Path) -> CoverageBundle {
    let mut bundle = CoverageBundle::from_reports(&Records::default());
    bundle.languages = crate::coverage_reports::Reports::load(root);
    let mut merged = Records::default();
    for text in report_texts(root, LCOV_REPORTS) {
        merged.update(parse_lcov(&text));
    }
    bundle.suffix = SuffixIndex::new(&merged);
    bundle
}
