//! The Rust suite a mutant runs (qinyuanj/dsh-uml-viewer#123).
//!
//! `cargo test` runs every test of the package even after one has failed, and
//! stable libtest has no `--fail-fast`: the flag is nightly-only, and this
//! repository does not take a nightly dependency. `cargo-nextest` is a separate
//! stable binary whose first-failure stop is what makes a killed mutant cheap:
//! a Rust mutant's cost sits in test execution (#112 measured ~87% of it). A
//! mutant nothing kills still runs the whole suite, so a verdict only moves in
//! the safe direction: `killed` stays `killed`.
//!
//! The stop is `--max-fail 1:immediate` rather than `--fail-fast`, which is the
//! same knob in nextest's default `wait` mode: it stops scheduling new tests
//! and waits for the ones already running. One test in this repository
//! (`scaling::levels_scenes_compile_near_linearly_across_four_sizes`) runs for
//! 68.9 s of the scene suite's 69.0 s, so a failing test cannot end a `wait` run
//! early — measured 69.8 s against 71.4 s for `cargo test`. `:immediate`
//! terminates the running tests instead: measured 2.69 s. nextest refuses both
//! flags together, so the run names only the mode it uses.
//!
//! The suite keeps `cargo test`'s selection, and the three knobs that could turn
//! a killed mutant into a false survivor are turned off explicitly:
//!
//! - `--retries 0`: a failure that passes on retry is not a survivor.
//! - `--ignore-default-filter`: a project's `default-filter` may not narrow the
//!   selection that decides the verdict.
//! - `--no-tests=fail`: an empty selection is a baseline failure, never a
//!   survivor.
//!
//! nextest does not run doctests and `cargo test` does, so the suite runs them
//! after the fail-fast pass: a mutant only a doctest kills stays killed. A
//! project's own timeout policy (nextest's `slow-timeout`) is left alone: it
//! can only make a failure louder, never turn one into a pass.

/// The fail-fast pass: the tests `cargo test` selects, stopped at the first
/// failure with the running tests terminated, and every knob that could hide a
/// failure or narrow the selection turned off.
pub const RUN: &str =
    "cargo nextest run --max-fail 1:immediate --retries 0 --ignore-default-filter --no-tests=fail";

/// The doctests nextest does not run and `cargo test` did.
pub const DOCTESTS: &str = "cargo test --doc";

/// The untimed compile step before the baseline: the test binaries the timed
/// runs then reuse, compiled and not run.
pub const BUILD: &str = "cargo test --no-run";

/// The suite for a Rust source: fail-fast first, so a killed mutant stops
/// there, then the doctests, which only a surviving mutant pays for.
pub fn command() -> String {
    format!("{RUN} && {DOCTESTS}")
}

/// Whether `command` is a Cargo test suite — `cargo test …` or this fail-fast
/// nextest run: the commands whose compile step warms the target directory and
/// whose per-command copies may reuse incremental compilation (#112).
pub fn is_suite(command: &str) -> bool {
    let mut words = command.split_whitespace();
    match (words.next(), words.next()) {
        (Some("cargo"), Some("test")) => true,
        (Some("cargo"), Some("nextest")) => words.next() == Some("run"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of the ticket: the run stops at the first failure, and stops
    /// the tests that are still running (`wait` mode would wait for the scene
    /// suite's 68.9 s test and give the stop back).
    #[test]
    fn the_suite_stops_at_the_first_failure_immediately() {
        let command = command();
        assert!(
            command.starts_with("cargo nextest run --max-fail 1:immediate "),
            "{command}"
        );
        assert!(!command.contains("--no-fail-fast"), "{command}");
        // nextest rejects `--fail-fast` together with `--max-fail`: naming both
        // would turn every baseline into a usage error.
        assert!(!command.contains("--fail-fast"), "{command}");
    }

    /// The red line: nothing in the suite may drop a test from the selection or
    /// let a failed test pass, because either turns a killed mutant into a false
    /// survivor.
    #[test]
    fn the_suite_cannot_narrow_the_selection_or_retry_a_failure() {
        let command = command();
        assert!(command.contains("--retries 0"), "{command}");
        assert!(command.contains("--ignore-default-filter"), "{command}");
        assert!(command.contains("--no-tests=fail"), "{command}");
        for narrowing in [
            " -E ",
            " --filterset ",
            " --exclude ",
            " --test ",
            " -p ",
            " --workspace ",
            " --lib",
            " --bins",
            " --all-targets",
        ] {
            assert!(!command.contains(narrowing), "{narrowing} in {command}");
        }
    }

    /// nextest never runs doctests; `cargo test` did, so a mutant only a doctest
    /// kills must still be killed.
    #[test]
    fn the_suite_still_runs_the_doctests_nextest_skips() {
        let command = command();
        assert!(command.ends_with(" && cargo test --doc"), "{command}");
        let (fail_fast, doctests) = command.split_once(" && ").expect("two passes");
        assert!(fail_fast.contains("--max-fail 1:immediate"), "{command}");
        assert_eq!(doctests, DOCTESTS);
    }

    /// The untimed build compiles the test binaries the nextest pass reuses.
    #[test]
    fn the_build_step_is_a_cargo_test_compile() {
        assert_eq!(BUILD, "cargo test --no-run");
        assert!(is_suite(BUILD));
    }

    /// Only Cargo's own suites count: the warm-up build and the incremental
    /// compile reuse must not follow an unrelated command.
    #[test]
    fn only_a_cargo_test_suite_is_a_suite() {
        for command in [
            "cargo test",
            "cargo test --doc",
            "cargo test --no-run",
            command().as_str(),
            BUILD,
        ] {
            assert!(is_suite(command), "{command}");
        }
        for command in [
            "cargo nextest",
            "cargo nextest list",
            "cargo llvm-cov --lcov",
            "cargo build",
            "pytest -q",
            "go test ./...",
        ] {
            assert!(!is_suite(command), "{command}");
        }
    }
}
