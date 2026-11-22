use std::{
    ffi::OsString,
    fs,
    path::Path,
    process::{Command, ExitCode},
};

pub const CLOCK: &str = "2026-11-07T12:00:00+00:00";

pub const ROOT: &str = "MUTATOR_CHILD_ROOT";
pub const ARGS: &str = "MUTATOR_CHILD_ARGS";
const SEPARATOR: char = '\u{1f}';

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

#[allow(dead_code)]
pub fn run_with_stdout(root: &Path, extra: &[&str]) -> (u8, String, String) {
    let stdout_path = root.with_extension(format!("stdout-{}", std::process::id()));
    let stderr_path = root.with_extension(format!("stderr-{}", std::process::id()));
    let stdout = fs::File::create(&stdout_path).unwrap();
    let stderr = fs::File::create(&stderr_path).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "support::child::run_driver", "--nocapture"])
        .env(ROOT, root)
        .env(ARGS, extra.join(&SEPARATOR.to_string()))
        .stdout(stdout)
        .stderr(stderr)
        .status()
        .unwrap();
    let stderr_text = fs::read_to_string(&stderr_path).unwrap_or_default();
    let stdout_text = fs::read_to_string(&stdout_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    assert!(
        status.success(),
        "the driver process failed: {stderr_text}{stdout_text}"
    );
    let code = stderr_text
        .lines()
        .find_map(|line| line.strip_prefix("MUTATOR_CHILD_EXIT="))
        .and_then(|number| number.parse().ok())
        .unwrap_or_else(|| panic!("the driver reported no exit code: {stderr_text}"));
    (code, stdout_text, stderr_text)
}
