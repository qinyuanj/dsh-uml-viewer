//! Differential mutation testing with isolated Linux test runs.
use std::{ffi::OsString, process::ExitCode};
mod cli;
mod engine;
mod isolation;
mod metrics;
mod sites;

pub fn run(args: &[OsString]) -> ExitCode {
    run_with_clock(args, || {
        quality_core::chrono::Local::now()
            .to_rfc3339_opts(quality_core::chrono::SecondsFormat::Secs, false)
    })
}
pub fn run_with_clock(args: &[OsString], clock: impl Fn() -> String) -> ExitCode {
    let args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    const HELP: &str = include_str!("../../../fixtures/mutate/help.txt");
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let options = match cli::parse(&args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}\n\n{HELP}");
            return ExitCode::from(1);
        }
    };
    if !cfg!(target_os = "linux") {
        eprintln!(
            "Mutation isolation is currently supported only on Linux; Windows and macOS are not supported by this build."
        );
        return ExitCode::from(1);
    }
    let root = options.scope.project_root();
    let mut files = if options.flag("--changed") {
        // The shared changed selection: source roots narrow it only when they
        // are given, and the positional filters and exclusions always apply.
        options
            .scope
            .select_changed(&options.positionals, &mut std::io::stderr())
    } else {
        options
            .scope
            .select(&options.positionals, &mut std::io::stderr())
    };
    files.retain(|path| {
        if quality_core::language::language_of(path) != Some(quality_core::language::Language::Rust)
        {
            eprintln!(
                "Skipping {}; Linux mutation currently supports Rust only.",
                path.display()
            );
            return false;
        }
        if quality_core::language::is_test_file(path) {
            eprintln!("Skipping test file {}", path.display());
            false
        } else {
            true
        }
    });
    if files.is_empty() {
        eprintln!("No source files to mutate.");
        return ExitCode::SUCCESS;
    }
    if !options.flag("--scan")
        && let Err(error) = engine::check_backups(&root)
    {
        eprintln!("{error}");
        return ExitCode::from(2);
    }
    let prepared = match engine::prepare(&root, &options, &files) {
        Ok(value) => value,
        Err(code) => return ExitCode::from(code as u8),
    };
    ExitCode::from(engine::mutate(&root, &options, prepared, clock) as u8)
}
