//! Seam: the `uml-viewer quality <crap|mutate|dry>` process boundary (#15,
//! ADR 0001 2026-11-08 appendix). One JSON version-1 result record on stdout;
//! the tool's own report goes to stderr. No `quality-result.json` is written.
//!
//! The acceptance cases are the public-entry cases of ADR 0001: argv fidelity,
//! the per-tool valid exit sets, missing declarations (exit 3), launch/config
//! errors (exit 1), differential mutation, `--mutate-all` scoped to the named
//! targets, all targets in one call, and an explicit error for a language or
//! platform whose tool is not ported.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A missing toolchain must never look like a pass (fixtures/mutate/README.md):
/// say so on the process's real stderr, which libtest does not capture, and fail
/// when `UML_VIEWER_REQUIRE_FAKE_TOOL=1` asks for the tool. The Windows fake
/// tool runs on the host Python, so these tests back real fixtures only where
/// that interpreter exists.
fn skip_fake_tool(reason: &str) {
    assert!(
        !std::env::var_os("UML_VIEWER_REQUIRE_FAKE_TOOL").is_some_and(|value| value == "1"),
        "UML_VIEWER_REQUIRE_FAKE_TOOL=1; {reason}"
    );
    use std::io::Write;
    let _ = std::io::stderr().write_all(format!("SKIP: {reason}\n").as_bytes());
}

/// A scratch project whose name contains a space and `&`, as the public-entry
/// acceptance requires; removed on drop.
struct Project {
    base: PathBuf,
    root: PathBuf,
}

impl Project {
    fn new() -> Project {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "uml-quality-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("my proj & one");
        fs::create_dir_all(&root).unwrap();
        let root = crate::crap::canonical(&root);
        Project { base, root }
    }

    fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn config(&self, text: &str) {
        self.write("uml-quality.json", text);
    }

    /// Run from a *different* directory: the project root is explicit.
    fn quality(&self, args: &[&str]) -> Output {
        self.quality_in(&self.base, args)
    }

    fn quality_in(&self, cwd: &Path, args: &[&str]) -> Output {
        let mut argv = args.to_vec();
        if !argv.is_empty() {
            argv.splice(1..1, ["--root", self.root.to_str().unwrap()]);
        }
        Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
            .arg("quality")
            .args(argv)
            .current_dir(cwd)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap()
    }

    fn record(&self, output: &Output) -> serde_json::Value {
        let text = String::from_utf8(output.stdout.clone()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1, "one record line on stdout: {text:?}");
        serde_json::from_str(lines[0]).unwrap_or_else(|error| panic!("{error}: {text:?}"))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn no_result_file(project: &Project) {
    assert!(
        !project
            .root
            .join(".uml-viewer/quality-result.json")
            .exists(),
        "the record is stdout, never a file"
    );
}

#[test]
fn quality_requires_an_operation_and_rejects_an_unknown_one() {
    let project = Project::new();
    for args in [vec![], vec!["bogus"]] {
        let output = project.quality(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "usage errors print no record");
        assert!(stderr(&output).contains("usage"), "{}", stderr(&output));
    }
}

#[test]
fn a_whole_tree_crap_run_publishes_its_snapshot_and_a_valid_record() {
    let project = Project::new();
    project.config(
        r#"{"version":1,"standardTools":{"sourceRoots":["crates"],"crapperOptions":["--no-coverage"]}}"#,
    );
    project.write(
        "crates/edn/src/value.rs",
        "pub fn add(a: i64, b: i64) -> i64 { a + b }\n",
    );
    let output = project.quality(&["crap"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let record = project.record(&output);
    assert_eq!(record["version"], 1);
    assert_eq!(record["operation"], "crap");
    assert_eq!(
        record["projectRoot"],
        serde_json::Value::String(project.root.to_string_lossy().into_owned())
    );
    assert_eq!(record["exitCode"], 0);
    assert_eq!(record["validResult"], true);
    assert_eq!(record["crapScope"], "whole-tree");
    assert_eq!(record["configured"], false);
    assert_eq!(record["tool"], "crapper");
    // crapper's own report is not mixed into the record stream.
    assert!(stderr(&output).contains("Wrote"), "{}", stderr(&output));
    let snapshot = fs::read_to_string(project.root.join(".metrics/crap.edn")).unwrap();
    assert!(
        snapshot.contains(r#"{:name "add", :namespace "crate::value", :complexity 1"#),
        "{snapshot}"
    );
    no_result_file(&project);
}

/// `uml-viewer quality <args> --root <root>` from a different directory.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn quality_at(cwd: &Path, root: &Path, args: &[&str]) -> Output {
    let mut argv = args.to_vec();
    argv.splice(1..1, ["--root", root.to_str().unwrap()]);
    Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("quality")
        .args(argv)
        .current_dir(cwd)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}

/// The record, checked against the real process exit: version 1, the
/// requested operation, this canonical root and the same exit code.
fn checked(output: &Output, operation: &str, root: &Path) -> serde_json::Value {
    let text = String::from_utf8(output.stdout.clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "one record line on stdout: {text:?}");
    let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(record["version"], 1, "{record}");
    assert_eq!(record["operation"], operation, "{record}");
    assert_eq!(
        record["projectRoot"].as_str(),
        Some(root.to_string_lossy().as_ref()),
        "{record}"
    );
    assert_eq!(
        record["exitCode"].as_i64(),
        output.status.code().map(i64::from),
        "the record carries the real exit: {record}"
    );
    let mut keys: Vec<&str> = record
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "configured",
            "crapScope",
            "exitCode",
            "operation",
            "projectRoot",
            "reason",
            "tool",
            "validResult",
            "version"
        ],
        "{record}"
    );
    record
}

/// A native fake tool that records its argv (NUL-terminated, so empty
/// strings and newlines survive) and exits with `code`. POSIX runs it with
/// `/bin/sh`; Windows with the host Python, skipped loudly when absent.
struct Fake {
    argv: Vec<String>,
    log: PathBuf,
}

impl Fake {
    fn new(project: &Project, code: i32) -> Option<Fake> {
        let log = project.base.join("argv.log");
        if cfg!(windows) {
            let probe = Command::new("python")
                .args(["-c", "import sys; print(sys.executable)"])
                .output();
            let Some(python) = probe
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            else {
                skip_fake_tool("no python on PATH for the Windows fake tool");
                return None;
            };
            let script = project.base.join("fake.py");
            fs::write(
                &script,
                format!(
                    "import sys\nopen(r'{}', 'wb').write(b''.join(a.encode('utf-8') + b'\\0' for a in sys.argv[1:]))\nprint('fake-stdout')\nsys.exit({code})\n",
                    log.display()
                ),
            )
            .unwrap();
            Some(Fake {
                argv: vec![python, script.to_string_lossy().into_owned()],
                log,
            })
        } else {
            let script = project.base.join("fake.sh");
            let body = if code < 0 {
                format!("kill -{} $$\n", -code)
            } else {
                format!("exit {code}\n")
            };
            fs::write(
                &script,
                format!(
                    "out='{}'\n: > \"$out\"\nfor a in \"$@\"; do printf '%s\\0' \"$a\" >> \"$out\"; done\necho fake-stdout\n{body}",
                    log.display()
                ),
            )
            .unwrap();
            Some(Fake {
                argv: vec!["/bin/sh".into(), script.to_string_lossy().into_owned()],
                log,
            })
        }
    }

    fn received(&self) -> Option<Vec<String>> {
        let bytes = fs::read(&self.log).ok()?;
        let text = String::from_utf8(bytes).unwrap();
        Some(text.split_terminator('\0').map(str::to_string).collect())
    }

    fn json_argv(&self, extra: &[&str]) -> String {
        let mut argv = self.argv.clone();
        argv.extend(extra.iter().map(|s| s.to_string()));
        serde_json::to_string(&argv).unwrap()
    }
}

fn operations(crap: &str, mutate: &str, dry: Option<&str>) -> String {
    let dry = dry.map(|d| format!(",\"dry\":{d}")).unwrap_or_default();
    format!(r#"{{"version":1,"operations":{{"crap":{crap},"mutate":{mutate}{dry}}}}}"#)
}

const HOSTILE: &[&str] = &["", "two words", "q\"uote's", "line\nbreak", "$HOME;&|*<>"];

#[test]
fn an_operation_override_appends_caller_args_verbatim_and_uses_its_declared_codes() {
    let project = Project::new();
    let Some(fake) = Fake::new(&project, 3) else {
        return;
    };
    let mutate = format!(
        r#"{{"argv":{},"forwardArgs":"append","validResultExitCodes":[0,3]}}"#,
        fake.json_argv(&["--fixed", "x y"])
    );
    let crap = r#"{"argv":["unused"],"forwardArgs":"none","scope":"whole-tree","validResultExitCodes":[0]}"#;
    project.config(&operations(crap, &mutate, None));
    let mut args = vec!["mutate"];
    args.extend_from_slice(HOSTILE);
    let output = project.quality(&args);
    let record = checked(&output, "mutate", &project.root);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert_eq!(
        record["validResult"], true,
        "3 is declared for this operation"
    );
    assert_eq!(record["configured"], true);
    assert_eq!(record["tool"], "configured");
    assert_eq!(record["crapScope"], serde_json::Value::Null);
    assert_eq!(record["reason"], serde_json::Value::Null);
    let mut expected = vec!["--fixed".to_string(), "x y".to_string()];
    expected.extend(HOSTILE.iter().map(|s| s.to_string()));
    assert_eq!(
        fake.received().unwrap(),
        expected,
        "fixed argv, then caller args verbatim"
    );
    // The command's stdout is folded into stderr; stdout is the record alone.
    assert!(
        stderr(&output).contains("fake-stdout"),
        "{}",
        stderr(&output)
    );
    no_result_file(&project);
}

#[test]
fn an_exit_3_without_a_declaration_is_not_a_valid_result() {
    let project = Project::new();
    let Some(fake) = Fake::new(&project, 3) else {
        return;
    };
    let crap = format!(
        r#"{{"argv":{},"forwardArgs":"append","scope":"targets","validResultExitCodes":[0]}}"#,
        fake.json_argv(&[])
    );
    let mutate = r#"{"argv":["unused"],"forwardArgs":"append","validResultExitCodes":[0]}"#;
    project.config(&operations(&crap, mutate, None));
    let output = project.quality(&["crap", "src/a.rs"]);
    let record = checked(&output, "crap", &project.root);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(record["validResult"], false, "exit 3 needs a declaration");
    assert_eq!(record["crapScope"], "targets");
    assert_eq!(fake.received().unwrap(), ["src/a.rs"]);
}

#[test]
fn a_whole_tree_override_ignores_caller_targets_and_a_failure_is_invalid() {
    let project = Project::new();
    let Some(fake) = Fake::new(&project, 1) else {
        return;
    };
    let crap = format!(
        r#"{{"argv":{},"forwardArgs":"none","scope":"whole-tree","validResultExitCodes":[0,1]}}"#,
        fake.json_argv(&["--full-tree"])
    );
    let mutate = r#"{"argv":["unused"],"forwardArgs":"append","validResultExitCodes":[0]}"#;
    let dry = format!(
        r#"{{"argv":{},"forwardArgs":"append","validResultExitCodes":[0]}}"#,
        fake.json_argv(&[])
    );
    project.config(&operations(&crap, mutate, Some(&dry)));
    let output = project.quality(&["crap", "src/narrow.rs"]);
    let record = checked(&output, "crap", &project.root);
    assert_eq!(record["crapScope"], "whole-tree");
    assert_eq!(record["validResult"], true, "1 is declared here");
    assert_eq!(
        fake.received().unwrap(),
        ["--full-tree"],
        "targets cannot narrow"
    );
    // The same failing command under dry, which declares only 0.
    let output = project.quality(&["dry"]);
    let record = checked(&output, "dry", &project.root);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        record["validResult"], false,
        "a tool failure is never valid"
    );
    assert_eq!(
        record["reason"],
        serde_json::Value::Null,
        "the tool did run"
    );
}

#[cfg(unix)]
#[test]
fn a_signal_terminated_operation_reports_128_plus_n_and_is_invalid() {
    let project = Project::new();
    let fake = Fake::new(&project, -15).unwrap();
    let crap = r#"{"argv":["unused"],"forwardArgs":"none","scope":"whole-tree","validResultExitCodes":[0]}"#;
    let mutate = format!(
        r#"{{"argv":{},"forwardArgs":"append","validResultExitCodes":[0,3]}}"#,
        fake.json_argv(&[])
    );
    project.config(&operations(crap, &mutate, None));
    let output = project.quality(&["mutate", "a.rs"]);
    let record = checked(&output, "mutate", &project.root);
    assert_eq!(output.status.code(), Some(143));
    assert_eq!(record["validResult"], false);
}

/// Configuration and launch errors exit 1 with an error record and launch
/// nothing; every configured operation is validated before any runs.
#[test]
fn configuration_and_launch_errors_exit_1_with_an_error_record() {
    let project = Project::new();
    let Some(fake) = Fake::new(&project, 0) else {
        return;
    };
    let good = format!(
        r#"{{"argv":{},"forwardArgs":"append","validResultExitCodes":[0]}}"#,
        fake.json_argv(&[])
    );
    let crap_good = format!(
        r#"{{"argv":{},"forwardArgs":"append","scope":"targets","validResultExitCodes":[0]}}"#,
        fake.json_argv(&[])
    );
    let cases: Vec<(String, &str, &str)> = vec![
        ("{not json".into(), "crap", "not valid JSON"),
        (
            r#"{"version":2,"standardTools":{}}"#.into(),
            "crap",
            "version 1",
        ),
        (r#"{"version":1}"#.into(), "crap", "version 1"),
        (
            r#"{"version":1,"standardTools":{},"aliasResultExitCodes":{"crap":[0]}}"#.into(),
            "crap",
            "alias",
        ),
        (
            format!(r#"{{"version":1,"operations":{{"crap":{crap_good}}}}}"#),
            "crap",
            "crap and mutate",
        ),
        (
            operations(
                &format!(
                    r#"{{"argv":{},"forwardArgs":"append","scope":"whole-tree","validResultExitCodes":[0]}}"#,
                    fake.json_argv(&[])
                ),
                &good,
                None,
            ),
            "mutate",
            "whole-tree CRAP requires forwardArgs none",
        ),
        (
            operations(
                &crap_good,
                r#"{"argv":["x"],"forwardArgs":"append","validResultExitCodes":[256]}"#,
                None,
            ),
            "crap",
            "0-255",
        ),
        (
            operations(
                &crap_good,
                &good,
                Some(r#"{"argv":[],"forwardArgs":"none","validResultExitCodes":[0]}"#),
            ),
            "crap",
            "invalid operations.dry",
        ),
        (
            r#"{"version":1,"standardTools":{"crapperOptions":["--exclude","x"]}}"#.into(),
            "crap",
            "may not hold scope",
        ),
        (
            r#"{"version":1,"standardTools":{"mutatorOptions":["--max-workers"]}}"#.into(),
            "mutate",
            "missing an option value",
        ),
        (
            r#"{"version":1,"standardTools":{"sourceRoots":["../outside"]}}"#.into(),
            "dry",
            "project-relative",
        ),
        (
            r#"{"version":1,"standardTools":{"sourceRoots":["missing"]}}"#.into(),
            "dry",
            "invalid business source root",
        ),
        (
            operations(
                &crap_good,
                r#"{"argv":["tool.cmd"],"forwardArgs":"append","validResultExitCodes":[0]}"#,
                None,
            ),
            "mutate",
            "native",
        ),
        (
            operations(
                &crap_good,
                r#"{"argv":["uml-no-such-tool-15"],"forwardArgs":"append","validResultExitCodes":[0]}"#,
                None,
            ),
            "mutate",
            "cannot launch uml-no-such-tool-15",
        ),
    ];
    for (config, operation, message) in cases {
        project.config(&config);
        let _ = fs::remove_file(&fake.log);
        let output = project.quality(&[operation]);
        let record = checked(&output, operation, &project.root);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{config}: {}",
            stderr(&output)
        );
        assert_eq!(record["validResult"], false, "{config}");
        let reason = record["reason"].as_str().unwrap_or_default();
        assert!(reason.contains(message), "{config}: {reason}");
        assert!(stderr(&output).contains(message), "{config}");
        assert!(fake.received().is_none(), "{config}: nothing may launch");
    }
}

#[test]
fn declared_standard_scope_cannot_be_overridden_by_refresh_arguments() {
    let project = Project::new();
    project.config(r#"{"version":1,"standardTools":{"sourceRoots":["crates"]}}"#);
    project.write("crates/a/src/lib.rs", "pub fn a() {}\n");
    for (operation, flag) in [
        ("crap", "-s"),
        ("dry", "--exclude"),
        ("mutate", "--source-root"),
    ] {
        let output = project.quality(&[operation, flag, "elsewhere"]);
        let record = checked(&output, operation, &project.root);
        assert_eq!(output.status.code(), Some(1), "{operation}");
        assert!(
            record["reason"]
                .as_str()
                .unwrap()
                .contains("declared by standardTools"),
            "{record}"
        );
        assert!(!project.root.join(".metrics").exists(), "{operation} ran");
    }
}

#[test]
fn mutation_targets_are_checked_before_launch() {
    let project = Project::new();
    project.config(
        r#"{"version":1,"standardTools":{"sourceRoots":["crates"],"exclude":["crates/**/tests.rs"]}}"#,
    );
    project.write("crates/a/src/lib.rs", "pub fn a() {}\n");
    project.write("crates/a/src/tests.rs", "pub fn t() {}\n");
    project.write("scripts/tool.rs", "pub fn s() {}\n");
    for (args, message) in [
        (
            vec!["--changed", "crates/a/src/lib.rs"],
            "--changed cannot be combined",
        ),
        (vec!["crates/a/src/gone.rs"], "does not exist"),
        (vec!["scripts/tool.rs"], "no business mutation targets"),
        (
            vec!["crates/a/src/tests.rs"],
            "no business mutation targets",
        ),
        (vec![], "no business mutation targets"),
    ] {
        let mut all = vec!["mutate"];
        all.extend(args.iter().copied());
        let output = project.quality(&all);
        let record = checked(&output, "mutate", &project.root);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert_eq!(record["tool"], "mutator");
        assert!(
            record["reason"].as_str().unwrap().contains(message),
            "{args:?}: {record}"
        );
    }
    let output = project.quality(&["mutate", "scripts/tool.rs"]);
    assert!(
        stderr(&output)
            .contains("uml: omitted mutation target outside business scope: scripts/tool.rs"),
        "{}",
        stderr(&output)
    );
    assert!(!project.root.join(".metrics").exists());
}

/// All seven languages reach their built-in tools. The Go scan has a real
/// site, so dispatch cannot silently turn it into an empty selection.
#[test]
fn all_quality_languages_reach_the_built_in_tools() {
    let project = Project::new();
    project.config(r#"{"version":1,"standardTools":{"sourceRoots":["src"]}}"#);
    project.write(
        "src/main.go",
        "package main\n\nfunc value() bool { return true }\n",
    );
    project.write("src/main.ts", "export function f() { return 1; }\n");
    #[cfg(any(target_os = "linux", windows))]
    {
        let output = project.quality(&["mutate", "--scan", "src/main.go"]);
        let record = checked(&output, "mutate", &project.root);
        let report = stderr(&output);
        let diagnostics = format!(
            "status: {}\nstdout: {}\nstderr: {report}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
        );
        assert_eq!(output.status.code(), Some(0), "{diagnostics}");
        assert_eq!(record["validResult"], true, "{diagnostics}");
        // quality reserves stdout for the checked result record; the built-in
        // mutator's report is redirected to stderr, including this boolean site.
        assert!(
            report.contains("Scan: 1 mutation sites in src/main.go"),
            "{diagnostics}"
        );
        assert!(report.contains("src/main.go:3 true -> false"), "{diagnostics}");
        assert!(report.contains("[defn-/value]"), "{diagnostics}");
        assert!(!report.contains("No source files"), "{diagnostics}");
        assert!(!project.root.join(".metrics").exists());
    }
    // After #7, dryer reads every ported language: the same mixed project is
    // a real run, not an empty success and not a "not ported" refusal.
    let output = project.quality(&["dry"]);
    let record = checked(&output, "dry", &project.root);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(record["validResult"], true);
    assert!(project.root.join(".metrics").join("dry.edn").exists());
}

#[cfg(not(target_os = "linux"))]
/// Windows runs mutation in project copies and macOS refuses it with a reason;
/// on Windows this is therefore not the platform refusal record.
#[cfg(not(windows))]
#[test]
fn mutation_off_linux_is_an_explicit_error_record() {
    let project = Project::new();
    project.config(r#"{"version":1,"standardTools":{"sourceRoots":["src"]}}"#);
    project.write("src/lib.rs", "pub fn a(x: i64) -> i64 { x + 1 }\n");
    let output = project.quality(&["mutate", "src/lib.rs"]);
    let record = checked(&output, "mutate", &project.root);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(record["validResult"], false);
    assert_eq!(record["tool"], "mutator");
    let stderr = stderr(&output);
    assert!(
        stderr.contains("not supported on macOS") || stderr.contains("not supported on"),
        "{stderr}"
    );
    assert!(!project.root.join(".metrics").exists());
}

#[test]
fn dry_runs_the_built_in_dryer_on_the_declared_scope() {
    let project = Project::new();
    project.config(
        r#"{"version":1,"standardTools":{"sourceRoots":["crates"],"exclude":["crates/**/tests.rs"]}}"#,
    );
    let body = "    let mut total = 0;\n    for item in items {\n        if *item > 3 {\n            total += item * 2;\n        } else {\n            total -= item;\n        }\n    }\n    total\n";
    project.write(
        "crates/a/src/lib.rs",
        &format!("pub fn left(items: &[i64]) -> i64 {{\n{body}}}\n\npub fn right(items: &[i64]) -> i64 {{\n{body}}}\n"),
    );
    project.write(
        "crates/a/src/tests.rs",
        &format!("pub fn excluded(items: &[i64]) -> i64 {{\n{body}}}\n"),
    );
    let output = project.quality(&["dry"]);
    let record = checked(&output, "dry", &project.root);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(record["validResult"], true);
    assert_eq!(record["tool"], "dryer");
    assert_eq!(record["crapScope"], serde_json::Value::Null);
    assert_eq!(record["configured"], false);
    let snapshot = fs::read_to_string(project.root.join(".metrics").join("dry.edn")).unwrap();
    assert!(snapshot.contains("left"), "{snapshot}");
    assert!(snapshot.contains("right"), "{snapshot}");
    assert!(
        !snapshot.contains("excluded"),
        "the declared exclusion applies: {snapshot}"
    );
    no_result_file(&project);
}

/// Without a manifest the default standard dispatch runs on the project.
#[test]
fn without_a_manifest_the_standard_tool_runs_on_the_project_root() {
    let project = Project::new();
    project.write("src/lib.rs", "pub fn one() -> i64 { 1 }\n");
    let output = project.quality(&["crap", "--no-coverage"]);
    let record = checked(&output, "crap", &project.root);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(record["tool"], "crapper");
    assert_eq!(record["crapScope"], "whole-tree");
    assert!(project.root.join(".metrics").join("crap.edn").exists());
}

#[cfg(target_os = "linux")]
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// The mutation fixture workspace (fixtures/mutate/rust) copied below the
/// spaced `&` path; its `project/` is the quality root.
#[cfg(target_os = "linux")]
fn mutation_project(project: &Project, options: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/rust");
    copy_tree(&source, &project.root);
    let root = project.root.join("project");
    fs::write(
        root.join("uml-quality.json"),
        format!(
            r#"{{"version":1,"standardTools":{{"sourceRoots":["app/src","util/src"],"mutatorOptions":{options}}}}}"#
        ),
    )
    .unwrap();
    root
}

#[cfg(target_os = "linux")]
fn snapshots(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    fn walk(dir: &Path, base: &Path, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, found);
            } else {
                found.push(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let base = root.join(".metrics").join("mutate");
    walk(&base, &base, &mut found);
    found.sort();
    found
}

/// `:refresh-mutate` and `:refresh-mutate-all` through the dispatcher: every
/// target in one call, `--mutate-all` only on the named targets, and an
/// unchanged rerun is differential. The test command fails only inside a
/// mutant's view, so every mutant is killed (exit 0) and a killed outcome is
/// reused by the next differential run (a survivor would be re-executed).
#[cfg(target_os = "linux")]
#[test]
fn mutation_is_differential_on_all_targets_in_one_call() {
    let project = Project::new();
    let root = mutation_project(
        &project,
        r#"["--max-workers","1","--no-coverage","--test-command","test ! -f \"$MUTATOR_VIEW/mutation\""]"#,
    );
    let output = quality_at(
        &project.base,
        &root,
        &[
            "mutate",
            "app/src/lib.rs",
            "app/src/more.rs",
            "--mutate-all",
        ],
    );
    let record = checked(&output, "mutate", &root);
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{err}");
    assert_eq!(record["validResult"], true, "{err}");
    assert_eq!(record["tool"], "mutator");
    assert!(err.contains("app/src/lib.rs: executed=1 reused=0"), "{err}");
    assert!(
        err.contains("app/src/more.rs: executed=1 reused=0"),
        "{err}"
    );
    assert_eq!(
        snapshots(&root),
        ["app.edn", "app/more.edn"],
        "--mutate-all touched only the named targets"
    );
    let before = fs::read(root.join(".metrics/mutate/app.edn")).unwrap();
    // :refresh-mutate: the same targets again, unchanged, reuse every outcome.
    // mutator reports the fully reusable run as "No mutations selected; keeping
    // the previous snapshots."; a rerun after one target changed shows the
    // other target's reused outcome below.
    let output = quality_at(
        &project.base,
        &root,
        &["mutate", "app/src/lib.rs", "app/src/more.rs"],
    );
    let record = checked(&output, "mutate", &root);
    let err = stderr(&output);
    assert_eq!(record["validResult"], true, "{err}");
    assert!(
        err.contains("No mutations selected; keeping the previous snapshots."),
        "{err}"
    );
    // Changing one of two targets in one call reuses the other target's
    // outcome and executes only the changed one.
    let changed = root.join("app/src/more.rs");
    fs::write(
        &changed,
        fs::read_to_string(&changed)
            .unwrap()
            .replace("x / 2", "x / 3"),
    )
    .unwrap();
    let output = quality_at(
        &project.base,
        &root,
        &["mutate", "app/src/lib.rs", "app/src/more.rs"],
    );
    let err = stderr(&output);
    assert!(err.contains("app/src/lib.rs: executed=0 reused=1"), "{err}");
    assert!(
        err.contains("app/src/more.rs: executed=1 reused=0"),
        "{err}"
    );
    // A new target joins the same call; the unchanged one is still reused.
    let output = quality_at(
        &project.base,
        &root,
        &["mutate", "util/src/lib.rs", "app/src/lib.rs"],
    );
    let record = checked(&output, "mutate", &root);
    let err = stderr(&output);
    assert_eq!(record["validResult"], true, "{err}");
    assert!(
        err.contains("util/src/lib.rs: executed=1 reused=0"),
        "{err}"
    );
    assert!(err.contains("app/src/lib.rs: executed=0 reused=1"), "{err}");
    assert_eq!(snapshots(&root), ["app.edn", "app/more.edn", "util.edn"]);
    // The differential rerun keeps the snapshot's outcomes.
    let after = fs::read_to_string(root.join(".metrics/mutate/app.edn")).unwrap();
    let before = String::from_utf8(before).unwrap();
    let outcomes = |text: &str| text.split(":outcomes").nth(1).unwrap().to_string();
    assert_eq!(outcomes(&after), outcomes(&before));
}

/// The contracts fixture (fixtures/quality/records.jsonl, consumed by
/// packages/contracts/tests/quality.test.ts) is real binary output. Run the
/// same scenarios and compare the lines byte for byte apart from the root.
#[test]
fn the_committed_contract_samples_are_the_binary_output() {
    let project = Project::new();
    project.config(
        r#"{"version":1,"standardTools":{"sourceRoots":["crates"],"crapperOptions":["--no-coverage"]}}"#,
    );
    project.write("crates/a/src/lib.rs", "pub fn a(x: i64) -> i64 { x + 1 }\n");
    let Some(python) = python_executable() else {
        skip_fake_tool("no python on PATH; the samples need a native fake tool");
        return;
    };
    // The file may be checked out with CRLF; the record line itself has no CR.
    let samples = include_str!("../../../fixtures/quality/records.jsonl").replace('\r', "");
    let samples = samples.as_str();
    let expected: Vec<serde_json::Value> = samples
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut actual = Vec::new();
    actual.push(project.quality(&["crap"]));
    actual.push(project.quality(&["dry"]));
    // With no target and no --changed, mutator is refused on every platform.
    actual.push(project.quality(&["mutate"]));
    project.config(&format!(
        r#"{{"version":1,"operations":{{
            "crap":{{"argv":[{python:?},"-c","raise SystemExit(1)"],"forwardArgs":"none","scope":"whole-tree","validResultExitCodes":[0]}},
            "mutate":{{"argv":[{python:?},"-c","raise SystemExit(3)"],"forwardArgs":"append","validResultExitCodes":[0,3]}}}}}}"#,
        python = python.to_string_lossy()
    ));
    actual.push(project.quality(&["crap"]));
    actual.push(project.quality(&["mutate", "x.rs"]));
    project.config(r#"{"version":1}"#);
    actual.push(project.quality(&["crap"]));
    assert_eq!(actual.len(), expected.len());
    for (output, expected) in actual.iter().zip(&expected) {
        let text = String::from_utf8(output.stdout.clone()).unwrap();
        let normalized = text.replace(
            &format!(r#""{}""#, escape_json(&project.root.to_string_lossy())),
            r#""/project""#,
        );
        assert_eq!(
            normalized.trim(),
            samples
                .lines()
                .find(|line| {
                    let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
                    &parsed == expected
                })
                .unwrap(),
            "the committed sample for {expected}"
        );
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn escape_json(text: &str) -> String {
    serde_json::to_string(text)
        .unwrap()
        .trim_matches('"')
        .to_string()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn python_executable() -> Option<std::ffi::OsString> {
    if cfg!(windows) {
        return None;
    }
    for candidate in ["python", "python3"] {
        let probe = Command::new(candidate)
            .args(["-c", "import sys; print(sys.executable)"])
            .output();
        if let Some(found) = probe.ok().filter(|o| o.status.success()) {
            return Some(String::from_utf8_lossy(&found.stdout).trim().into());
        }
    }
    None
}

/// Every named target reaches mutator unexpanded and in the order named
/// (ADR 0001 2026-11-01): a directory that contains a root, one inside a
/// root, and a file. `--scan` lists the sites mutator selected, so the
/// forwarded targets are observable without any test run.
#[cfg(target_os = "linux")]
#[test]
fn named_directories_and_files_reach_one_mutation_call_in_order() {
    let project = Project::new();
    let root = mutation_project(&project, r#"["--no-coverage","--scan"]"#);
    let output = quality_at(
        &project.base,
        &root,
        &["mutate", "app", "util/src", "app/src/lib.rs"],
    );
    let record = checked(&output, "mutate", &root);
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{err}");
    assert_eq!(record["validResult"], true);
    for file in ["app/src/lib.rs", "app/src/more.rs", "util/src/lib.rs"] {
        assert!(
            err.contains(&format!("Scan: 1 mutation sites in {file}")),
            "{err}"
        );
    }
    // A named directory outside every root is named on stderr and dropped;
    // with only such targets the call is refused without launching.
    let output = quality_at(&project.base, &root, &["mutate", "app/tests"]);
    let record = checked(&output, "mutate", &root);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("uml: omitted mutation target outside business scope: app/tests"),
        "{}",
        stderr(&output)
    );
    assert_eq!(record["validResult"], false);
}

/// `:refresh-mutate` with `--changed`: mutator takes the git-changed set,
/// narrowed by the declared roots and exclusions, with no named target.
#[cfg(target_os = "linux")]
#[test]
fn changed_mutation_selects_the_changed_files_inside_the_declared_scope() {
    let project = Project::new();
    let root = mutation_project(&project, r#"["--no-coverage","--scan"]"#);
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .expect("git is required for --changed");
        assert!(output.status.success(), "{args:?}: {:?}", output);
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t15@example.invalid"]);
    git(&["config", "user.name", "t15"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    let changed = root.join("app/src/more.rs");
    fs::write(
        &changed,
        fs::read_to_string(&changed)
            .unwrap()
            .replace("x / 2", "x / 4"),
    )
    .unwrap();
    // util stays unchanged; an out-of-scope change is not selected either.
    fs::write(
        root.join("Cargo.toml"),
        fs::read_to_string(root.join("Cargo.toml")).unwrap(),
    )
    .unwrap();
    let output = quality_at(&project.base, &root, &["mutate", "--changed"]);
    let record = checked(&output, "mutate", &root);
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{err}");
    assert_eq!(record["validResult"], true);
    assert!(
        err.contains("Scan: 1 mutation sites in app/src/more.rs"),
        "{err}"
    );
    assert!(!err.contains("app/src/lib.rs"), "unchanged file: {err}");
    assert!(!err.contains("util/src/lib.rs"), "unchanged file: {err}");
}

/// The standard route's own exit sets, from the real crapper: a threshold
/// finding is exit 2 and valid; coverage that joins no function is exit 3
/// and not valid without a declaration.
#[test]
fn crapper_threshold_findings_are_valid_but_an_unmeasured_threshold_is_not() {
    let project = Project::new();
    project.write(
        "src/lib.rs",
        "\npub fn choose(x: bool) -> i32 {\n    if x { 1 } else { 0 }\n}\n",
    );
    // Custom coverage writes its report through the host shell on both
    // platforms, so --full-tree can still score a threshold without a test run.
    let config = |coverage: &str| {
        project.write("src/lcov.txt", coverage);
        project.write("coverage/.keep", "");
        let command = if cfg!(windows) {
            "type src\\lcov.txt > coverage\\lcov.info"
        } else {
            "cat src/lcov.txt > coverage/lcov.info"
        };
        project.config(&format!(
            r#"{{"version":1,"standardTools":{{"sourceRoots":["src"],"crapperOptions":["--coverage-command",{},"--threshold","1"]}}}}"#,
            serde_json::to_string(command).unwrap()
        ));
    };
    config("SF:src/lib.rs\nDA:2,0\nDA:3,0\nLF:2\nLH:0\nend_of_record\n");
    let output = project.quality(&["crap"]);
    let record = checked(&output, "crap", &project.root);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(record["validResult"], true, "crapper's valid set is 0/2");
    assert!(project.root.join(".metrics").join("crap.edn").exists());
    project.write("src/other.rs", "// no functions here\n");
    config("SF:src/other.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n");
    let output = project.quality(&["crap"]);
    let record = checked(&output, "crap", &project.root);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert_eq!(record["validResult"], false, "3 needs a declaration");
    assert_eq!(record["reason"], serde_json::Value::Null, "crapper ran");
}

/// `operations` requires crap and mutate; an omitted dry stays on the
/// standard route (`configured: false`, `tool: dryer`).
#[test]
fn an_operations_manifest_without_dry_keeps_the_standard_dryer() {
    let project = Project::new();
    let operation = r#"{"argv":["unused"],"forwardArgs":"none","validResultExitCodes":[0]}"#;
    let crap = r#"{"argv":["unused"],"forwardArgs":"none","scope":"whole-tree","validResultExitCodes":[0]}"#;
    project.config(&format!(
        r#"{{"version":1,"operations":{{"crap":{crap},"mutate":{operation}}}}}"#
    ));
    project.write("src/lib.rs", "pub fn one() -> i64 { 1 }\n");
    let output = project.quality(&["dry"]);
    let record = checked(&output, "dry", &project.root);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(record["configured"], false);
    assert_eq!(record["tool"], "dryer");
    assert!(project.root.join(".metrics").join("dry.edn").exists());
}

/// A real crap and dry run over this repository's own Rust fixture project,
/// copied to scratch so the checkout is never measured in place.
#[test]
fn quality_runs_crap_and_dry_on_the_repository_rust_fixture() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-fixture");
    let project = Project::new();
    for entry in walk_files(&source) {
        let relative = entry.strip_prefix(&source).unwrap();
        let text = fs::read_to_string(&entry).unwrap();
        project.write(&relative.to_string_lossy().replace('\\', "/"), &text);
    }
    project.config(
        r#"{"version":1,"standardTools":{"sourceRoots":["src"],"crapperOptions":["--no-coverage"]}}"#,
    );
    let output = project.quality(&["crap"]);
    let record = checked(&output, "crap", &project.root);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(record["validResult"], true);
    let snapshot = fs::read_to_string(project.root.join(".metrics").join("crap.edn")).unwrap();
    assert!(
        snapshot.contains(":namespace \"crate::console\""),
        "{snapshot}"
    );
    let output = project.quality(&["dry"]);
    let record = checked(&output, "dry", &project.root);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(record["validResult"], true);
    assert!(project.root.join(".metrics").join("dry.edn").exists());
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(walk_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    found
}
