use crate::binary;
#[cfg(target_os = "linux")]
use crate::binary_env;
use std::fs;

/// A copy of the Lua fixture project: a root module, a nested module, a spec
/// file with a space in its name and an untested source.
#[cfg(target_os = "linux")]
fn lua_project(name: &str) -> std::path::PathBuf {
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
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/lua/project");
    let directory =
        std::env::temp_dir().join(format!("mutate-lua-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    copy(&source, &directory.join("project"));
    directory.join("project")
}

#[cfg(target_os = "linux")]
fn remove_lua_project(root: &std::path::Path) {
    let directory = root.parent().unwrap();
    assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
    let name = directory.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("mutate-lua-cli-"), "{name}");
    fs::remove_dir_all(directory).unwrap();
}

/// PITFALLS: a sharded path must really run its shards. One line is emitted
/// per launched shard, so the root module's two specs produce two lines.
#[cfg(target_os = "linux")]
#[test]
fn lua_coverage_runs_busted_in_multiple_shards() {
    let root = lua_project("shards");
    let output = binary_env(
        [
            "mutate",
            "--root",
            root.to_str().unwrap(),
            "--max-workers",
            "1",
            "--timeout-factor",
            "20",
            "--mutate-all",
            "src/calc.lua",
            "src/odd name.lua",
            "nested/lib/mathx.lua",
        ],
        &[("MUTATOR_LUA_SHARDS", "2")],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(3), "{stderr}");
    assert_eq!(
        stderr.matches("Busted in 2 coverage shards").count(),
        2,
        "two Busted coverage shards really ran:\n{stderr}"
    );
    assert!(root.join(".metrics/mutate/calc.edn").is_file());
    // The collector, the stats and the report live in the view scratch.
    assert!(!root.join("target/coverage").exists(), "coverage leak");
    assert!(!root.join("coverage").exists(), "coverage leak");
    remove_lua_project(&root);
}

/// Scanning inventories the Lua sites: no suite, no coverage, no snapshots.
#[cfg(target_os = "linux")]
#[test]
fn lua_scan_inventories_sites_without_running_anything() {
    let root = lua_project("scan");
    let output = binary_env(["mutate", "--root", root.to_str().unwrap(), "--scan"], &[]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Scan: 12 mutation sites in src/calc.lua"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Scan: 3 mutation sites in nested/lib/mathx.lua"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Scan: 1 mutation sites in src/odd name.lua"),
        "{stdout}"
    );
    // The Lua spellings: `~=` becomes `==`, `//` becomes `/`, and the private
    // `local function hidden` keeps its defn-/ identity.
    assert!(stdout.contains("~= -> =="), "{stdout}");
    assert!(stdout.contains("// -> /"), "{stdout}");
    assert!(stdout.contains("[defn-/hidden]"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("coverage shards"), "{stderr}");
    assert!(!root.join(".metrics").exists());
    remove_lua_project(&root);
}

/// `--select-specs` must really filter the suite. Each spec file appends its
/// name and Busted's argv to an external marker when it loads. Only the `*`
/// mutant of `M.double` (line 8) is selected, and its covering spec is
/// `spec/double unit_spec.lua`: the full suite's command lists both specs,
/// covering first, while a selected run's commands name only that one.
#[cfg(target_os = "linux")]
#[test]
fn select_specs_runs_only_the_covering_specs() {
    fn mutant_lines(root: &std::path::Path, select: bool) -> Vec<String> {
        let marker = root.with_extension("marker");
        let _ = fs::remove_file(&marker);
        let mut args = vec![
            "mutate".to_string(),
            "--root".into(),
            root.to_string_lossy().into_owned(),
            "--max-workers".into(),
            "1".into(),
            "--timeout-factor".into(),
            "20".into(),
            "--mutate-all".into(),
            "--lines".into(),
            "8".into(),
            "src/calc.lua".into(),
        ];
        if select {
            args.insert(args.len() - 1, "--select-specs".into());
        }
        let output = binary_env(
            args,
            &[
                ("MUTATOR_LUA_SPEC_MARKER", marker.to_str().unwrap()),
                ("MUTATOR_LUA_SHARDS", "1"),
            ],
        );
        let code = output.status.code().unwrap_or(-1);
        assert!(
            code == 0 || code == 3,
            "exit {code}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = fs::read_to_string(&marker).expect("the specs marked their runs");
        let lines: Vec<String> = text
            .lines()
            .filter(|line| line.contains("--no-keep-going"))
            .map(str::to_string)
            .collect();
        let _ = fs::remove_file(&marker);
        lines
    }
    let root = lua_project("filter");
    // The full suite lists every spec, covering one first.
    let full = mutant_lines(&root, false);
    assert_eq!(full.len(), 1, "one mutant run: {full:?}");
    assert!(full[0].contains("spec/add_spec.lua"), "{full:?}");
    assert!(full[0].contains("spec/double unit_spec.lua"), "{full:?}");
    // Selected specs: the pristine subset check and the mutant run, both with
    // only the covering spec — the non-covering spec is never an argument.
    let selected = mutant_lines(&root, true);
    assert_eq!(
        selected.len(),
        2,
        "subset check and mutant run: {selected:?}"
    );
    for line in &selected {
        assert!(line.contains("spec/double unit_spec.lua"), "{line}");
        assert!(!line.contains("spec/add_spec.lua"), "{line}");
    }
    remove_lua_project(&root);
}

/// Without the Lua toolchain the run refuses before any test: no snapshot is
/// published and the sources stay as they were.
#[cfg(target_os = "linux")]
#[test]
fn lua_mutation_requires_the_pinned_cluacov_toolchain() {
    let root = lua_project("toolchain");
    let empty = std::env::temp_dir().join(format!("mutate-lua-empty-{}", std::process::id()));
    fs::create_dir_all(&empty).unwrap();
    let output = binary_env(
        [
            "mutate",
            "--root",
            root.to_str().unwrap(),
            "--max-workers",
            "1",
            "--mutate-all",
            "src/calc.lua",
        ],
        &[("PATH", empty.to_str().unwrap())],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("No Lua 5.4 interpreter"), "{stderr}");
    assert!(!root.join(".metrics").exists());
    assert_eq!(
        fs::read(root.join("src/calc.lua")).unwrap(),
        fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/mutate/lua/project/src/calc.lua")
        )
        .unwrap()
    );
    let _ = fs::remove_dir_all(&empty);
    remove_lua_project(&root);
}

/// The shared selection: `uml-viewer crap` and `uml-viewer mutate` must pick
/// the same Lua sources for the same `-s` and `--exclude`.
#[cfg(target_os = "linux")]
#[test]
fn lua_selection_agrees_with_crap_on_the_same_roots_and_excludes() {
    fn namespaces(text: &str) -> Vec<String> {
        let mut names: Vec<String> = text
            .lines()
            .filter(|line| line.contains(":namespace "))
            .map(|line| {
                // Both snapshots quote the namespace; what follows it differs
                // (crap's entry fields versus mutate's outcomes map).
                line.split(":namespace ")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .nth(1)
                    .unwrap()
                    .to_string()
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }
    fn mutate_namespaces(root: &std::path::Path) -> Vec<String> {
        let directory = root.join(".metrics/mutate");
        let mut names = Vec::new();
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "edn") {
                names.extend(namespaces(&fs::read_to_string(&path).unwrap()));
            }
        }
        names.sort();
        names.dedup();
        names
    }
    for (case, flags, expected) in [
        ("all", vec!["-s", "src"], vec!["calc", "odd name"]),
        (
            "exclude",
            vec!["-s", "src", "--exclude", "src/odd name.lua"],
            vec!["calc"],
        ),
        ("nested", vec!["-s", "nested"], vec!["nested.lib.mathx"]),
    ] {
        let root = lua_project(case);
        let mut args = vec![
            "crap".to_string(),
            "--root".into(),
            root.to_string_lossy().into_owned(),
            "--no-coverage".into(),
        ];
        args.extend(flags.iter().map(|flag| flag.to_string()));
        let output = binary_env(args, &[]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "crap {case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let crap = namespaces(&fs::read_to_string(root.join(".metrics/crap.edn")).unwrap());
        assert_eq!(crap, expected, "crap selection for {case}");

        let mut args = vec![
            "mutate".to_string(),
            "--root".into(),
            root.to_string_lossy().into_owned(),
            "--no-coverage".into(),
            "--max-workers".into(),
            "1".into(),
            "--timeout-factor".into(),
            "20".into(),
            "--lines".into(),
            "4".into(),
        ];
        args.extend(flags.iter().map(|flag| flag.to_string()));
        let output = binary_env(args, &[]);
        let code = output.status.code().unwrap_or(-1);
        assert!(code == 0 || code == 3, "mutate {case}: exit {code}");
        assert_eq!(
            mutate_namespaces(&root),
            expected,
            "mutate selection {case}"
        );
        remove_lua_project(&root);
    }
}

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
