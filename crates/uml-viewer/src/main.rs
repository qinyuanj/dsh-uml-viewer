//! The single uml-viewer binary (#134). It only dispatches: each role is a
//! subcommand whose implementation lives in its own workspace library crate
//! (`crates/<role>`), exposing `pub fn run(args: &[OsString]) -> ExitCode`.
//! All roles are ported (#134-#139); the standalone `viewer` window was
//! deleted with its crate (dsh-uml-viewer#30), so the panel's `resident`
//! session is the only viewer. Arguments after the subcommand are passed on
//! literally (no re-quoting, no lossy UTF-8 conversion). A missing or
//! unknown subcommand is a usage error.

use std::ffi::OsString;
use std::process::ExitCode;

type Entry = fn(&'static str, &[OsString]) -> ExitCode;

/// Role table: each role wraps its crate's `run`, e.g. `|_, args| ir::run(args)`.
/// The Herdr companion roles left with the companion (#18).
const ROLES: &[(&str, &str, Entry)] = &[
    (
        "ir",
        "generate IR from a policy and source tree, headless",
        |_, args| ir::run(args),
    ),
    (
        "scene",
        "export scene geometry for each focus, or one view's frame (--frame), headless",
        |_, args| frame::run_scene(args),
    ),
    (
        "resident",
        "viewer session over stdin/stdout: JSON actions in, scene frames and events out",
        |_, args| viewer_session::run_resident(args),
    ),
    (
        "crap",
        "score every function with the CRAP metric, .metrics/crap.edn",
        |_, args| crapper::run(args),
    ),
    (
        "mutate",
        "differential mutation testing in isolated test runs (Linux, Windows)",
        |_, args| mutator::run(args),
    ),
    (
        "dry",
        "find duplicate structures, .metrics/dry.edn",
        |_, args| dryer::run(args),
    ),
    (
        "quality",
        "run crap|mutate|dry per uml-quality.json, print the version-1 result record",
        |_, args| quality_dispatch::run(args),
    ),
];

/// Command-line usage error.
const USAGE: u8 = 2;

fn usage() -> String {
    let mut text = String::from("usage: uml-viewer <subcommand> [args]\n\nsubcommands:\n");
    for (name, summary, _) in ROLES {
        text.push_str(&format!("  {name:<22}{summary}\n"));
    }
    text
}

fn version_requested(args: &[OsString]) -> bool {
    args.len() == 1 && args[0] == "--version"
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if version_requested(&args) {
        println!("uml-viewer {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let first = args.first().and_then(|a| a.to_str());
    if matches!(first, Some("--help" | "-h")) {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if let Some(&(name, _, entry)) = ROLES.iter().find(|(name, _, _)| Some(*name) == first) {
        return entry(name, &args[1..]);
    }
    let message = match args.first() {
        None => "缺少子命令".to_owned(),
        Some(arg) if arg.to_string_lossy().starts_with("--") => {
            format!("未知选项 {}", arg.to_string_lossy())
        }
        Some(arg) => format!("未知子命令 {}", arg.to_string_lossy()),
    };
    eprintln!("uml-viewer: {message}\n{}", usage());
    ExitCode::from(USAGE)
}

#[cfg(test)]
mod tests {
    use super::version_requested;
    use std::ffi::OsString;

    #[test]
    fn version_requires_exact_standalone_argument() {
        assert!(version_requested(&[OsString::from("--version")]));
        assert!(!version_requested(&[]));
        assert!(!version_requested(&[OsString::from("-V")]));
        assert!(!version_requested(&[
            OsString::from("--version"),
            OsString::from("extra"),
        ]));
        assert!(!version_requested(&[
            OsString::from("ir"),
            OsString::from("--version"),
        ]));
    }
}
