//! Existing CRAP gates remain in the native Windows CI filter.
use super::crap::{Project, stderr, stdout};
use std::fs;

#[test]
fn full_tree_empty_custom_coverage_and_snapshot_publication_gate() {
    let p = Project::new();
    p.write(".metrics/crap.edn", "old\n");
    let result = p.crap(&["--full-tree", "--coverage-command", "exit 3"]);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(p.snapshot().as_deref(), Some("old\n"));
    assert!(stderr(&result).contains("Coverage command exited 3"));
    let result = p.crap(&["--full-tree", "--no-coverage"]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert_eq!(p.snapshot().as_deref(), Some("{:entries []}\n"));
    let result = p.crap(&["--changed", "--no-coverage"]);
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(stdout(&result), "No source files to analyze.\n");
}

#[test]
fn full_tree_builtin_failure_preserves_previous_snapshot() {
    let p = Project::new();
    p.write("a.rs", "pub fn a() {}\n");
    let result = p.crap(&["--full-tree", "--threshold", "0"]);
    assert_eq!(result.status.code(), Some(1), "{}", stderr(&result));
    assert!(stderr(&result).contains("Coverage failed"));
    assert_eq!(p.snapshot(), None);
}

#[test]
fn empty_option_and_full_tree_usage_errors_keep_old_output_contract() {
    let p = Project::new();
    p.write("a.rs", "pub fn a() {}\n");
    for (args, message) in [
        (vec!["--full-tree", "a.rs"], "--full-tree cannot be combined with path arguments"),
        (vec!["--full-tree", "--changed"], "--full-tree cannot be combined with --changed"),
        (vec!["--full-tree", "--use-existing-coverage"], "--full-tree cannot be combined with --use-existing-coverage"),
        (vec!["--no-coverage", "--coverage-command", "true"], "--no-coverage cannot be combined with --coverage-command"),
    ] {
        let result = p.crap(&args);
        assert_eq!(result.status.code(), Some(1));
        assert!(stderr(&result).starts_with(&format!("{message}\n\nUsage:")));
    }
}
