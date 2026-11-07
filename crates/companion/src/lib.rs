//! Headless companion roles for the single uml-viewer binary (#134/#138):
//! `companion-preflight`, `companion-worker`, `companion-runner` and
//! `companion-controller`, with the Herdr session backend, ownership rules,
//! durable recovery and the POSIX boundary they stand on. Behaviour follows
//! `docs/herdr-backend-contract.md` and the ADR 0007 platform table; the code
//! is a port of `love/uml/{domain,adapters}/{companion,herdr,recovery,...}.lua`.

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
