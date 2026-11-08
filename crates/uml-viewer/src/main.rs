//! The single uml-viewer binary (#134). It only dispatches: each role is a
//! subcommand whose implementation lives in its own workspace library crate
//! (`crates/<role>`), exposing `pub fn run(args: &[OsString]) -> ExitCode`.
//! All roles are ported (#134-#139).
//! Arguments after the subcommand are passed on literally (no re-quoting,
//! no lossy UTF-8 conversion). Without a subcommand, all arguments go to
//! the viewer, so `uml-viewer diagram.edn` opens a diagram.

use std::ffi::OsString;
use std::process::ExitCode;

type Entry = fn(&'static str, &[OsString]) -> ExitCode;

/// Role table. A later ticket that ports a role replaces `unported` with
/// a wrapper around its crate's `run`, e.g. `|_, args| ir::run(args)`.
const ROLES: &[(&str, &str, Entry)] = &[
    ("viewer", "egui viewer (default)", |_, args| {
        viewer::run(args)
    }),
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
        "differential mutation testing, isolated Linux test runs",
        |_, args| mutator::run(args),
    ),
    ("companion-worker", "Herdr companion worker", |_, args| {
        companion::run_worker(args)
    }),
    ("companion-runner", "Herdr companion runner", |_, args| {
        companion::run_runner(args)
    }),
    (
        "companion-controller",
        "companion recovery controller",
        |_, args| companion::run_controller(args),
    ),
    (
        "companion-preflight",
        "read-only Herdr backend preflight",
        |_, args| companion::run_preflight(args),
    ),
    (
        "companion-smoke",
        "isolated Herdr acceptance harness",
        |_, args| companion::run_smoke(args),
    ),
];

/// Command-line usage error.
const USAGE: u8 = 2;

fn usage() -> String {
    let mut text = String::from("usage: uml-viewer [<subcommand>] [args]\n\nsubcommands:\n");
    for (name, summary, _) in ROLES {
        text.push_str(&format!("  {name:<22}{summary}\n"));
    }
    text
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let first = args.first().and_then(|a| a.to_str());
    if matches!(first, Some("--help" | "-h")) {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if let Some(&(name, _, entry)) = ROLES.iter().find(|(name, _, _)| Some(*name) == first) {
        return entry(name, &args[1..]);
    }
    if first.is_some_and(|a| a.starts_with("--")) {
        eprintln!("uml-viewer: 未知选项 {}\n{}", first.unwrap(), usage());
        return ExitCode::from(USAGE);
    }
    let (name, _, entry) = ROLES[0];
    entry(name, &args)
}
