//! Native Windows: every command, coverage, baseline and mutant included,
//! runs in a fresh copy of the project at its worker slot (ADR 0010).
//!
//! The copy is the original's `CopyViews`: build output becomes an empty
//! directory, directory links and junctions are refused before a command runs,
//! file links are materialised as independent files, and `HOME`, `USERPROFILE`,
//! `TEMP`, `TMP`, `APPDATA` and `LOCALAPPDATA` point beside the copy. The
//! command's process tree is #3's kill-on-close Job Object in
//! `quality_core::process`; there is no second containment implementation.
//!
//! A call's `--copy` paths are placed beside the copy at their real
//! project-relative position (ADR 0016): a dependency outside the project root,
//! such as the contracts a package imports from its repository's root, is
//! otherwise missing from the copy and the baseline fails before any mutant.
use crate::{
    interrupts,
    isolation::{self, Completed, Failure},
    scratch,
};
use quality_core::process::{self, Ended, Program};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

/// The original's `HIDDEN`: build output a copy shows as an empty directory.
const HIDDEN: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".idea",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    ".metrics",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".clj-kondo",
    ".gradle",
    ".uml-viewer",
    ".toolcache",
];

/// Home, profile, temp and app-data beside the copy, never inside it.
const REDIRECTED: &[(&str, &str)] = &[
    ("HOME", "home"),
    ("USERPROFILE", "home"),
    ("TEMP", "temp"),
    ("TMP", "temp"),
    ("APPDATA", "appdata/Roaming"),
    ("LOCALAPPDATA", "appdata/Local"),
];

pub fn describe() -> &'static str {
    "copy (a fresh copy of the project per command)"
}

/// The outside paths one call declared, keyed by the project root it runs on.
///
/// Every view of a call must show the same `--copy` paths, and views are created
/// in more places than the CLI reaches (coverage, baseline, mutants), so the set
/// is recorded once per call instead of threaded through each one. A root keeps
/// only its latest call: a call without `--copy` clears it, and a slot deletes
/// its previous command's copies first, so nothing is inherited.
pub fn remember_outside(root: &Path, outside: &[isolation::Outside]) {
    let mut registered = registry();
    if outside.is_empty() {
        registered.remove(root);
    } else {
        registered.insert(root.to_path_buf(), outside.to_vec());
    }
}

/// The outside paths this call declared for `root`; empty without `--copy`.
pub fn outside(root: &Path) -> Vec<isolation::Outside> {
    registry().get(root).cloned().unwrap_or_default()
}

fn registry() -> MutexGuard<'static, BTreeMap<PathBuf, Vec<isolation::Outside>>> {
    static OUTSIDE: OnceLock<Mutex<BTreeMap<PathBuf, Vec<isolation::Outside>>>> = OnceLock::new();
    OUTSIDE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One slot of the pool: a copy per command, plus the file a mutant carries.
pub struct Slot {
    directory: PathBuf,
}
impl Slot {
    pub fn reused(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
        }
    }
    fn project(&self) -> PathBuf {
        self.directory.join("project")
    }
    fn home(&self) -> PathBuf {
        self.directory.join("home")
    }
    fn mark(&self) -> PathBuf {
        self.directory.join("mutation")
    }

    /// Reset the slot and copy the project into it. `relative` and `data` are
    /// the mutated source this command must see, if any.
    fn copy(&self, root: &Path, mutation: Option<(&Path, &str)>) -> io::Result<()> {
        // The whole slot is fresh for every command, including the copy's
        // parent: a mutant's `..` write must not greet the next mutant.
        for entry in fs::read_dir(&self.directory)? {
            scratch::force_remove(&entry?.path())?;
        }
        let project = self.project();
        scratch::private_directory(&project)?;
        copy_tree(root, &project, &hide_build_output(root))?;
        for outside in outside(root) {
            self.place(&outside)?;
        }
        for (_, folder) in REDIRECTED {
            fs::create_dir_all(self.home().join(folder))?;
        }
        let mark = self.mark();
        scratch::force_remove(&mark)?;
        if let Some((path, source)) = mutation {
            scratch::private_file(&mark)?.write_all(source.as_bytes())?;
            let destination = path
                .strip_prefix(root)
                .map_err(|_| io::Error::other("the mutated source is outside the project root"))?;
            let destination = project.join(destination);
            // Cargo treats a source no newer than its last build as unchanged;
            // every copy is new, but the mutated file is rewritten last.
            fs::write(&destination, source)?;
        }
        Ok(())
    }

    /// How `cwd`, an absolute path in the project, maps into the copy.
    fn cwd(&self, root: &Path, cwd: &Path) -> io::Result<PathBuf> {
        let relative = cwd
            .strip_prefix(root)
            .map_err(|_| io::Error::other("the command's directory is outside the project"))?;
        Ok(self.project().join(relative))
    }

    /// Place one `--copy` path beside the copy, where the copy's `..` reaches it.
    ///
    /// The slot was just wiped, so the bytes are copied again from the real path
    /// for every command: a mutant's write to `..` cannot greet the next
    /// command, and an outside path that changed mid-call is seen as it is now.
    /// Nothing is left behind when the call ends, because the whole slot is
    /// inside the scratch root the pool removes.
    fn place(&self, outside: &isolation::Outside) -> io::Result<()> {
        let source = &outside.source;
        let destination = self.directory.join(&outside.relative);
        let target = format!("{} beside the copy", source.display());
        // The same link rules as the project copy: a linked directory is
        // refused, a linked file is materialised as independent bytes.
        if fs::metadata(source)
            .map_err(|error| cannot_place(&target, error))?
            .is_dir()
        {
            fs::create_dir_all(&destination).map_err(|error| cannot_place(&target, error))?;
            copy_tree(source, &destination, &[]).map_err(|error| cannot_place(&target, error))?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| cannot_place(&target, error))?;
            }
            fs::copy(source, &destination).map_err(|error| cannot_place(&target, error))?;
        }
        Ok(())
    }

    pub fn run(
        &self,
        root: &Path,
        cwd: &Path,
        command: &str,
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        self.run_program(&Program::Shell(command), root, cwd, mutation, timeout)
    }

    /// An argv command reaches the shared MSVCRT quoting of the process
    /// boundary instead of a shell string, as the original's argv plans do.
    pub fn run_argv(
        &self,
        root: &Path,
        cwd: &Path,
        argv: &[String],
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        self.run_program(&Program::Argv(argv), root, cwd, mutation, timeout)
    }

    fn run_program(
        &self,
        program: &Program,
        root: &Path,
        cwd: &Path,
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        self.copy(root, mutation).map_err(|error| {
            let text = error.to_string();
            if isolation_failure(&text) {
                Failure::Unavailable(text)
            } else {
                Failure::Message(text)
            }
        })?;
        let cwd = self
            .cwd(root, cwd)
            .map_err(|error| Failure::Message(error.to_string()))?;
        let output_path = self.directory.join("output");
        scratch::force_remove(&output_path).map_err(message)?;
        let output = scratch::private_file(&output_path).map_err(message)?;
        let home = self.home();
        let mut environment: Vec<(OsString, OsString)> = REDIRECTED
            .iter()
            .map(|(name, folder)| {
                (
                    OsString::from(name),
                    home.join(folder).as_os_str().to_os_string(),
                )
            })
            .collect();
        let copy = self.project();
        environment.push((
            "CARGO_TARGET_DIR".into(),
            copy.join("target/mutator-build").into_os_string(),
        ));
        // The installed toolchains stay reachable after the home moves.
        if std::env::var_os("CARGO_HOME").is_none() {
            let default = quality_core::paths::home().join(".cargo");
            if default.is_dir() {
                environment.push(("CARGO_HOME".into(), default.into_os_string()));
            }
        }
        if std::env::var_os("RUSTUP_HOME").is_none() {
            let default = quality_core::paths::home().join(".rustup");
            if default.is_dir() {
                environment.push(("RUSTUP_HOME".into(), default.into_os_string()));
            }
        }
        // JVM tools use java.user.home for dependency repositories. Cached
        // jars are independent copies; downloads and lockfiles stay beside
        // this worker, rather than writing to the user's Maven repository.
        let rendered = program.display();
        if rendered.contains("mvn") || rendered.contains("clj") || rendered.contains("clojure") {
            let jvm_home = home.join("home");
            let repository = quality_core::paths::home().join(".m2");
            if repository.is_dir() {
                let destination = jvm_home.join(".m2");
                fs::create_dir_all(&destination).map_err(message)?;
                copy_tree(&repository, &destination, &[]).map_err(message)?;
            }
            let previous = std::env::var("JAVA_TOOL_OPTIONS").unwrap_or_default();
            environment.push((
                "JAVA_TOOL_OPTIONS".into(),
                format!("{previous} -Duser.home=\"{}\"", jvm_home.display()).into(),
            ));
        }
        environment.push((
            "MUTATOR_VIEW".into(),
            self.directory.clone().into_os_string(),
        ));
        let started = Instant::now();
        let running = process::start_supervised_with_env_removals(
            program,
            &cwd,
            &environment,
            &crate::child_env::inherited_removals(),
            &output,
        )
        .map_err(message)?;
        drop(output);
        let code = match process::supervise(running, timeout, || interrupts::pending().is_some())
            .map_err(message)?
        {
            Ended::Exited(code) => code,
            Ended::Timeout => 124,
            Ended::Stopped => return Err(Failure::Interrupted),
        };
        Ok(Completed {
            code,
            elapsed: started.elapsed(),
            output: fs::read_to_string(output_path).unwrap_or_default(),
        })
    }
}

impl Slot {
    /// The copy's root: reports a command wrote live here, never in the project.
    pub fn project_root(&self) -> PathBuf {
        self.project()
    }
}

fn message(error: io::Error) -> Failure {
    Failure::Message(error.to_string())
}

/// Whether a copy failure is an isolation failure rather than a test result.
///
/// Freezing the project and placing an outside path both fail before any command
/// runs, so neither is a test outcome: the run exits 1 (isolation unavailable)
/// instead of 2 (a failing suite). Every message either step writes starts with
/// "Copy isolation cannot", and the placement failure names its path.
fn isolation_failure(text: &str) -> bool {
    text.starts_with("Copy isolation cannot")
}

/// A `--copy` path that could not be placed names itself: a missing dependency
/// must never look like a broken project.
fn cannot_place(target: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("Copy isolation cannot place {target}: {error}"),
    )
}

/// `copy_tree`: a hidden directory becomes empty, a linked directory is
/// refused (it could route a mutant back into the checkout), a file link is
/// materialised, and every other file is copied with its timestamps.
fn copy_tree(source: &Path, destination: &Path, hidden: &[PathBuf]) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&path)?;
        if hidden.contains(&path) {
            // Hidden build output is an empty directory, as in an overlay.
            fs::create_dir(&target)?;
            continue;
        }
        if is_link(&metadata) {
            if metadata.is_dir() {
                return Err(io::Error::other(format!(
                    "Copy isolation cannot freeze linked directory: {}",
                    path.display()
                )));
            }
            let resolved = fs::canonicalize(&path).map_err(|_| {
                io::Error::other(format!(
                    "Copy isolation cannot freeze unresolved link: {}",
                    path.display()
                ))
            })?;
            if !resolved.is_file() {
                return Err(io::Error::other(format!(
                    "Copy isolation cannot freeze unresolved link: {}",
                    path.display()
                )));
            }
            fs::copy(resolved, target)?;
        } else if metadata.is_dir() {
            fs::create_dir(&target)?;
            copy_tree(&path, &target, hidden)?;
        } else if metadata.is_file() {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// A symlink, or any reparse point (a junction too), as the original sees it.
fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
}

/// Build output under `directory` is hidden: the copy gets an empty directory.
pub fn hide_build_output(root: &Path) -> Vec<PathBuf> {
    HIDDEN
        .iter()
        .map(|name| root.join(name))
        .filter(|path| path.exists())
        .collect()
}
