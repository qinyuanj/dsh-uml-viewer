//! Native Windows: every command, coverage, baseline and mutant included,
//! runs in a fresh copy of the project at its worker slot (ADR 0010).
//!
//! The copy is the original's `CopyViews`: build output becomes an empty
//! directory, directory links and junctions are refused before a command runs,
//! file links are materialised as independent files, and `HOME`, `USERPROFILE`,
//! `TEMP`, `TMP`, `APPDATA` and `LOCALAPPDATA` point beside the copy. The
//! command's process tree is #3's kill-on-close Job Object in
//! `quality_core::process`; there is no second containment implementation.
use crate::{
    interrupts,
    isolation::{Completed, Failure},
    scratch,
};
use quality_core::process::{self, Ended, Program};
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
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
        let project = self.project();
        scratch::force_remove(&project)?;
        scratch::force_remove(&self.home())?;
        scratch::private_directory(&project)?;
        copy_tree(root, &project, &hide_build_output(root))?;
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

    pub fn run(
        &self,
        root: &Path,
        cwd: &Path,
        command: &str,
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        self.copy(root, mutation)
            .map_err(|error| Failure::Message(error.to_string()))?;
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
        environment.push((
            "MUTATOR_VIEW".into(),
            self.directory.clone().into_os_string(),
        ));
        let started = Instant::now();
        let running =
            process::start_supervised(&Program::Shell(command), &cwd, &environment, &output)
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

/// A report the command wrote inside the copy, never in the project.
pub fn report(slot: &Slot, relative: &Path) -> PathBuf {
    slot.project().join(relative)
}

fn message(error: io::Error) -> Failure {
    Failure::Message(error.to_string())
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
