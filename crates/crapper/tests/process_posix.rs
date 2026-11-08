//! The same captured process boundary ends POSIX descendants after leader exit.
#![cfg(unix)]
#![allow(clippy::disallowed_methods)]
use quality_core::process::{Program, capture};
use std::fs;

#[test]
fn captured_leader_exit_ends_descendants_and_closes_inherited_pipes() {
    let root = std::env::temp_dir().join(format!("crapper-process-posix-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let source = root.join("helper.rs");
    fs::write(&source, r#"
use std::{fs, process::{Command, Stdio}};
fn main() {
    if std::env::args().nth(1).is_some() { std::thread::sleep(std::time::Duration::from_secs(120)); return; }
    let child = Command::new(std::env::current_exe().unwrap()).arg("sleep")
        .stdout(Stdio::inherit()).stderr(Stdio::inherit()).spawn().unwrap();
    fs::write("child.pid", child.id().to_string()).unwrap();
    println!("leader stdout"); eprintln!("leader stderr");
}
"#).unwrap();
    let status = std::process::Command::new("rustc").arg("--edition=2024").arg(&source).arg("-o").arg(root.join("helper")).status().unwrap();
    assert!(status.success());
    let argv = vec![root.join("helper").to_string_lossy().into_owned()];
    let output = capture(&Program::Argv(&argv), &root).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"leader stdout\n");
    assert_eq!(output.stderr, b"leader stderr\n");
    let pid: i32 = fs::read_to_string(root.join("child.pid")).unwrap().parse().unwrap();
    unsafe { libc::kill(pid, 0); }
    // If the harness fails before reaping, this group is still owned by this
    // test and must not leave the sleeping helper behind.
    let handle = unsafe { libc::kill(-pid, 0) };
    if handle == 0 { unsafe { libc::kill(pid, libc::SIGKILL); } }
    let alive = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        unsafe { libc::kill(pid, 0) != 0 }
    });
    assert!(alive, "captured descendant survived leader exit");
    let _ = fs::remove_dir_all(root);
}
