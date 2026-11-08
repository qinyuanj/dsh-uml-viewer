//! One integration test binary per crate: every binary here links the
//! whole dependency stack, so separate files relink it once each. A crate
//! that does this sets `autotests = false` and declares its files below.
// Test code is the outside harness: it launches the binary under test itself.
#![allow(clippy::disallowed_methods)]

mod dispatch;
mod resident;
mod scene_cli;

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Run the binary the way every test here does. A dispatch fault can route
/// any arguments to the viewer, which writes project state into its working
/// directory and opens a window; a private scratch directory and no display
/// keep such a run out of the tree under test and let it fail fast.
pub fn binary<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "uml-viewer-bin-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&scratch).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .args(args)
        .current_dir(&scratch)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&scratch);
    output
}

/// With `autotests = false` Cargo no longer finds test files on its own;
/// a file not declared as a module of its crate's `main.rs` would never run.
#[test]
fn every_test_file_is_a_module() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut missing = Vec::new();
    for krate in fs::read_dir(&crates).unwrap() {
        let tests = krate.unwrap().path().join("tests");
        let Ok(main) = fs::read_to_string(tests.join("main.rs")) else {
            continue;
        };
        for file in fs::read_dir(&tests).unwrap() {
            let path = file.unwrap().path();
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let is_rs = path.extension().is_some_and(|e| e == "rs");
            if is_rs && stem != "main" && !main.contains(&format!("mod {stem};")) {
                missing.push(path);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "not declared in tests/main.rs: {missing:?}"
    );
}
