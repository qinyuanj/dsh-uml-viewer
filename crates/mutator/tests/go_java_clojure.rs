//! Go, Java and Clojure mutation runs against the pinned original's bytes.
//!
//! Linux/WSL is the acceptance platform. Java needs a JDK and Maven on PATH
//! with the fixture's dependencies already in the local repository; Clojure
//! needs a working `clj`; Go needs `go`. A missing toolchain is a loud SKIP
//! (and a failure when `MUTATOR_REQUIRE_TOOLCHAINS=1`), never a silent pass.
#![cfg(any(target_os = "linux", windows))]
#![allow(clippy::disallowed_methods)]
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const CLOCK: &str = "2026-11-07T12:00:00+00:00";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate")
}

/// Print a skip reason so it is visible even when libtest captures the test's
/// own stderr: write to the process's real stderr, not the captured buffer.
fn loud(message: &str) {
    use std::io::Write;
    let _ = std::io::stderr().write_all(message.as_bytes());
}

fn skip(reason: &str) {
    if std::env::var_os("MUTATOR_REQUIRE_TOOLCHAINS").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        panic!("required mutation toolchain unavailable: {reason}");
    }
    loud(&format!(
        "SKIP: {reason}; set MUTATOR_REQUIRE_TOOLCHAINS=1 to make this a failure\n"
    ));
}

/// A command that ran successfully.
fn works(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn toolchain(language: &str) -> bool {
    let available = match language {
        "go" => works("go", &["version"]),
        "java" => works("mvn", &["-v"]) && works("java", &["-version"]),
        "clojure" => works("clj", &["--version"]),
        _ => unreachable!(),
    };
    if !available {
        skip(&format!("the real {language} toolchain is not on PATH"));
    }
    available
}

struct Fixture {
    directory: PathBuf,
    project: PathBuf,
    language: &'static str,
}

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

fn files_under(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().unwrap().is_symlink() {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}

impl Fixture {
    fn new(language: &'static str) -> Option<Self> {
        if !toolchain(language) {
            return None;
        }
        Some(Self::copy_project(language))
    }

    fn copy_project(language: &'static str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "mutator-gjc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let project = directory.join("project");
        copy(&fixture_dir().join(language).join("project"), &project);
        Self {
            directory,
            project,
            language,
        }
    }

    fn targets(&self) -> Vec<&'static str> {
        match self.language {
            "go" => vec!["demo/calc.go", "widgets/widget.go"],
            "java" => vec!["src/main/java/demo/Board.java"],
            "clojure" => vec!["src/demo/core.clj"],
            _ => unreachable!(),
        }
    }

    fn run(&self, workers: usize, extra: &[&str]) -> ExitCode {
        let mut args = vec![
            OsString::from("--root"),
            self.project.clone().into_os_string(),
            OsString::from("--max-workers"),
            OsString::from(workers.to_string()),
            OsString::from("--timeout-factor"),
            OsString::from("20"),
        ];
        args.extend(extra.iter().map(OsString::from));
        mutator::run_with_clock(&args, || CLOCK.to_string())
    }

    fn goldens(&self) -> BTreeMap<String, Vec<u8>> {
        files_under(&fixture_dir().join(self.language).join("goldens"))
            .into_iter()
            .map(|(key, bytes)| {
                let bytes = if cfg!(windows) {
                    String::from_utf8(bytes)
                        .unwrap()
                        .replace(":isolation :namespace-view", ":isolation :copy")
                        .into_bytes()
                } else {
                    bytes
                };
                (key.replace('/', std::path::MAIN_SEPARATOR_STR), bytes)
            })
            .collect()
    }

    fn published(&self) -> BTreeMap<String, Vec<u8>> {
        files_under(&self.project.join(".metrics/mutate"))
    }

    fn assert_untouched(&self, before: &BTreeMap<String, Vec<u8>>) {
        let after = files_under(&self.project);
        let sources: BTreeMap<_, _> = after
            .iter()
            .filter(|(name, _)| !name.starts_with(".metrics/") && !name.starts_with(".metrics\\"))
            .collect();
        let before: BTreeMap<_, _> = before.iter().collect();
        assert_eq!(sources, before, "{} sources changed", self.language);
        for name in after.keys() {
            assert!(
                !name.starts_with("target/")
                    && !name.starts_with("coverage/")
                    && !name.contains("__pycache__")
                    && !name.contains(".cpcache"),
                "artifact leaked into the project: {name}"
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let name = self
            .directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !self.directory.starts_with(std::env::temp_dir()) || !name.starts_with("mutator-gjc-") {
            eprintln!(
                "Not removing unexpected fixture path {}",
                self.directory.display()
            );
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.directory) {
            eprintln!(
                "Cannot remove fixture {}: {error}",
                self.directory.display()
            );
        }
    }
}

/// Acceptance: snapshots equal the pinned original's bytes, and an unchanged
/// differential rerun keeps them.
fn golden_run(language: &'static str) {
    let Some(fixture) = Fixture::new(language) else {
        return;
    };
    let before = files_under(&fixture.project);
    let golden = fixture.goldens();
    assert!(!golden.is_empty(), "{language}: no goldens");
    let mut args = vec!["--mutate-all"];
    args.extend(fixture.targets());
    let first = fixture.run(1, &args);
    let published = fixture.published();
    assert_eq!(published, golden, "{language} first run bytes");
    // Surviving mutants in the goldens make this exit 3.
    assert_eq!(first, ExitCode::from(3), "{language} first run");
    fixture.assert_untouched(&before);
    let rerun = fixture.run(1, &fixture.targets());
    assert_eq!(rerun, ExitCode::from(3), "{language} rerun");
    assert_eq!(fixture.published(), golden, "{language} rerun bytes");
    fixture.assert_untouched(&before);
}

#[test]
fn go_snapshots_are_the_pinned_originals_bytes() {
    golden_run("go");
}

#[cfg(target_os = "linux")]
#[test]
fn java_snapshots_are_the_pinned_originals_bytes() {
    golden_run("java");
}

#[cfg(target_os = "linux")]
#[test]
fn clojure_snapshots_are_the_pinned_originals_bytes() {
    golden_run("clojure");
}

/// More workers, same bytes (the #9 scheduler serves the new languages too).
fn parallel_run(language: &'static str) {
    let Some(single) = Fixture::new(language) else {
        return;
    };
    let Some(parallel) = Fixture::new(language) else {
        return;
    };
    let mut args = vec!["--mutate-all"];
    args.extend(single.targets());
    single.run(1, &args);
    parallel.run(2, &args);
    assert_eq!(
        single.published(),
        parallel.published(),
        "{language} workers"
    );
    assert_eq!(
        parallel.published(),
        parallel.goldens(),
        "{language} golden"
    );
}

#[test]
fn go_workers_publish_the_same_bytes_as_one_worker() {
    parallel_run("go");
}

#[cfg(target_os = "linux")]
#[test]
fn java_workers_publish_the_same_bytes_as_one_worker() {
    parallel_run("java");
}

#[cfg(target_os = "linux")]
#[test]
fn clojure_workers_publish_the_same_bytes_as_one_worker() {
    parallel_run("clojure");
}

/// `--scan` inventories sites without coverage, tests or metrics.
#[test]
fn scan_writes_no_metrics() {
    for language in ["go", "java", "clojure"] {
        let fixture = Fixture::copy_project(language);
        let mut args = vec![
            OsString::from("--root"),
            fixture.project.clone().into_os_string(),
            OsString::from("--scan"),
        ];
        args.extend(fixture.targets().iter().map(OsString::from));
        assert_eq!(
            mutator::run_with_clock(&args, || CLOCK.to_string()),
            ExitCode::SUCCESS
        );
        assert!(!fixture.project.join(".metrics").exists(), "{language}");
        assert!(!fixture.project.join("target").exists(), "{language}");
        assert!(!fixture.project.join("coverage").exists(), "{language}");
    }
}
