#![cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    directory: PathBuf,
    project: PathBuf,
}
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
    fn create(relative: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "mutator-acceptance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let copy_to = if relative.is_empty() {
            directory.clone()
        } else {
            directory.join(relative)
        };
        copy(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/rust"),
            &copy_to,
        );
        Self {
            project: copy_to.join("project"),
            directory,
        }
    }
    fn new() -> Self {
        Self::create("")
    }
    /// A project whose absolute path contains a skipped directory name.
    fn under_skip_named_directory() -> Self {
        Self::create("build")
    }
    fn root(&self) -> PathBuf {
        self.project.clone()
    }
    fn scratch(&self) -> &Path {
        &self.directory
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
        let name = self
            .directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        // Never remove a path this fixture did not create.
        if !self.directory.starts_with(std::env::temp_dir())
            || !name.starts_with("mutator-acceptance-")
        {
            eprintln!(
                "Not removing unexpected fixture path {}",
                self.directory.display()
            );
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.directory) {
            eprintln!(
                "Cannot remove fixture {}: {error}",
                self.directory.display()
            );
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

#[test]
fn changed_selection_keeps_files_below_a_skip_named_directory() {
    // The shared changed walk compares project-relative components, so a
    // project below ".../build/..." is not mistaken for build output.
    let fixture = Fixture::under_skip_named_directory();
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
            .is_file()
    );
}

#[test]
fn line_filter_keeps_the_carried_outcomes_of_excluded_sites() {
    let fixture = Fixture::new();
    let command = "grep -q 'x + x' app/src/lib.rs && grep -q 'x / 2' app/src/more.rs";
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            command,
            "app/src/lib.rs",
            "app/src/more.rs"
        ]),
        ExitCode::SUCCESS
    );
    let lib = fixture.root().join("app/src/lib.rs");
    let source = fs::read_to_string(&lib).unwrap();
    fs::write(&lib, source.replace("{\n    x + x", "{\n\n    x + x")).unwrap();
    // Line 5 selects only the changed lib form; more.rs stays filtered out but
    // keeps the killed outcome its earlier run recorded.
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--test-command",
            command,
            "--lines",
            "5",
            "app/src/lib.rs",
            "app/src/more.rs"
        ]),
        ExitCode::SUCCESS
    );
    let more = fs::read_to_string(fixture.root().join(".metrics/mutate/app/more.edn")).unwrap();
    assert!(more.contains(":killed 1"), "{more}");
    assert!(more.contains(":survived 0"), "{more}");
}

#[test]
fn command_views_are_private_and_their_artifacts_are_owner_only() {
    let fixture = Fixture::new();
    let recorded = fixture.scratch().join("views.txt");
    let command = format!(
        "{{ printf 'dir %s\\n' \"$(stat -c %a \"$MUTATOR_VIEW\")\"; \
         printf 'path %s\\n' \"$MUTATOR_VIEW\"; \
         if [ -f \"$MUTATOR_VIEW/mutation\" ]; then \
           printf 'file %s\\n' \"$(stat -c %a \"$MUTATOR_VIEW/mutation\")\"; fi; }} >> {}",
        recorded.display()
    );
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &command,
            "app/src/lib.rs"
        ]),
        // The recording command always succeeds, so the mutant survives.
        ExitCode::from(3)
    );
    let text = fs::read_to_string(&recorded).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let modes = |prefix: &str| -> Vec<&str> {
        lines
            .iter()
            .filter_map(|line| line.strip_prefix(prefix))
            .collect()
    };
    let directories = modes("dir ");
    let paths = modes("path ");
    let files = modes("file ");
    assert!(directories.len() >= 2, "{text}");
    assert!(directories.iter().all(|mode| *mode == "700"), "{text}");
    assert!(files.len() >= 1, "{text}");
    assert!(files.iter().all(|mode| *mode == "600"), "{text}");
    for path in paths {
        let path = Path::new(path);
        let name = path.file_name().and_then(|name| name.to_str()).unwrap();
        assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
        let tail = name.rsplit('-').next().unwrap();
        assert!(
            name.starts_with("mutator-")
                && tail.len() >= 16
                && tail.chars().all(|c| c.is_ascii_hexdigit()),
            "{name}"
        );
    }
}

#[test]
fn commands_write_to_the_view_home_and_temp_not_the_callers() {
    let fixture = Fixture::new();
    let recorded = fixture.scratch().join("environment.txt");
    let command = format!(
        "printf '%s\\n' \"$HOME\" \"$TMPDIR\" \"$XDG_CACHE_HOME\" >> {}",
        recorded.display()
    );
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
    let home = fixture.root().join("target/mutator-home");
    let text = fs::read_to_string(&recorded).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() >= 6, "{text}");
    for chunk in lines.chunks(3) {
        assert_eq!(Path::new(chunk[0]), home, "{text}");
        assert_eq!(Path::new(chunk[1]), home.join("tmp"), "{text}");
        assert_eq!(Path::new(chunk[2]), home.join(".cache"), "{text}");
    }
}

#[test]
fn a_setsid_descendant_cannot_outlive_its_view() {
    let fixture = Fixture::new();
    let escaped = fixture.scratch().join("escaped-daemon");
    let command = format!(
        "setsid sh -c 'sleep 2; touch {}' & exit 0",
        escaped.display()
    );
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
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(
        !escaped.exists(),
        "a setsid descendant must not outlive its view"
    );
}

#[test]
fn a_symlinked_snapshot_directory_is_refused_before_deleting_anything() {
    let fixture = Fixture::new();
    let victim = fixture.scratch().join("victim");
    fs::create_dir_all(victim.join("empty")).unwrap();
    fs::write(victim.join("keep.txt"), "keep\n").unwrap();
    let metrics = fixture.root().join(".metrics");
    fs::create_dir_all(&metrics).unwrap();
    std::os::unix::fs::symlink(&victim, metrics.join("mutate")).unwrap();
    assert_eq!(
        fixture.run(&["--no-coverage", "--mutate-all", "app/src/lib.rs"]),
        ExitCode::from(2)
    );
    assert!(
        victim.join("empty").is_dir(),
        "nothing outside the project may be deleted or published"
    );
    assert_eq!(
        fs::read_to_string(victim.join("keep.txt")).unwrap(),
        "keep\n"
    );
}
