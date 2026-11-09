//! Python and TypeScript mutation runs against the pinned original's bytes.
//!
//! Linux is the acceptance platform: the runs need real coverage.py/pytest and
//! Node/npm. A missing toolchain is a loud SKIP (and a failure when
//! `MUTATOR_REQUIRE_TOOLCHAINS=1`), never a silent pass. The site and form
//! parity tests live in `python_ts_sites.rs` and run natively too.
#![cfg(target_os = "linux")]
// Test code is the outside harness; it creates the fixture tool links itself.
#![allow(clippy::disallowed_methods)]
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const CLOCK: &str = "2026-11-07T12:00:00+00:00";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate")
}

fn missing_toolchains_are_fatal() -> bool {
    std::env::var_os("MUTATOR_REQUIRE_TOOLCHAINS").is_some_and(|value| value == "1")
}

/// Print a skip reason so it is visible even when libtest captures the test's
/// own stderr: write to the process's real stderr, not the captured buffer.
fn loud(message: &str) {
    use std::io::Write;
    let _ = std::io::stderr().write_all(message.as_bytes());
}

fn skip(reason: &str) {
    if missing_toolchains_are_fatal() {
        panic!("required mutation toolchain unavailable: {reason}");
    }
    loud(&format!(
        "SKIP: {reason}; set MUTATOR_REQUIRE_TOOLCHAINS=1 to make this a failure\n"
    ));
}

/// The isolated project virtualenv that supplies coverage.py and pytest.
fn python_venv() -> Option<PathBuf> {
    let Some(venv) = std::env::var_os("MUTATOR_TEST_PYTHON_VENV").map(PathBuf::from) else {
        skip("MUTATOR_TEST_PYTHON_VENV is not set (no isolated coverage.py/pytest venv)");
        return None;
    };
    let interpreter = venv.join("bin/python");
    let available = std::process::Command::new(&interpreter)
        .args(["-c", "import coverage, pytest"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !available {
        skip(&format!(
            "{} cannot import coverage and pytest",
            interpreter.display()
        ));
        return None;
    }
    Some(venv)
}

fn node_available() -> bool {
    let node = std::process::Command::new("node")
        .args(["--experimental-strip-types", "-e", ""])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let npm = std::process::Command::new("npm")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !(node && npm) {
        skip("node with type stripping and npm are not on PATH");
    }
    node && npm
}

struct Fixture {
    directory: PathBuf,
    project: PathBuf,
    language: &'static str,
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
fn project_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if entry.file_type().unwrap().is_symlink() {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}
impl Fixture {
    fn new(language: &'static str) -> Option<Self> {
        let venv = match language {
            "python" => python_venv(),
            "typescript" => node_available().then(PathBuf::new),
            _ => unreachable!(),
        }?;
        let directory = std::env::temp_dir().join(format!(
            "mutator-python-ts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let project = directory.join("project");
        copy(&fixture_dir().join(language).join("project"), &project);
        if language == "python" {
            for package in [PathBuf::from("."), PathBuf::from("legacy")] {
                std::os::unix::fs::symlink(&venv, project.join(package).join(".venv")).unwrap();
            }
        }
        Some(Self {
            directory,
            project,
            language,
        })
    }
    fn targets(&self) -> Vec<&'static str> {
        match self.language {
            "python" => vec!["src/shapes/geometry.py", "legacy/tally/core.py"],
            "typescript" => vec!["src/calc.ts", "src/view.tsx"],
            _ => unreachable!(),
        }
    }
    fn run(&self, workers: usize, extra: &[&str]) -> ExitCode {
        let mut args = vec![
            OsString::from("--root"),
            self.project.clone().into_os_string(),
            OsString::from("--max-workers"),
            OsString::from(workers.to_string()),
            OsString::from("--timeout-factor"),
            OsString::from("20"),
        ];
        args.extend(extra.iter().map(OsString::from));
        mutator::run_with_clock(&args, || CLOCK.to_string())
    }
    fn golden(&self) -> BTreeMap<String, Vec<u8>> {
        let base = fixture_dir().join(self.language).join("goldens");
        let mut found = BTreeMap::new();
        fn walk(base: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(directory).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(base, &path, found);
                } else {
                    found.insert(
                        path.strip_prefix(base)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                        fs::read(&path).unwrap(),
                    );
                }
            }
        }
        walk(&base, &base, &mut found);
        found
    }
    fn published(&self) -> BTreeMap<String, Vec<u8>> {
        let base = self.project.join(".metrics/mutate");
        let mut found = BTreeMap::new();
        fn walk(base: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(directory) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(base, &path, found);
                } else {
                    found.insert(
                        path.strip_prefix(base)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                        fs::read(&path).unwrap(),
                    );
                }
            }
        }
        walk(&base, &base, &mut found);
        found
    }
    /// No coverage report, test cache or build output reaches the project.
    fn assert_project_untouched(&self, before: &BTreeMap<String, Vec<u8>>) {
        let after = project_files(&self.project);
        let sources: BTreeMap<_, _> = after
            .iter()
            .filter(|(name, _)| !name.starts_with(".metrics/"))
            .collect();
        let before: BTreeMap<_, _> = before.iter().collect();
        assert_eq!(sources, before, "{} sources changed", self.language);
        for name in after.keys() {
            assert!(
                !name.starts_with("target/")
                    && !name.starts_with("coverage/")
                    && !name.starts_with("node_modules/")
                    && !name.contains("__pycache__")
                    && !name.contains(".coverage")
                    && !name.ends_with(".pyc"),
                "artifact leaked into the project: {name}"
            );
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let name = self
            .directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !self.directory.starts_with(std::env::temp_dir())
            || !name.starts_with("mutator-python-ts-")
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

/// Acceptance: both languages' snapshots equal the pinned original's bytes,
/// and an unchanged differential rerun keeps them.
fn golden_run(language: &'static str) {
    let Some(fixture) = Fixture::new(language) else {
        return;
    };
    let before = project_files(&fixture.project);
    let golden = fixture.golden();
    assert!(!golden.is_empty());
    let mut args = vec!["--mutate-all"];
    args.extend(fixture.targets());
    let first = fixture.run(1, &args);
    let published = fixture.published();
    assert_eq!(published, golden, "{language} first run bytes");
    // The surviving mutants the goldens record make this exit 3.
    assert_eq!(first, ExitCode::from(3), "{language} first run");
    fixture.assert_project_untouched(&before);
    let rerun = fixture.run(1, &fixture.targets());
    assert_eq!(rerun, ExitCode::from(3), "{language} rerun");
    assert_eq!(fixture.published(), golden, "{language} rerun bytes");
    fixture.assert_project_untouched(&before);
}

#[test]
fn python_snapshots_are_the_pinned_originals_bytes() {
    golden_run("python");
}

#[test]
fn typescript_snapshots_are_the_pinned_originals_bytes() {
    golden_run("typescript");
}

/// The #9 scheduler serves the new languages too: more workers, same bytes.
fn parallel_run(language: &'static str) {
    let Some(single) = Fixture::new(language) else {
        return;
    };
    let Some(parallel) = Fixture::new(language) else {
        return;
    };
    let mut args = vec!["--mutate-all"];
    args.extend(single.targets());
    single.run(1, &args);
    parallel.run(2, &args);
    assert_eq!(
        single.published(),
        parallel.published(),
        "{language} workers"
    );
    assert_eq!(parallel.published(), parallel.golden(), "{language} golden");
}

#[test]
fn python_workers_publish_the_same_bytes_as_one_worker() {
    parallel_run("python");
}

#[test]
fn typescript_workers_publish_the_same_bytes_as_one_worker() {
    parallel_run("typescript");
}

/// `--mutate-all` is a decision, not a wider selection: only the named file
/// publishes a snapshot.
#[test]
fn python_mutate_all_publishes_only_the_named_file() {
    let Some(fixture) = Fixture::new("python") else {
        return;
    };
    let code = fixture.run(1, &["--mutate-all", "legacy/tally/core.py"]);
    assert_eq!(code, ExitCode::SUCCESS);
    let published = fixture.published();
    assert_eq!(published.len(), 1);
    assert!(published.contains_key("legacy.tally.core.edn"));
}

/// `--scan` inventories sites without coverage, tests or metrics.
#[test]
fn python_and_typescript_scan_writes_no_metrics() {
    for language in ["python", "typescript"] {
        let Some(fixture) = Fixture::new(language) else {
            return;
        };
        let mut args = vec![
            OsString::from("--root"),
            fixture.project.clone().into_os_string(),
            OsString::from("--scan"),
        ];
        // --scan conflicts with --max-workers in the original's CLI.
        args.extend(fixture.targets().iter().map(OsString::from));
        assert_eq!(
            mutator::run_with_clock(&args, || CLOCK.to_string()),
            ExitCode::SUCCESS
        );
        assert!(!fixture.project.join(".metrics").exists(), "{language}");
        assert!(!fixture.project.join("target").exists(), "{language}");
        assert!(!fixture.project.join("coverage").exists(), "{language}");
    }
}

/// Ported `engine._remember_baseline`: unittest discovery that finds nothing
/// fails the baseline instead of letting every mutant survive.
#[test]
fn python_unittest_discovery_with_no_tests_fails_the_baseline() {
    let Some(venv) = python_venv() else {
        return;
    };
    let directory = std::env::temp_dir().join(format!(
        "mutator-python-ts-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let project = directory.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("pyproject.toml"),
        "[project]\nname = \"empty\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/demo.py"),
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&venv, project.join(".venv")).unwrap();
    let code = mutator::run_with_clock(
        &[
            OsString::from("--root"),
            project.clone().into_os_string(),
            OsString::from("--no-coverage"),
            OsString::from("--mutate-all"),
            OsString::from("--max-workers"),
            OsString::from("1"),
            OsString::from("src/demo.py"),
        ],
        || CLOCK.to_string(),
    );
    assert_eq!(code, ExitCode::from(2), "an empty suite must not mutate");
    assert!(!project.join(".metrics").exists());
    assert_eq!(
        fs::read_to_string(project.join("src/demo.py")).unwrap(),
        "def add(a, b):\n    return a + b\n"
    );
    // The guard itself: a command that exits 0 but reports an empty suite is a
    // failed baseline, even where unittest's own exit status would not fail.
    let code = mutator::run_with_clock(
        &[
            OsString::from("--root"),
            project.clone().into_os_string(),
            OsString::from("--no-coverage"),
            OsString::from("--mutate-all"),
            OsString::from("--max-workers"),
            OsString::from("1"),
            OsString::from("--test-command"),
            OsString::from("printf 'Ran 0 tests in 0.000s\\n'; exit 0"),
            OsString::from("src/demo.py"),
        ],
        || CLOCK.to_string(),
    );
    assert_eq!(
        code,
        ExitCode::from(2),
        "Ran 0 tests must fail the baseline"
    );
    assert!(!project.join(".metrics").exists());
    fs::remove_dir_all(&directory).unwrap();
}

/// Ported `test_snapshot_bytes.py`: one Python file, one killed mutant, the
/// original's exact LF bytes with this platform's `:namespace-view`.
#[test]
fn a_single_python_file_publishes_the_original_snapshot_bytes() {
    let available = std::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !available {
        skip("python3 is not on PATH for the snapshot-bytes case");
        return;
    }
    let directory = std::env::temp_dir().join(format!(
        "mutator-python-ts-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("calc.py"),
        b"def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    let code = mutator::run_with_clock(
        &[
            OsString::from("--root"),
            directory.clone().into_os_string(),
            OsString::from("--no-coverage"),
            OsString::from("--mutate-all"),
            OsString::from("--max-workers"),
            OsString::from("1"),
            OsString::from("--test-command"),
            OsString::from("python3 -c 'from calc import add; assert add(2, 1) == 3'"),
            OsString::from(directory.join("calc.py").to_string_lossy().into_owned()),
        ],
        || CLOCK.to_string(),
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let expected = b"{:version 1 :tested-at \"2026-11-07T12:00:00+00:00\" :isolation :namespace-view :source \"calc.py\" :namespace \"calc\" :outcomes {\"[\\\"calc.py\\\",\\\"calc\\\",\\\"defn/add\\\",28,29,\\\"+\\\",\\\"-\\\"]\" :killed} :forms [{:id \"defn/add\" :kind \"defn\" :file \"calc.py\" :line 1 :end-line 2 :hash \"7f5d06820e37639e83a860f5e4c06bc64504526affd8a2c0203143ecf47713a4\" :killed 1 :survived 0 :uncovered 0 :sites 1}]}\n";
    let actual = fs::read(directory.join(".metrics/mutate/calc.edn")).unwrap();
    assert_eq!(actual, expected, "original snapshot bytes");
    fs::remove_dir_all(&directory).unwrap();
}
