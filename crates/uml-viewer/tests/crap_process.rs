//! Native process-tree contract at the real `uml-viewer crap` CLI seam.
//! The coverage command and its descendants are Rust executables, not Python.
#![cfg(windows)]

use super::crap::{Project, canonical};
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn TerminateProcess(handle: *mut c_void, code: u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

// These handles pin the process identities, so a reused PID cannot pass a test
// or cause cleanup to terminate an unrelated process.
struct Descendant(*mut c_void);
impl Descendant {
    fn open(pid: u32) -> Self {
        let handle = unsafe { OpenProcess(0x0010_0001, 0, pid) }; // SYNCHRONIZE | PROCESS_TERMINATE
        if handle.is_null() {
            assert_eq!(Some(87), std::io::Error::last_os_error().raw_os_error());
        }
        Self(handle)
    }

    fn ended(&self) -> bool {
        self.0.is_null() || unsafe { WaitForSingleObject(self.0, 5_000) } == 0
    }
}
impl Drop for Descendant {
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
use std::{env, fs, process::{Command, Stdio}, thread, time::Duration};
fn main() {
    let mode = env::args().nth(1).unwrap();
    if mode == "sleep" {
        thread::sleep(Duration::from_secs(120));
        return;
    }
    let child = Command::new(env::current_exe().unwrap()).arg("sleep")
        .stdin(Stdio::null()).spawn().unwrap();
    fs::write("grandchild.pid", child.id().to_string()).unwrap();
    fs::write("leader.pid", std::process::id().to_string()).unwrap();
    println!("leader stdout");
    eprintln!("leader stderr");
    if mode == "wait" { thread::sleep(Duration::from_secs(120)); }
    fs::create_dir_all("coverage").unwrap();
    fs::write("coverage/lcov.info", "SF:src/a.rs\nFN:1,value\nFNDA:1,value\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n").unwrap();
}
"#;

fn launch(project: &Project, mode: &str) -> Cli {
    project.write("src/a.rs", "pub fn value() -> i32 { 42 }\n");
    let source = project.write("helper/process.rs", HELPER);
    let executable = project.root.join("helper/coverage helper 中.exe");
    let status = Command::new("rustc")
        .arg("--edition=2024")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .status()
        .unwrap();
    assert!(status.success(), "compile the native coverage fixture");
    let executable = canonical(&executable);
    let command = format!("\"{}\" {mode}", executable.display());
    let stdout = fs::File::create(project.root.join("cli.stdout")).unwrap();
    let stderr = fs::File::create(project.root.join("cli.stderr")).unwrap();
    Cli(Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("crap").arg("--root").arg(&project.root)
        .arg("--coverage-command").arg(command)
        .current_dir(&project.root)
        .stdout(stdout).stderr(stderr).spawn().unwrap())
}

fn pid(path: &Path) -> u32 {
    let start = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(pid) = text.parse()
        {
            return pid;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "PID never written: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn normal_coverage_shell_exit_ends_descendants_and_preserves_streams() {
    let project = Project::new();
    let mut cli = launch(&project, "exit");
    let descendant = Descendant::open(pid(&project.root.join("grandchild.pid")));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = cli.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "CLI did not return after shell exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let stderr = fs::read_to_string(project.root.join("cli.stderr")).unwrap();
    assert!(status.success(), "{stderr}");
    assert!(
        descendant.ended(),
        "coverage descendant survived normal shell exit"
    );
    assert!(
        fs::read_to_string(project.root.join("cli.stdout"))
            .unwrap()
            .contains("leader stdout")
    );
    assert!(stderr.contains("leader stderr"));
    assert!(project.snapshot().is_some());
}

#[test]
fn abrupt_cli_termination_ends_coverage_descendants() {
    let project = Project::new();
    let mut cli = launch(&project, "wait");
    let descendant = Descendant::open(pid(&project.root.join("grandchild.pid")));
    let leader = Descendant::open(pid(&project.root.join("leader.pid")));
    cli.0.kill().unwrap();
    let status = cli.0.wait().unwrap();
    assert!(!status.success());
    assert!(
        leader.ended(),
        "coverage shell survived abrupt CLI termination"
    );
    assert!(
        descendant.ended(),
        "coverage descendant survived abrupt CLI termination"
    );
}
