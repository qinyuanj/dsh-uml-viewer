#![cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}
fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args([
            "-c",
            "user.email=fixture@example.com",
            "-c",
            "user.name=Fixture",
        ])
        .args(args)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mutator-acceptance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        copy(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/rust"),
            &path,
        );
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("project")
    }
    fn scratch(&self) -> &Path {
        &self.0
    }
    fn commit_baseline(&self) {
        let root = self.root();
        git(&root, &["init", "--quiet"]);
        git(&root, &["add", "--all"]);
        git(&root, &["commit", "--quiet", "-m", "baseline"]);
    }
    fn run(&self, extra: &[&str]) -> ExitCode {
        let mut args = vec![
            OsString::from("--root"),
            self.root().into_os_string(),
            OsString::from("--max-workers"),
            OsString::from("1"),
        ];
        args.extend(extra.iter().map(OsString::from));
        mutator::run_with_clock(&args, || "2026-11-07T12:00:00+00:00".to_string())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let temporary = std::env::temp_dir();
        let name = self
            .0
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        // Never remove a path this fixture did not create.
        if self.0.parent() != Some(temporary.as_path()) || !name.starts_with("mutator-acceptance-")
        {
            eprintln!("Not removing unexpected fixture path {}", self.0.display());
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Cannot remove fixture {}: {error}", self.0.display());
        }
    }
}

#[test]
fn rust_snapshots_match_original_bytes_and_unchanged_differential_keeps_them() {
    let fixture = Fixture::new();
    let files = ["app.edn", "app/more.edn", "util.edn"];
    let targets = [
        "--mutate-all",
        "--timeout-factor",
        "20",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    assert_eq!(fixture.run(&targets), ExitCode::SUCCESS);
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/goldens");
    for file in files {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            fs::read(golden.join(file)).unwrap(),
            "{file}"
        );
    }
    assert_eq!(fixture.run(&targets[3..]), ExitCode::SUCCESS);
    for file in files {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            fs::read(golden.join(file)).unwrap(),
            "{file}"
        );
    }
    assert!(
        !fixture.root().join("target").exists(),
        "all Cargo/test artifacts belong to overlays"
    );
}

#[test]
fn baseline_and_mutant_writes_are_discarded_and_exit_contract_is_preserved() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.root().join("app/src/lib.rs")).unwrap();
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            "touch BASELINE; touch MUTANT; true",
            "app/src/lib.rs"
        ]),
        ExitCode::from(3)
    );
    assert!(!fixture.root().join("BASELINE").exists());
    assert!(!fixture.root().join("MUTANT").exists());
    assert_eq!(
        fs::read(fixture.root().join("app/src/lib.rs")).unwrap(),
        before
    );
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            "touch BASELINE; false",
            "app/src/lib.rs"
        ]),
        ExitCode::from(2)
    );
    assert!(!fixture.root().join("BASELINE").exists());
}

#[test]
fn custom_coverage_is_isolated_and_changed_digests_are_reexecuted() {
    let fixture = Fixture::new();
    let command = "mkdir -p coverage; touch COVERAGE_WRITE; printf 'SF:%s/app/src/lib.rs\\nDA:3,1\\nDA:4,1\\nDA:5,1\\nLF:3\\nLH:3\\nend_of_record\\n' \"$PWD\" > coverage/lcov.info";
    assert_eq!(
        fixture.run(&[
            "--coverage-command",
            command,
            "--mutate-all",
            "app/src/lib.rs"
        ]),
        ExitCode::SUCCESS
    );
    assert!(!fixture.root().join("COVERAGE_WRITE").exists());
    assert!(!fixture.root().join("coverage").exists());
    assert_eq!(
        fixture.run(&["--coverage-command", command, "app/src/lib.rs"]),
        ExitCode::SUCCESS
    );
    let path = fixture.root().join("app/src/lib.rs");
    let source = fs::read_to_string(&path).unwrap();
    fs::write(&path, source.replace("x + x", "x + x + 0")).unwrap();
    assert_eq!(
        fixture.run(&["--coverage-command", command, "app/src/lib.rs"]),
        ExitCode::from(3)
    );
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        source.replace("x + x", "x + x + 0")
    );
}

#[test]
fn default_coverage_write_is_also_discarded() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run(&["--mutate-all", "app/src/lib.rs"]),
        ExitCode::SUCCESS
    );
    assert!(!fixture.root().join("target").exists());
    assert!(fixture.root().join(".metrics/mutate/app.edn").is_file());
}

#[test]
fn changed_selection_keeps_untracked_rust_files_without_source_roots() {
    let fixture = Fixture::new();
    fixture.commit_baseline();
    fs::write(
        fixture.root().join("app/src/extra.rs"),
        "pub fn extra(x: i64) -> i64 {\n    x + x\n}\n",
    )
    .unwrap();
    assert_eq!(
        fixture.run(&["--changed", "--no-coverage", "--test-command", "true"]),
        ExitCode::from(3)
    );
    assert!(
        fixture
            .root()
            .join(".metrics/mutate/app/extra.edn")
            .is_file(),
        "the changed source must be mutated: {}",
        fixture.root().join(".metrics").display()
    );
}

#[test]
fn changed_selection_still_honours_filters_roots_and_exclusions() {
    let fixture = Fixture::new();
    fixture.commit_baseline();
    let lib = fixture.root().join("app/src/lib.rs");
    fs::write(
        &lib,
        format!("{}// changed\n", fs::read_to_string(&lib).unwrap()),
    )
    .unwrap();
    fs::write(
        fixture.root().join("app/src/extra.rs"),
        "pub fn extra(x: i64) -> i64 {\n    x + x\n}\n",
    )
    .unwrap();
    // A positional text filter narrows the changed selection like any other.
    assert_eq!(
        fixture.run(&[
            "--changed",
            "--no-coverage",
            "--test-command",
            "true",
            "no-such-file-name"
        ]),
        ExitCode::SUCCESS
    );
    assert!(!fixture.root().join(".metrics/mutate").exists());
    // The source root keeps changed files; the exclusion removes one of them.
    assert_eq!(
        fixture.run(&[
            "--changed",
            "-s",
            "app/src",
            "--exclude",
            "app/src/extra.rs",
            "--no-coverage",
            "--test-command",
            "true"
        ]),
        ExitCode::from(3)
    );
    assert!(fixture.root().join(".metrics/mutate/app.edn").is_file());
    assert!(
        !fixture
            .root()
            .join(".metrics/mutate/app/extra.edn")
            .exists()
    );
}

#[test]
fn differential_rerun_republishes_a_snapshot_when_a_form_gained_no_sites() {
    let fixture = Fixture::new();
    let command = "grep -q 'x + x' app/src/lib.rs";
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            command,
            "app/src/lib.rs"
        ]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.root().join(".metrics/mutate/app.edn");
    let before = fs::read_to_string(&snapshot).unwrap();
    assert!(before.contains(":killed 1"), "{before}");
    let source = fixture.root().join("app/src/lib.rs");
    fs::write(
        &source,
        format!(
            "{}\npub fn no_sites() {{}}\n",
            fs::read_to_string(&source).unwrap()
        ),
    )
    .unwrap();
    // No site is selected, but the function digests changed, so the snapshot
    // is republished with the new form set instead of being kept as it was.
    assert_eq!(
        fixture.run(&["--no-coverage", "--test-command", command, "app/src/lib.rs"]),
        ExitCode::SUCCESS
    );
    let after = fs::read_to_string(&snapshot).unwrap();
    assert_ne!(after, before);
    assert!(after.contains("defn/no_sites"), "{after}");
    assert!(after.contains(":killed 1"), "{after}");
    // The pinned original republishes exactly these bytes for the same edit.
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mutate/goldens/app-no-sites.edn");
    assert_eq!(after.as_bytes(), fs::read(&golden).unwrap().as_slice());
}

#[test]
fn stale_backups_refuse_the_run_before_any_test_executes() {
    let fixture = Fixture::new();
    let backup = fixture.root().join("target/mutator-backup/app.rs");
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    fs::write(&backup, "stale mutant backup\n").unwrap();
    let marker = fixture.scratch().join("ran");
    let command = format!("echo ran >> {}", marker.display());
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &command,
            "app/src/lib.rs"
        ]),
        ExitCode::from(2)
    );
    assert!(
        !marker.exists(),
        "no test may run while an older mutator's backups exist"
    );
    assert!(!fixture.root().join(".metrics/mutate").exists());
    // The original refuses on a file, not on an empty backup directory.
    fs::remove_file(&backup).unwrap();
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &command,
            "app/src/lib.rs"
        ]),
        ExitCode::from(3)
    );
    assert!(marker.exists());
}
