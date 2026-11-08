//! Native Windows mutation: one fresh project copy per command, every process
//! contained by #3's kill-on-close Job Object (ADR 0010).
//!
//! Test code is the outside harness; it launches the fixture tools itself.
#![cfg(windows)]
#![allow(clippy::disallowed_methods)]
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The original writes `:isolation :copy` on Windows and `:namespace-view` on
/// Linux. Everything else in the snapshot is the committed golden's bytes.
fn golden_copy(file: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mutate/goldens")
        .join(file);
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.matches(":isolation :namespace-view").count(),
        1,
        "{file} must name exactly one isolation level"
    );
    text.replace(":isolation :namespace-view", ":isolation :copy")
        .into_bytes()
}

/// Every published snapshot under `.metrics/mutate`, by project-relative path.
fn snapshot_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, found);
            } else if path.extension().is_some_and(|ext| ext == "edn") {
                let key = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                found.insert(key, fs::read(&path).unwrap());
            }
        }
    }
    let base = root.join(".metrics/mutate");
    let mut found = BTreeMap::new();
    walk(&base, &base, &mut found);
    found
}

struct Fixture {
    directory: PathBuf,
    project: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "mutator-windows-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let project = directory.join("project");
        for crate_name in ["app", "util"] {
            let from = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/mutate/rust/project")
                .join(crate_name);
            let to = project.join(crate_name);
            fs::create_dir_all(to.join("src")).unwrap();
            fs::create_dir_all(to.join("tests")).unwrap();
            for part in [
                "Cargo.toml",
                "src/lib.rs",
                "src/more.rs",
                "tests/app.rs",
                "tests/below.rs",
            ] {
                let source = from.join(part);
                if source.is_file() {
                    fs::copy(&source, to.join(part)).unwrap();
                }
            }
        }
        fs::write(
            project.join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\", \"util\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        // A copy at the worker's fixed path has no `../../extdep`. The original
        // helper substitutes an absolute dependency for the same reason.
        let extdep =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/rust/extdep");
        let manifest = fs::read_to_string(project.join("app/Cargo.toml"))
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("extdep ="))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            project.join("app/Cargo.toml"),
            format!(
                "{manifest}\nextdep = {{ path = \"{}\" }}\n",
                quality_core::paths::posix(&extdep)
            ),
        )
        .unwrap();
        Self { directory, project }
    }
    fn root(&self) -> PathBuf {
        self.project.clone()
    }
    fn run(&self, extra: &[&str]) -> ExitCode {
        self.run_many(1, extra)
    }
    fn run_many(&self, workers: usize, extra: &[&str]) -> ExitCode {
        let mut args = vec![
            "--root".into(),
            self.root().into_os_string(),
            "--max-workers".into(),
            workers.to_string().into(),
        ];
        args.extend(extra.iter().map(std::ffi::OsString::from));
        mutator::run_with_clock(&args, || "2026-11-07T12:00:00+00:00".to_string())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let name = self
            .directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !self.directory.starts_with(std::env::temp_dir())
            || !name.starts_with("mutator-windows-")
        {
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

/// A scenario that runs no command must not pass silently.
fn require(condition: bool, reason: &str) {
    if !condition {
        panic!("SKIPPED: {reason}");
    }
}

/// A native helper the fixture commands run: it writes the LCOV the pinned
/// oracle's coverage produced, with `SF:` paths relative to its own working
/// directory, i.e. inside the copy, never in the real project.
const HELPER: &str = r#"
use std::{env, fs, path::Path};
fn main() {
    let mode = env::args().nth(1).unwrap();
    if mode == "coverage" {
        let mut text = String::new();
        for (file, line) in [("util/src/lib.rs", 2), ("app/src/lib.rs", 4), ("app/src/more.rs", 2)] {
            let cwd = env::current_dir().unwrap();
            let source = cwd.join(file).to_string_lossy().replace('\\', "/");
            text.push_str(&format!("SF:{source}\nDA:{line},1\nLH:1\nLF:1\nend_of_record\n"));
        }
        fs::create_dir_all("coverage").unwrap();
        fs::write("coverage/lcov.info", text).unwrap();
        return;
    }
    if mode == "write" {
        // Writes to the working directory, the parent, and every redirected
        // home, then fails: a mutant's writes must reach nobody else.
        let mut folders = vec![Path::new(".").to_path_buf(), Path::new("..").to_path_buf()];
        for name in ["HOME", "USERPROFILE", "TEMP", "TMP", "APPDATA", "LOCALAPPDATA"] {
            folders.push(Path::new(&env::var(name).unwrap()).to_path_buf());
        }
        for folder in &folders {
            if folder.join("leak").exists() {
                println!("saw an earlier mutant's write: {}", folder.display());
                return;
            }
        }
        for folder in &folders {
            fs::write(folder.join("leak"), "x").unwrap();
        }
        std::process::exit(1);
    }
}
"#;

/// Compile the helper once per fixture; commands are `cmd` strings, so the
/// executable path is quoted.
fn helper(directory: &Path) -> PathBuf {
    let source = directory.join("helper.rs");
    fs::write(&source, HELPER).unwrap();
    let executable = directory.join("helper.exe");
    let status = std::process::Command::new("rustc")
        .arg("--edition=2021")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .status()
        .unwrap();
    assert!(status.success(), "compile the native fixture helper");
    executable
}

fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// Real `cargo test` in per-command copies, with the coverage report a
/// deterministic command writes. Unlike the default-coverage scenario this
/// needs no LLVM component, so it runs on every Windows host.
#[test]
fn real_cargo_in_copies_publishes_the_original_bytes() {
    let fixture = Fixture::new();
    let helper = helper(&fixture.directory);
    let coverage = format!("{} coverage", quoted(&helper));
    let targets = [
        "--coverage-command",
        &coverage,
        "--mutate-all",
        "--timeout-factor",
        "40",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    assert_eq!(fixture.run(&targets), ExitCode::SUCCESS);
    for file in ["app.edn", "app/more.edn", "util.edn"] {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            golden_copy(file),
            "{file}"
        );
    }
    // The unchanged differential rerun keeps those exact bytes.
    let rerun = [
        "--coverage-command",
        &coverage,
        "--timeout-factor",
        "40",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    assert_eq!(fixture.run(&rerun), ExitCode::SUCCESS);
    for file in ["app.edn", "app/more.edn", "util.edn"] {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            golden_copy(file),
            "{file}"
        );
    }
    assert!(
        !fixture.root().join("target").exists(),
        "all Cargo/test artifacts belong to copies"
    );
}

#[test]
fn windows_copies_publish_the_original_bytes_with_copy_isolation() {
    // cargo llvm-cov needs llvm-profdata from the toolchain's llvm-tools.
    let llvm = PathBuf::from(std::env::var_os("RUSTUP_HOME").unwrap_or_default())
        .join("toolchains")
        .join("1.99.1-x86_64-pc-windows-msvc/lib/rustlib/x86_64-pc-windows-msvc/bin/llvm-profdata.exe");
    require(
        quality_core::process::which("cargo-llvm-cov").is_some() && llvm.is_file(),
        "cargo-llvm-cov and the pinned toolchain's llvm-tools must be installed",
    );
    let fixture = Fixture::new();
    let targets = [
        "--mutate-all",
        "--timeout-factor",
        "20",
        "util/src/lib.rs",
        "app/src/lib.rs",
        "app/src/more.rs",
    ];
    assert_eq!(fixture.run(&targets), ExitCode::SUCCESS);
    for file in ["app.edn", "app/more.edn", "util.edn"] {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            golden_copy(file),
            "{file}"
        );
    }
    assert_eq!(fixture.run(&targets[3..]), ExitCode::SUCCESS);
    for file in ["app.edn", "app/more.edn", "util.edn"] {
        assert_eq!(
            fs::read(fixture.root().join(".metrics/mutate").join(file)).unwrap(),
            golden_copy(file),
            "{file}"
        );
    }
    assert!(
        !fixture.root().join("target").exists(),
        "all Cargo/test artifacts belong to copies"
    );
}
