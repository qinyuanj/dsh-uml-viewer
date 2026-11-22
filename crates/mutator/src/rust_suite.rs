pub const RUN: &str =
    "cargo nextest run --max-fail 1:immediate --retries 0 --ignore-default-filter --no-tests=fail";

pub const DOCTESTS: &str = "cargo test --doc";

pub const BUILD: &str = "cargo test --no-run";

pub fn command() -> String {
    format!("{RUN} && {DOCTESTS}")
}

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

    #[test]
    fn the_suite_stops_at_the_first_failure_immediately() {
        let command = command();
        assert!(
            command.starts_with("cargo nextest run --max-fail 1:immediate "),
            "{command}"
        );
        assert!(!command.contains("--no-fail-fast"), "{command}");
        assert!(!command.contains("--fail-fast"), "{command}");
    }

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

    #[test]
    fn the_suite_still_runs_the_doctests_nextest_skips() {
        let command = command();
        assert!(command.ends_with(" && cargo test --doc"), "{command}");
        let (fail_fast, doctests) = command.split_once(" && ").expect("two passes");
        assert!(fail_fast.contains("--max-fail 1:immediate"), "{command}");
        assert_eq!(doctests, DOCTESTS);
    }

    #[test]
    fn the_build_step_is_a_cargo_test_compile() {
        assert_eq!(BUILD, "cargo test --no-run");
        assert!(is_suite(BUILD));
    }

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
