//! Run one mutator call in a process of its own, where its stderr is real.
//!
//! libtest keeps a thread-local capture of the process's streams and hands it to
//! every thread a test spawns, so a run inside a test never reaches the real
//! stderr: `eprintln!` in the CLI lands in libtest's buffer and is printed only
//! if the test fails. A fresh process of this test binary, launched without that
//! capture, writes to the file the test hands it.
//!
//! `run` launches `run_driver`, which reads the arguments from the environment
//! and reports the run's exit code on stderr beside the CLI's own lines.
use std::{
    ffi::OsString,
    fs,
    path::Path,
    process::{Command, ExitCode},
};

/// The fixed clock the fixtures pin, as the CLI's own tests do.
pub const CLOCK: &str = "2026-11-07T12:00:00+00:00";

/// One argument per entry: no shell, no quoting, no separator collision.
pub const ROOT: &str = "MUTATOR_CHILD_ROOT";
pub const ARGS: &str = "MUTATOR_CHILD_ARGS";
const SEPARATOR: char = '\u{1f}';

/// The child half: run the call this process was given and report its exit code.
#[test]
fn run_driver() {
    let Some(root) = std::env::var_os(ROOT) else {
        return;
    };
    let mut args = vec![
        OsString::from("--root"),
        root,
        OsString::from("--max-workers"),
        OsString::from("1"),
    ];
    if let Ok(extra) = std::env::var(ARGS) {
        args.extend(extra.split(SEPARATOR).map(OsString::from));
    }
    let code = mutator::run_with_clock(&args, || CLOCK.to_string());
    eprintln!("MUTATOR_CHILD_EXIT={}", exit_number(code));
}

fn exit_number(code: ExitCode) -> u8 {
    for number in 0..=u8::MAX {
        if code == ExitCode::from(number) {
            return number;
        }
    }
    255
}

/// Run the mutator on `root` with `extra` in a child process of this binary,
/// and return its exit code with everything it wrote to stderr.
pub fn run(root: &Path, extra: &[&str]) -> (u8, String) {
    let stderr_path = root.with_extension(format!("stderr-{}", std::process::id()));
    let stderr = fs::File::create(&stderr_path).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "support::child::run_driver", "--nocapture"])
        .env(ROOT, root)
        .env(ARGS, extra.join(&SEPARATOR.to_string()))
        .stdout(std::process::Stdio::null())
        .stderr(stderr)
        .status()
        .unwrap();
    let text = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stderr_path);
    assert!(status.success(), "the driver process failed: {text}");
    let code = text
        .lines()
        .find_map(|line| line.strip_prefix("MUTATOR_CHILD_EXIT="))
        .and_then(|number| number.parse().ok())
        .unwrap_or_else(|| panic!("the driver reported no exit code: {text}"));
    (code, text)
}
