//! `uml-viewer crap`: score every function with the CRAP metric and publish
//! `.metrics/crap.edn`, byte for byte as the Python crapper does (ADR 0012).
//! Selection, syntax trees, coverage and publication come from
//! `quality-core`; this crate holds the formula, the report and the CLI.
//!
//! Exit status: 0 analysed, 1 usage / coverage / publication failure,
//! 2 `--threshold` exceeded, 3 `--threshold` with coverage but nothing
//! measured (2 and 3 after the snapshot is published).

mod cli;
mod crap;

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use quality_core::coverage::load_bundle;
use quality_core::functions::supported;
use quality_core::language::language_of;
use quality_core::paths::posix;
use quality_core::runner::run_coverage_with_lua_shards;
use quality_core::select::Scope;

use crate::cli::{HELP, Options, Parsed};
use crate::crap::{Entry, analyze, format_report, metrics_path, render_edn};

pub fn run(args: &[OsString]) -> ExitCode {
    let args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    ExitCode::from(status(&args))
}

fn status(args: &[String]) -> u8 {
    match cli::parse(args) {
        Parsed::Help => {
            print!("{HELP}");
            0
        }
        Parsed::Usage(message) => {
            eprint!("{message}\n\n{HELP}");
            1
        }
        Parsed::Analyze(options) => analyze_project(&options),
    }
}

/// Under WSL2, a project on a Windows drive is slow and breaks symlinks.
fn platform_warning(root: &Path) -> Option<String> {
    (!cfg!(windows) && posix(root).starts_with("/mnt/")).then(|| {
        format!(
            "{} is on a Windows drive. Under WSL2, clone the project under ~/ \
             for speed and working symlinks.",
            root.display()
        )
    })
}

fn analyze_project(options: &Options) -> u8 {
    let scope = Scope {
        root: options.project_root.clone(),
        source_roots: options.source_roots.clone(),
        exclusions: options.exclusions.clone(),
    };
    let root = scope.project_root();
    if let Some(warning) = platform_warning(&root) {
        eprintln!("{warning}");
    }
    if options.full_tree
        && let Err(message) = scope.check_whole_tree()
    {
        eprintln!("Source selection failed: {message}");
        return 1;
    }
    let files = {
        let mut stderr = std::io::stderr();
        if options.changed {
            scope.select_changed(&options.positionals, &mut stderr)
        } else {
            scope.select(&options.positionals, &mut stderr)
        }
    };
    if let Some(file) = files
        .iter()
        .find(|file| language_of(file).is_some_and(|l| !supported(l)))
    {
        let language = language_of(file).map_or("", |language| language.name());
        eprintln!(
            "uml-viewer crap does not read {language} yet ({}); see \
             qinyuanj/dsh-uml-viewer#4 and #5.",
            quality_core::paths::shown(file, &root)
        );
        return 1;
    }
    if files.is_empty() && !options.full_tree && options.coverage_command.is_none() {
        println!("No source files to analyze.");
        return 0;
    }

    let metrics = metrics_path(&root);
    let mut bundle = None;
    if !options.no_coverage && (!files.is_empty() || options.coverage_command.is_some()) {
        if !options.use_existing_coverage {
            match run_coverage_with_lua_shards(
                &root,
                &files,
                options.coverage_command.as_deref(),
                options.lua_shards,
                options.full_tree,
            ) {
                Ok(found) => bundle = found,
                Err(error) => {
                    eprintln!(
                        "Coverage failed: {error} {} was not replaced.",
                        metrics.display()
                    );
                    return 1;
                }
            }
        }
        if bundle.is_none() {
            bundle = Some(load_bundle(&root));
        }
    }

    if files.is_empty() && !options.full_tree {
        println!("No source files to analyze; custom coverage validated.");
        return 0;
    }
    if files.is_empty() {
        eprintln!(
            "{}",
            if options.coverage_command.is_none() {
                "Publishing empty business tree; no coverage ran."
            } else {
                "Publishing empty business tree after custom coverage validation."
            }
        );
    }
    let entries = match analyze(&files, &root, bundle.as_ref()) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("Report rendering failed: {error}");
            return 1;
        }
    };
    let report = format_report(&entries);
    if let Err(error) = quality_core::atomic::write_bytes(&metrics, render_edn(&entries).as_bytes())
    {
        eprintln!("Snapshot publication failed: {error}");
        return 1;
    }
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(report.as_bytes());
    let _ = stdout.flush();
    eprintln!("Wrote {}", metrics.display());
    if !options.no_coverage {
        count_unscored(&entries);
    }
    threshold_status(&entries, options.threshold, !options.no_coverage)
}

fn count_unscored(entries: &[Entry]) {
    let unscored = entries.iter().filter(|entry| entry.crap.is_none()).count();
    if unscored > 0 {
        eprintln!("{unscored} of {} functions have no coverage", entries.len());
    }
}

/// 0 pass, 2 exceeded, 3 unmeasured: a threshold run with coverage in which
/// no analysed function was scored cannot pass.
fn threshold_status(entries: &[Entry], threshold: Option<f64>, coverage_requested: bool) -> u8 {
    let Some(threshold) = threshold else {
        return 0;
    };
    let worst = entries
        .iter()
        .filter_map(|entry| entry.crap)
        .fold(None, |worst: Option<f64>, crap| {
            Some(worst.map_or(crap, |w| w.max(crap)))
        });
    let Some(worst) = worst else {
        if coverage_requested && !entries.is_empty() {
            eprintln!("No function's coverage was measured, so --threshold cannot pass.");
            return 3;
        }
        return 0;
    };
    if worst <= threshold {
        return 0;
    }
    eprintln!("CRAP threshold exceeded: {worst:.1} > {threshold:.1}");
    2
}
