//! Windows argv and extensionless launcher behavior through process::output.
//! External-CLI descendant tests live in uml-viewer/tests/crap_process.rs.
#![cfg(any(windows, unix))]
#![allow(clippy::disallowed_methods)]

use quality_core::process::{Program, output};
#[cfg(windows)]
use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

fn directory() -> PathBuf {
    static SERIAL: AtomicU32 = AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!(
        "crapper-process-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[cfg(unix)]
#[test]
fn posix_normal_exit_ends_process_group_and_inherited_pipes() {
    let root = directory();
    let command = "sleep 120 & echo $! > descendant.pid; printf leader-output; printf leader-error >&2; exit 7";
    let start = std::time::Instant::now();
    let output = output(&Program::Shell(command), &root).unwrap();
    assert_eq!(Some(7), output.status.code());
    assert_eq!(b"leader-output", output.stdout.as_slice());
    assert_eq!(b"leader-error", output.stderr.as_slice());
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    let pid: i32 = fs::read_to_string(root.join("descendant.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let gone = unsafe { kill(pid, 0) } != 0;
    // A reparented descendant can briefly remain a zombie until init reaps it.
    let zombie = cfg!(target_os = "linux")
        && fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.split(") ")
                .nth(1)
                .is_some_and(|state| state.starts_with('Z'))
        });
    assert!(
        gone || zombie,
        "descendant survived POSIX process-group cleanup"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
fn explicit_extensionless_executable_and_native_arguments_survive_launch() {
    let root = directory();
    let target = root.join("tool").join("coverage helper 中.exe");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let source = root.join("argv.rs");
    fs::write(&source, r#"
use std::{env, fs, io::Write, process::{Command, Stdio}, thread, time::Duration};
fn main() {
    if env::args().nth(1).as_deref() == Some("sleep") { thread::sleep(Duration::from_secs(120)); return; }
    let bytes = env::args_os().skip(1).flat_map(|s| { let mut b = s.into_encoded_bytes(); b.push(0); b }).collect::<Vec<_>>();
    fs::write("argv.bin", &bytes).unwrap();
    let child = Command::new(env::current_exe().unwrap()).arg("sleep")
        .stdout(Stdio::inherit()).stderr(Stdio::inherit()).spawn().unwrap();
    fs::write("captured.pid", child.id().to_string()).unwrap();
    std::io::stdout().write_all(&[0, b'x', 0xff]).unwrap();
}
"#).unwrap();
    let status = std::process::Command::new("rustc")
        .arg("--edition=2024")
        .arg(&source)
        .arg("-o")
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success());
    let extensionless = target.with_extension("");
    let args = [
        "space here",
        "a&b",
        "a|b",
        "",
        "a\"b",
        "trail\\",
        "unicode中",
        "slash\\\"quote",
    ];
    let argv = std::iter::once(extensionless.to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| arg.to_string()))
        .collect::<Vec<_>>();
    let output = output(&Program::Argv(&argv), &root).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, [0, b'x', 0xff]);
    let expected: Vec<u8> = args
        .iter()
        .flat_map(|arg| {
            let mut bytes = arg.as_bytes().to_vec();
            bytes.push(0);
            bytes
        })
        .collect();
    assert_eq!(fs::read(root.join("argv.bin")).unwrap(), expected);
    let pid: u32 = fs::read_to_string(root.join("captured.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(ended(pid), "captured process descendant was not cleaned up");
    let _ = fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
fn batch_arguments_are_literal_through_percent_star() {
    let root = directory();
    fs::write(
        root.join("probe.cmd"),
        "@echo off\r\nnode -e \"console.log(JSON.stringify(process.argv.slice(1)))\" %*\r\n",
    )
    .unwrap();
    let args = [
        "%SPEC_LITERAL_ARG%",
        "space here",
        "a&b",
        "a|b",
        "a^b",
        "a!b",
        "",
        "a\"b",
        "trail\\",
        "quote\" & percent%CD% | unicode中",
        "slash\\\"quote",
    ];
    let argv = std::iter::once(root.join("probe.cmd").to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| arg.to_string()))
        .collect::<Vec<_>>();
    let output = output(&Program::Argv(&argv), &root).unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "{text}: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = format!(
        "[{}]\n",
        args.iter()
            .map(|arg| format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(text, expected);
    let _ = fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
fn explicit_extensionless_batch_path_uses_its_launcher() {
    let root = directory();
    fs::write(root.join("probe.cmd"), "@exit /b 0\r\n").unwrap();
    let argv = vec![root.join("probe").to_string_lossy().into_owned()];
    assert!(
        output(&Program::Argv(&argv), &root)
            .unwrap()
            .status
            .success()
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
fn installed_vitest_uses_its_native_node_entry_and_literal_argv() {
    if quality_core::process::which("node").is_none() {
        return;
    }
    let root = directory();
    let launcher = root.join("node_modules/.bin/vitest.cmd");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::write(&launcher, "@exit /b 99\r\n").unwrap();
    let package = root.join("node_modules/vitest");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"bin":{"vitest":"vitest.mjs"}}"#,
    )
    .unwrap();
    fs::write(
        package.join("vitest.mjs"),
        "console.log(JSON.stringify(process.argv.slice(2)))\n",
    )
    .unwrap();
    let args = ["run", "report %CD% & spaced", "a\"b"];
    let argv = std::iter::once(launcher.to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| arg.to_string()))
        .collect::<Vec<_>>();
    let output = output(&Program::Argv(&argv), &root).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[\"run\",\"report %CD% & spaced\",\"a\\\"b\"]\n"
    );
    let _ = fs::remove_dir_all(root);
}

/// `cmd.exe` reads its `/c` text from the raw command line, not from CRT argv:
/// quoting the switch (`"/c" "set"`) leaves a stray quote after it, so cmd runs
/// `"set` and exits 1. Mutator's environment probe reaches cmd through this
/// boundary, so quoting an argument that needs no quotes broke that probe.
#[cfg(windows)]
#[test]
fn cmd_slash_c_switch_reaches_cmd_unquoted() {
    capture_with_watchdog("cmd_slash_c_switch_reaches_cmd_unquoted", |root| {
        let argv = ["cmd.exe", "/d", "/c", "set"].map(str::to_string).to_vec();
        let output = output(&Program::Argv(&argv), root).unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            Some(0),
            output.status.code(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // `set` without a name prints the child's environment.
        assert!(
            stdout
                .to_ascii_lowercase()
                .lines()
                .any(|line| line.starts_with("path=")),
            "{stdout}"
        );
    });
}

#[cfg(windows)]
#[test]
fn windows_capture_drains_large_stdout_and_stderr_before_waiting() {
    capture_with_watchdog(
        "windows_capture_drains_large_stdout_and_stderr_before_waiting",
        |root| {
            // Each stream exceeds the pipe capacity independently. Reading only
            // stdout before waiting would still deadlock on stderr.
            let command =
                "for /l %i in (1,1,8192) do @(echo stdout-payload& 1>&2 echo stderr-payload)";
            let output = output(&Program::Shell(command), root).unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, b"stdout-payload\r\n".repeat(8192));
            assert_eq!(output.stderr, b"stderr-payload\r\n".repeat(8192));
        },
    );
}

/// Run capture in a disposable copy of this test process so a pipe deadlock
/// fails this test within 15 seconds instead of hanging the entire CI job.
#[cfg(windows)]
fn capture_with_watchdog(name: &str, check: impl FnOnce(&std::path::Path)) {
    const CHILD: &str = "CRAPPER_CAPTURE_WATCHDOG_CHILD";
    if let Some(root) = std::env::var_os(CHILD) {
        check(&PathBuf::from(root));
        return;
    }

    let root = directory();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(CHILD, &root)
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if started.elapsed() >= std::time::Duration::from_secs(15) {
            // Killing the helper closes its kill-on-close job and ends the
            // blocked command too. Do not join a thread stuck inside output.
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    };
    let _ = fs::remove_dir_all(root);
    assert!(
        status.is_some(),
        "captured output exceeded the 15-second watchdog"
    );
    assert!(status.unwrap().success(), "captured-output helper failed");
}

#[cfg(windows)]
fn ended(pid: u32) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn TerminateProcess(handle: *mut c_void, code: u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    unsafe {
        let handle = OpenProcess(0x0010_0001, 0, pid);
        if handle.is_null() {
            return true;
        }
        let gone = WaitForSingleObject(handle, 5_000) == 0;
        if !gone {
            TerminateProcess(handle, 1);
            WaitForSingleObject(handle, 5_000);
        }
        CloseHandle(handle);
        gone
    }
}
