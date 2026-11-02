//! One integration test binary per crate: every binary here links the
//! whole dependency stack, so separate files relink it once each. A crate
//! that does this sets `autotests = false` and declares its files below.

mod dispatch;
mod scene_cli;

use std::fs;
use std::path::Path;

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
