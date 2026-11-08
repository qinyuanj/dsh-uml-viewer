use crate::binary;
use std::fs;

#[test]
fn mutate_binary_exposes_original_help_and_usage_exit() {
    let output = binary(["mutate", "--nope", "--help"]);
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/mutate/help.txt"
        ))
        .unwrap()
    );
    assert!(output.stderr.is_empty());
    let output = binary(["mutate", "--nope"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output
            .stderr
            .starts_with(b"Unknown option: --nope\n\nUsage:")
    );
}
#[cfg(not(target_os = "linux"))]
#[test]
fn non_linux_mutation_refuses_without_running_tests() {
    let output = binary(["mutate"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("only on Linux"));
}

#[cfg(target_os = "linux")]
fn fixture_project() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    fn copy(from: &std::path::Path, to: &std::path::Path) {
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
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/rust");
    let directory = std::env::temp_dir().join(format!(
        "mutate-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    copy(&source, &directory);
    directory.join("project")
}

#[cfg(target_os = "linux")]
fn remove_fixture(root: &std::path::Path) {
    let directory = root.parent().unwrap();
    let name = directory.file_name().unwrap().to_str().unwrap();
    assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
    assert!(name.starts_with("mutate-cli-"), "{name}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn mutate_usage_errors_are_the_original_bytes_and_messages() {
    let help = fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/mutate/help.txt"
    ))
    .unwrap();
    for (arguments, message) in [
        (vec!["--nope"], "Unknown option: --nope"),
        (
            vec!["--timeout-factor", "fast"],
            "could not convert string to float: 'fast'",
        ),
        (
            vec!["--mutation-warning", "many"],
            "invalid literal for int() with base 10: 'many'",
        ),
        (
            vec!["--lines", "x"],
            "invalid literal for int() with base 10: 'x'",
        ),
        (
            vec!["--scan", "--max-workers", "2"],
            "--scan cannot be combined with --max-workers",
        ),
    ] {
        let output = binary(std::iter::once("mutate").chain(arguments.iter().copied()));
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let mut expected = format!("{message}\n\n").into_bytes();
        expected.extend_from_slice(&help);
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&expected),
            "{arguments:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn mutate_scan_reports_sites_without_coverage_or_snapshot_writes() {
    let root = fixture_project();
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--scan",
        "app/src/lib.rs",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Scan: 1 mutation sites in app/src/lib.rs"),
        "{stdout}"
    );
    assert!(!root.join(".metrics").exists());
    assert!(!root.join("target").exists());
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--scan",
        "--lines",
        "999",
        "app/src/lib.rs",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Scan: 0 mutation sites"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    remove_fixture(&root);
}

#[cfg(target_os = "linux")]
#[test]
fn mutate_timings_warning_and_verbose_report_like_the_original() {
    let root = fixture_project();
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--no-coverage",
        "--mutate-all",
        "--timings",
        "--mutation-warning",
        "0",
        "--verbose",
        "--test-command",
        "true",
        "app/src/lib.rs",
    ]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("WARNING: 1 covered mutations selected in app/src/lib.rs."),
        "{stderr}"
    );
    assert!(stderr.contains("timing baseline: "), "{stderr}");
    assert!(stderr.contains("timing mutation-run: "), "{stderr}");
    assert!(!stderr.contains("timing coverage: "), "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "app/src/lib.rs:4 app/twice + -> - executed reason=explicit-all previous=none result=survived id=["
        ),
        "{stdout}"
    );
    remove_fixture(&root);
}

#[cfg(target_os = "linux")]
#[test]
fn mutate_binary_refuses_stale_backups_before_any_test() {
    let root = fixture_project();
    let backup = root.join("target/mutator-backup/app.rs");
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    fs::write(&backup, "stale\n").unwrap();
    let marker = root.parent().unwrap().join("ran");
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--no-coverage",
        "--mutate-all",
        "--test-command",
        &format!("touch {}", marker.display()),
        "app/src/lib.rs",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists(), "no test may run while backups exist");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Found backups from an older mutator under"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    remove_fixture(&root);
}

#[cfg(target_os = "linux")]
fn wait_until(condition: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    condition()
}

#[cfg(target_os = "linux")]
fn lines(path: &std::path::Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A host process is gone, allowing for the kernel's reaping.
#[cfg(target_os = "linux")]
fn gone(pid: u32) -> bool {
    wait_until(|| !std::path::Path::new(&format!("/proc/{pid}")).exists())
}

/// Any live namespace still holds the overlay mounted on the scratch root.
#[cfg(target_os = "linux")]
fn mounted(path: &str) -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str().map(str::to_string) else {
            continue;
        };
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(info) = fs::read_to_string(entry.path().join("mountinfo")) else {
            continue;
        };
        if info.contains(path) {
            return true;
        }
    }
    false
}

/// The hostile command of the interrupt cases: every mutant records the worker
/// slot's scratch root and its own host pid, then sleeps far past its timeout.
/// The baseline stays quick so the mutant timeout stays short.
#[cfg(target_os = "linux")]
fn hanging_command(scratch: &std::path::Path) -> (String, std::path::PathBuf, std::path::PathBuf) {
    let roots = scratch.join("roots.txt");
    let pids = scratch.join("pids.txt");
    let command = format!(
        "if [ -f \"$MUTATOR_VIEW/mutation\" ]; then \
           echo \"$(dirname \"$MUTATOR_VIEW\")\" >> {roots}; \
           awk '/^NSpid:/{{print $2}}' /proc/self/status >> {pids}; \
           sleep 60; fi; \
         sleep 0.4; true",
        roots = roots.display(),
        pids = pids.display()
    );
    (command, roots, pids)
}

/// A project with two mutation sites in one file, so two workers hang at once.
#[cfg(target_os = "linux")]
fn hanging_project() -> std::path::PathBuf {
    let root = fixture_project();
    fs::write(
        root.join("app/src/extra.rs"),
        "pub fn extra(x: i64) -> i64 { x + x + x }\n",
    )
    .unwrap();
    root
}

#[cfg(target_os = "linux")]
fn spawn_hanging(root: &std::path::Path, command: &str) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .args([
            "mutate",
            "--root",
            root.to_str().unwrap(),
            "--max-workers",
            "2",
            "--no-coverage",
            "--mutate-all",
            "--timeout-factor",
            "20",
            "--test-command",
            command,
            "app/src/extra.rs",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

#[cfg(target_os = "linux")]
fn signal(child: &std::process::Child, name: &str) {
    let status = std::process::Command::new("kill")
        .args(["-s", name, &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "kill -s {name}");
}

/// Ported from the original `test_interrupt.py`: an interrupted run kills its
/// tests, removes its scratch and publishes nothing for unfinished files.
#[cfg(target_os = "linux")]
#[test]
fn an_interrupted_run_kills_its_tests_and_removes_its_scratch() {
    for (name, code) in [("TERM", 143), ("INT", 130)] {
        let root = hanging_project();
        let source = fs::read(root.join("app/src/extra.rs")).unwrap();
        let scratch = root.parent().unwrap();
        let (command, roots, pids) = hanging_command(scratch);
        let mut child = spawn_hanging(&root, &command);
        let started = wait_until(|| lines(&roots).len() >= 2 && lines(&pids).len() >= 2);
        if !started {
            child.kill().ok();
            let output = child.wait_with_output().unwrap();
            panic!(
                "no mutant started: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        signal(&child, name);
        let output = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(code), "{stderr}");
        assert!(
            stderr.contains(&format!(
                "Interrupted by SIG{name}; unfinished files keep their previous snapshots."
            )),
            "{stderr}"
        );
        for pid in lines(&pids) {
            let pid: u32 = pid.parse().unwrap();
            assert!(gone(pid), "test process {pid} survived the interrupt");
        }
        for root in lines(&roots) {
            assert!(!mounted(&root), "the overlay of {root} outlived the run");
            assert!(
                !std::path::Path::new(&root).exists(),
                "scratch {root} survived the interrupt"
            );
        }
        assert_eq!(fs::read(root.join("app/src/extra.rs")).unwrap(), source);
        assert!(
            !root.join(".metrics").exists(),
            "an unfinished file publishes no snapshot"
        );
        remove_fixture(&root);
    }
}

/// Ported from the original killed-run case: SIGKILL leaves nothing running or
/// mounted, and the next run reclaims the scratch the killed one abandoned.
#[cfg(target_os = "linux")]
#[test]
fn a_killed_run_leaves_nothing_running_and_the_next_run_sweeps_its_scratch() {
    use std::os::unix::process::ExitStatusExt;
    let root = hanging_project();
    let source = fs::read(root.join("app/src/extra.rs")).unwrap();
    let scratch = root.parent().unwrap().to_path_buf();
    let (command, roots, pids) = hanging_command(&scratch);
    let mut child = spawn_hanging(&root, &command);
    let started = wait_until(|| lines(&roots).len() >= 2 && lines(&pids).len() >= 2);
    if !started {
        child.kill().ok();
        let output = child.wait_with_output().unwrap();
        panic!(
            "no mutant started: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    signal(&child, "KILL");
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.signal(), Some(9));
    for pid in lines(&pids) {
        let pid: u32 = pid.parse().unwrap();
        assert!(gone(pid), "test process {pid} outlived a killed mutator");
    }
    let abandoned = lines(&roots);
    assert!(!abandoned.is_empty());
    for root in &abandoned {
        assert!(
            !mounted(root),
            "the overlay of {root} outlived its namespace"
        );
        assert!(
            std::path::Path::new(root).exists(),
            "a killed run leaves its scratch for the next one: {root}"
        );
    }
    assert_eq!(fs::read(root.join("app/src/extra.rs")).unwrap(), source);
    assert!(!root.join(".metrics").exists());
    // The next call reclaims the abandoned root, then mutates normally.
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--no-coverage",
        "--mutate-all",
        "--max-workers",
        "1",
        "--test-command",
        "true",
        "app/src/lib.rs",
    ]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for root in &abandoned {
        assert!(
            !std::path::Path::new(root).exists(),
            "the next run swept {root}"
        );
    }
    remove_fixture(&root);
}

/// The real binary runs Python and TypeScript projects end to end: coverage is
/// measured, an untested file is reported uncovered, and the snapshots are the
/// pinned original's bytes apart from the real clock. Byte-exact equality with
/// an injected clock lives in mutator's `python_ts` tests.
#[cfg(target_os = "linux")]
#[test]
fn python_and_typescript_projects_mutate_through_the_real_binary() {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
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
    fn snapshots(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        fn walk(
            base: &Path,
            directory: &Path,
            found: &mut std::collections::BTreeMap<String, Vec<u8>>,
        ) {
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
        let base = root.join(".metrics/mutate");
        let mut found = std::collections::BTreeMap::new();
        walk(&base, &base, &mut found);
        found
    }
    let venv = std::env::var_os("MUTATOR_TEST_PYTHON_VENV").map(PathBuf::from);
    let node = Command::new("node")
        .args(["--experimental-strip-types", "-e", ""])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    for language in ["python", "typescript"] {
        let available = match language {
            "python" => venv.as_ref().is_some_and(|venv| {
                Command::new(venv.join("bin/python"))
                    .args(["-c", "import coverage, pytest"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|status| status.success())
            }),
            _ => node,
        };
        if !available {
            assert!(
                std::env::var_os("MUTATOR_REQUIRE_TOOLCHAINS").as_deref()
                    != Some(std::ffi::OsStr::new("1")),
                "required mutation toolchain unavailable for {language}"
            );
            eprintln!("SKIP: {language} toolchain unavailable (venv/node)");
            continue;
        }
        let directory = std::env::temp_dir().join(format!(
            "mutate-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let root = directory.join("project");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/mutate")
            .join(language);
        copy(&fixture.join("project"), &root);
        if language == "python"
            && let Some(venv) = &venv
        {
            std::os::unix::fs::symlink(venv, root.join(".venv")).unwrap();
            std::os::unix::fs::symlink(venv, root.join("legacy/.venv")).unwrap();
        }
        let targets: &[&str] = match language {
            "python" => &["src/shapes/geometry.py", "legacy/tally/core.py"],
            _ => &["src/calc.ts", "src/view.tsx"],
        };
        let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
            .arg("mutate")
            .arg("--root")
            .arg(&root)
            .args([
                "--max-workers",
                "1",
                "--timeout-factor",
                "20",
                "--mutate-all",
            ])
            .args(targets)
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3), "{language}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("Linux mutation currently supports Rust only"),
            "{language}: {stderr}"
        );
        if language == "typescript" {
            assert!(
                stderr.contains(
                    "No coverage data for src/view.tsx; its sites are uncovered. Pass --no-coverage to run them anyway."
                ),
                "{stderr}"
            );
        }
        let published = snapshots(&root.join(".metrics/mutate"));
        let golden = snapshots(&fixture.join("goldens"));
        assert_eq!(published.len(), golden.len(), "{language}");
        for (name, bytes) in &published {
            let expected = &golden[name];
            // Only the real clock differs; every other byte is the original's.
            let clock = |bytes: &[u8]| {
                let text = String::from_utf8(bytes.to_vec()).unwrap();
                text.split(":tested-at \"")
                    .enumerate()
                    .map(|(index, part)| {
                        if index == 0 {
                            part.to_string()
                        } else {
                            format!("CLOCK{}", part.split_once('"').unwrap().1)
                        }
                    })
                    .collect::<String>()
            };
            assert_eq!(clock(bytes), clock(expected), "{language} {name}");
            assert!(bytes.starts_with(b"{:version 1 :tested-at \""), "{name}");
        }
        assert!(!root.join("target").exists(), "{language} wrote target/");
        assert!(
            !root.join("coverage").exists(),
            "{language} wrote coverage/"
        );
        assert!(
            !root.join("node_modules").exists(),
            "{language} wrote node_modules"
        );
        assert!(!root.join("src/shapes/__pycache__").exists());
        remove_fixture(&root);
    }
}
