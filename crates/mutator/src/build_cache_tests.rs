//! The target directory is kept by position, never by name.
//!
//! Every command wipes its whole slot (`crate::copies::Slot::copy`), and
//! `crate::slot` says why: a mutant's writes must not reach the next command.
//! A Cargo target directory that outlives commands therefore has to sit *above*
//! the wiped region, the way a Lua collection's `lua-coverage` scratch does,
//! and be opened by absolute path. These tests pin that position and the one
//! thing the position costs — forcing the previous mutation's crate back to the
//! project's own bytes — so a later change that moves the directory inside a
//! worker slot, or that teaches the wipe a name to skip, fails here.
use super::build_cache::{self, BuildCache};
use crate::{scratch, workers::Workers};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// A scratch root holding one empty project directory.
fn project() -> (scratch::Root, PathBuf) {
    let temporary = scratch::Root::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    (temporary, project)
}

/// `cmd /d /c set`: the one command that reports the environment a child was
/// given, on a project that needs no toolchain.
fn environment_of(view: &crate::isolation::View, project: &Path) -> String {
    let argv = vec!["cmd.exe".into(), "/d".into(), "/c".into(), "set".into()];
    let result = view
        .run_argv(project, project, &argv, None, Some(Duration::from_secs(30)))
        .unwrap_or_else(|error| panic!("production isolation unavailable or failed: {error}"));
    assert_eq!(result.code, 0, "{}", result.output);
    result.output
}

/// One variable's value, as `cmd /d /c set` printed it.
fn variable(environment: &str, name: &str) -> String {
    environment
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            key.eq_ignore_ascii_case(name).then(|| value.to_owned())
        })
        .unwrap_or_else(|| panic!("no {name} in:\n{environment}"))
}

/// The ticket's decisive property: a command's preparation wipes its slot and
/// leaves the target directory alone, because the directory is not inside it.
#[test]
fn a_workers_target_directory_survives_the_wipe_a_command_performs() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let workers = Workers::new(&project, 1).unwrap();
    let view = workers.view(0);
    let target = view
        .build()
        .expect("a worker view names the target directory it shares")
        .directory()
        .to_path_buf();
    let marker = target.join("marker");
    fs::write(&marker, "compiled").unwrap();
    environment_of(&view, &project);
    assert!(
        marker.exists(),
        "the command's slot wipe removed the target directory at {}",
        target.display()
    );
}

/// The command Cargo actually runs is pointed at that directory, not at one
/// inside the copy it will lose.
#[test]
fn a_command_sees_the_target_directory_above_its_copy() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let workers = Workers::new(&project, 1).unwrap();
    let view = workers.view(0);
    let target = view
        .build()
        .expect("a worker view names the target directory it shares")
        .directory()
        .to_path_buf();
    let environment = environment_of(&view, &project);
    assert_eq!(
        variable(&environment, "CARGO_TARGET_DIR"),
        target.to_string_lossy(),
        "the command did not build into the target directory it shares"
    );
}

/// A view with no pool of its own keeps Cargo's output inside its copy: the
/// coverage command, which opens its own view, builds exactly as it always did.
#[test]
fn a_view_without_a_target_directory_builds_inside_its_copy() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let view = crate::isolation::View::new(&project).unwrap();
    assert!(view.build().is_none());
    let environment = environment_of(&view, &project);
    let target = PathBuf::from(variable(&environment, "CARGO_TARGET_DIR"));
    let tail: PathBuf = ["beside", "project", "target", "mutator-build"]
        .iter()
        .collect();
    assert!(
        target.ends_with(&tail),
        "a view with no target directory of its own builds into {} instead of inside its copy",
        target.display()
    );
}

/// No component of the target directory's path is a worker slot, so the wiped
/// region cannot contain it.
#[test]
fn the_target_directory_is_a_sibling_of_every_worker_slot() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let workers = Workers::new(&project, 2).unwrap();
    for index in 0..2 {
        let build = workers
            .view(index)
            .build()
            .expect("a worker view names its target directory")
            .directory()
            .to_path_buf();
        assert_eq!(
            build.parent().unwrap().file_name().unwrap(),
            build_cache::DIRECTORY
        );
        // The wiped region is `<pool>/worker-<n>`; the target directory is
        // `<pool>/target-cache/worker-<n>`, a sibling directory, so no slot
        // contains it. Naming the whole path is the assertion: a change that
        // puts it under a slot fails here.
        let pool = build.parent().unwrap().parent().unwrap();
        assert_eq!(
            build,
            pool.join(build_cache::DIRECTORY)
                .join(format!("worker-{index}")),
            "the target directory is not a sibling of the slot the wipe removes"
        );
        assert!(
            build.is_dir(),
            "the pool did not create {}",
            build.display()
        );
        assert_eq!(
            build.file_name().unwrap().to_string_lossy(),
            format!("worker-{index}"),
            "the target directory is not named for its worker"
        );
    }
}

/// One per worker: two slots never share compiled objects.
#[test]
fn each_worker_has_its_own_target_directory() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let workers = Workers::new(&project, 2).unwrap();
    let first = workers.view(0).build().unwrap().directory().to_path_buf();
    let second = workers.view(1).build().unwrap().directory().to_path_buf();
    assert_ne!(first, second, "two workers share one target directory");
}

/// The wiring itself: a command that mutates a source records it in the
/// worker's target directory, and the command after it spends the note. This is
/// what makes the kept directory safe — without it a restored source keeps the
/// object Cargo built from the mutant (0 crates rebuilt).
#[test]
fn a_command_that_mutates_a_source_is_remembered_for_the_next_one() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let source = project.join("src");
    fs::create_dir(&source).unwrap();
    let source = source.join("lib.rs");
    fs::write(&source, "pub fn a() -> u32 { 1 }\n").unwrap();
    let workers = Workers::new(&project, 1).unwrap();
    let view = workers.view(0);
    let build = view
        .build()
        .expect("a worker view names its target directory")
        .clone();
    view.run(
        &project,
        &project,
        "exit 0",
        Some((&source, "pub fn a() -> u32 { 2 }\n")),
        Some(Duration::from_secs(30)),
    )
    .unwrap_or_else(|error| panic!("production isolation unavailable or failed: {error}"));
    assert_eq!(
        build.previous().as_deref(),
        Some(Path::new("src/lib.rs")),
        "the mutated source was not recorded for the command after this one"
    );
    // The next command mutates nothing, so the note is spent forcing it once.
    view.run(
        &project,
        &project,
        "exit 0",
        None,
        Some(Duration::from_secs(30)),
    )
    .unwrap_or_else(|error| panic!("production isolation unavailable or failed: {error}"));
    assert!(
        build.previous().is_none(),
        "the note was not spent, so every later command would force it again"
    );
}

/// The correctness trade the kept directory costs: the file the last command
/// mutated is restored with its original, older time, so Cargo would call its
/// crate fresh and reuse the mutant's object. Forcing it is what keeps a mutant
/// from being judged on code that is not its own.
#[test]
fn the_previous_mutation_is_forced_back_to_the_projects_bytes() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let build = BuildCache::new(root.join("target-cache").join("worker-0"));
    fs::create_dir_all(build.directory()).unwrap();
    let source = project.join("source.rs");
    fs::write(&source, "original").unwrap();
    // A copy is a copy: the file keeps the project's original time.
    let original = SystemTime::now() - Duration::from_secs(3600);
    fs::OpenOptions::new()
        .write(true)
        .open(&source)
        .unwrap()
        .set_modified(original)
        .unwrap();
    build.remember(Path::new("source.rs")).unwrap();
    assert_eq!(build.previous().as_deref(), Some(Path::new("source.rs")));
    build.force_previous(&project).unwrap();
    let forced = fs::metadata(&source).unwrap().modified().unwrap();
    assert!(
        forced > original + Duration::from_secs(60),
        "the previous mutation was not forced"
    );
    // Once forced, the crate is rebuilt from the bytes it has, so the note is
    // spent: a later command that mutates nothing forces nothing.
    assert!(build.previous().is_none());
}

/// A note naming a source the project no longer has is spent, not an error.
#[test]
fn forcing_a_source_the_project_lost_is_harmless() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let build = BuildCache::new(root.join("target-cache").join("worker-0"));
    fs::create_dir_all(build.directory()).unwrap();
    build.remember(Path::new("gone.rs")).unwrap();
    build.force_previous(&project).unwrap();
    assert!(build.previous().is_none());
}

/// A worker that has not mutated anything yet forces nothing.
#[test]
fn a_fresh_target_directory_forces_nothing() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let build = BuildCache::new(root.join("target-cache").join("worker-0"));
    fs::create_dir_all(build.directory()).unwrap();
    assert!(build.previous().is_none());
    build.force_previous(&project).unwrap();
}
