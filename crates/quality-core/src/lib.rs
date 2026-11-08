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

// Shared syntax, serialization and OS primitives for quality tool crates.
pub use {chrono, edn, libc, serde_json, sha2, tree_sitter};

pub mod atomic;
pub mod coverage;
pub mod functions;
pub mod grammar;
pub mod language;
pub mod paths;
pub mod process;
pub mod runner;
pub mod select;
pub mod snapshot;
