#![allow(clippy::disallowed_methods)]

mod support;

mod crap;
mod crap_changed;
mod crap_gates;
mod crap_goldens;
mod crap_process;
mod crap_stamp;
mod dispatch;
mod dry;
mod dry_goldens;
mod dry_languages;
mod ir_scope;
mod mutate;
mod mutate_windows;
mod quality;
mod quality_selection;
mod resident;
mod resident_overlays;
mod scene_cli;

pub use support::{binary, binary_env};

use std::fs;
use std::path::Path;

#[test]
fn every_test_file_is_a_module() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut missing = Vec::new();
    let mut duplicated = Vec::new();
    for krate in fs::read_dir(&crates).unwrap() {
        let tests = krate.unwrap().path().join("tests");
        let Ok(main) = fs::read_to_string(tests.join("main.rs")) else {
            continue;
        };
        let contract = fs::read_to_string(tests.join("toolchain_contract.rs")).unwrap_or_default();
        for file in fs::read_dir(&tests).unwrap() {
            let path = file.unwrap().path();
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let is_rs = path.extension().is_some_and(|e| e == "rs");
            if !is_rs || matches!(stem.as_str(), "main" | "toolchain_contract") {
                continue;
            }
            let decl = format!("mod {stem};");
            let in_main = main.contains(&decl);
            let in_contract = contract.contains(&decl);
            if !in_main && !in_contract {
                missing.push(path);
            } else if in_main && in_contract && stem != "support" {
                duplicated.push(path);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "not declared in tests/main.rs or tests/toolchain_contract.rs: {missing:?}"
    );
    assert!(
        duplicated.is_empty(),
        "declared in both tests/main.rs and tests/toolchain_contract.rs: {duplicated:?}"
    );
}
