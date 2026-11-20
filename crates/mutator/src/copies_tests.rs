#![cfg_attr(test, allow(clippy::disallowed_methods))]
//! Copy isolation's two boundaries (#102): what a copy excludes, and which
//! links it refuses.
//!
//! A copy is the only isolation Windows has (ADR 0002), so these tests use the
//! same unprivileged directory junction the runtime meets, and none of them
//! needs a toolchain or a test command.
use super::{copy_tree, hidden_paths};
use crate::scratch;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// A scratch root holding one empty project directory.
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

/// Copy `project` into a sibling directory and return that copy.
fn copy(project: &Path) -> std::io::Result<PathBuf> {
    let destination = project.with_file_name("copy");
    fs::create_dir(&destination)?;
    copy_tree(project, &destination, &hidden_paths(project))?;
    Ok(destination)
}

/// The ticket's decisive case: a link whose target is gone refuses nothing.
///
/// A dangling link cannot carry a write out of the copy, so the copy skips it.
/// This is the shape that stopped every mutation run in this repository: a
/// leftover test fixture, and a package manager's broken store entry.
#[test]
fn a_link_to_nothing_is_skipped_instead_of_refusing_the_project() {
    let (temporary, project) = scratch_project();
    fs::create_dir(project.join("src")).unwrap();
    fs::write(project.join("src/lib.rs"), b"pub fn one() {}\n").unwrap();
    // The link exists first and its target is removed after, which is how a
    // stale test fixture and a package manager's dead store entry appear: a
    // reparse point whose target is no longer there.
    let target = temporary.path().join("never-built");
    fs::create_dir(&target).unwrap();
    junction(&project.join("src").join("gone"), &target);
    fs::remove_dir(&target).unwrap();

    let copy = copy(&project).expect("a dangling link must not refuse the project");

    assert_eq!(
        fs::read(copy.join("src/lib.rs")).unwrap(),
        b"pub fn one() {}\n"
    );
    assert!(
        !copy.join("src/gone").exists(),
        "a link to nothing is skipped, not copied"
    );
}

/// The safety property the ticket forbids trading away: a link to a real
/// directory is still refused, because it is a route a mutant's write could
/// take back into the checkout.
#[test]
fn a_link_to_a_real_directory_is_still_refused() {
    let (temporary, project) = scratch_project();
    let outside = temporary.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"original").unwrap();
    junction(&project.join("bridge"), &outside);

    let error = copy(&project).expect_err("a linked directory must be refused");

    assert!(error.to_string().contains("linked directory"), "{error}");
    assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"original");
}

/// The overlay rule this ticket must keep: a hidden directory is present and
/// empty in the copy, never absent.
#[test]
fn a_hidden_directory_is_an_empty_directory_in_the_copy() {
    let (_temporary, project) = scratch_project();
    fs::write(project.join(".gitignore"), b"generated/\n").unwrap();
    fs::create_dir(project.join("generated")).unwrap();
    fs::write(project.join("generated/keep.txt"), b"output").unwrap();
    fs::write(project.join("lib.rs"), b"pub fn one() {}\n").unwrap();

    let hidden = hidden_paths(&project);
    assert_eq!(hidden, vec![project.join("generated")]);

    let copy = copy(&project).unwrap();
    assert!(copy.join("generated").is_dir(), "hidden means present");
    assert_eq!(
        fs::read_dir(copy.join("generated")).unwrap().count(),
        0,
        "hidden means empty"
    );
    assert_eq!(fs::read(copy.join("lib.rs")).unwrap(), b"pub fn one() {}\n");
}

/// The pnpm layout that made the whole repository unmutatable: a workspace
/// package carries its own `node_modules`, full of links into the store, below
/// a directory the project does not ignore at its root.
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

    let copy = copy(&project).expect("a package's dependency tree must not refuse the project");

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

/// An excluded name reaches the list from the project's own declaration, not
/// from a list in this module: only a literal, top-level directory line counts.
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
