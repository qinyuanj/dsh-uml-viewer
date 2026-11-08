//! The built-in Go, Java and Clojure coverage runners, executed for real.
//!
//! Every golden scenario reads reports already on disk, so a regression in a
//! runner's argv, module discovery, report path or report loading would pass
//! them. Each case below runs the toolchain on `fixtures/crap/<language>` and
//! asserts the published snapshot came from the report the runner wrote.
//! A case whose toolchain is missing skips with a message on stderr
//! (`cargo test ... -- --nocapture` prints it); CI installs Go, JDK/Maven and
//! the Clojure CLI, so there the cases really run. Never a silent pass.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::crap::{Project, stderr};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "target" || name == "Cargo.lock" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(&name));
        } else {
            fs::copy(&path, to.join(&name)).unwrap();
        }
    }
}

fn project(language: &str) -> Project {
    let project = Project::new();
    copy_tree(&repo().join("fixtures/crap").join(language), &project.root);
    project
}

/// Whether `program` is an executable on PATH, honouring PATHEXT on Windows.
fn on_path(program: &str) -> bool {
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(program);
            if candidate.is_file() {
                return true;
            }
            if cfg!(windows) {
                for extension in ["exe", "cmd", "bat"] {
                    if directory.join(format!("{program}.{extension}")).is_file() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn skip(language: &str, reason: &str) {
    let message = format!("the real {language} coverage runner: {reason}");
    assert!(
        !std::env::var_os("CRAPPER_REQUIRE_TOOLCHAINS").is_some_and(|value| value == "1"),
        "CRAPPER_REQUIRE_TOOLCHAINS=1; {message}"
    );
    eprintln!("SKIP {message}");
}

fn clj_usable() -> Result<(), String> {
    if !on_path("clj") {
        return Err("`clj` is not on PATH".to_string());
    }
    let output = Command::new("clj")
        .arg("--version")
        .output()
        .map_err(|error| format!("`clj --version` could not start: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "`clj --version` exited {}; stdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn published(p: &Project) -> String {
    p.snapshot().expect("a published snapshot")
}

/// The published entry line for `namespace`/`name`, or a failure.
fn entry<'a>(snapshot: &'a str, namespace: &str, name: &str) -> &'a str {
    snapshot
        .lines()
        .find(|line| {
            line.contains(&format!(":name \"{name}\""))
                && line.contains(&format!(":namespace \"{namespace}\""))
        })
        .unwrap_or_else(|| panic!("no entry {namespace}/{name} in\n{snapshot}"))
}

#[test]
fn real_go_coverage_runner_measures_the_fixture() {
    if !on_path("go") {
        return skip("Go", "`go` is not on PATH");
    }
    let p = project("go");
    let result = p.crap(&[]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    // `go test ./... -coverprofile` writes this and the loader reads it.
    assert!(p.root.join("target/coverage/go/coverage.out").is_file());
    let snapshot = published(&p);
    assert_eq!(snapshot.lines().filter(|l| l.contains(":name ")).count(), 9);
    // Statement counts come from the fixture's source and tests: Branchy
    // covers 4 of 5 statements, Simple all of one.
    assert_eq!(
        entry(&snapshot, "example.com/quality/go", "Branchy"),
        r#"  {:name "Branchy", :namespace "example.com/quality/go", :complexity 5, :coverage 80.0, :crap 5.2}"#
    );
    assert_eq!(
        entry(&snapshot, "example.com/quality/go", "Simple"),
        r#"  {:name "Simple", :namespace "example.com/quality/go", :complexity 1, :coverage 100.0, :crap 1.0}"#
    );
    // A package with no test is measured as 0%, not left N/A.
    assert_eq!(
        entry(&snapshot, "example.com/quality/go/module", "First"),
        r#"  {:name "First", :namespace "example.com/quality/go/module", :complexity 1, :coverage 0.0, :crap 2.0}"#
    );
    assert!(
        !stderr(&result).contains("have no coverage"),
        "{}",
        stderr(&result)
    );
}

#[test]
fn real_java_coverage_runner_measures_the_fixture() {
    if !(on_path("mvn") && on_path("java")) {
        return skip("Java", "`mvn` and `java` are not both on PATH");
    }
    let p = project("java");
    let result = p.crap(&[]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert!(p.root.join("target/site/jacoco/jacoco.xml").is_file());
    let snapshot = published(&p);
    assert_eq!(
        snapshot.lines().filter(|l| l.contains(":name ")).count(),
        12
    );
    // JaCoCo instruction counters: covered and uncovered methods stay exact.
    assert_eq!(
        entry(&snapshot, "demo.Example", "stable"),
        r#"  {:name "stable", :namespace "demo.Example", :complexity 1, :coverage 100.0, :crap 1.0}"#
    );
    assert_eq!(
        entry(&snapshot, "demo.Example", "uncovered"),
        r#"  {:name "uncovered", :namespace "demo.Example", :complexity 2, :coverage 0.0, :crap 6.0}"#
    );
    // Nested classes join through `Function::jacoco_class`.
    assert_eq!(
        entry(&snapshot, "demo.Example.Inner", "tick"),
        r#"  {:name "tick", :namespace "demo.Example.Inner", :complexity 1, :coverage 100.0, :crap 1.0}"#
    );
    assert!(
        !stderr(&result).contains("have no coverage"),
        "{}",
        stderr(&result)
    );
}

#[test]
fn real_clojure_coverage_runner_measures_the_fixture() {
    if let Err(reason) = clj_usable() {
        return skip("Clojure", &reason);
    }
    let p = project("clojure");
    let result = p.crap(&[]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    // Cloverage writes both the HTML form counts and an LCOV report; the
    // HTML must win, so these form counts are the expectation.
    assert!(p.root.join("target/coverage/demo/core.clj.html").is_file());
    assert!(p.root.join("target/coverage/lcov.info").is_file());
    let snapshot = published(&p);
    assert_eq!(snapshot.lines().filter(|l| l.contains(":name ")).count(), 8);
    assert_eq!(
        entry(&snapshot, "demo.core", "after-characters"),
        r#"  {:name "after-characters", :namespace "demo.core", :complexity 1, :coverage 100.0, :crap 1.0}"#
    );
    assert_eq!(
        entry(&snapshot, "demo.core", "hidden"),
        r#"  {:name "hidden", :namespace "demo.core", :complexity 1, :coverage 50.0, :crap 1.125}"#
    );
    assert_eq!(
        entry(&snapshot, "demo.core", "choose"),
        r#"  {:name "choose", :namespace "demo.core", :complexity 2, :coverage 80.0, :crap 2.032}"#
    );
    assert!(
        !stderr(&result).contains("have no coverage"),
        "{}",
        stderr(&result)
    );
}
