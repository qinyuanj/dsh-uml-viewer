//! Python and TypeScript mutation runs against the pinned original's bytes.
//!
//! Both delivered hosts run this: native Windows x64 through per-command project
//! copies, and Linux (kept, not delivered) through overlay views. The runs need
//! real coverage.py/pytest and Node/npm. A missing toolchain is a loud SKIP (and
//! a failure when `MUTATOR_REQUIRE_TOOLCHAINS=1`), never a silent pass: the
//! `windows-quality` job runs this file with that variable set. The site and
//! form parity tests live in `python_ts_sites.rs` and run natively too.
#![cfg(any(target_os = "linux", windows))]
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

/// Whether `interpreter` can import the two tools a Python fixture needs.
fn imports_coverage_and_pytest(interpreter: &Path) -> bool {
    std::process::Command::new(interpreter)
        .args(["-c", "import coverage, pytest"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The isolation level a snapshot records here: the two values of the crate's
/// private `isolation::level`, which this integration test cannot call.
const LEVEL: &str = if cfg!(windows) {
    "copy"
} else {
    "namespace-view"
};

/// The interpreter that supplies coverage.py and pytest to a Python fixture.
///
/// Linux takes the isolated venv named by `MUTATOR_TEST_PYTHON_VENV`, which the
/// fixture links into the project at `.venv` so the mounted view carries it.
/// Native Windows cannot use that link: a worker copy shows `.venv` as an empty
/// directory and materialises no directory link, so the fixture takes the
/// `python` on PATH instead, where the `windows-quality` job installs
/// coverage.py and pytest.
fn python_interpreter() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let interpreter = PathBuf::from("python");
        if !imports_coverage_and_pytest(&interpreter) {
            skip("python on PATH cannot import coverage and pytest");
            return None;
        }
        Some(interpreter)
    }
    #[cfg(not(windows))]
    {
        let Some(venv) = std::env::var_os("MUTATOR_TEST_PYTHON_VENV").map(PathBuf::from) else {
            skip("MUTATOR_TEST_PYTHON_VENV is not set (no isolated coverage.py/pytest venv)");
            return None;
        };
        let interpreter = venv.join("bin/python");
        if !imports_coverage_and_pytest(&interpreter) {
            skip(&format!(
                "{} cannot import coverage and pytest",
                interpreter.display()
            ));
            return None;
        }
        Some(venv)
    }
}

fn node_available() -> bool {
    let node = std::process::Command::new("node")
        .args(["--experimental-strip-types", "-e", ""])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    // Windows' npm is a .cmd shim, and CreateProcess needs the extension.
    let npm = std::process::Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" })
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
/// A path relative to `base`, written with `/` as a snapshot writes it. Windows
/// keeps backslashes in `strip_prefix`'s remainder, and every name this file
/// compares or filters is a project-relative path, not a host path (ADR 0010).
fn relative_name(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
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
                found.insert(relative_name(&path, base), fs::read(&path).unwrap());
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}
impl Fixture {
    fn new(language: &'static str) -> Option<Self> {
        match language {
            "python" => {
                python_interpreter()?;
            }
            "typescript" => {
                if !node_available() {
                    return None;
                }
            }
            _ => unreachable!(),
        }
        let directory = std::env::temp_dir().join(format!(
            "mutator-python-ts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let project = directory.join("project");
        copy(&fixture_dir().join(language).join("project"), &project);
        // The mounted view carries the project's `.venv` link, so the fixture
        // links the isolated venv in. A Windows worker copy shows `.venv` as an
        // empty directory, so the fixture there uses the interpreter on PATH.
        #[cfg(not(windows))]
        if language == "python" {
            let venv = PathBuf::from(std::env::var_os("MUTATOR_TEST_PYTHON_VENV")?);
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
                    found.insert(relative_name(&path, base), fs::read(&path).unwrap());
                }
            }
        }
        walk(&base, &base, &mut found);
        found
            .into_iter()
            .map(|(name, bytes)| (name.clone(), expected_golden(&bytes, self.language, &name)))
            .collect()
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
                    found.insert(relative_name(&path, base), fs::read(&path).unwrap());
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

/// The bytes a snapshot must have on this host.
///
/// The only field the host decides is `:isolation`: `:namespace-view` on Linux,
/// `:copy` on native Windows. The goldens were recorded on Linux, so that one
/// field is rewritten for the host and everything else is compared byte for
/// byte. A golden that no longer records the field fails here instead of
/// silently comparing two different strings.
fn expected_golden(golden: &[u8], language: &str, name: &str) -> Vec<u8> {
    const RECORDED: &str = ":isolation :namespace-view";
    let text = String::from_utf8(golden.to_vec()).unwrap();
    if LEVEL == "namespace-view" {
        return text.into_bytes();
    }
    assert!(
        text.contains(RECORDED),
        "{language}/{name} records the isolation level"
    );
    text.replacen(RECORDED, &format!(":isolation :{LEVEL}"), 1)
        .into_bytes()
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
    // Every verdict is now reusable: the first run's survivors were executed,
    // but an identical rerun neither executes nor reports them.
    assert_eq!(rerun, ExitCode::SUCCESS, "{language} rerun");
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
    let Some(venv) = python_interpreter() else {
        return;
    };
    // Only the mounted view needs the link; the copy hides `.venv`.
    #[cfg(windows)]
    let _ = &venv;
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
    // The copy hides `.venv`, so only the mounted view needs this link.
    #[cfg(not(windows))]
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
    // A command that exits 0 while reporting an empty suite, in the host shell.
    const EMPTY_SUITE: &str = if cfg!(windows) {
        "echo Ran 0 tests in 0.000s & exit 0"
    } else {
        "printf 'Ran 0 tests in 0.000s\\n'; exit 0"
    };
    let code = mutator::run_with_clock(
        &[
            OsString::from("--root"),
            project.clone().into_os_string(),
            OsString::from("--no-coverage"),
            OsString::from("--mutate-all"),
            OsString::from("--max-workers"),
            OsString::from("1"),
            OsString::from("--test-command"),
            OsString::from(EMPTY_SUITE),
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
    // The interpreter is `python3` on POSIX and `python` on Windows; the test
    // command below has to be spelled for the same host shell.
    const PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };
    const TEST_COMMAND: &str = if cfg!(windows) {
        "python -c \"from calc import add; assert add(2, 1) == 3\""
    } else {
        "python3 -c 'from calc import add; assert add(2, 1) == 3'"
    };
    let available = std::process::Command::new(PYTHON)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !available {
        skip(&format!(
            "{PYTHON} is not on PATH for the snapshot-bytes case"
        ));
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
            OsString::from(TEST_COMMAND),
            OsString::from(directory.join("calc.py").to_string_lossy().into_owned()),
        ],
        || CLOCK.to_string(),
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let golden = b"{:version 1 :tested-at \"2026-11-07T12:00:00+00:00\" :isolation :namespace-view :source \"calc.py\" :namespace \"calc\" :test-input \"08fcfabcd2d4fe8a32665beea0249601c0d72f6a1f7ab5595ce2aa90b5311985\" :test-input-scope \"global-v1\" :outcomes {\"[\\\"calc.py\\\",\\\"calc\\\",\\\"defn/add\\\",28,29,\\\"+\\\",\\\"-\\\"]\" :killed} :forms [{:id \"defn/add\" :kind \"defn\" :file \"calc.py\" :line 1 :end-line 2 :hash \"7f5d06820e37639e83a860f5e4c06bc64504526affd8a2c0203143ecf47713a4\" :killed 1 :survived 0 :uncovered 0 :sites 1}]}\n";
    let expected = expected_golden(golden, "python", "calc.edn");
    let actual = fs::read(directory.join(".metrics/mutate/calc.edn")).unwrap();
    assert_eq!(actual, expected, "original snapshot bytes");
    fs::remove_dir_all(&directory).unwrap();
}
