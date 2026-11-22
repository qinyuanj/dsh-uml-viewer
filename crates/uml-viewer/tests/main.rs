#![allow(clippy::disallowed_methods)]

mod crap;
mod crap_changed;
mod crap_gates;
mod crap_goldens;
mod crap_languages;
mod crap_process;
mod crap_runner_smoke;
mod crap_stamp;
mod dispatch;
mod dry;
mod dry_goldens;
mod dry_languages;
mod ir_scope;
mod mutate;
mod mutate_lua;
mod mutate_windows;
mod quality;
mod quality_selection;
mod resident;
mod resident_overlays;
mod scene_cli;

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

pub fn binary<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    binary_env(args, &[])
}

pub fn binary_env<I, S>(args: I, environment: &[(&str, &str)]) -> Output
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
        .envs(environment.iter().copied())
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&scratch);
    output
}

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
