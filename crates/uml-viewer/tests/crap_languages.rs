//! Real coverage toolchains, not fake report writers. The committed EDN was
//! produced by the pinned Python oracle, never transcribed by hand. Dependency
//! installation is local to the scratch copy; see fixtures/crap/live/README.md.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::crap::{Project, stderr};

fn fixture(language: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/crap/live")
        .join(language)
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

fn run(program: &str, args: &[&str], cwd: &Path, cache: &Path) -> std::io::Result<Output> {
    let program = if cfg!(windows) && program == "npm" {
        "npm.cmd"
    } else {
        program
    };
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("npm_config_cache", cache)
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .env("NO_PROXY", "localhost,127.0.0.1")
        .output()
}

fn available(program: &str, args: &[&str], project: &Project, language: &str) -> bool {
    let probe = match run(
        program,
        args,
        &project.root,
        &project.root.join("tool-cache"),
    ) {
        Ok(output) => output,
        Err(error) => {
            let message = format!("{language} interpreter {program} not found: {error}");
            assert!(
                !std::env::var_os("CRAPPER_REQUIRE_TOOLCHAINS").is_some_and(|v| v == "1"),
                "CRAPPER_REQUIRE_TOOLCHAINS=1; {message}"
            );
            eprintln!("SKIP: {message}");
            return false;
        }
    };
    if probe.status.success() {
        return true;
    }
    let message = format!(
        "{language} live coverage toolchain unavailable: {program} {args:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&probe.stdout),
        String::from_utf8_lossy(&probe.stderr),
    );
    assert!(
        !std::env::var_os("CRAPPER_REQUIRE_TOOLCHAINS").is_some_and(|v| v == "1"),
        "CRAPPER_REQUIRE_TOOLCHAINS=1; {message}"
    );
    eprintln!("SKIP: {message}");
    false
}

fn check_snapshot(project: &Project, language: &str, output: &Output) {
    assert_eq!(output.status.code(), Some(0), "stderr:\n{}", stderr(output));
    let actual = project.root.join(".metrics/crap.edn");
    let expected = fixture(language).join("expected.edn");
    // A missing oracle is a broken fixture, not a platform-dependent skip.
    let expected = fs::read(&expected).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; generate it from the pinned oracle",
            expected.display()
        )
    });
    assert_eq!(fs::read(actual).unwrap(), expected);
    let group = if language == "typescript-measured" {
        "typescript"
    } else {
        language
    };
    assert!(
        project
            .root
            .join("target/coverage")
            .join(group)
            .join("lcov.info")
            .is_file()
    );
}

fn python_available(project: &Project) -> bool {
    available(
        std::env::var("CRAPPER_TEST_PYTHON")
            .as_deref()
            .unwrap_or(if cfg!(windows) { "python" } else { "python3" }),
        &["-c", "import coverage; assert coverage.__version__ >= '7'"],
        project,
        "Python",
    )
}

#[test]
fn missing_lua_toolchain_keeps_the_previous_snapshot() {
    let project = Project::new();
    copy_tree(&fixture("lua").join("project"), &project.root);
    let bin = project.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    project.write("bin/not-lua.txt", "toolchain deliberately unavailable\n");
    let report = project.write("target/coverage/lua/lcov.info", "stale report\n");
    let snapshot = project.write(".metrics/crap.edn", "previous snapshot\n");
    let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .args(["crap", "--root"])
        .arg(&project.root)
        .current_dir(&project.root)
        .env("PATH", &bin)
        .env("COMSPEC", "C:/Windows/System32/cmd.exe")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("Coverage failed:"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read_to_string(snapshot).unwrap(), "previous snapshot\n");
    assert!(
        !report.exists(),
        "managed stale coverage was invalidated before probing"
    );
}

#[test]
fn lua_fixture_no_coverage_does_not_read_existing_reports() {
    let project = Project::new();
    copy_tree(&fixture("lua").join("project"), &project.root);
    let output = project.crap(&["--no-coverage"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        fs::read(project.root.join(".metrics/crap.edn")).unwrap(),
        fs::read(fixture("lua").join("no-coverage.edn")).unwrap()
    );
}

#[test]
fn lua_real_cluacov_and_sharded_busted_match_the_oracle() {
    let project = Project::new();
    copy_tree(&fixture("lua").join("project"), &project.root);
    let lua = "lua5.4";
    if !available(
        lua,
        &[
            "-e",
            "assert(_VERSION == 'Lua 5.4'); pcall(require, 'luarocks.loader'); assert(require('cluacov.version') == '1.0.0'); require('busted.runner'); require('luacov.reporter.lcov')",
        ],
        &project,
        "Lua",
    ) {
        return;
    }
    let output = project.crap(&["--lua-shards", "2"]);
    check_snapshot(&project, "lua", &output);
    assert!(
        !project
            .root
            .join("target/coverage/lua/luacov.shard-1.stats.out")
            .exists()
    );
}

#[test]
fn lua_multiple_modules_and_quoted_untested_sources_match_the_oracle() {
    let project = Project::new();
    copy_tree(&fixture("lua").join("modules/project"), &project.root);
    if !available(
        "lua5.4",
        &[
            "-e",
            "assert(_VERSION == 'Lua 5.4'); pcall(require, 'luarocks.loader'); assert(require('cluacov.version') == '1.0.0')",
        ],
        &project,
        "Lua",
    ) {
        return;
    }
    let output = project.crap(&["src", "nested"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        fs::read(project.root.join(".metrics/crap.edn")).unwrap(),
        fs::read(fixture("lua").join("modules/expected.edn")).unwrap()
    );
    assert!(
        project
            .root
            .join("target/coverage/lua/nested/lcov.info")
            .is_file()
    );
}

#[test]
fn failing_lua_shard_keeps_snapshot_and_removes_shard_artifacts() {
    let project = Project::new();
    copy_tree(&fixture("lua").join("project"), &project.root);
    if !available(
        "lua5.4",
        &[
            "-e",
            "assert(_VERSION == 'Lua 5.4'); pcall(require, 'luarocks.loader'); assert(require('cluacov.version') == '1.0.0')",
        ],
        &project,
        "Lua",
    ) {
        return;
    }
    project.write(
        "spec/fail_spec.lua",
        "it('fails', function() assert.is_true(false) end)\n",
    );
    let snapshot = project.write(".metrics/crap.edn", "previous snapshot\n");
    let output = project.crap(&["--lua-shards", "3"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("in coverage shard 3 of 3"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read_to_string(snapshot).unwrap(), "previous snapshot\n");
    let managed = project.root.join("target/coverage/lua");
    assert!(!fs::read_dir(managed).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("luacov.shard-")
    }));
}

#[test]
fn python_real_unittest_and_coverage_match_the_oracle() {
    let project = Project::new();
    copy_tree(&fixture("python").join("project"), &project.root);
    if !python_available(&project) {
        return;
    }
    let output = project.crap(&[]);
    check_snapshot(&project, "python", &output);
    assert!(
        !project.root.join(".venv").exists(),
        "no environment installation is needed"
    );
}

#[test]
fn python_failing_tests_still_publish_the_measured_report() {
    let project = Project::new();
    copy_tree(&fixture("python").join("project"), &project.root);
    if !python_available(&project) {
        return;
    }
    fs::write(
        project.root.join("tests/test_calc.py"),
        fs::read_to_string(project.root.join("tests/test_calc.py"))
            .unwrap()
            .replace(
                "self.assertEqual(6, choose(3))",
                "self.assertEqual(0, choose(3))",
            ),
    )
    .unwrap();
    let output = project.crap(&[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stderr(&output).contains("Coverage was still recorded."));
    assert!(
        project
            .root
            .join("target/coverage/python/lcov.info")
            .is_file()
    );
    let expected = fs::read(fixture("python").join("failing.edn")).unwrap();
    assert_eq!(
        fs::read(project.root.join(".metrics/crap.edn")).unwrap(),
        expected
    );
}

#[test]
fn typescript_node_type_stripping_reports_real_coverage_bytes() {
    let project = Project::new();
    copy_tree(
        &fixture("typescript-measured").join("project"),
        &project.root,
    );
    if !node_tools_available(&project) || !install_c8(&project) {
        return;
    }
    let output = project.crap(&[]);
    check_snapshot(&project, "typescript-measured", &output);
}

fn node_tools_available(project: &Project) -> bool {
    available("node", &["--version"], project, "TypeScript")
        && available("npm", &["--version"], project, "TypeScript")
}

fn install_c8(project: &Project) -> bool {
    let output = run(
        "npm",
        &[
            "install",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--no-global",
        ],
        &project.root,
        &project.root.join("npm-cache"),
    )
    .expect("npm installed after availability probe");
    if output.status.success() {
        return true;
    }
    let message = format!(
        "local c8 installation failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !std::env::var_os("CRAPPER_REQUIRE_TOOLCHAINS").is_some_and(|v| v == "1"),
        "{message}"
    );
    eprintln!("SKIP: {message}");
    false
}

#[test]
fn typescript_real_node_and_c8_match_the_oracle() {
    let project = Project::new();
    copy_tree(&fixture("typescript").join("project"), &project.root);
    if !node_tools_available(&project) || !install_c8(&project) {
        return;
    }
    let output = project.crap(&[]);
    check_snapshot(&project, "typescript", &output);
    assert_eq!(
        fs::read_to_string(project.root.join("package.json")).unwrap(),
        fs::read_to_string(fixture("typescript").join("project/package.json")).unwrap()
    );
}

#[test]
fn typescript_failing_tests_still_publish_the_measured_report() {
    let project = Project::new();
    copy_tree(&fixture("typescript").join("project"), &project.root);
    if !node_tools_available(&project) || !install_c8(&project) {
        return;
    }
    project.write("src/calc.test.js",
        "import assert from 'node:assert/strict';\nimport test from 'node:test';\nimport { choose } from './calc.js';\ntest('fails', () => assert.equal(choose(3), 0));\n");
    let output = project.crap(&[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stderr(&output).contains("TypeScript coverage exited 1"));
    assert!(
        project
            .root
            .join("target/coverage/typescript/lcov.info")
            .is_file()
    );
    let expected = fs::read(fixture("typescript").join("failing.edn")).unwrap();
    assert_eq!(
        fs::read(project.root.join(".metrics/crap.edn")).unwrap(),
        expected
    );
}
