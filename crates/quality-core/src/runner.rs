//! Run each language's coverage tool and leave the reports for the loader.
//!
//! A custom coverage command must write a fresh, valid `coverage/lcov.info`
//! or the run fails. A built-in runner that fails leaves its language N/A,
//! unless the run is strict (a whole-tree refresh), where it fails too.
//! Built-in runners for Python, TypeScript and Lua arrive with #4. The
//! coverage stamp mutator reuses is shared here (#3).

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::coverage::{CoverageBundle, read_fresh_lcov};
use crate::language::{Language, language_of};
use crate::paths::{home, posix, resolve};
use crate::process::{Program, run, which};

/// Coverage could not be produced; the snapshot must not be published.
#[derive(Clone, Debug, PartialEq)]
pub struct CoverageError(pub String);

impl fmt::Display for CoverageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn warn(message: &str) {
    eprintln!("{message}");
}

/// Run `program` in `cwd` after echoing it; 127 when it did not start.
pub fn run_shell(program: &Program, cwd: &Path) -> i32 {
    warn(&format!("+ ({}) {}", cwd.display(), program.display()));
    match run(program, cwd) {
        Ok(code) => code,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match program {
                Program::Argv(argv) if which(&argv[0]).is_none() => warn(&format!(
                    "Coverage command failed to start: {} is not on PATH",
                    argv[0]
                )),
                _ => warn(&format!("Coverage command failed to start: {error}")),
            }
            127
        }
        Err(error) => {
            warn(&format!("Coverage command failed to start: {error}"));
            127
        }
    }
}

/// The nearest directory from `start` up to `stop` holding `marker`.
pub fn nearest(start: &Path, marker: &str, stop: &Path) -> Option<PathBuf> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };
    let stop = resolve(stop);
    loop {
        if current.join(marker).is_file() {
            return Some(current);
        }
        let parent = current.parent()?.to_path_buf();
        if current == stop || parent == current {
            return None;
        }
        current = parent;
    }
}

/// The modules (directories holding `marker`) that hold `language` files.
pub fn modules_with(
    files: &[PathBuf],
    language: Language,
    marker: &str,
    root: &Path,
) -> Vec<PathBuf> {
    let root = resolve(root);
    let found: BTreeSet<String> = files
        .iter()
        .filter(|file| language_of(file) == Some(language))
        .filter_map(|file| nearest(file, marker, &root))
        .map(|module| posix(&resolve(&module)))
        .collect();
    found.into_iter().map(PathBuf::from).collect()
}

/// `target/coverage/<language>[/<module slug>]/lcov.info`.
pub fn coverage_report(root: &Path, module: &Path, language: &str) -> PathBuf {
    let root = resolve(root);
    let module = resolve(module);
    let base = root.join("target").join("coverage").join(language);
    match crate::paths::relative_posix(&module, &root) {
        Some(relative) if relative != "." => {
            base.join(relative.replace('/', "__")).join("lcov.info")
        }
        _ => base.join("lcov.info"),
    }
}

fn run_reported(argv: &[String], module: &Path, language: &str, report: &Path) -> bool {
    let code = run_shell(&Program::Argv(argv), module);
    if code != 0 {
        warn(&format!(
            "{language} coverage exited {code} in {}. {language} coverage will be N/A.",
            module.display()
        ));
        return false;
    }
    if !report.is_file() {
        warn(&format!(
            "{language} coverage wrote no {}. {language} coverage will be N/A.",
            report.display()
        ));
        return false;
    }
    true
}

fn strings(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|arg| arg.to_string()).collect()
}

fn install_llvm_cov() -> bool {
    let home = home();
    if which("rustup").is_some()
        && run_shell(
            &Program::Argv(&strings(&[
                "rustup",
                "component",
                "add",
                "llvm-tools-preview",
            ])),
            &home,
        ) != 0
    {
        warn("rustup component add llvm-tools-preview failed.");
    }
    let code = run_shell(
        &Program::Argv(&strings(&[
            "cargo",
            "install",
            "cargo-llvm-cov",
            "--locked",
        ])),
        &home,
    );
    code == 0 && which("cargo-llvm-cov").is_some()
}

#[derive(Clone, Copy)]
enum RustTool {
    LlvmCov,
    Tarpaulin,
}

fn rust_tool() -> Option<RustTool> {
    if which("cargo-llvm-cov").is_some() {
        return Some(RustTool::LlvmCov);
    }
    if which("cargo-tarpaulin").is_some() {
        return Some(RustTool::Tarpaulin);
    }
    warn("Neither cargo-llvm-cov nor cargo-tarpaulin is installed. Installing cargo-llvm-cov.");
    if install_llvm_cov() {
        return Some(RustTool::LlvmCov);
    }
    warn("Rust coverage will be N/A.");
    None
}

fn rust_command(tool: RustTool, report: &Path) -> Vec<String> {
    let report_text = report.display().to_string();
    match tool {
        RustTool::LlvmCov => {
            strings(&["cargo", "llvm-cov", "--lcov", "--output-path", &report_text])
        }
        RustTool::Tarpaulin => {
            let directory = report.parent().unwrap_or(report).display().to_string();
            strings(&[
                "cargo",
                "tarpaulin",
                "--out",
                "Lcov",
                "--output-dir",
                &directory,
            ])
        }
    }
}

fn cover_rust(root: &Path, files: &[PathBuf]) -> bool {
    let modules = modules_with(files, Language::Rust, "Cargo.toml", root);
    if modules.is_empty() {
        warn("No Cargo.toml; skipping Rust coverage.");
        return false;
    }
    let Some(tool) = rust_tool() else {
        return false;
    };
    let mut results = Vec::new();
    for module in modules {
        let report = coverage_report(root, &module, "rust");
        if let Some(parent) = report.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::remove_file(&report);
        results.push(run_reported(
            &rust_command(tool, &report),
            &module,
            "Rust",
            &report,
        ));
    }
    results.into_iter().all(|ok| ok)
}

fn invalidate(paths: &[PathBuf]) -> Result<(), CoverageError> {
    for path in paths {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CoverageError(format!(
                    "Cannot invalidate managed coverage artifact: {error}"
                )));
            }
        }
    }
    Ok(())
}

fn run_custom(root: &Path, command: &str) -> Result<CoverageBundle, CoverageError> {
    let report = root.join("coverage").join("lcov.info");
    invalidate(&[
        report.clone(),
        root.join("coverage").join("luacov.stats.out"),
        root.join("luacov.stats.out"),
    ])?;
    let code = run_shell(&Program::Shell(command), root);
    if code != 0 {
        return Err(CoverageError(format!(
            "Coverage command exited {code} in {}.",
            root.display()
        )));
    }
    let records = read_fresh_lcov(&report, root).map_err(|error| {
        CoverageError(format!(
            "Required LCOV report invalid or unavailable: {}: {error}",
            report.display()
        ))
    })?;
    Ok(CoverageBundle::fresh(&records, root))
}

/// Produce coverage for `files`: `command` in the host shell, or each
/// language's own runner. Some(bundle) for a validated custom report; None
/// when the reports are on disk for `coverage::load_bundle`. A custom
/// failure is an error; a built-in one is too when `strict`.
pub fn run_coverage(
    root: &Path,
    files: &[PathBuf],
    command: Option<&str>,
    strict: bool,
) -> Result<Option<CoverageBundle>, CoverageError> {
    let root = resolve(root);
    let before = crate::stamp::fingerprint(&root);
    let mut failed: Vec<&str> = files
        .iter()
        .filter_map(|file| language_of(file))
        .map(Language::name)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let result = if let Some(command) = command {
        run_custom(&root, command)
            .map(Some)
            .inspect(|_| failed.clear())
    } else {
        let languages: BTreeSet<Language> =
            files.iter().filter_map(|file| language_of(file)).collect();
        failed.clear();
        if languages.contains(&Language::Rust) && !cover_rust(&root, files) {
            failed.push(Language::Rust.name());
        }
        if languages.contains(&Language::Go) && !crate::runner_go::cover(&root, files) {
            failed.push(Language::Go.name());
        }
        if languages.contains(&Language::Java) && !crate::runner_java::cover(&root, files) {
            failed.push(Language::Java.name());
        }
        if languages.contains(&Language::Clojure) && !crate::runner_clojure::cover(&root) {
            failed.push(Language::Clojure.name());
        }
        Ok(None)
    };
    crate::stamp::record(&root, files, command, &failed, &before);
    let bundle = result?;
    if strict && !failed.is_empty() {
        failed.sort();
        return Err(CoverageError(format!(
            "No usable coverage for {}.",
            failed.join(", ")
        )));
    }
    Ok(bundle)
}

/// Python-compatible freshness lookup for mutator and dryer.
pub fn stale_files(root: &Path, files: &[PathBuf], command: Option<&str>) -> Vec<PathBuf> {
    crate::stamp::stale_files(root, files, command)
}
