//! Differential mutation testing in isolated test runs (Linux, Windows).
use quality_core::language::{Language, language_of};
use std::{ffi::OsString, path::Path, process::ExitCode};
#[cfg(any(windows, target_os = "linux"))]
mod child_env;
#[cfg(all(test, any(windows, target_os = "linux")))]
mod child_env_tests;
mod cli;
#[cfg(windows)]
mod copies;
mod engine;
mod interrupts;
mod isolation;
pub mod lua;
mod metrics;
mod operators;
mod python;
pub mod queue;
mod scratch;
pub mod sites;
mod typescript;
mod workers;

/// Whether this port mutates `language` yet: Rust and Lua (Go, Java and
/// Clojure follow with qinyuanj/dsh-uml-viewer#14). Quality dispatch refuses
/// a selection holding any other language instead of reporting it empty.
pub fn mutates(language: quality_core::language::Language) -> bool {
    matches!(
        language,
        quality_core::language::Language::Rust | quality_core::language::Language::Lua
    )
}

pub fn run(args: &[OsString]) -> ExitCode {
    run_with_clock(args, || {
        quality_core::chrono::Local::now()
            .to_rfc3339_opts(quality_core::chrono::SecondsFormat::Secs, false)
    })
}
pub fn run_with_clock(args: &[OsString], clock: impl Fn() -> String + Sync) -> ExitCode {
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
            eprint!("{error}\n\n{HELP}");
            return ExitCode::from(1);
        }
    };
    if let Some(refusal) = platform_refusal(
        std::env::consts::OS,
        if cfg!(windows) {
            quality_core::platform::windows_build().unwrap_or(0)
        } else {
            0
        },
    ) {
        eprintln!("{refusal}");
        return ExitCode::from(1);
    }
    if let Some(warning) = quality_core::platform::warning(
        quality_core::platform::Host::current(),
        &options.scope.project_root(),
    ) {
        eprintln!("{warning}");
    }
    let root = options.scope.project_root();
    // A run that arrives after a killed one reclaims that run's scratch first.
    // A live concurrent run's root keeps its owner lock and is left alone.
    if let Err(error) = scratch::sweep(&root) {
        eprintln!("Cannot sweep abandoned mutation scratch: {error}");
    }
    // From here until the end of the call a stop signal stops the work instead
    // of killing the process mid-cleanup.
    let _armed = interrupts::Armed::new();
    println!("Isolation: {}", isolation::describe());
    let code = run_mutation(&root, &options, clock);
    if let Some(signum) = interrupts::pending() {
        scratch::discard_all();
        eprintln!(
            "Interrupted by {}; unfinished files keep their previous snapshots.",
            interrupts::name(signum)
        );
        return ExitCode::from((128 + signum) as u8);
    }
    code
}
/// The platform decision, before selection and before any command.
///
/// Pure in the declared host, so the macOS refusal is testable anywhere.
/// macOS has neither mechanism: the original's `NamespaceViews` calls
/// `unshare`, which does not exist there, so the pinned Python crashes with a
/// traceback. This port says so on one line instead; exit 1 is the original's
/// isolation-refusal code, and `crap`/`dry` are unaffected.
fn platform_refusal(os: &str, build: u32) -> Option<String> {
    if os == "macos" {
        return Some(
            "Mutation testing is not supported on macOS: mutator runs every mutant in a \
             private overlay view on Linux or in a per-worker project copy on native \
             Windows, and neither mechanism exists on macOS. Mutator never runs a weaker \
             isolation. CRAP and dry work on this machine."
                .into(),
        );
    }
    let host = if os == "windows" {
        quality_core::platform::Host::Windows
    } else {
        quality_core::platform::Host::Unix
    };
    if os != "windows" && os != "linux" {
        return Some(format!("Mutation testing is not supported on {os}."));
    }
    quality_core::platform::refusal(
        quality_core::platform::Tool::Mutator,
        host,
        (os == "windows").then_some(build),
    )
}

/// Full-suite Lua files share one worker queue; every other file runs on its
/// own, exactly as the original's `cli._shares_queue` decides.
pub fn shares_queue(file: &Path, test_command: Option<&str>, select_specs: bool) -> bool {
    language_of(file) == Some(Language::Lua) && test_command.is_none() && !select_specs
}

fn run_mutation(
    root: &Path,
    options: &cli::Options,
    clock: impl Fn() -> String + Sync,
) -> ExitCode {
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
        if !matches!(
            quality_core::language::language_of(path),
            Some(
                quality_core::language::Language::Rust
                    | quality_core::language::Language::Lua
                    | quality_core::language::Language::Python
                    | quality_core::language::Language::TypeScript
            )
        ) {
            eprintln!(
                "Skipping {}; this Linux mutation build mutates Rust, Lua, Python and TypeScript sources.",
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
        && let Err(error) = engine::check_backups(root)
    {
        eprintln!("{error}");
        return ExitCode::from(2);
    }
    let mut timings = engine::Timings::default();
    let prepared = match engine::prepare(root, options, &files, &mut timings) {
        Ok(value) => value,
        Err(code) => return ExitCode::from(code as u8),
    };
    let started = std::time::Instant::now();
    let status = engine::mutate(root, options, prepared, clock, &mut timings);
    if !options.flag("--scan") {
        timings.add("mutation-run", started.elapsed());
    }
    if options.flag("--timings") {
        eprint!("{}", timings.summary());
    }
    ExitCode::from(status as u8)
}

#[cfg(test)]
mod tests {
    use super::platform_refusal;

    #[test]
    fn macos_is_refused_with_a_reason_and_linux_is_not() {
        let refusal = platform_refusal("macos", 0).unwrap();
        assert!(refusal.contains("not supported on macOS"), "{refusal}");
        assert!(refusal.contains("CRAP and dry work"), "{refusal}");
        assert_eq!(platform_refusal("linux", 0), None);
        assert_eq!(platform_refusal("windows", 19045), None);
        assert!(
            platform_refusal("windows", 19044)
                .unwrap()
                .contains("19045")
        );
    }
}
