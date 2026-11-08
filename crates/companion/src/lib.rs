//! Headless companion roles for the single uml-viewer binary (#134/#138):
//! `companion-preflight`, `companion-worker`, `companion-runner` and
//! `companion-controller`, with the Herdr session backend, ownership rules,
//! durable recovery and the POSIX boundary they stand on. Behaviour follows
//! `docs/herdr-backend-contract.md` and the ADR 0007 platform table; the code
//! is a port of `love/uml/{domain,adapters}/{companion,herdr,recovery,...}.lua`.
// Tests are the outside harness: they build and launch fixtures and children
// the adapter is tested against. The lib target still lints production code.
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub(crate) mod agent;
pub(crate) mod herdr;
pub(crate) mod host;
pub(crate) mod json;
pub(crate) mod lifecycle;
pub mod mailbox;
pub(crate) mod ownership;
/// The OS boundary: `posix.rs` on Linux and macOS, `windows.rs` on
/// Windows (ADR 0010). Both expose the same free functions.
#[cfg_attr(windows, path = "windows.rs")]
#[allow(clippy::disallowed_methods)] // the adapter is the one process boundary
pub(crate) mod posix;
pub(crate) mod preflight;
pub(crate) mod prompt;
pub mod record;
pub(crate) mod recovery;
pub(crate) mod role;
pub(crate) mod search;
pub(crate) mod text;
pub(crate) mod vocab;
pub mod worker;

pub use agent::CodingAgent;
pub use vocab::{Failure, Result};

/// `pub fn run(args: &[OsString]) -> ExitCode` for each `companion-*` role,
/// as the binary's ROLES table expects.
pub fn run_preflight(args: &[std::ffi::OsString]) -> std::process::ExitCode {
    role::preflight(args)
}

pub fn run_worker(args: &[std::ffi::OsString]) -> std::process::ExitCode {
    role::worker(args)
}

pub fn run_runner(args: &[std::ffi::OsString]) -> std::process::ExitCode {
    role::runner(args)
}

pub fn run_controller(args: &[std::ffi::OsString]) -> std::process::ExitCode {
    role::controller(args)
}

pub fn run_smoke(args: &[std::ffi::OsString]) -> std::process::ExitCode {
    role::smoke(args)
}

/// The stdout of `program` (looked up on PATH) run with `args` through the
/// OS adapter: literal argv, bounded by the adapter's timeout. `None` when it
/// is not on PATH or did not run to an exit. The one launch for callers
/// outside this crate (clippy.toml forbids `Command::new` elsewhere).
pub fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    let executable = search::Search::native()
        .candidates(&path, program)
        .into_iter()
        .flatten()
        .find(|candidate| std::path::Path::new(candidate).is_file())?;
    let argv: Vec<std::ffi::OsString> = std::iter::once(executable.as_str())
        .chain(args.iter().copied())
        .map(std::ffi::OsString::from)
        .collect();
    let result = posix::run(&argv, None, &[]);
    result.exit.map(|_| result.out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_stdout_runs_a_path_program_through_the_adapter() {
        let version = super::command_stdout("rustc", &["--version"]).unwrap();
        assert!(version.starts_with("rustc "), "{version}");
        assert_eq!(
            None,
            super::command_stdout("uml-viewer-no-such-program", &[])
        );
    }
}
