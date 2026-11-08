use quality_core::select::Scope;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Default)]
pub struct Options {
    pub scope: Scope,
    pub positionals: Vec<String>,
    pub flags: BTreeSet<String>,
    pub values: BTreeMap<String, String>,
    pub lines: Option<BTreeSet<usize>>,
    pub timeout_factor: f64,
}
impl Options {
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        scope: Scope {
            root: PathBuf::from("."),
            ..Scope::default()
        },
        timeout_factor: 10.0,
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
                "--max-workers" => {
                    if value.parse::<usize>().ok().filter(|n| *n > 0).is_none() {
                        return Err("--max-workers requires a positive integer".into());
                    }
                    if value != "1" {
                        return Err(
                            "this Linux mutation build supports --max-workers 1 only".into()
                        );
                    }
                }
                "--timeout-factor" => {
                    options.timeout_factor = value
                        .parse()
                        .map_err(|_| "--timeout-factor requires a positive number")?;
                    if options.timeout_factor <= 0.0 || !options.timeout_factor.is_finite() {
                        return Err("--timeout-factor requires a positive number".into());
                    }
                }
                "--mutation-warning" => {
                    value
                        .parse::<i64>()
                        .map_err(|_| "--mutation-warning requires an integer")?;
                }
                "--lines" => {
                    let mut lines = BTreeSet::new();
                    for piece in value.split(',').map(str::trim).filter(|v| !v.is_empty()) {
                        let number = piece
                            .parse::<usize>()
                            .map_err(|_| "--lines requires positive line numbers")?;
                        if number == 0 {
                            return Err("--lines requires positive line numbers".into());
                        }
                        lines.insert(number);
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
