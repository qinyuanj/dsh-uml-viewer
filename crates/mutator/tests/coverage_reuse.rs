//! Coverage in the standard path: crapper's stamp decides what is regenerated,
//! and the generated TypeScript/Python scripts run where the command runs — in
//! a copy of the project on native Windows (ADR 0017).
//!
//! Test code is the outside harness; the fixtures launch `npm`/`node` (and, when
//! `coverage.py` is importable, `python`) themselves. A missing toolchain is a
//! loud SKIP and a failure under `MUTATOR_REQUIRE_TOOLCHAINS=1`.
#![cfg(any(target_os = "linux", windows))]
#![allow(clippy::disallowed_methods)]
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const CLOCK: &str = "2026-11-07T12:00:00+00:00";

/// Print a skip reason on the process's real stderr, where libtest's capture
/// cannot hide it.
fn skip(reason: &str) {
    if std::env::var_os("MUTATOR_REQUIRE_TOOLCHAINS").is_some_and(|value| value == "1") {
        panic!("required mutation toolchain unavailable: {reason}");
    }
    use std::io::Write;
    let _ = std::io::stderr().write_all(
        format!("SKIP: {reason}; set MUTATOR_REQUIRE_TOOLCHAINS=1 to make this a failure\n")
            .as_bytes(),
    );
}

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// `python -c 'import <module>'` succeeds: the interpreter and that module exist.
fn python_can(module: &str) -> bool {
    std::process::Command::new(python())
        .args(["-c", &format!("import {module}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `npm` is a batch file on Windows, which CreateProcess refuses; the mutator's
/// own plan reaches it through its batch shim, so the check spells `npm.cmd`.
fn node_available() -> bool {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    ["node", npm].iter().all(|tool| {
        std::process::Command::new(tool)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "mutator-coverage-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        Self { directory }
    }
    fn root(&self) -> PathBuf {
        self.directory.clone()
    }
    fn write(&self, relative: &str, text: &str) {
        let path = self.directory.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn run(&self, extra: &[&str]) -> ExitCode {
        let mut args = vec![
            "--root".to_string(),
            self.root().to_string_lossy().into_owned(),
            "--max-workers".to_string(),
            "1".to_string(),
        ];
        args.extend(extra.iter().map(|arg| arg.to_string()));
        let args: Vec<std::ffi::OsString> = args.iter().map(std::ffi::OsString::from).collect();
        mutator::run_with_clock(&args, || CLOCK.to_string())
    }
    /// The one published snapshot, as text.
    fn snapshot(&self) -> String {
        fn walk(directory: &Path) -> Option<PathBuf> {
            for entry in fs::read_dir(directory).ok()?.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(found) = walk(&path) {
                        return Some(found);
                    }
                } else if path.extension().is_some_and(|ext| ext == "edn") {
                    return Some(path);
                }
            }
            None
        }
        let found = walk(&self.directory.join(".metrics/mutate")).expect("a published snapshot");
        fs::read_to_string(found).unwrap()
    }

    /// crapper's two products for this project: its LCOV report and the
    /// version-1 stamp that says the language is fresh for this tree.
    fn record_crapper_coverage(&self) {
        let source = self.directory.join("src/book.ts");
        self.write(
            "target/coverage/typescript/lcov.info",
            &format!(
                "SF:{}\nDA:2,1\nDA:3,1\nend_of_record\n",
                quality_core::paths::posix(&source)
            ),
        );
        quality_core::stamp::record(
            &self.directory,
            &[source],
            None,
            &[],
            &quality_core::stamp::fingerprint(&self.directory),
        );
        assert!(
            quality_core::stamp::stamp_path(&self.directory).is_file(),
            "the fixture must record a stamp"
        );
    }

    /// crapper's report with a stamp the tree has since outgrown: the report is
    /// still on disk and the stamp names the language, but it is stale.
    fn stale_crapper_coverage(&self) {
        self.record_crapper_coverage();
        self.write(
            "CHANGED.md",
            "the tree moved on after crapper recorded it\n",
        );
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
            || !name.starts_with("mutator-coverage-")
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

const BOOK: &str = "export function twice(x: number): number {\n  return x + x;\n}\n";

/// The test command fails on any mutant: it compares the source to its own
/// bytes, so "killed" also proves the mutant ran.
const TEST_JS: &str = r#"
const fs = require('fs');
const pristine = 'export function twice(x: number): number {\n  return x + x;\n}\n';
const source = fs.readFileSync('src/book.ts', 'utf8');
process.exit(source === pristine ? 0 : 1);
"#;

/// The stamp is what the standard path consults: with it, the reports crapper
/// already wrote are read and no coverage tool runs (the project has none).
#[test]
fn crapper_stamp_reuses_typescript_coverage_in_the_standard_path() {
    if !node_available() {
        skip("node and npm are not on PATH");
        return;
    }
    let fixture = Fixture::new("reuse");
    fixture.write(
        "package.json",
        "{\"name\":\"fixture\",\"private\":true,\"scripts\":{\"test\":\"node test.js\"}}\n",
    );
    fixture.write("src/book.ts", BOOK);
    fixture.write("test.js", TEST_JS);
    fixture.record_crapper_coverage();
    assert_eq!(
        fixture.run(&["--mutate-all", "src/book.ts"]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":uncovered 0"), "{snapshot}");
    assert!(snapshot.contains(":survived 0"), "{snapshot}");
}

/// crapper's report is still on disk, but the stamp no longer covers this
/// tree: the report is not this run's result. The project's own coverage script
/// writes nothing, so the site has no coverage at all — the old report must not
/// mark it covered and let a mutant be "killed" by data from an earlier run.
#[test]
fn a_stale_stamp_leaves_the_old_typescript_report_unread() {
    if !node_available() {
        skip("node and npm are not on PATH");
        return;
    }
    let fixture = Fixture::new("stale-report");
    fixture.write(
        "package.json",
        "{\"name\":\"fixture\",\"private\":true,\"scripts\":{\"coverage\":\"node no-report.js\",\"test\":\"node test.js\"}}\n",
    );
    fixture.write("src/book.ts", BOOK);
    fixture.write("test.js", TEST_JS);
    fixture.write(
        "no-report.js",
        "// This run's coverage produces no report.\n",
    );
    fixture.stale_crapper_coverage();
    assert_eq!(
        fixture.run(&["--mutate-all", "src/book.ts"]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":uncovered 1"), "{snapshot}");
    assert!(snapshot.contains(":killed 0"), "{snapshot}");
}

/// The same guard on the `--test-command`-only path, which reads the stamp
/// too: a stale language's report on disk is not this run's coverage.
#[test]
fn a_stale_stamp_leaves_the_old_typescript_report_unread_with_a_test_command() {
    if !node_available() {
        skip("node and npm are not on PATH");
        return;
    }
    let fixture = Fixture::new("stale-report-test-command");
    fixture.write(
        "package.json",
        "{\"name\":\"fixture\",\"private\":true,\"scripts\":{\"coverage\":\"node no-report.js\",\"test\":\"node test.js\"}}\n",
    );
    fixture.write("src/book.ts", BOOK);
    fixture.write("test.js", TEST_JS);
    fixture.write(
        "no-report.js",
        "// This run's coverage produces no report.\n",
    );
    fixture.stale_crapper_coverage();
    assert_eq!(
        fixture.run(&[
            "--mutate-all",
            "--test-command",
            "node test.js",
            "src/book.ts"
        ]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":uncovered 1"), "{snapshot}");
    assert!(snapshot.contains(":killed 0"), "{snapshot}");
}

/// Without a stamp every language is stale, so the same project collects its
/// own TypeScript coverage — through the generated script, in the copy on
/// Windows. The `coverage` script is the project's own and needs no network.
#[test]
fn typescript_coverage_is_collected_when_the_stamp_is_missing() {
    if !node_available() {
        skip("node and npm are not on PATH");
        return;
    }
    let fixture = Fixture::new("typescript");
    fixture.write(
        "package.json",
        "{\"name\":\"fixture\",\"private\":true,\"scripts\":{\"coverage\":\"node write-lcov.js\",\"test\":\"node test.js\"}}\n",
    );
    fixture.write("src/book.ts", BOOK);
    fixture.write("test.js", TEST_JS);
    fixture.write(
        "write-lcov.js",
        r#"
const fs = require('fs');
fs.mkdirSync('coverage', {recursive: true});
fs.writeFileSync('coverage/lcov.info', 'SF:src/book.ts\nDA:2,1\nDA:3,1\nend_of_record\n');
"#,
    );
    assert_eq!(
        fixture.run(&["--mutate-all", "src/book.ts"]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":uncovered 0"), "{snapshot}");
}

const MODEL_PY: &str = "def add(a, b):\n    return a + b\n";
const TEST_PY: &str = "import unittest\n\nfrom src import model\n\n\nclass TestAdd(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(model.add(1, 2), 3)\n";

/// The same for Python: no stamp, so the generated script runs coverage.py and
/// writes its report where the command runs. `coverage.py` is the tool anyway.
#[test]
fn python_coverage_is_collected_when_the_stamp_is_missing() {
    if !python_can("coverage") {
        skip("python with coverage.py is not on PATH");
        return;
    }
    let fixture = Fixture::new("python");
    fixture.write("pyproject.toml", "[project]\nname = \"fixture\"\n");
    fixture.write("src/model.py", MODEL_PY);
    fixture.write("test_model.py", TEST_PY);
    fixture.write("src/__init__.py", "");
    assert_eq!(
        fixture.run(&["--mutate-all", "src/model.py"]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":uncovered 0"), "{snapshot}");
}

/// A package directory whose name carries cmd metacharacters. An unquoted
/// `echo` ends at `&` and runs the tail as a command; `(`/`)` make cmd reject
/// the whole `/c` line before anything on it runs, so the coverage script never
/// reaches its own command and the report is silently missing.
#[cfg(windows)]
const META_PACKAGE: &str = "client & (old)";

/// The package's name and its report path reach the TypeScript script only in
/// its diagnostics, and cmd parses those before the coverage command runs: an
/// unquoted `(` rejects the whole `/c` line and an unquoted `&` ends the `echo`,
/// so LCOV is never written and every site is uncovered. The Vitest branch is
/// the one that emits both diagnostics; its stub also proves the reports
/// directory stayed package-relative inside a path full of metacharacters.
#[cfg(windows)]
#[test]
fn typescript_coverage_survives_metacharacters_in_the_package_path() {
    if !node_available() {
        skip("node and npm are not on PATH");
        return;
    }
    let fixture = Fixture::new("typescript-metacharacters");
    fixture.write(
        &format!("{META_PACKAGE}/package.json"),
        "{\"name\":\"fixture\",\"private\":true,\"scripts\":{\"test\":\"vitest run\"}}\n",
    );
    fixture.write(&format!("{META_PACKAGE}/src/book.ts"), BOOK);
    fixture.write(
        &format!("{META_PACKAGE}/node_modules/@vitest/coverage-v8/.keep"),
        "",
    );
    fixture.write(
        &format!("{META_PACKAGE}/node_modules/vitest-stub.js"),
        r#"
const fs = require('fs');
const path = require('path');
if (process.argv.includes('--coverage')) {
  const reports = process.argv
    .find((arg) => arg.startsWith('--coverage.reportsDirectory='))
    .split('=')[1];
  if (path.isAbsolute(reports)) {
    console.error('the reports directory must be relative to the package, got ' + reports);
    process.exit(1);
  }
  fs.mkdirSync(reports, {recursive: true});
  fs.writeFileSync(path.join(reports, 'lcov.info'), 'SF:src/book.ts\nDA:2,1\nDA:3,1\nend_of_record\n');
  process.exit(0);
}
const pristine = 'export function twice(x: number): number {\n  return x + x;\n}\n';
process.exit(fs.readFileSync('src/book.ts', 'utf8') === pristine ? 0 : 1);
"#,
    );
    fixture.write(
        &format!("{META_PACKAGE}/node_modules/.bin/vitest.cmd"),
        "@echo off\r\nnode \"%~dp0..\\vitest-stub.js\" %*\r\n",
    );
    let source = format!("{META_PACKAGE}/src/book.ts");
    assert_eq!(
        fixture.run(&["--mutate-all", source.as_str()]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":uncovered 0"), "{snapshot}");
}

/// The same for Python, whose pytest branch carries the package name in two
/// more `echo` texts. `pytest.ini` makes that the branch under test; a host
/// without pytest runs its unittest fallback instead, and the assertion is the
/// same either way: the script survived its own diagnostics and wrote LCOV.
#[cfg(windows)]
#[test]
fn python_coverage_survives_metacharacters_in_the_package_path() {
    if !python_can("coverage") {
        skip("python with coverage.py is not on PATH");
        return;
    }
    let fixture = Fixture::new("python-metacharacters");
    fixture.write(
        &format!("{META_PACKAGE}/pyproject.toml"),
        "[project]\nname = \"fixture\"\n",
    );
    fixture.write(&format!("{META_PACKAGE}/pytest.ini"), "[pytest]\n");
    fixture.write(&format!("{META_PACKAGE}/src/__init__.py"), "");
    fixture.write(&format!("{META_PACKAGE}/src/model.py"), MODEL_PY);
    fixture.write(&format!("{META_PACKAGE}/test_model.py"), TEST_PY);
    let source = format!("{META_PACKAGE}/src/model.py");
    assert_eq!(
        fixture.run(&["--mutate-all", source.as_str()]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":uncovered 0"), "{snapshot}");
}

/// The Vitest branch — no `coverage` script of the project's own, which is the
/// branch a package like kacha's client takes. A stub `vitest.cmd` stands in for
/// the tool: no install, no network, but the real command shape, an absolute
/// shim path plus a package-relative reports directory. The stub refuses an
/// absolute reports directory, so this fails if a Windows script ever spells
/// the real project's path again.
#[cfg(windows)]
#[test]
fn typescript_vitest_branch_runs_its_shim_in_the_copy() {
    if !node_available() {
        skip("node and npm are not on PATH");
        return;
    }
    let fixture = Fixture::new("vitest");
    fixture.write(
        "package.json",
        "{\"name\":\"fixture\",\"private\":true,\"scripts\":{\"test\":\"vitest run\"}}\n",
    );
    fixture.write("src/book.ts", BOOK);
    // An installed provider keeps the script away from `npm install`.
    fixture.write("node_modules/@vitest/coverage-v8/.keep", "");
    fixture.write(
        "node_modules/vitest-stub.js",
        r#"
const fs = require('fs');
const path = require('path');
if (process.argv.includes('--coverage')) {
  const reports = process.argv
    .find((arg) => arg.startsWith('--coverage.reportsDirectory='))
    .split('=')[1];
  const includes = process.argv.filter((arg) => arg.startsWith('--coverage.include='));
  if (path.isAbsolute(reports)) {
    console.error('the reports directory must be relative to the package, got ' + reports);
    process.exit(1);
  }
  if (includes.length !== 1 || includes[0] !== '--coverage.include=src/book.ts') {
    console.error('unexpected includes: ' + JSON.stringify(process.argv));
    process.exit(1);
  }
  fs.mkdirSync(reports, {recursive: true});
  fs.writeFileSync(path.join(reports, 'lcov.info'), 'SF:src/book.ts\nDA:2,1\nDA:3,1\nend_of_record\n');
  process.exit(0);
}
const pristine = 'export function twice(x: number): number {\n  return x + x;\n}\n';
process.exit(fs.readFileSync('src/book.ts', 'utf8') === pristine ? 0 : 1);
"#,
    );
    // cmd.exe cannot execute npm's POSIX shim, so the Windows branch has to pick
    // this one. The POSIX shim exists too, exactly as npm writes both.
    fixture.write("node_modules/.bin/vitest", "#!/bin/sh\nexit 0\n");
    fixture.write(
        "node_modules/.bin/vitest.cmd",
        "@echo off\r\nnode \"%~dp0..\\vitest-stub.js\" %*\r\n",
    );
    assert_eq!(
        fixture.run(&["--mutate-all", "src/book.ts"]),
        ExitCode::SUCCESS
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":uncovered 0"), "{snapshot}");
}
