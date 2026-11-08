//! The `standardTools` route: the built-in `crap`, `mutate` and `dry`
//! subcommands with the declared scope, as `uml_quality.py`
//! `standard_command` builds their argv (ADR 0001 2026-11-01 appendix):
//!
//! - crap: `--full-tree --source-root R… --exclude G… <crapperOptions> <args>`
//! - mutate: `-s R… --exclude G… <mutatorOptions> <options> <targets>`
//! - dry: `-s R… --exclude G… <dryerOptions> <args>`
//!
//! Each runs in this process at the project root, so the tool keeps its own
//! signal handling and exit contract.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use quality_core::language::{Language, language_of};
use quality_core::paths::{relative_posix, resolve};
use quality_core::select::{Scope, excluded};

use crate::config::{Operation, Standard, VALUES};
use crate::{Outcome, Refusal};

const SCOPE_FLAGS: &[&str] = &["--root", "--source-root", "-s", "--exclude"];

pub fn run(
    root: &Path,
    operation: Operation,
    standard: &Standard,
    forward: &[String],
) -> Result<Outcome, Refusal> {
    let (tool, codes) = operation.tool();
    let refuse = |message: String| Refusal {
        message,
        exit_code: 1,
        crap_scope: (operation == Operation::Crap).then_some("whole-tree"),
        configured: false,
        tool: tool.to_string(),
    };
    if standard.declared
        && forward
            .iter()
            .any(|arg| SCOPE_FLAGS.contains(&arg.as_str()))
    {
        return Err(refuse(
            "business scope is declared by standardTools and cannot be overridden by refresh \
             arguments"
                .to_string(),
        ));
    }
    let args = arguments(root, operation, standard, forward).map_err(refuse)?;
    if let Some(message) = unported(root, operation, standard, &args) {
        return Err(refuse(message));
    }
    let exit_code = in_project(root, || run_tool(operation, &args)).map_err(refuse)?;
    Ok(Outcome {
        exit_code,
        valid_result: codes.contains(&exit_code),
        crap_scope: (operation == Operation::Crap).then_some("whole-tree"),
        configured: false,
        tool: tool.to_string(),
    })
}

/// `-s R…` (or `--source-root R…`) then `--exclude G…`.
fn scope_args(standard: &Standard, flag: &str) -> Vec<String> {
    let roots = standard
        .source_roots
        .iter()
        .flat_map(|root| [flag.to_string(), root.clone()]);
    let exclusions = standard
        .exclude
        .iter()
        .flat_map(|glob| ["--exclude".to_string(), glob.clone()]);
    roots.chain(exclusions).collect()
}

fn arguments(
    root: &Path,
    operation: Operation,
    standard: &Standard,
    forward: &[String],
) -> Result<Vec<String>, String> {
    let options = standard.options(operation).to_vec();
    Ok(match operation {
        Operation::Crap => [
            vec!["--full-tree".to_string()],
            scope_args(standard, "--source-root"),
            options,
            forward.to_vec(),
        ]
        .concat(),
        Operation::Dry => [scope_args(standard, "-s"), options, forward.to_vec()].concat(),
        Operation::Mutate => {
            let (extra, selection) = targets(root, standard, forward)?;
            [scope_args(standard, "-s"), options, extra, selection].concat()
        }
    })
}

/// Caller options and named targets; `--changed` travels as a target.
fn split(forward: &[String]) -> Result<(Vec<String>, Vec<String>), String> {
    let mut options = Vec::new();
    let mut names = Vec::new();
    let mut index = 0;
    while index < forward.len() {
        let arg = &forward[index];
        if SCOPE_FLAGS.contains(&arg.as_str()) {
            return Err("mutation scope is declared by standardTools".to_string());
        }
        if arg == "--changed" || !arg.starts_with('-') {
            names.push(arg.clone());
        } else {
            options.push(arg.clone());
            if VALUES.contains(&arg.as_str()) {
                index += 1;
                let value = forward
                    .get(index)
                    .ok_or_else(|| format!("{arg} is missing its value"))?;
                options.push(value.clone());
            }
        }
        index += 1;
    }
    Ok((options, names))
}

/// The declared roots, resolved; the project root without any.
fn roots(root: &Path, standard: &Standard) -> Vec<PathBuf> {
    if standard.source_roots.is_empty() {
        return vec![root.to_path_buf()];
    }
    standard
        .source_roots
        .iter()
        .map(|name| resolve(&root.join(name)))
        .collect()
}

/// `targets()`: every target in one call. `--changed` alone, or named
/// files and directories forwarded as project-relative paths in the order
/// named; out-of-scope targets are dropped with a note; nothing left is
/// refused without launching.
fn targets(
    root: &Path,
    standard: &Standard,
    forward: &[String],
) -> Result<(Vec<String>, Vec<String>), String> {
    let (options, names) = split(forward)?;
    if names.iter().any(|name| name == "--changed") && names.iter().any(|name| name != "--changed")
    {
        // mutator takes only the changed set when given both.
        return Err(
            "--changed cannot be combined with named mutation targets; run them separately"
                .to_string(),
        );
    }
    let roots = roots(root, standard);
    let mut selection: Vec<String> = Vec::new();
    for name in &names {
        if let Some(chosen) = target(root, name, &roots, &standard.exclude)?
            && !selection.contains(&chosen)
        {
            selection.push(chosen);
        }
    }
    if selection.is_empty() {
        return Err("no business mutation targets; name files or pass --changed".to_string());
    }
    Ok((options, selection))
}

fn target(
    root: &Path,
    name: &str,
    roots: &[PathBuf],
    exclusions: &[String],
) -> Result<Option<String>, String> {
    if name == "--changed" {
        return Ok(Some(name.to_string()));
    }
    let path = root.join(name);
    if path
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(format!("mutation does not accept symbolic links: {name}"));
    }
    if !path.exists() {
        return Err(format!("mutation target does not exist: {name}"));
    }
    let canonical = resolve(&path);
    let Some(relative) = relative_posix(&canonical, root) else {
        return Err(format!("mutation target is outside the project: {name}"));
    };
    let business = roots.iter().any(|source| canonical.starts_with(source))
        && !excluded(&canonical, root, exclusions);
    let holds = canonical.is_dir()
        && roots.iter().any(|source| source.starts_with(&canonical))
        && !excluded(&canonical, root, exclusions);
    if business || holds {
        return Ok(Some(relative));
    }
    eprintln!("uml: omitted mutation target outside business scope: {relative}");
    Ok(None)
}

/// The languages each built-in tool reads, checked on its own selection so
/// an unported language is an explicit error rather than an empty success.
fn unported(
    root: &Path,
    operation: Operation,
    standard: &Standard,
    args: &[String],
) -> Option<String> {
    let reads: fn(Language) -> bool = match operation {
        Operation::Crap => return None, // crapper reads all seven languages.
        Operation::Mutate => mutator::mutates,
        Operation::Dry => dryer::scans,
    };
    let scope = Scope {
        root: root.to_path_buf(),
        source_roots: standard.source_roots.clone(),
        exclusions: standard.exclude.clone(),
    };
    let positionals = positionals(args);
    let mut notes = std::io::sink();
    let files = in_project(root, || {
        Ok(if args.iter().any(|arg| arg == "--changed") {
            scope.select_changed(&positionals, &mut notes)
        } else {
            scope.select(&positionals, &mut notes)
        })
    })
    .ok()?;
    let (file, language) = files.iter().find_map(|file| {
        language_of(file)
            .filter(|language| !reads(*language))
            .map(|language| (file, language))
    })?;
    let (tool, _) = operation.tool();
    Some(format!(
        "{tool} is not ported for {} yet ({}); this project's {} run is refused instead of \
         reporting an empty result",
        language.name(),
        relative_posix(file, root).unwrap_or_else(|| quality_core::paths::posix(file)),
        operation.name(),
    ))
}

/// Non-option arguments, skipping each value-taking option's value.
fn positionals(args: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if VALUES.contains(&arg.as_str())
            || ["-s", "--source-root", "--exclude"].contains(&arg.as_str())
        {
            index += 2;
            continue;
        }
        if !arg.starts_with('-') {
            found.push(arg.clone());
        }
        index += 1;
    }
    found
}

/// Run `work` with the project root as the current directory, as the
/// Python dispatch ran its tools (`cwd=root`).
fn in_project<T>(root: &Path, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let previous = std::env::current_dir().map_err(|error| error.to_string())?;
    std::env::set_current_dir(root)
        .map_err(|error| format!("cannot enter {}: {error}", root.display()))?;
    let result = work();
    let _ = std::env::set_current_dir(previous);
    result
}

fn run_tool(operation: Operation, args: &[String]) -> Result<u8, String> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let code = crate::redirect::stdout_to_stderr(|| match operation {
        Operation::Crap => crapper::run(&args),
        Operation::Mutate => mutator::run(&args),
        Operation::Dry => dryer::run(&args),
    });
    Ok(exit_byte(code))
}

/// The byte a tool's `ExitCode` stands for. The tools build theirs with
/// `ExitCode::from(u8)` (or SUCCESS/FAILURE), and `ExitCode` exposes no
/// accessor, so compare against every byte.
fn exit_byte(code: ExitCode) -> u8 {
    (0..=u8::MAX)
        .find(|byte| ExitCode::from(*byte) == code)
        .unwrap_or(1)
}
