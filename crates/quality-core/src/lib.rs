//! Shared core of the quality tools (ADR 0012): crapper, mutator and dryer
//! depend on this crate and never on each other, so all three select the
//! same files, read the same syntax trees and the same coverage.
//!
//! - [`select`]: the source walk, `-s` roots, `--exclude` globs, named
//!   paths and text filters ([`select::Scope`]).
//! - [`language`]: language by extension, test files, skipped directories.
//! - [`grammar`]: the tree-sitter language table and node helpers.
//! - [`functions`]: functions and methods with namespace, span and
//!   cyclomatic complexity, per language.
//! - [`coverage`]: LCOV reports and per-function coverage.
//! - [`runner`]: coverage runners and custom coverage commands.
//! - [`process`]: the one process boundary for tool launches.
//! - [`atomic`]: atomic snapshot publication.
//! - [`snapshot`]: the `.metrics` EDN writer (number format, key order).
//! - [`paths`]: Python-compatible `resolve` / `as_posix`.
//! - [`platform`]: the shared pre-selection refusal and WSL `/mnt` warning.
//! - [`number`]: Python `float()` for number options; [`digits`] is the
//!   Unicode decimal digit table it shares with dryer's `int()` counts.

// Shared syntax, serialization and OS primitives for quality tool crates.
pub use {chrono, edn, libc, serde_json, sha2, tree_sitter};

pub mod atomic;
pub mod changed;
pub mod coverage;
mod coverage_reports;
pub mod digits;
pub mod functions;
pub mod grammar;
pub mod language;
pub mod native_coverage;
pub mod number;
pub mod paths;
pub mod platform;
pub mod process;
mod report_files;
pub mod runner;
mod runner_clojure;
mod runner_go;
mod runner_java;
pub mod runners;
pub mod select;
pub mod snapshot;
pub mod stamp;
