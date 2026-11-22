#![cfg(any(target_os = "linux", windows))]
#![allow(clippy::disallowed_methods)]
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};
use support::child;

static NEXT: AtomicUsize = AtomicUsize::new(0);
const CLOCK: &str = "2026-11-07T12:00:00+00:00";

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

fn python_can(module: &str) -> bool {
    std::process::Command::new(python())
        .args(["-c", &format!("import {module}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

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

    fn record_crapper_coverage(&self) {
        let root = quality_core::paths::resolve(&self.directory);
        let source = root.join("src/book.ts");
        self.write(
            "target/coverage/typescript/lcov.info",
            &format!(
                "SF:{}\nDA:2,1\nDA:3,1\nend_of_record\n",
                quality_core::paths::posix(&source)
            ),
        );
        quality_core::stamp::record(
            &root,
            &[source],
            None,
            &[],
            &quality_core::stamp::snapshot(&root),
        );
        assert!(
            quality_core::stamp::stamp_path(&self.directory).is_file(),
            "the fixture must record a stamp"
        );
    }

    fn stale_crapper_coverage(&self) {
        self.record_crapper_coverage();
        self.write(
            "CHANGED.md",
            "the tree moved on after crapper recorded it\n",
        );
    }

    fn clear_snapshots(&self) {
        let base = self.directory.join(".metrics/mutate");
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
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

const TEST_JS: &str = r#"
const fs = require('fs');
const pristine = 'export function twice(x: number): number {\n  return x + x;\n}\n';
const source = fs.readFileSync('src/book.ts', 'utf8');
process.exit(source === pristine ? 0 : 1);
"#;

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
    let (code, stderr) = child::run(&fixture.root(), &["--mutate-all", "src/book.ts"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("Reusing typescript coverage from crapper's last run."),
        "{stderr}"
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":uncovered 0"), "{snapshot}");
    assert!(snapshot.contains(":survived 0"), "{snapshot}");
}

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

#[cfg(windows)]
const META_PACKAGE: &str = "client & (old)";

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

#[cfg(windows)]
const STUB: &str = r#"
use std::process::{Command, exit};
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["-c", "import pytest"] {
        exit(1);
    }
    let status = Command::new(r"@PYTHON@").args(&args).status().unwrap();
    exit(status.code().unwrap_or(1));
}
"#;
#[cfg(windows)]
fn stub_interpreter(fixture: &Fixture, package: &str) {
    let real = std::process::Command::new(python())
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    let real = String::from_utf8(real.stdout).unwrap().trim().to_string();
    fixture.write(
        &format!("{package}/.venv/stub.rs"),
        &STUB.replace("@PYTHON@", &real),
    );
    let source = fixture.directory.join(package).join(".venv/stub.rs");
    let stub = fixture
        .directory
        .join(package)
        .join(".venv/Scripts/python.exe");
    fs::create_dir_all(stub.parent().unwrap()).unwrap();
    let status = std::process::Command::new("rustc")
        .arg("--edition=2021")
        .arg(&source)
        .arg("-o")
        .arg(&stub)
        .status()
        .unwrap();
    assert!(status.success(), "compile the stub interpreter");
}

#[cfg(windows)]
#[test]
fn python_coverage_survives_metacharacters_in_the_package_path() {
    if !python_can("coverage") {
        skip("python with coverage.py is not on PATH");
        return;
    }
    if !python_can("pytest") {
        skip("python with pytest is not on PATH: the baseline runs -m pytest");
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
    stub_interpreter(&fixture, META_PACKAGE);
    let source = format!("{META_PACKAGE}/src/model.py");
    let (code, stderr) = child::run(&fixture.root(), &["--mutate-all", source.as_str()]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("pytest is missing in"),
        "the unittest fallback must be the branch under test: {stderr}"
    );
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":uncovered 0"), "{snapshot}");
}

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

#[test]
fn reuse_coverage_publishes_the_same_snapshot_as_the_stamped_path() {
    if !node_available() {
        skip("node and npm are not on PATH");
        return;
    }
    let fixture = Fixture::new("reuse-equivalence");
    fixture.write(
        "package.json",
        "{\"name\":\"fixture\",\"private\":true,\"scripts\":{\"test\":\"node test.js\"}}\n",
    );
    fixture.write("src/book.ts", BOOK);
    fixture.write("test.js", TEST_JS);
    fixture.record_crapper_coverage();

    fixture.clear_snapshots();
    let (stamped, stamped_stderr) = child::run(&fixture.root(), &["--mutate-all", "src/book.ts"]);
    let stamped_snapshot = fixture.snapshot();

    fixture.clear_snapshots();
    let (reused, reused_stderr) = child::run(
        &fixture.root(),
        &["--mutate-all", "--reuse-coverage", "src/book.ts"],
    );
    let reused_snapshot = fixture.snapshot();

    assert_eq!(stamped, reused, "A:\n{stamped_stderr}\nB:\n{reused_stderr}");
    assert_eq!(
        stamped_snapshot, reused_snapshot,
        "the reuse run published a different verdict"
    );
    assert!(stamped_snapshot.contains(":killed 1"), "{stamped_snapshot}");
    assert!(
        stamped_snapshot.contains(":uncovered 0"),
        "{stamped_snapshot}"
    );
    for (label, stderr) in [("stamped", &stamped_stderr), ("reused", &reused_stderr)] {
        assert!(
            !stderr.contains("No coverage data"),
            "{label} read no report:\n{stderr}"
        );
    }
}

#[test]
fn reuse_coverage_reads_a_report_the_stamp_rejects() {
    if !node_available() {
        skip("node and npm are not on PATH");
        return;
    }
    let fixture = Fixture::new("reuse-stale");
    fixture.write(
        "package.json",
        "{\"name\":\"fixture\",\"private\":true,\"scripts\":{\"test\":\"node test.js\"}}\n",
    );
    fixture.write("src/book.ts", BOOK);
    fixture.write("test.js", TEST_JS);
    fixture.stale_crapper_coverage();
    fixture.clear_snapshots();
    let (_, stderr) = child::run(
        &fixture.root(),
        &["--mutate-all", "--reuse-coverage", "src/book.ts"],
    );
    let snapshot = fixture.snapshot();
    assert!(
        snapshot.contains(":killed 1"),
        "reuse must read the report without checking the stamp:\n{snapshot}\n{stderr}"
    );
}

#[test]
fn survived_verdicts_are_reused_until_a_test_input_changes() {
    let fixture = Fixture::new("survived-reuse");
    fixture.write("src/calc.py", "def add(a, b):\n    return a + b\n");
    fixture.write("tests/test_calc.py", "def test_add():\n    assert True\n");
    const PASS: &str = if cfg!(windows) {
        "cmd /c exit 0"
    } else {
        "true"
    };

    let (code, stdout, stderr) = child::run_with_stdout(
        &fixture.root(),
        &[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            PASS,
            "src/calc.py",
        ],
    );
    assert_eq!(code, 3, "{stdout}{stderr}");
    assert!(stdout.contains("executed=1 reused=0"), "{stdout}{stderr}");
    let snapshot = fixture.snapshot();
    assert!(snapshot.contains(":survived 1"), "{snapshot}");

    let (code, stdout, stderr) = child::run_with_stdout(
        &fixture.root(),
        &["--no-coverage", "--test-command", PASS, "src/calc.py"],
    );
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("No mutations selected; keeping the previous snapshots.")
            && !stdout.contains("executed="),
        "{stdout}{stderr}"
    );
    assert_eq!(fixture.snapshot(), snapshot, "reuse changed the verdict");

    fixture.write(
        "tests/test_calc.py",
        "def test_add():\n    assert 1 + 1 == 2\n",
    );
    let (code, stdout, stderr) = child::run_with_stdout(
        &fixture.root(),
        &["--no-coverage", "--test-command", PASS, "src/calc.py"],
    );
    assert_eq!(code, 3, "{stdout}{stderr}");
    assert!(stdout.contains("executed=1 reused=0"), "{stdout}{stderr}");
}

#[test]
fn reuse_coverage_without_reports_is_an_error() {
    let fixture = Fixture::new("reuse-missing");
    fixture.write("src/calc.py", "def add(a, b):\n    return a + b\n");
    const PASS: &str = if cfg!(windows) {
        "cmd /c exit 0"
    } else {
        "true"
    };
    let (code, stderr) = child::run(
        &fixture.root(),
        &[
            "--mutate-all",
            "--reuse-coverage",
            "--test-command",
            PASS,
            "src/calc.py",
        ],
    );
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("--reuse-coverage found no coverage reports"),
        "{stderr}"
    );
    assert!(
        !fixture.root().join(".metrics/mutate").exists(),
        "a failed reuse run must not publish a snapshot"
    );
}
