use quality_core::{platform, select::Scope};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
mod clojure;
mod count;
mod discover;
mod extract;
mod names;
mod normalize;
mod report;
mod scan;

#[cfg(test)]
mod original;

const HELP: &str = include_str!("../../../fixtures/dry/help.txt");
pub(crate) struct Options {
    scope: Scope,
    pub(crate) positionals: Vec<String>,
    pub(crate) threshold: f64,
    pub(crate) min_lines: usize,
    pub(crate) min_nodes: usize,
    pub(crate) format: String,
    changed: bool,
}
pub(crate) enum Parsed {
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
pub(crate) fn parse(args: &[String]) -> Parsed {
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
                            "--threshold" => quality_core::number::python_float(value)
                                .map(|v| options.threshold = v)
                                .ok_or_else(|| "--threshold requires a number".to_string()),
                            "--min-lines" | "--min-nodes" => match count::parse(value) {
                                Ok(v) => {
                                    if args[i] == "--min-lines" {
                                        options.min_lines = v
                                    } else {
                                        options.min_nodes = v
                                    }
                                    Ok(())
                                }
                                Err(count::CountError::NotInteger) => {
                                    Err(format!("{} requires an integer", args[i]))
                                }
                                Err(count::CountError::Negative) => {
                                    Err(format!("{} requires a non-negative integer", args[i]))
                                }
                            },
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
pub fn scans(language: quality_core::language::Language) -> bool {
    matches!(
        language,
        quality_core::language::Language::Rust
            | quality_core::language::Language::Go
            | quality_core::language::Language::Java
            | quality_core::language::Language::Clojure
            | quality_core::language::Language::Python
            | quality_core::language::Language::TypeScript
            | quality_core::language::Language::Lua
    )
}

/// The binary's dry role. It preserves the original argument and exit contract.
pub fn run(args: &[OsString]) -> ExitCode {
    run_on(platform::Host::current(), args)
}

fn run_on(host: platform::Host, args: &[OsString]) -> ExitCode {
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
    let root = scope.project_root();
    if let Some(warning) = platform::warning(host, &root) {
        eprintln!("{warning}");
    }
    let files = if options.changed {
        discover::select_changed(&scope, &options.positionals, &mut std::io::stderr())
    } else {
        discover::select(&scope, &options.positionals, &mut std::io::stderr())
    };
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
    let (entries, warnings) =
        match scan::scan_files(&files, &root, options.min_lines, options.min_nodes) {
            Ok(found) => found,
            Err(error) => {
                eprintln!("dryer: {error}");
                return ExitCode::FAILURE;
            }
        };
    for warning in warnings {
        eprintln!("dryer: {warning}");
    }
    let candidates = scan::duplicates(entries, options.threshold);
    let payload = report::edn(&candidates);
    let path = match report::write_metrics(&candidates, &root) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("dryer: {error}");
            return ExitCode::FAILURE;
        }
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn scratch(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("dsh-dry-{name}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn root_args(root: &Path) -> Vec<OsString> {
        vec!["--root".into(), root.as_os_str().to_owned()]
    }

    #[test]
    fn native_windows_scans_on_any_build() {
        let root = scratch("any-windows-build");
        let exit = run_on(platform::Host::Windows, &root_args(&root));
        assert_eq!(ExitCode::SUCCESS, exit);
        assert!(!root.join(".metrics").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
