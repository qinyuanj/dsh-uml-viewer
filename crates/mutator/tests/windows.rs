//! Native Windows mutation: one fresh project copy per command, every process
//! contained by #3's kill-on-close Job Object (ADR 0010).
//!
//! Test code is the outside harness; it launches the fixture tools itself.
#![cfg(windows)]
#![allow(clippy::disallowed_methods)]
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Serialize calls so a scratch root newly created by this process belongs to
/// the active fixture. Console handlers are also process-global.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The original writes `:isolation :copy` on Windows and `:namespace-view` on
/// Linux. Everything else in the snapshot is the committed golden's bytes.
fn golden_copy(file: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mutate/goldens")
        .join(file);
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.matches(":isolation :namespace-view").count(),
        1,
        "{file} must name exactly one isolation level"
    );
    text.replace(":isolation :namespace-view", ":isolation :copy")
        .into_bytes()
}

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
                    .replace('\\', "/");
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
    scratch_directory: PathBuf,
    scratch_before: BTreeSet<PathBuf>,
}

fn process_scratch(temporary: &Path) -> BTreeSet<PathBuf> {
    let mine = format!("mutator-{}-", std::process::id());
    fs::read_dir(temporary)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(&mine))
        })
        .collect()
}
impl Fixture {
    fn new() -> Self {
        let scratch_directory = std::env::temp_dir();
        let scratch_before = process_scratch(&scratch_directory);
        let directory = std::env::temp_dir().join(format!(
            "mutator-windows-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let project = directory.join("project");
        for crate_name in ["app", "util"] {
            let from = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/mutate/rust/project")
                .join(crate_name);
            let to = project.join(crate_name);
            fs::create_dir_all(to.join("src")).unwrap();
            fs::create_dir_all(to.join("tests")).unwrap();
            for part in [
                "Cargo.toml",
                "src/lib.rs",
                "src/more.rs",
                "tests/app.rs",
                "tests/below.rs",
            ] {
                let source = from.join(part);
                if source.is_file() {
                    fs::copy(&source, to.join(part)).unwrap();
                }
            }
        }
        fs::write(
            project.join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\", \"util\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        // A copy at the worker's fixed path has no `../../extdep`. The original
        // helper substitutes an absolute dependency for the same reason.
        let extdep =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/rust/extdep");
        let manifest = fs::read_to_string(project.join("app/Cargo.toml"))
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("extdep ="))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            project.join("app/Cargo.toml"),
            format!(
                "{manifest}\nextdep = {{ path = \"{}\" }}\n",
                quality_core::paths::posix(&extdep)
            ),
        )
        .unwrap();
        Self {
            directory,
            project,
            scratch_directory,
            scratch_before,
        }
    }
    fn leaked_scratch(&self) -> Vec<PathBuf> {
        process_scratch(&self.scratch_directory)
            .difference(&self.scratch_before)
            .cloned()
            .collect()
    }
    fn assert_no_scratch(&self) {
        let left = self.leaked_scratch();
        assert!(left.is_empty(), "fixture copies left behind: {left:?}");
    }
    fn root(&self) -> PathBuf {
        self.project.clone()
    }
    fn run(&self, extra: &[&str]) -> ExitCode {
        self.run_many(1, extra)
    }
    fn run_many(&self, workers: usize, extra: &[&str]) -> ExitCode {
        let mut args = vec![
            "--root".into(),
            self.root().into_os_string(),
            "--max-workers".into(),
            workers.to_string().into(),
        ];
        args.extend(extra.iter().map(std::ffi::OsString::from));
        let code = mutator::run_with_clock(&args, || "2026-11-07T12:00:00+00:00".to_string());
        // Every command must remove the copies it created, not only the last
        // one of a scenario. Another process's roots are not this fixture's.
        self.assert_no_scratch();
        code
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
            || !name.starts_with("mutator-windows-")
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

/// A scenario that runs no command must not pass silently.
/// False means the scenario did not run, loudly; with
/// `MUTATOR_REQUIRE_TOOLCHAINS=1` (CI) a missing toolchain is a failure.
fn require(condition: bool, reason: &str) -> bool {
    if condition {
        return true;
    }
    assert!(
        !std::env::var_os("MUTATOR_REQUIRE_TOOLCHAINS").is_some_and(|v| v == "1"),
        "MUTATOR_REQUIRE_TOOLCHAINS=1; {reason}"
    );
    eprintln!("SKIP: {reason}");
    false
}

/// A native helper the fixture commands run: it writes the LCOV the pinned
/// oracle's coverage produced, with `SF:` paths relative to its own working
/// directory, i.e. inside the copy, never in the real project.
const HELPER: &str = r#"
use std::{env, fs, io::Write, path::Path, process::Command, time::{Duration, Instant}};
fn append(path: &str, line: String) {
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    file.write_all(line.as_bytes()).unwrap();
}
fn arg(n: usize) -> String { env::args().nth(n).unwrap() }
fn mutated() -> bool {
    Path::new(&env::var("MUTATOR_VIEW").unwrap()).join("mutation").is_file()
}
fn main() {
    let mode = arg(1);
    if mode == "coverage" {
        let mut text = String::new();
        for (file, line) in [("util/src/lib.rs", 2), ("app/src/lib.rs", 4), ("app/src/more.rs", 2)] {
            let cwd = env::current_dir().unwrap();
            let source = cwd.join(file).to_string_lossy().replace('\\', "/");
            text.push_str(&format!("SF:{source}\nDA:{line},1\nLH:1\nLF:1\nend_of_record\n"));
        }
        fs::create_dir_all("coverage").unwrap();
        fs::write("coverage/lcov.info", text).unwrap();
        if let Some(marker) = env::args().nth(2) {
            append(&marker, "coverage\n".to_string());
        }
        return;
    }
    if mode == "phases" {
        let phase = if mutated() { "mutant" } else { "baseline" };
        append(&arg(2), format!("{phase}\n"));
        return;
    }
    if mode == "slot" {
        std::thread::sleep(Duration::from_millis(150));
        let phase = if mutated() { "mutant" } else { "baseline" };
        append(&arg(2), format!("{phase} {}\n", env::var("MUTATOR_VIEW").unwrap()));
        return;
    }
    if mode == "barrier" {
        let source = fs::read_to_string("app/src/extra.rs").unwrap();
        let name = if source.contains("x - x + x") { "inner" }
            else if source.contains("x + x - x") { "outer" }
            else { return };
        for round in ["r1", "r2"] {
            let directory = Path::new(&arg(2)).join(round);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(name), "").unwrap();
            let start = Instant::now();
            loop {
                let other = fs::read_dir(&directory).unwrap().flatten()
                    .any(|entry| entry.file_name() != *name);
                if other { break; }
                assert!(start.elapsed() < Duration::from_secs(30), "no other mutant arrived");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        return;
    }
    if mode == "write" {
        if !mutated() { return; }  // the pristine baseline passes
        // Writes to the working directory, the parent, and every redirected
        // home, then fails: a mutant's writes must reach nobody else.
        let mut folders = vec![Path::new(".").to_path_buf(), Path::new("..").to_path_buf()];
        for name in ["HOME", "USERPROFILE", "TEMP", "TMP", "APPDATA", "LOCALAPPDATA"] {
            folders.push(Path::new(&env::var(name).unwrap()).to_path_buf());
        }
        for folder in &folders {
            if folder.join("leak").exists() {
                println!("saw an earlier mutant's write: {}", folder.display());
                return;
            }
        }
        for folder in &folders {
            fs::write(folder.join("leak"), "x").unwrap();
        }
        std::process::exit(1);
    }
    if mode == "write-link" {
        if !mutated() { return; }
        fs::write("alias.txt", "changed").unwrap();
        std::process::exit(1);
    }
    if mode == "sleep" {
        use std::os::windows::fs::OpenOptionsExt;
        // Keep a real deny-delete handle in the copy until process termination.
        let held = fs::OpenOptions::new().write(true).create_new(true).share_mode(0)
            .open(format!("held-{}", std::process::id())).unwrap();
        fs::write(arg(2), "ready").unwrap();
        std::thread::sleep(Duration::from_secs(120));
        drop(held);
        return;
    }
    if mode == "daemon" || mode == "hang" {
        // The leader cannot exit before its descendant owns the file handle.
        // This covers handle release, not just eventual process disappearance.
        let ready = format!("{}.ready-{}", arg(2), std::process::id());
        let child = Command::new(env::current_exe().unwrap()).arg("sleep").arg(&ready)
            .stdin(std::process::Stdio::null()).spawn().unwrap();
        let start = Instant::now();
        while !Path::new(&ready).is_file() {
            assert!(start.elapsed() < Duration::from_secs(30), "descendant did not open its file");
            std::thread::sleep(Duration::from_millis(10));
        }
        append(&arg(2), format!("{} {}\n", std::process::id(), child.id()));
        if mode == "hang" && mutated() {
            std::thread::sleep(Duration::from_secs(120));
        }
        return;
    }
    panic!("unknown helper mode {mode}");
}
"#;

/// Compile the helper once per fixture; commands are `cmd` strings, so the
/// executable path is quoted.
fn helper(directory: &Path) -> PathBuf {
    let source = directory.join("helper.rs");
    fs::write(&source, HELPER).unwrap();
    let executable = directory.join("helper.exe");
    let status = std::process::Command::new("rustc")
        .arg("--edition=2021")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .status()
        .unwrap();
    assert!(status.success(), "compile the native fixture helper");
    executable
}

fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// Exercise ownership selection under a private synthetic temp directory: the
/// live unrelated owner and earlier roots must survive, while a new own root
/// must be reported. No real mutator root of another task is touched.
#[test]
fn scratch_assertion_ignores_other_owners_but_detects_new_fixture_roots() {
    let _serial = serial();
    let mut fixture = Fixture::new();
    let helper = helper(&fixture.directory);
    fixture.scratch_directory = fixture.directory.join("synthetic-temp");
    fs::create_dir(&fixture.scratch_directory).unwrap();
    let earlier = fixture
        .scratch_directory
        .join(format!("mutator-{}-earlier", std::process::id()));
    fs::create_dir(&earlier).unwrap();
    fixture.scratch_before = process_scratch(&fixture.scratch_directory);

    // A live process actually holds a deny-delete file under the unrelated
    // root. This cannot accidentally name or remove another task's temp data.
    let unrelated = fixture.scratch_directory.join("mutator-other-owner");
    fs::create_dir(&unrelated).unwrap();
    let ready = fixture.directory.join("unrelated-ready");
    let mut child = std::process::Command::new(helper)
        .arg("sleep")
        .arg(&ready)
        .current_dir(&unrelated)
        .spawn()
        .unwrap();
    let _cleanup = Watched::open(child.id());
    let started = std::time::Instant::now();
    while !ready.is_file() {
        assert!(started.elapsed() < Duration::from_secs(30));
        assert!(
            child.try_wait().unwrap().is_none(),
            "owner exited before readiness"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fixture.assert_no_scratch();
    assert!(unrelated.is_dir());
    assert!(earlier.is_dir());
    assert!(
        child.try_wait().unwrap().is_none(),
        "unrelated owner is alive"
    );

    let own = fixture
        .scratch_directory
        .join(format!("mutator-{}-new", std::process::id()));
    fs::create_dir(&own).unwrap();
    assert_eq!(fixture.leaked_scratch(), vec![own]);
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        unrelated.is_dir(),
        "the assertion must not delete other roots"
    );
}

/// Real `cargo test` in per-command copies, with the coverage report a
/// deterministic command writes. Unlike the default-coverage scenario this
/// needs no LLVM component, so it runs on every Windows host.
#[test]
fn real_cargo_in_copies_publishes_the_original_bytes() {
    let _serial = serial();
    let fixture = Fixture::new();
    let helper = helper(&fixture.directory);
    let coverage = format!("{} coverage", quoted(&helper));
    let targets = [
        "--coverage-command",
        &coverage,
        "--mutate-all",
        "--timeout-factor",
        "40",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    assert_eq!(fixture.run(&targets), ExitCode::SUCCESS);
    for file in ["app.edn", "app/more.edn", "util.edn"] {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            golden_copy(file),
            "{file}"
        );
    }
    // The unchanged differential rerun keeps those exact bytes.
    let rerun = [
        "--coverage-command",
        &coverage,
        "--timeout-factor",
        "40",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    assert_eq!(fixture.run(&rerun), ExitCode::SUCCESS);
    for file in ["app.edn", "app/more.edn", "util.edn"] {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            golden_copy(file),
            "{file}"
        );
    }
    assert!(
        !fixture.root().join("target").exists(),
        "all Cargo/test artifacts belong to copies"
    );
}

#[test]
fn windows_copies_publish_the_original_bytes_with_copy_isolation() {
    let _serial = serial();
    // cargo llvm-cov needs llvm-profdata from the toolchain's llvm-tools.
    // llvm-profdata ships with the active toolchain's llvm-tools component.
    let sysroot = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    let sysroot = PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim().to_string());
    let host = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .unwrap();
    let host = String::from_utf8_lossy(&host.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap()
        .to_string();
    let llvm = sysroot
        .join("lib/rustlib")
        .join(host)
        .join("bin/llvm-profdata.exe");
    if !require(
        quality_core::process::which("cargo-llvm-cov").is_some() && llvm.is_file(),
        "cargo-llvm-cov and the pinned toolchain's llvm-tools must be installed",
    ) {
        return;
    }
    let fixture = Fixture::new();
    let targets = [
        "--mutate-all",
        "--timeout-factor",
        "20",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    assert_eq!(fixture.run(&targets), ExitCode::SUCCESS);
    for file in ["app.edn", "app/more.edn", "util.edn"] {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            golden_copy(file),
            "{file}"
        );
    }
    assert_eq!(fixture.run(&targets[3..]), ExitCode::SUCCESS);
    for file in ["app.edn", "app/more.edn", "util.edn"] {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            golden_copy(file),
            "{file}"
        );
    }
    assert!(
        !fixture.root().join("target").exists(),
        "all Cargo/test artifacts belong to copies"
    );
}

/// Ported from `test_copy_isolation`: each mutant writes into its working
/// directory, the project parent and every redirected home, then fails. No
/// write may reach the real project, the real home, or a later mutant.
#[test]
fn mutant_writes_reach_neither_the_project_nor_the_user_nor_a_later_mutant() {
    let _serial = serial();
    let fixture = Fixture::new();
    helper(&fixture.directory);
    // One worker, so every mutant runs at the same path after the previous.
    let code = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let command = format!("{} write", quoted(&fixture.directory.join("helper.exe")));
                fixture.run(&[
                    "--no-coverage",
                    "--mutate-all",
                    "--test-command",
                    &command,
                    "app/src/lib.rs",
                    "app/src/more.rs",
                ])
            })
            .join()
            .unwrap()
    });
    assert_eq!(code, ExitCode::SUCCESS, "every mutant fails its own test");
    fn leaks(base: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in fs::read_dir(base).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == "leak") {
                found.push(path);
            } else if path.is_dir() {
                found.extend(leaks(&path));
            }
        }
        found
    }
    assert_eq!(leaks(&fixture.root()), Vec::<PathBuf>::new());
    // The user's real home, profile, temp and app-data hold no `leak` either:
    // a mutant that escaped the redirection would have written exactly there.
    for name in [
        "HOME",
        "USERPROFILE",
        "TEMP",
        "TMP",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        let real = std::env::var_os(name).unwrap();
        assert!(
            !Path::new(&real).join("leak").exists(),
            "{name} ({real:?}) received a mutant's write"
        );
    }
    // New roots of this serialized fixture must disappear. Other processes'
    // roots and leaks already reported by earlier fixtures are not ours.
    fixture.assert_no_scratch();
}

/// One coverage run and one baseline per (command, cwd), shared by every
/// target file; each file still publishes its own snapshot.
#[test]
fn one_coverage_and_one_baseline_are_shared_across_target_files() {
    let _serial = serial();
    let fixture = Fixture::new();
    let helper = helper(&fixture.directory);
    let marker = fixture.directory.join("phases.txt");
    let coverage = format!("{} coverage {}", quoted(&helper), quoted(&marker));
    let command = format!("{} phases {}", quoted(&helper), quoted(&marker));
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
                "app/src/more.rs",
            ]
        ),
        ExitCode::from(3),
        "every mutant's command succeeds, so all three sites survive"
    );
    let text = fs::read_to_string(&marker).unwrap();
    let count = |phase: &str| text.lines().filter(|line| *line == phase).count();
    assert_eq!(count("coverage"), 1, "{text}");
    assert_eq!(count("baseline"), 1, "{text}");
    assert_eq!(count("mutant"), 3, "{text}");
    let mut published: Vec<String> = snapshot_bytes(&fixture.root()).into_keys().collect();
    published.sort();
    assert_eq!(published, ["app.edn", "app/more.edn", "util.edn"]);
}

/// The pool belongs to the call: two files of two sites each use two slots,
/// and the slots are removed with it.
#[test]
fn worker_slots_are_reused_across_target_files_and_removed_afterwards() {
    let _serial = serial();
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
    let helper = helper(&fixture.directory);
    let log = fixture.directory.join("views.txt");
    let command = format!("{} slot {}", quoted(&helper), quoted(&log));
    assert_eq!(
        fixture.run_many(
            2,
            &[
                "--no-coverage",
                "--mutate-all",
                "--test-command",
                &command,
                "app/src/extra.rs",
                "util/src/extra.rs",
            ]
        ),
        ExitCode::from(3)
    );
    let text = fs::read_to_string(&log).unwrap();
    let mutants: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("mutant "))
        .map(|line| line["mutant ".len()..].trim())
        .collect();
    let views: std::collections::BTreeSet<&str> = mutants.iter().copied().collect();
    assert_eq!(views.len(), 2, "{text}");
    assert_eq!(mutants.len(), 4, "{text}");
    for view in views {
        assert!(
            view.contains("worker-0") || view.contains("worker-1"),
            "not a pool slot: {view}"
        );
        assert!(!Path::new(view).exists(), "slot left behind: {view}");
    }
}

/// With two workers two mutants are alive at once: two rendezvous rounds,
/// whose second-round marks a sequential run can never produce.
#[test]
fn concurrent_mutants_run_in_parallel_copies() {
    let _serial = serial();
    let fixture = Fixture::new();
    fs::write(
        fixture.root().join("app/src/extra.rs"),
        "pub fn extra(x: i64) -> i64 { x + x + x }\n",
    )
    .unwrap();
    let helper = helper(&fixture.directory);
    let barrier = fixture.directory.join("barrier");
    let command = format!("{} barrier {}", quoted(&helper), quoted(&barrier));
    assert_eq!(
        fixture.run_many(
            2,
            &[
                "--no-coverage",
                "--mutate-all",
                "--test-command",
                &command,
                "app/src/extra.rs",
            ]
        ),
        ExitCode::from(3)
    );
    let mut ready: Vec<String> = fs::read_dir(barrier.join("r2"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    ready.sort();
    assert_eq!(ready, ["inner", "outer"], "both mutants ran at once");
}

/// `--mutate-all` is a decision, never a wider selection.
#[test]
fn mutate_all_publishes_only_the_named_files() {
    let _serial = serial();
    let fixture = Fixture::new();
    let helper = helper(&fixture.directory);
    let coverage = format!("{} coverage", quoted(&helper));
    assert_eq!(
        fixture.run(&[
            "--coverage-command",
            &coverage,
            "--mutate-all",
            "--timeout-factor",
            "40",
            "app/src/lib.rs",
        ]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        snapshot_bytes(&fixture.root())
            .into_keys()
            .collect::<Vec<_>>(),
        ["app.edn"]
    );
    // `-s` still narrows the selection, so the same flag selects nothing.
    assert_eq!(
        fixture.run(&[
            "--coverage-command",
            &coverage,
            "--mutate-all",
            "-s",
            "util/src",
            "app/src/lib.rs",
        ]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        snapshot_bytes(&fixture.root())
            .into_keys()
            .collect::<Vec<_>>(),
        ["app.edn"]
    );
}

// --- Process containment -------------------------------------------------

use std::ffi::c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn TerminateProcess(handle: *mut c_void, code: u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

/// An open handle pins the process identity, so a reused PID cannot pass.
struct Watched(*mut c_void);
impl Watched {
    fn open(pid: u32) -> Self {
        Self(unsafe { OpenProcess(0x0010_0001, 0, pid) }) // SYNCHRONIZE | PROCESS_TERMINATE
    }
    fn ended(&self) -> bool {
        self.0.is_null() || unsafe { WaitForSingleObject(self.0, 10_000) } == 0
    }
}
impl Drop for Watched {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                if WaitForSingleObject(self.0, 0) != 0 {
                    TerminateProcess(self.0, 1);
                    WaitForSingleObject(self.0, 5_000);
                }
                CloseHandle(self.0);
            }
        }
    }
}

/// Run `args` while the main thread opens a handle on every process the
/// command starts, so "no leftover process" is checked by identity.
fn watching(fixture: &Fixture, pids: &Path, args: &[&str]) -> (ExitCode, Vec<u32>) {
    let pids = pids.to_path_buf();
    let (code, pids) = std::thread::scope(|scope| {
        let run = scope.spawn(|| fixture.run(args));
        let mut seen = std::collections::BTreeSet::new();
        let start = std::time::Instant::now();
        loop {
            if let Ok(text) = fs::read_to_string(&pids) {
                for pid in text.split_whitespace().flat_map(str::parse::<u32>) {
                    seen.insert(pid);
                }
            }
            if run.is_finished() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(120),
                "run did not finish"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        (run.join().unwrap(), seen)
    });
    assert!(!pids.is_empty(), "the fixture really started processes");
    (code, pids.into_iter().collect())
}

/// Every recorded pid must be gone; an open handle pins the identity while
/// the process still lives, so a reused pid cannot make this pass.
fn assert_all_ended(pids: &[u32]) {
    for pid in pids {
        let watched = Watched::open(*pid);
        assert!(watched.ended(), "process {pid} survived its view");
    }
}

use std::time::Duration;

/// A command whose leader exits normally still loses its descendants: the
/// kill-on-close Job Object, not the leader's lifetime, owns them.
#[test]
fn a_normal_exit_leaves_no_descendant_and_no_copy() {
    let _serial = serial();
    let fixture = Fixture::new();
    let helper = helper(&fixture.directory);
    let pids = fixture.directory.join("pids.txt");
    let command = format!("{} daemon {}", quoted(&helper), quoted(&pids));
    let (code, watched) = watching(
        &fixture,
        &pids,
        &[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &command,
            "app/src/more.rs",
        ],
    );
    assert_eq!(code, ExitCode::from(3), "the command exits 0: survived");
    assert_all_ended(&watched);
    fixture.assert_no_scratch();
}

/// A mutant that hangs is killed at its timeout together with the descendant
/// it spawned, and the copies are removed.
#[test]
fn a_timed_out_mutant_leaves_no_process_and_no_copy() {
    let _serial = serial();
    let fixture = Fixture::new();
    let helper = helper(&fixture.directory);
    let pids = fixture.directory.join("pids.txt");
    let command = format!("{} hang {}", quoted(&helper), quoted(&pids));
    let (code, watched) = watching(
        &fixture,
        &pids,
        &[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            &command,
            "app/src/more.rs",
        ],
    );
    assert_eq!(code, ExitCode::SUCCESS, "the timed-out mutant is killed");
    assert_all_ended(&watched);
    let snapshot = fs::read_to_string(fixture.root().join(".metrics/mutate/app/more.edn")).unwrap();
    assert!(snapshot.contains(":killed 1"), "{snapshot}");
    assert!(snapshot.contains(":survived 0"), "{snapshot}");
    fixture.assert_no_scratch();
}

/// Ported from `test_copy_refuses_directory_junctions...`: a linked directory
/// is a route back into the checkout, so the run refuses before any mutant.
#[test]
fn a_linked_directory_is_refused_before_any_command_runs() {
    let _serial = serial();
    let fixture = Fixture::new();
    let keep = fixture.root().join("keep.txt");
    fs::write(&keep, b"original").unwrap();
    let helper = helper(&fixture.directory);
    // A junction needs no privilege on Windows.
    let status = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/j"])
        .arg(fixture.root().join("bridge"))
        .arg(&fixture.directory)
        .status()
        .unwrap();
    assert!(status.success(), "create the directory junction");
    let previous = fixture.root().join(".metrics/mutate/app.edn");
    fs::create_dir_all(previous.parent().unwrap()).unwrap();
    fs::write(&previous, b"previous snapshot\n").unwrap();
    let command = format!("{} write", quoted(&helper));
    let code = fixture.run(&[
        "--no-coverage",
        "--mutate-all",
        "--test-command",
        &command,
        "app/src/lib.rs",
    ]);
    assert_eq!(
        code,
        ExitCode::from(1),
        "isolation refusal, not a test result"
    );
    assert_eq!(fs::read(&previous).unwrap(), b"previous snapshot\n");
    assert_eq!(fs::read(&keep).unwrap(), b"original");
    fs::remove_dir(fixture.root().join("bridge")).unwrap();
}

/// A file symlink becomes an independent file in the copy: writing through it
/// cannot reach the link's target.
#[test]
fn a_file_symlink_is_materialised_as_an_independent_file() {
    let _serial = serial();
    let fixture = Fixture::new();
    let keep = fixture.directory.join("keep.txt");
    fs::write(&keep, b"original").unwrap();
    let alias = fixture.root().join("alias.txt");
    if let Err(error) = std::os::windows::fs::symlink_file(&keep, &alias) {
        // A loud skip, and a failure with MUTATOR_REQUIRE_TOOLCHAINS=1 (CI):
        // a panic labelled SKIPPED read like a pass.
        if !require(false, &format!("this host cannot create file symlinks: {error}")) {
            return;
        }
    }
    let helper = helper(&fixture.directory);
    let pids = fixture.directory.join("pids.txt");
    let _ = pids;
    let command = format!("{} write-link", quoted(&helper));
    let code = fixture.run(&[
        "--no-coverage",
        "--mutate-all",
        "--test-command",
        &command,
        "app/src/more.rs",
    ]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(fs::read(&keep).unwrap(), b"original");
    fs::remove_file(&alias).unwrap();
}
