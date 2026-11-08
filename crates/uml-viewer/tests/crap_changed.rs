//! Ports of crapper 4068857 changed selection cases through the real CLI.
use super::crap::{Project, stderr, stdout};
use std::process::Command;

fn git(project: &Project, args: &[&str]) {
    let result = Command::new("git")
        .args(args)
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr(&result));
}

#[test]
fn changed_selects_existing_git_changes_inside_scope_with_filters_and_exclusions() {
    let p = Project::new();
    p.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    for name in ["edit", "keep", "gone", "rename", "exclude"] {
        p.write(
            &format!("src/{name}.rs"),
            &format!("pub fn {name}() {{}}\n"),
        );
    }
    git(&p, &["init", "-q"]);
    git(&p, &["add", "."]);
    git(
        &p,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "base",
        ],
    );
    p.write("src/edit.rs", "pub fn edited() {}\n");
    p.write("src/exclude.rs", "pub fn excluded() {}\n");
    p.write("src/space name.rs", "pub fn untracked() {}\n");
    p.write("other/outside.rs", "pub fn outside() {}\n");
    p.write("target/skipped.rs", "pub fn skipped() {}\n");
    p.write("src/tests/test.rs", "pub fn test_file() {}\n");
    std::fs::remove_file(p.root.join("src/gone.rs")).unwrap();
    git(&p, &["mv", "src/rename.rs", "src/renamed.rs"]);
    let result = p.crap(&[
        "--changed",
        "--no-coverage",
        "-s",
        "src",
        "--exclude",
        "src/exclude.rs",
    ]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert_eq!(p.names(), ["edited", "rename", "untracked"]);
    // Existing positional paths do not replace the changed set; missing ones filter it.
    let result = p.crap(&["--changed", "--no-coverage", "src/keep.rs", "space name"]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert_eq!(p.names(), ["untracked"]);
    // Python ignores an existing path positional with --changed; only a
    // missing native-separator spelling becomes a (nonmatching) text filter.
    let native = if cfg!(windows) {
        "src\\edit.rs"
    } else {
        "src/edit.rs"
    };
    let result = p.crap(&["--changed", "--no-coverage", native]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert_eq!(
        p.names(),
        ["edited", "excluded", "outside", "rename", "untracked"]
    );
}

#[test]
fn changed_under_nested_project_root_does_not_include_repo_siblings() {
    let p = Project::new();
    git(&p, &["init", "-q"]);
    p.write("nested/a.rs", "pub fn nested() {}\n");
    p.write("other.rs", "pub fn other() {}\n");
    let result = super::crap::crap_in(
        &p.root,
        &p.root.join("nested"),
        &["--changed", "--no-coverage"],
    );
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    let snapshot = std::fs::read_to_string(p.root.join("nested/.metrics/crap.edn")).unwrap();
    assert!(snapshot.contains(":name \"nested\""));
    assert!(!snapshot.contains(":name \"other\""));
}

#[test]
fn changed_git_failure_is_an_empty_selection_and_preserves_snapshot() {
    let p = Project::new();
    p.write("a.rs", "pub fn a() {}\n");
    p.write(".metrics/crap.edn", "old\n");
    let result = p.crap(&["--changed", "--no-coverage"]);
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(stdout(&result), "No source files to analyze.\n");
    assert!(stderr(&result).contains("git"));
    assert_eq!(p.snapshot().as_deref(), Some("old\n"));
}
