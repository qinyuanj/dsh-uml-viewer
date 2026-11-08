//! Candidate duplicate structures, ported from Python dryer b3f72ad.
use quality_core::select::Scope;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
mod normalize;
mod report;
mod scan;

const HELP: &str = include_str!("../../../fixtures/dry/help.txt");
struct Options {
    scope: Scope,
    positionals: Vec<String>,
    threshold: f64,
    min_lines: usize,
    min_nodes: usize,
    format: String,
    changed: bool,
}
enum Parsed {
    Help,
    Error(String, u8),
    Scan(Options),
}
fn take(args: &[String], at: usize) -> Result<&str, String> {
    match args.get(at + 1) {
        Some(value) if !value.is_empty() && !value.starts_with('-') => Ok(value),
        _ => Err(format!("{} requires a value", args[at])),
    }
}
fn parse(args: &[String]) -> Parsed {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        return Parsed::Help;
    }
    let mut options = Options {
        scope: Scope {
            root: PathBuf::from("."),
            ..Scope::default()
        },
        positionals: Vec::new(),
        threshold: 0.82,
        min_lines: 4,
        min_nodes: 20,
        format: "text".to_string(),
        changed: false,
    };
    let mut i = 0;
    let error = |message: String| Parsed::Error(format!("{message}\n\n{HELP}"), 1);
    while i < args.len() {
        match args[i].as_str() {
            "-s" | "--source-root" | "--exclude" => match take(args, i) {
                Ok(value) => {
                    if args[i] == "--exclude" {
                        options.scope.exclusions.push(value.to_string());
                    } else {
                        options.scope.source_roots.push(value.to_string());
                    }
                }
                Err(message) => return error(message),
            },
            "--root" | "--threshold" | "--min-lines" | "--min-nodes" | "--format" => {
                match take(args, i) {
                    Ok(value) => {
                        let result: Result<(), String> = match args[i].as_str() {
                            "--root" => {
                                options.scope.root = value.into();
                                Ok(())
                            }
                            "--threshold" => match value.trim().to_ascii_lowercase() {
                                v if matches!(v.as_str(), "nan" | "+nan" | "-nan") => {
                                    options.threshold = f64::NAN;
                                    Ok(())
                                }
                                v if matches!(
                                    v.as_str(),
                                    "inf" | "+inf" | "infinity" | "+infinity"
                                ) =>
                                {
                                    options.threshold = f64::INFINITY;
                                    Ok(())
                                }
                                v if matches!(v.as_str(), "-inf" | "-infinity") => {
                                    options.threshold = f64::NEG_INFINITY;
                                    Ok(())
                                }
                                v => v
                                    .trim()
                                    .replace('_', "")
                                    .parse()
                                    .map(|v| options.threshold = v)
                                    .map_err(|_| "--threshold requires a number".to_string()),
                            },
                            "--min-lines" | "--min-nodes" => value
                                .parse()
                                .map(|v| {
                                    if args[i] == "--min-lines" {
                                        options.min_lines = v
                                    } else {
                                        options.min_nodes = v
                                    }
                                })
                                .map_err(|_| format!("{} requires an integer", args[i])),
                            "--format" => {
                                options.format = value.to_string();
                                Ok(())
                            }
                            _ => unreachable!(),
                        };
                        if let Err(message) = result {
                            return error(message);
                        }
                    }
                    Err(message) => return error(message),
                }
            }
            "--edn" => options.format = "edn".to_string(),
            "--text" => options.format = "text".to_string(),
            "--changed" => options.changed = true,
            option if option.starts_with('-') => return error(format!("Unknown option: {option}")),
            _ => options.positionals.push(args[i].clone()),
        }
        i += if matches!(
            args[i].as_str(),
            "--root"
                | "-s"
                | "--source-root"
                | "--exclude"
                | "--threshold"
                | "--min-lines"
                | "--min-nodes"
                | "--format"
        ) {
            2
        } else {
            1
        };
    }
    if !matches!(options.format.as_str(), "text" | "edn") {
        return Parsed::Error(format!("Unknown format: {}\n", options.format), 2);
    }
    if let Some(message) = options.scope.exclusion_error() {
        return error(message);
    }
    Parsed::Scan(options)
}
/// The binary's dry role. It preserves the original argument and exit contract.
pub fn run(args: &[OsString]) -> ExitCode {
    let args: Vec<_> = args
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    let options = match parse(&args) {
        Parsed::Help => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Parsed::Error(message, code) => {
            eprint!("{message}");
            return ExitCode::from(code);
        }
        Parsed::Scan(options) => options,
    };
    let scope = options.scope;
    let files = if options.changed {
        scope.select_changed(&options.positionals, &mut std::io::stderr())
    } else {
        scope.select(&options.positionals, &mut std::io::stderr())
    };
    let root = scope.project_root();
    if let Some(file) = files.iter().find(|path| !path.starts_with(&root)) {
        eprintln!(
            "dryer: Source {} is outside the project root",
            quality_core::paths::posix(file)
        );
        return ExitCode::FAILURE;
    }
    if files.is_empty() {
        println!("No source files to analyze.");
        return ExitCode::SUCCESS;
    }
    if let Some(language) = files
        .iter()
        .filter_map(|path| quality_core::language::language_of(path))
        .find(|language| *language != quality_core::language::Language::Rust)
    {
        eprintln!(
            "dryer: language {} is not ported yet (qinyuanj/dsh-uml-viewer#7)",
            language.name()
        );
        return ExitCode::FAILURE;
    }
    let entries = match scan::scan(&files, &root, options.min_lines, options.min_nodes) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("dryer: {error}");
            return ExitCode::FAILURE;
        }
    };
    let candidates = scan::duplicates(entries, options.threshold);
    let payload = report::edn(&candidates);
    let path = root.join(".metrics/dry.edn");
    if let Err(error) = quality_core::atomic::write_bytes(&path, payload.as_bytes()) {
        eprintln!("dryer: {error}");
        return ExitCode::FAILURE;
    }
    print!(
        "{}",
        if options.format == "edn" {
            payload
        } else {
            report::text(&candidates)
        }
    );
    eprintln!("Wrote {}", path.display());
    ExitCode::SUCCESS
}
