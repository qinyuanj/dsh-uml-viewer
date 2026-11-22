use super::build_cache::{self, BuildCache};
use crate::{scratch, workers::Workers};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

fn project() -> (scratch::Root, PathBuf) {
    let temporary = scratch::Root::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    (temporary, project)
}

fn environment_of(view: &crate::isolation::View, project: &Path) -> String {
    let argv = vec!["cmd.exe".into(), "/d".into(), "/c".into(), "set".into()];
    let result = view
        .run_argv(project, project, &argv, None, Some(Duration::from_secs(30)))
        .unwrap_or_else(|error| panic!("production isolation unavailable or failed: {error}"));
    assert_eq!(result.code, 0, "{}", result.output);
    result.output
}

fn variable(environment: &str, name: &str) -> String {
    environment
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            key.eq_ignore_ascii_case(name).then(|| value.to_owned())
        })
        .unwrap_or_else(|| panic!("no {name} in:\n{environment}"))
}

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

#[test]
fn each_worker_has_its_own_target_directory() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let workers = Workers::new(&project, 2).unwrap();
    let first = workers.view(0).build().unwrap().directory().to_path_buf();
    let second = workers.view(1).build().unwrap().directory().to_path_buf();
    assert_ne!(first, second, "two workers share one target directory");
}

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

#[test]
fn the_previous_mutation_is_forced_back_to_the_projects_bytes() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let build = BuildCache::new(root.join("target-cache").join("worker-0"));
    fs::create_dir_all(build.directory()).unwrap();
    let source = project.join("source.rs");
    fs::write(&source, "original").unwrap();
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
    assert!(build.previous().is_none());
}

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

#[test]
fn a_fresh_target_directory_forces_nothing() {
    let (_temporary, root) = project();
    let project = fs::canonicalize(&root).unwrap();
    let build = BuildCache::new(root.join("target-cache").join("worker-0"));
    fs::create_dir_all(build.directory()).unwrap();
    assert!(build.previous().is_none());
    build.force_previous(&project).unwrap();
}
