#![cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mutator-acceptance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        copy(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/rust"),
            &path,
        );
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("project")
    }
    fn run(&self, extra: &[&str]) -> ExitCode {
        let mut args = vec![
            OsString::from("--root"),
            self.root().into_os_string(),
            OsString::from("--max-workers"),
            OsString::from("1"),
        ];
        args.extend(extra.iter().map(OsString::from));
        mutator::run_with_clock(&args, || "2026-11-07T12:00:00+00:00".to_string())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn rust_snapshots_match_original_bytes_and_unchanged_differential_keeps_them() {
    let fixture = Fixture::new();
    let files = ["app.edn", "app/more.edn", "util.edn"];
    let targets = [
        "--mutate-all",
        "--timeout-factor",
        "20",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    assert_eq!(fixture.run(&targets), ExitCode::SUCCESS);
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/goldens");
    for file in files {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            fs::read(golden.join(file)).unwrap(),
            "{file}"
        );
    }
    assert_eq!(fixture.run(&targets[3..]), ExitCode::SUCCESS);
    for file in files {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            fs::read(golden.join(file)).unwrap(),
            "{file}"
        );
    }
    assert!(
        !fixture.root().join("target").exists(),
        "all Cargo/test artifacts belong to overlays"
    );
}

#[test]
fn baseline_and_mutant_writes_are_discarded_and_exit_contract_is_preserved() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.root().join("app/src/lib.rs")).unwrap();
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            "touch BASELINE; touch MUTANT; true",
            "app/src/lib.rs"
        ]),
        ExitCode::from(3)
    );
    assert!(!fixture.root().join("BASELINE").exists());
    assert!(!fixture.root().join("MUTANT").exists());
    assert_eq!(
        fs::read(fixture.root().join("app/src/lib.rs")).unwrap(),
        before
    );
    assert_eq!(
        fixture.run(&[
            "--no-coverage",
            "--mutate-all",
            "--test-command",
            "touch BASELINE; false",
            "app/src/lib.rs"
        ]),
        ExitCode::from(2)
    );
    assert!(!fixture.root().join("BASELINE").exists());
}

#[test]
fn custom_coverage_is_isolated_and_changed_digests_are_reexecuted() {
    let fixture = Fixture::new();
    let command = "mkdir -p coverage; touch COVERAGE_WRITE; printf 'SF:%s/app/src/lib.rs\\nDA:3,1\\nDA:4,1\\nDA:5,1\\nLF:3\\nLH:3\\nend_of_record\\n' \"$PWD\" > coverage/lcov.info";
    assert_eq!(
        fixture.run(&[
            "--coverage-command",
            command,
            "--mutate-all",
            "app/src/lib.rs"
        ]),
        ExitCode::SUCCESS
    );
    assert!(!fixture.root().join("COVERAGE_WRITE").exists());
    assert!(!fixture.root().join("coverage").exists());
    assert_eq!(
        fixture.run(&["--coverage-command", command, "app/src/lib.rs"]),
        ExitCode::SUCCESS
    );
    let path = fixture.root().join("app/src/lib.rs");
    let source = fs::read_to_string(&path).unwrap();
    fs::write(&path, source.replace("x + x", "x + x + 0")).unwrap();
    assert_eq!(
        fixture.run(&["--coverage-command", command, "app/src/lib.rs"]),
        ExitCode::from(3)
    );
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        source.replace("x + x", "x + x + 0")
    );
}

#[test]
fn default_coverage_write_is_also_discarded() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run(&["--mutate-all", "app/src/lib.rs"]),
        ExitCode::SUCCESS
    );
    assert!(!fixture.root().join("target").exists());
    assert!(fixture.root().join(".metrics/mutate/app.edn").is_file());
}
