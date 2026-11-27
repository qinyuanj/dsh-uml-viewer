#![allow(clippy::disallowed_methods)]

use std::{
    ffi::OsString,
    fs,
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn pristine_rejects_zero_discovered_tests_without_external_python() {
    let _serial = crate::support::serial();
    let dir = std::env::temp_dir().join(format!(
        "mutator-seam-pristine-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        "pub fn flag(x: bool) -> i32 { if x { 1 } else { 0 } }\n",
    )
    .unwrap();
    let command = if cfg!(windows) {
        "echo Ran 0 tests in 0.001s"
    } else {
        "printf 'Ran 0 tests in 0.001s\\n'"
    };
    let args: Vec<OsString> = [
        "--root",
        &dir.to_string_lossy(),
        "--no-coverage",
        "--mutate-all",
        "--max-workers",
        "1",
        "--test-command",
        command,
        "src/lib.rs",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let code = mutator::run_with_clock(&args, || "2026-11-07T12:00:00+00:00".to_string());
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(code, ExitCode::from(2));
}
