#![cfg_attr(test, allow(clippy::disallowed_methods))]
use super::{Exclusions, copy_tree, hidden_paths, is_cargo_test_command, runs_javascript};
use crate::scratch;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const SUITE: &str = crate::rust_suite::RUN;

fn scratch_project() -> (scratch::Root, PathBuf) {
    let temporary = scratch::Root::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    (temporary, project)
}

/// A directory junction is Windows' unprivileged directory link (a symlink
/// needs a privilege or developer mode).
fn junction(link: &Path, target: &Path) {
    let status = Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .unwrap();
    assert!(status.success(), "mklink /J failed for {}", link.display());
}

fn copy(project: &Path, command: &str) -> std::io::Result<PathBuf> {
    let destination = project.with_file_name("copy");
    fs::create_dir(&destination)?;
    copy_tree(
        project,
        &destination,
        &Exclusions::project(project, command),
    )?;
    Ok(destination)
}

#[test]
fn a_link_to_nothing_is_skipped_instead_of_refusing_the_project() {
    let (temporary, project) = scratch_project();
    fs::create_dir(project.join("src")).unwrap();
    fs::write(project.join("src/lib.rs"), b"pub fn one() {}\n").unwrap();
    let target = temporary.path().join("never-built");
    fs::create_dir(&target).unwrap();
    junction(&project.join("src").join("gone"), &target);
    fs::remove_dir(&target).unwrap();

    let copy = copy(&project, "cargo test").expect("a dangling link must not refuse the project");

    assert_eq!(
        fs::read(copy.join("src/lib.rs")).unwrap(),
        b"pub fn one() {}\n"
    );
    assert!(
        !copy.join("src/gone").exists(),
        "a link to nothing is skipped, not copied"
    );
}

#[test]
fn a_link_to_a_real_directory_is_still_refused() {
    let (temporary, project) = scratch_project();
    let outside = temporary.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"original").unwrap();
    junction(&project.join("bridge"), &outside);

    let error = copy(&project, "cargo test").expect_err("a linked directory must be refused");

    assert!(error.to_string().contains("linked directory"), "{error}");
    assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"original");
}

#[test]
fn a_hidden_directory_is_an_empty_directory_in_the_copy() {
    let (_temporary, project) = scratch_project();
    fs::write(project.join(".gitignore"), b"generated/\n").unwrap();
    fs::create_dir(project.join("generated")).unwrap();
    fs::write(project.join("generated/keep.txt"), b"output").unwrap();
    fs::write(project.join("lib.rs"), b"pub fn one() {}\n").unwrap();

    let hidden = hidden_paths(&project);
    assert_eq!(hidden, vec![project.join("generated")]);

    let copy = copy(&project, "cargo test").unwrap();
    assert!(copy.join("generated").is_dir(), "hidden means present");
    assert_eq!(
        fs::read_dir(copy.join("generated")).unwrap().count(),
        0,
        "hidden means empty"
    );
    assert_eq!(fs::read(copy.join("lib.rs")).unwrap(), b"pub fn one() {}\n");
}

#[test]
fn a_package_dependency_tree_is_hidden_at_any_depth() {
    let (temporary, project) = scratch_project();
    let store = temporary.path().join("store");
    fs::create_dir_all(store.join("vitest")).unwrap();
    fs::write(store.join("vitest/package.json"), b"{}").unwrap();
    let package = project.join("packages").join("app");
    fs::create_dir_all(package.join("src")).unwrap();
    fs::create_dir_all(package.join("node_modules")).unwrap();
    fs::write(
        package.join("src").join("lib.ts"),
        b"export const one = 1;\n",
    )
    .unwrap();
    // pnpm links each dependency to its store entry with a directory link.
    junction(
        &package.join("node_modules").join("vitest"),
        &store.join("vitest"),
    );

    let copy = copy(&project, "cargo test")
        .expect("a package's dependency tree must not refuse the project");

    assert!(copy.join("packages/app/node_modules").is_dir());
    assert_eq!(
        fs::read_dir(copy.join("packages/app/node_modules"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read(copy.join("packages/app/src/lib.ts")).unwrap(),
        b"export const one = 1;\n"
    );
}

#[test]
fn the_project_ignore_rules_supply_the_excluded_names() {
    let (_temporary, _project) = scratch_project();
    for (line, name) in [
        ("# a comment", None),
        ("!keep/", None),
        ("nested/path/", None),
        ("run-*/", None),
        ("build/", Some("build")),
        ("/cache", Some("cache")),
    ] {
        assert_eq!(
            super::literal_directory(line),
            name.map(str::to_owned),
            "{line}"
        );
    }
}

#[test]
fn a_javascript_command_keeps_the_dependency_tree() {
    let (_temporary, project) = scratch_project();
    fs::write(project.join(".gitignore"), b"node_modules/\n").unwrap();
    let package = project.join("packages").join("app");
    fs::create_dir_all(package.join("src")).unwrap();
    fs::create_dir_all(package.join("node_modules").join(".bin")).unwrap();
    fs::write(
        package.join("src").join("book.ts"),
        b"export const one = 1;\n",
    )
    .unwrap();
    fs::write(
        package.join("node_modules").join(".bin").join("vitest.cmd"),
        b"@echo off\r\n",
    )
    .unwrap();

    let copy = copy(&project, "npm test").expect("a JavaScript command needs its dependencies");

    assert_eq!(
        fs::read(
            copy.join("packages")
                .join("app")
                .join("node_modules")
                .join(".bin")
                .join("vitest.cmd")
        )
        .unwrap(),
        b"@echo off\r\n"
    );
}

#[test]
fn only_a_javascript_command_asks_for_a_dependency_tree() {
    for (command, javascript) in [
        ("cargo test", false),
        ("cargo test --no-run", false),
        (SUITE, false),
        ("pytest -q", false),
        ("go test ./...", false),
        ("npm test", true),
        ("pnpm run vitest", true),
        ("node --test out/book.js", true),
    ] {
        assert_eq!(runs_javascript(command), javascript, "{command}");
    }
}

#[test]
fn the_fail_fast_nextest_suite_is_a_cargo_test_command() {
    let suite = crate::rust_suite::command();
    for command in ["cargo test", "cargo test --no-run", suite.as_str(), SUITE] {
        assert!(is_cargo_test_command(command), "{command}");
    }
    for command in [
        "cargo nextest",
        "cargo nextest list",
        "cargo llvm-cov --lcov",
        "npm test",
    ] {
        assert!(!is_cargo_test_command(command), "{command}");
    }
}

#[test]
fn a_copy_failure_is_isolation_not_a_test_outcome() {
    let failure = super::copy_failure(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "access denied",
    ));

    let crate::isolation::Failure::Unavailable(text) = failure else {
        panic!("expected an isolation failure");
    };
    assert!(text.starts_with("Copy isolation cannot"), "{text}");
    assert!(text.contains("access denied"), "{text}");
}

#[test]
fn a_named_copy_failure_keeps_its_own_message() {
    let named = "Copy isolation cannot place a fixture beside the copy: denied";
    let failure = super::copy_failure(std::io::Error::other(named));

    let crate::isolation::Failure::Unavailable(text) = failure else {
        panic!("expected an isolation failure");
    };
    assert_eq!(named, text);
}
