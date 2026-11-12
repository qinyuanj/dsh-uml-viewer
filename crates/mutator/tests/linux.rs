#![cfg(target_os = "linux")]
// Test code is the outside harness; it launches the fixture tools itself.
#![allow(clippy::disallowed_methods)]
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
/// Every published snapshot under `.metrics/mutate`, by project-relative path.
fn snapshot_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, found);
            } else if path.extension().is_some_and(|ext| ext == "edn") {
                let key = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                found.insert(key, fs::read(&path).unwrap());
            }
        }
    }
    let base = root.join(".metrics/mutate");
    let mut found = BTreeMap::new();
    walk(&base, &base, &mut found);
    found
}
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
        self.run_many(1, extra)
    }
    fn run_many(&self, workers: usize, extra: &[&str]) -> ExitCode {
        let mut args = vec![
            OsString::from("--root"),
            self.root().into_os_string(),
            OsString::from("--max-workers"),
            OsString::from(workers.to_string()),
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

/// `--copy` (ADR 0015) is accepted and copies nothing on Linux: the namespace
/// view is mounted over the real project root, so `..` already reaches the real
/// parent directory and a path beside the project is readable without help. The
/// snapshot must be the same bytes with and without the option.
#[test]
fn copy_is_accepted_and_copies_nothing_in_the_namespace_view() {
    let command = |fixture: &Fixture| {
        let parent = quality_core::process::shlex_quote(&fixture.directory.display().to_string());
        format!(
            // The parent is the real one, not an overlay above it; the outside
            // bytes are readable; and nothing was copied into the view.
            "test \"$(readlink -f ..)\" = {parent} && \
             grep -q copy-value ../contracts/value.txt && \
             test ! -e \"$MUTATOR_VIEW/contracts\" && \
             ! test -f \"$MUTATOR_VIEW/mutation\""
        )
    };
    let plain = Fixture::new();
    fs::create_dir_all(plain.directory.join("contracts")).unwrap();
    fs::write(plain.directory.join("contracts/value.txt"), "copy-value\n").unwrap();
    let without = command(&plain);
    assert_eq!(
        plain.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &without,
            "app/src/lib.rs"
        ]),
        ExitCode::SUCCESS
    );
    let copied = Fixture::new();
    fs::create_dir_all(copied.directory.join("contracts")).unwrap();
    fs::write(copied.directory.join("contracts/value.txt"), "copy-value\n").unwrap();
    let with = command(&copied);
    assert_eq!(
        copied.run(&[
            "--no-coverage",
            "--mutate-all",
            "--copy",
            "../contracts",
            "--test-command",
            &with,
            "app/src/lib.rs"
        ]),
        ExitCode::SUCCESS
    );
    let snapshot = |fixture: &Fixture| fs::read(fixture.root().join(".metrics/mutate/app.edn"));
    assert_eq!(snapshot(&plain).unwrap(), snapshot(&copied).unwrap());
}

/// Acceptance: with `--max-workers N` for N > 1 every published snapshot is
/// byte-identical to the single-worker run with the same injected clock. The
/// comparison covers every file the two runs published, not a sample.
#[test]
fn multiple_workers_publish_the_same_bytes_as_one_worker() {
    let targets = [
        "--mutate-all",
        "--timeout-factor",
        "20",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    let single = Fixture::new();
    assert_eq!(single.run(&targets), ExitCode::SUCCESS);
    let parallel = Fixture::new();
    assert_eq!(parallel.run_many(3, &targets), ExitCode::SUCCESS);
    let expected = snapshot_bytes(&single.root());
    assert_eq!(expected.len(), 3, "{}", single.root().display());
    assert_eq!(snapshot_bytes(&parallel.root()), expected);
    // Both runs are the pinned original's bytes, not merely each other's.
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/goldens");
    for (file, bytes) in &expected {
        assert_eq!(
            *bytes,
            fs::read(golden.join(file)).unwrap(),
            "{file} differs from the original's bytes"
        );
    }
}

/// Ported from the original `test_concurrent_mutants_use_private_workers`:
/// with two workers, two mutants of one file are alive at the same time.
#[test]
fn concurrent_mutants_run_in_parallel_worker_views() {
    let fixture = Fixture::new();
    // Two mutation sites and no numeric literal: `(x + x) + x`.
    fs::write(
        fixture.root().join("app/src/extra.rs"),
        "pub fn extra(x: i64) -> i64 { x + x + x }\n",
    )
    .unwrap();
    let barrier = fixture.scratch().join("barrier");
    for round in ["r1", "r2"] {
        fs::create_dir_all(barrier.join(round)).unwrap();
    }
    // Two rendezvous rounds. In round one each mutant waits for the other;
    // only a run that really had both alive reaches round two, where they wait
    // for each other again. A sequential run leaves the first round's mark
    // behind, but never a round-two mark: the second mutant is killed waiting
    // there. A stale mark therefore cannot make this pass by accident.
    let command = format!(
        "f={b}; \
         if grep -q 'x - x + x' app/src/extra.rs; then n=inner; \
         elif grep -q 'x + x - x' app/src/extra.rs; then n=outer; \
         else exit 0; fi; \
         wait_for() {{ i=0; \
           while [ -z \"$(find \"$1\" -type f -not -name \"$n\" | head -1)\" ]; do \
             i=$((i+1)); [ $i -lt 60 ] || exit 99; sleep 0.02; done; }}; \
         touch \"$f/r1/$n\"; wait_for \"$f/r1\"; \
         touch \"$f/r2/$n\"; wait_for \"$f/r2\"; true",
        b = barrier.display()
    );
    assert_eq!(
        fixture.run_many(
            2,
            &[
                "--no-coverage",
                "--mutate-all",
                "--test-command",
                &command,
                "app/src/extra.rs"
            ]
        ),
        // Both mutants see the other one and succeed, so both survive.
        ExitCode::from(3)
    );
    let mut ready: Vec<String> = fs::read_dir(barrier.join("r2"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    ready.sort();
    assert_eq!(
        ready,
        ["inner", "outer"],
        "both mutants must run at once, not one after the other"
    );
    let snapshot =
        fs::read_to_string(fixture.root().join(".metrics/mutate/app/extra.edn")).unwrap();
    assert!(snapshot.contains(":sites 2"), "{snapshot}");
    assert!(snapshot.contains(":survived 2"), "{snapshot}");
}

/// Acceptance: one coverage run and one baseline are shared by every target
/// file of a single call, and each target still publishes its own snapshot.
#[test]
fn one_coverage_run_and_one_baseline_are_shared_by_every_target_file() {
    let fixture = Fixture::new();
    let marker = fixture.scratch().join("phases.txt");
    let command = format!(
        "if [ -f \"$MUTATOR_VIEW/mutation\" ]; then echo mutant >> {marker}; \
         else echo baseline >> {marker}; fi; true",
        marker = marker.display()
    );
    let coverage = format!(
        "printf 'coverage\n' >> {marker}; mkdir -p coverage; \
         printf 'SF:%s/util/src/lib.rs\nDA:2,1\nLH:1\nLF:1\nend_of_record\n\
         SF:%s/app/src/lib.rs\nDA:4,1\nLH:1\nLF:1\nend_of_record\n\
         SF:%s/app/src/more.rs\nDA:2,1\nLH:1\nLF:1\nend_of_record\n' \
         \"$PWD\" \"$PWD\" \"$PWD\" > coverage/lcov.info",
        marker = marker.display()
    );
    assert_eq!(
        fixture.run_many(
            2,
            &[
                "--coverage-command",
                &coverage,
                "--mutate-all",
                "--test-command",
                &command,
                "util/src/lib.rs",
                "app/src/lib.rs",
                "app/src/more.rs"
            ]
        ),
        // Every mutant's command succeeds, so all three sites survive.
        ExitCode::from(3)
    );
    let text = fs::read_to_string(&marker).unwrap();
    let phases: Vec<&str> = text.lines().collect();
    let count = |phase: &str| phases.iter().filter(|line| **line == phase).count();
    assert_eq!(count("coverage"), 1, "{text}");
    assert_eq!(count("baseline"), 1, "{text}");
    assert_eq!(count("mutant"), 3, "{text}");
    // Each target file published its own snapshot.
    let mut published: Vec<String> = snapshot_bytes(&fixture.root()).into_keys().collect();
    published.sort();
    assert_eq!(published, ["app.edn", "app/more.edn", "util.edn"]);
}

/// Acceptance: worker slots belong to the call, not to the file or the
/// command. Two files of two sites each use the same two slots.
#[test]
fn worker_slots_are_reused_across_target_files() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root().join("app/src/extra.rs"),
        "pub fn alpha(x: i64) -> i64 { x + x + x }\n",
    )
    .unwrap();
    fs::write(
        fixture.root().join("util/src/extra.rs"),
        "pub fn beta(y: i64) -> i64 { y + y + y }\n",
    )
    .unwrap();
    let recorded = fixture.scratch().join("workers.txt");
    let command = format!(
        "sleep 0.15; \
         if grep -q 'x - x + x' app/src/extra.rs; then n=app-inner; \
         elif grep -q 'x + x - x' app/src/extra.rs; then n=app-outer; \
         elif grep -q 'y - y + y' util/src/extra.rs; then n=util-inner; \
         elif grep -q 'y + y - y' util/src/extra.rs; then n=util-outer; \
         else n=baseline; fi; \
         printf '%s %s\n' \"$MUTATOR_VIEW\" \"$n\" >> {recorded}; true",
        recorded = recorded.display()
    );
    assert_eq!(
        fixture.run_many(
            2,
            &[
                "--no-coverage",
                "--mutate-all",
                "--test-command",
                &command,
                "app/src/extra.rs",
                "util/src/extra.rs"
            ]
        ),
        ExitCode::from(3)
    );
    let text = fs::read_to_string(&recorded).unwrap();
    let lines: Vec<(String, String)> = text
        .lines()
        .map(|line| line.split_once(' ').unwrap())
        .map(|(view, name)| (view.to_string(), name.to_string()))
        .collect();
    let views: BTreeSet<&str> = lines.iter().map(|(view, _)| view.as_str()).collect();
    assert_eq!(views.len(), 2, "{text}");
    let mut mutants: Vec<&str> = lines
        .iter()
        .filter(|(_, name)| name != "baseline")
        .map(|(_, name)| name.as_str())
        .collect();
    mutants.sort();
    assert_eq!(
        mutants,
        ["app-inner", "app-outer", "util-inner", "util-outer"],
        "{text}"
    );
    // Both target files' mutants ran in the same two slots: the pool belongs
    // to the call, not to the file.
    for file in ["app", "util"] {
        let used: BTreeSet<&str> = lines
            .iter()
            .filter(|(_, name)| name.starts_with(file))
            .map(|(view, _)| view.as_str())
            .collect();
        assert_eq!(used, views, "{file} used {used:?} of {views:?}");
    }
}

/// Acceptance: `--mutate-all` changes decisions, never the selection, and only
/// the files named on the command line publish snapshots.
#[test]
fn mutate_all_applies_only_to_the_explicitly_selected_files() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run_many(
            2,
            &[
                "--no-coverage",
                "--mutate-all",
                "--test-command",
                "true",
                "app/src/lib.rs"
            ]
        ),
        ExitCode::from(3)
    );
    let published = snapshot_bytes(&fixture.root());
    assert_eq!(
        published.keys().cloned().collect::<Vec<String>>(),
        ["app.edn"]
    );
    assert!(
        String::from_utf8_lossy(&published["app.edn"]).contains(":survived 1"),
        "the selected site must execute under --mutate-all"
    );
    // A source root still narrows the named files: the flag widens no scope.
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            "true",
            "-s",
            "app/src",
            "util/src/lib.rs"
        ]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        snapshot_bytes(&fixture.root())
            .keys()
            .cloned()
            .collect::<Vec<String>>(),
        ["app.edn"]
    );
    // A second file, alone: still exactly one snapshot for it.
    let other = Fixture::new();
    assert_eq!(
        other.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            "true",
            "util/src/lib.rs"
        ]),
        ExitCode::from(3)
    );
    assert_eq!(
        snapshot_bytes(&other.root())
            .keys()
            .cloned()
            .collect::<Vec<String>>(),
        ["util.edn"]
    );
}

/// Whether a host process is gone, waiting out the kernel's reaping.
fn gone(pid: u32) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if !Path::new(&format!("/proc/{pid}")).exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    !Path::new(&format!("/proc/{pid}")).exists()
}

/// A command that leaves a `sleep` behind, recording its host pid in `pids`.
/// Every command gets its own file; `tail` decides its exit status and runs
/// once the pid is recorded.
fn leaves_a_child(fixture: &Fixture, ignore_term: bool, tail: &str) -> (String, PathBuf) {
    let helper = fixture.scratch().join("leave.sh");
    let trap = if ignore_term { "trap '' TERM\n" } else { "" };
    fs::write(
        &helper,
        format!(
            "#!/bin/sh\n{trap}awk '/^NSpid:/{{print $2}}' /proc/self/status > \"$1\"\nexec sleep 600\n"
        ),
    )
    .unwrap();
    let pids = fixture.scratch().join("pids");
    fs::create_dir_all(&pids).unwrap();
    let command = format!(
        "own=$(mktemp {pids}/pid.XXXXXX); sh {helper} \"$own\" & \
         i=0; while [ ! -s \"$own\" ]; do i=$((i+1)); [ $i -lt 100 ] || exit 98; sleep 0.01; done; \
         {tail}",
        pids = pids.display(),
        helper = helper.display()
    );
    (command, pids)
}

fn recorded_pids(directory: &Path) -> Vec<u32> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|text| text.trim().parse().ok())
        .collect();
    pids.sort_unstable();
    pids
}

/// Ported from the original `test_reap.py`: a command's leftovers do not
/// outlive the view that ran it.
#[test]
fn a_child_left_by_a_passing_command_is_reaped() {
    let fixture = Fixture::new();
    let (command, pidfile) = leaves_a_child(&fixture, false, "true");
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &command,
            "app/src/lib.rs"
        ]),
        // The command passes, so the mutant survives.
        ExitCode::from(3)
    );
    let pids = recorded_pids(&pidfile);
    assert!(pids.len() >= 2, "baseline and mutant both ran: {pids:?}");
    for pid in pids {
        assert!(gone(pid), "child {pid} outlived its view");
    }
}

#[test]
fn a_failing_command_keeps_its_exit_code_and_loses_its_child() {
    let fixture = Fixture::new();
    let (command, pidfile) = leaves_a_child(
        &fixture,
        false,
        "if [ -f \"$MUTATOR_VIEW/mutation\" ]; then exit 3; fi; true",
    );
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &command,
            "app/src/lib.rs"
        ]),
        // A failing mutant is a killed mutant, not a failed run.
        ExitCode::SUCCESS
    );
    for pid in recorded_pids(&pidfile) {
        assert!(gone(pid), "child {pid} outlived its view");
    }
}

#[test]
fn a_child_that_ignores_sigterm_is_killed_with_its_view() {
    let fixture = Fixture::new();
    let (command, pidfile) = leaves_a_child(&fixture, true, "true");
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
    for pid in recorded_pids(&pidfile) {
        assert!(gone(pid), "child {pid} survived its view");
    }
}

/// Ported from the original `test_runtime_failure_removes_workers`: a mutant
/// that never finishes is killed, the source stays untouched and the call's
/// scratch root is removed.
#[test]
fn a_timed_out_mutant_is_killed_and_the_scratch_root_is_removed() {
    let fixture = Fixture::new();
    let source = fixture.root().join("app/src/lib.rs");
    let before = fs::read(&source).unwrap();
    let recorded = fixture.scratch().join("roots.txt");
    let command = format!(
        "echo \"$(dirname \"$MUTATOR_VIEW\")\" >> {recorded}; \
         if [ -f \"$MUTATOR_VIEW/mutation\" ]; then sleep 10; fi; true",
        recorded = recorded.display()
    );
    let started = std::time::Instant::now();
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &command,
            "app/src/lib.rs"
        ]),
        ExitCode::SUCCESS
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(9),
        "the mutant must be killed at its timeout"
    );
    assert_eq!(fs::read(&source).unwrap(), before);
    let roots: BTreeSet<String> = fs::read_to_string(&recorded)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert!(!roots.is_empty());
    for root in roots {
        assert!(!Path::new(&root).exists(), "{root} survived the run");
    }
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
fn worker_views_are_private_and_their_artifacts_are_owner_only() {
    let fixture = Fixture::new();
    let recorded = fixture.scratch().join("views.txt");
    let command = format!(
        "root=$(dirname \"$MUTATOR_VIEW\"); {{ \
         printf 'root %s %s\n' \"$(stat -c %a \"$root\")\" \"$root\"; \
         printf 'view %s %s\n' \"$(stat -c %a \"$MUTATOR_VIEW\")\" \"$MUTATOR_VIEW\"; \
         if [ -f \"$MUTATOR_VIEW/mutation\" ]; then \
           printf 'file %s\n' \"$(stat -c %a \"$MUTATOR_VIEW/mutation\")\"; fi; \
         }} >> {}",
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
    let pairs = |prefix: &str| -> Vec<(String, String)> {
        lines
            .iter()
            .filter_map(|line| line.strip_prefix(prefix))
            .map(|rest| rest.split_once(' ').unwrap())
            .map(|(mode, path)| (mode.to_string(), path.to_string()))
            .collect()
    };
    let roots = pairs("root ");
    let views = pairs("view ");
    let files: Vec<String> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("file "))
        .map(str::to_string)
        .collect();
    assert!(views.len() >= 2, "{text}");
    assert!(views.iter().all(|(mode, _)| mode == "700"), "{text}");
    assert!(!files.is_empty(), "{text}");
    assert!(files.iter().all(|mode| mode == "600"), "{text}");
    // One scratch root for the whole call: private, directly under the temp
    // directory, with an unpredictable name.
    let scratch: BTreeSet<&str> = roots.iter().map(|(_, path)| path.as_str()).collect();
    assert_eq!(scratch.len(), 1, "{text}");
    for (mode, root) in &roots {
        assert_eq!(mode, "700", "{text}");
        let root = Path::new(root);
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        let name = root.file_name().and_then(|name| name.to_str()).unwrap();
        let tail = name.rsplit('-').next().unwrap();
        assert!(
            name.starts_with("mutator-")
                && tail.len() >= 16
                && tail.chars().all(|c| c.is_ascii_hexdigit()),
            "{name}"
        );
    }
    // Slots are named and reused: a worker is not a fresh directory per command.
    for (_, view) in &views {
        let view = Path::new(view);
        let name = view.file_name().and_then(|name| name.to_str()).unwrap();
        assert!(
            name.strip_prefix("worker-")
                .is_some_and(|index| index.parse::<usize>().is_ok()),
            "{name}"
        );
        assert!(
            scratch.contains(view.parent().unwrap().to_str().unwrap()),
            "{}",
            view.display()
        );
    }
    // Nothing the call created survives it.
    for (_, root) in &roots {
        assert!(!Path::new(root).exists(), "{root} survived the run");
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
