use quality_core::select::Scope;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// The original's default: warn when a file selects more covered mutations.
const DEFAULT_MUTATION_WARNING: i64 = 50;

#[derive(Default)]
pub struct Options {
    pub scope: Scope,
    pub positionals: Vec<String>,
    pub flags: BTreeSet<String>,
    pub values: BTreeMap<String, String>,
    pub lines: Option<BTreeSet<usize>>,
    pub timeout_factor: f64,
    pub mutation_warning: i64,
    /// `--max-workers`: mutants to run at once; absent means every core.
    pub max_workers: Option<usize>,
    /// `--copy`: paths outside the project root that every view must show.
    /// May be repeated; validated by `isolation::resolve_outside`.
    pub copies: Vec<String>,
}
impl Options {
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

/// Python `int()`: surrounding whitespace, an optional sign and underscores
/// between digits.
fn python_int(value: &str) -> Option<i64> {
    let text = value.trim();
    let (sign, digits) = match text.as_bytes().first() {
        Some(b'+') => (1, &text[1..]),
        Some(b'-') => (-1, &text[1..]),
        _ => (1, text),
    };
    if digits.is_empty() {
        return None;
    }
    let mut cleaned = String::new();
    for (index, ch) in digits.char_indices() {
        if ch.is_ascii_digit() {
            cleaned.push(ch);
        } else if ch == '_' {
            let previous = digits[..index].chars().next_back();
            let next = digits[index + 1..].chars().next();
            if !previous.is_some_and(|c| c.is_ascii_digit())
                || !next.is_some_and(|c| c.is_ascii_digit())
            {
                return None;
            }
        } else {
            return None;
        }
    }
    cleaned.parse::<i64>().ok().map(|number| sign * number)
}

/// Python `float()`: surrounding whitespace and digit-group underscores.
fn python_float(value: &str) -> Option<f64> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    let cleaned: String = text.chars().filter(|ch| *ch != '_').collect();
    cleaned.parse::<f64>().ok()
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        scope: Scope {
            root: PathBuf::from("."),
            ..Scope::default()
        },
        timeout_factor: 10.0,
        mutation_warning: DEFAULT_MUTATION_WARNING,
        ..Options::default()
    };
    let flags = [
        "--no-coverage",
        "--use-existing-coverage",
        "--reuse-coverage",
        "--changed",
        "--scan",
        "--mutate-all",
        "--since-last-run",
        "--select-specs",
        "--full-suite",
        "--paranoid-workers",
        "--timings",
        "--verbose",
    ];
    let values = [
        "-s",
        "--source-root",
        "--exclude",
        "--root",
        "--coverage-command",
        "--test-command",
        "--busted-task",
        "--timeout-factor",
        "--mutation-warning",
        "--max-workers",
        "--lines",
        "--copy",
    ];
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if flags.contains(&arg) {
            options.flags.insert(arg.to_string());
        } else if values.contains(&arg) {
            index += 1;
            let value = args
                .get(index)
                .filter(|value| !value.is_empty() && !value.starts_with('-'))
                .ok_or_else(|| format!("{arg} requires a value"))?;
            match arg {
                "--root" => options.scope.root = PathBuf::from(value),
                "-s" | "--source-root" => options.scope.source_roots.push(value.clone()),
                "--exclude" => options.scope.exclusions.push(value.clone()),
                // Repeatable: every value is an outside path the call needs.
                "--copy" => options.copies.push(value.clone()),
                "--max-workers" => {
                    options.max_workers = Some(
                        python_int(value)
                            .filter(|number| *number > 0)
                            .ok_or("--max-workers requires a positive integer")?
                            as usize,
                    );
                }
                "--timeout-factor" => {
                    let factor = python_float(value)
                        .ok_or_else(|| format!("could not convert string to float: '{value}'"))?;
                    if factor <= 0.0 || !factor.is_finite() {
                        return Err("--timeout-factor requires a positive number".into());
                    }
                    options.timeout_factor = factor;
                }
                "--mutation-warning" => {
                    options.mutation_warning = python_int(value).ok_or_else(|| {
                        format!("invalid literal for int() with base 10: '{value}'")
                    })?;
                }
                "--lines" => {
                    let mut lines = BTreeSet::new();
                    for piece in value.split(',').map(str::trim).filter(|v| !v.is_empty()) {
                        let number = python_int(piece).ok_or_else(|| {
                            format!("invalid literal for int() with base 10: '{piece}'")
                        })?;
                        if number < 1 {
                            return Err("--lines requires positive line numbers".into());
                        }
                        lines.insert(number as usize);
                    }
                    if lines.is_empty() {
                        return Err("--lines requires a line number".into());
                    }
                    options.lines = Some(lines);
                }
                _ => (),
            }
            options.values.insert(arg.to_string(), value.clone());
        } else if arg.starts_with('-') {
            return Err(format!("Unknown option: {arg}"));
        } else {
            options.positionals.push(arg.to_string());
        }
        index += 1;
    }
    for (first, second) in [
        ("--select-specs", "--full-suite"),
        ("--busted-task", "--test-command"),
        ("--mutate-all", "--since-last-run"),
        ("--no-coverage", "--coverage-command"),
        ("--scan", "--mutate-all"),
        ("--scan", "--max-workers"),
    ] {
        let set = |name| options.flag(name) || options.values.contains_key(name);
        if set(first) && set(second) {
            return Err(format!("{first} cannot be combined with {second}"));
        }
    }
    if let Some(error) = options.scope.exclusion_error() {
        return Err(error);
    }
    Ok(options)
}
