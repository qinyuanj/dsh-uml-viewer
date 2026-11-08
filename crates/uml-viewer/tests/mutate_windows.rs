//! Real `uml-viewer mutate` on native Windows: abrupt kill, console interrupt
//! and the abandoned-copy sweep, observed through process handles and the
//! filesystem, not through internals.
#![cfg(windows)]
#![allow(clippy::disallowed_methods)]

use std::{
    ffi::c_void,
    fs,
    path::PathBuf,
    process::{Child, Command},
    time::{Duration, Instant},
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn TerminateProcess(handle: *mut c_void, code: u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
}

/// A handle pins the identity, so a reused pid cannot pass or be killed.
struct Watched(*mut c_void);
impl Watched {
    fn open(pid: u32) -> Self {
        Self(unsafe { OpenProcess(0x0010_0001, 0, pid) })
    }
    fn ended(&self) -> bool {
        self.0.is_null() || unsafe { WaitForSingleObject(self.0, 15_000) } == 0
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

struct Cli(Child);
impl Drop for Cli {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const HELPER: &str = r#"
use std::{env, fs, io::Write, process::Command, time::Duration};
fn main() {
    let mode = env::args().nth(1).unwrap();
    if mode == "sleep" {
        std::thread::sleep(Duration::from_secs(120));
        return;
    }
    let child = Command::new(env::current_exe().unwrap()).arg("sleep")
        .stdin(std::process::Stdio::null()).spawn().unwrap();
    let mut file = fs::OpenOptions::new().create(true).append(true).open(arg(2)).unwrap();
    file.write_all(format!("{} {}\n", std::process::id(), child.id()).as_bytes()).unwrap();
    if Path::new(&env::var("MUTATOR_VIEW").unwrap()).join("mutation").is_file() {
        std::thread::sleep(Duration::from_secs(120));
    }
}
fn arg(n: usize) -> String { env::args().nth(n).unwrap() }
use std::path::Path;
"#;

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "mutate-win-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let project = directory.join("project");
        fs::create_dir_all(project.join("app/src")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        fs::write(
            project.join("app/Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            project.join("app/src/lib.rs"),
            "pub fn twice(x: i64) -> i64 {\n    x + x\n}\n",
        )
        .unwrap();
        let source = directory.join("helper.rs");
        fs::write(&source, HELPER).unwrap();
        let status = Command::new("rustc")
            .arg("--edition=2021")
            .arg(&source)
            .arg("-o")
            .arg(directory.join("helper.exe"))
            .status()
            .unwrap();
        assert!(status.success(), "compile the fixture helper");
        Self { directory }
    }
    fn project(&self) -> PathBuf {
        self.directory.join("project")
    }
    fn command(&self) -> String {
        format!(
            "\"{}\" hang \"{}\"",
            self.directory.join("helper.exe").display(),
            self.directory.join("pids.txt").display()
        )
    }
    fn spawn(&self, new_group: bool) -> Cli {
        use std::os::windows::process::CommandExt;
        let stdout = fs::File::create(self.directory.join("cli.stdout")).unwrap();
        let stderr = fs::File::create(self.directory.join("cli.stderr")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_uml-viewer"));
        command
            .arg("mutate")
            .arg("--root")
            .arg(self.project())
            .args(["--no-coverage", "--mutate-all", "--test-command"])
            .arg(self.command())
            .arg("app/src/lib.rs")
            .stdout(stdout)
            .stderr(stderr);
        if new_group {
            // A new process group is what GenerateConsoleCtrlEvent addresses.
            command.creation_flags(0x0000_0200);
        }
        Cli(command.spawn().unwrap())
    }
    fn pids(&self) -> Vec<u32> {
        fs::read_to_string(self.directory.join("pids.txt"))
            .map(|text| {
                text.split_whitespace()
                    .flat_map(str::parse::<u32>)
                    .collect()
            })
            .unwrap_or_default()
    }
    fn scratch_roots(&self, pid: u32) -> Vec<PathBuf> {
        let prefix = format!("mutator-{pid}-");
        fs::read_dir(std::env::temp_dir())
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let name = self
            .directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !self.directory.starts_with(std::env::temp_dir()) || !name.starts_with("mutate-win-cli-")
        {
            eprintln!("Not removing unexpected path {}", self.directory.display());
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

fn wait_for(condition: impl Fn() -> bool, what: &str) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// An abruptly killed CLI must not leave a test process behind, and its
/// abandoned copies are reclaimed by the next run.
#[test]
fn abrupt_cli_kill_ends_the_mutants_and_the_next_run_sweeps_the_copies() {
    let fixture = Fixture::new();
    let mut cli = fixture.spawn(false);
    wait_for(|| !fixture.pids().is_empty(), "the mutant started");
    let watched: Vec<Watched> = fixture
        .pids()
        .iter()
        .map(|pid| Watched::open(*pid))
        .collect();
    let pid = cli.0.id();
    unsafe {
        TerminateProcess(cli.0.as_raw_handle(), 1);
    }
    cli.0.wait().unwrap();
    for process in &watched {
        assert!(process.ended(), "a mutant survived the CLI's abrupt kill");
    }
    let abandoned = fixture.scratch_roots(pid);
    assert!(!abandoned.is_empty(), "the killed run left its copies");
    // The next call sweeps them before it does anything else.
    let output = crate::binary([
        "mutate",
        "--root",
        &fixture.project().to_string_lossy(),
        "--scan",
        "app/src/lib.rs",
    ]);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(fixture.scratch_roots(pid), Vec::<PathBuf>::new());
    // The unfinished file published nothing.
    assert!(!fixture.project().join(".metrics/mutate/app.edn").exists());
}

/// Ctrl+Break stops the run: the mutants' Job Objects are terminated, the
/// copies are removed and the exit is 128 + SIGBREAK.
#[test]
fn ctrl_break_interrupts_the_run_and_removes_the_copies() {
    let fixture = Fixture::new();
    let mut cli = fixture.spawn(true);
    // The handler is installed before this line is printed; we wait for either
    // of them as the signal that the CLI is ready to receive the event.
    wait_for(
        || {
            fs::read_to_string(fixture.directory.join("cli.stdout"))
                .is_ok_and(|text| text.contains("Isolation:"))
            || !fixture.pids().is_empty()
        },
        "the run armed its console handler",
    );
    wait_for(|| !fixture.pids().is_empty(), "the mutant started");
    let watched: Vec<Watched> = fixture
        .pids()
        .iter()
        .map(|pid| Watched::open(*pid))
        .collect();
    let pid = cli.0.id();
    let mut status = None;
    for attempt in 0..3 {
        if unsafe { GenerateConsoleCtrlEvent(1, pid) } == 0 {
            panic!(
                "SKIPPED: this session cannot deliver Ctrl+Break: {}",
                std::io::Error::last_os_error()
            );
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Some(code) = cli.0.try_wait().unwrap() {
                status = Some(code);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if status.is_some() {
            break;
        }
        assert!(attempt < 2, "Ctrl+Break did not stop the run");
    }
    let status = status.unwrap();
    assert_eq!(status.code(), Some(149), "128 + SIGBREAK(21)");
    let stderr = fs::read_to_string(fixture.directory.join("cli.stderr")).unwrap();
    assert!(stderr.contains("Interrupted by SIGBREAK"), "{stderr}");
    for process in &watched {
        assert!(process.ended(), "a mutant survived Ctrl+Break");
    }
    assert_eq!(fixture.scratch_roots(pid), Vec::<PathBuf>::new());
    assert!(!fixture.project().join(".metrics/mutate/app.edn").exists());
}

use std::os::windows::io::AsRawHandle;
