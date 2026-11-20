//! A Cargo target directory that outlives the copy a command runs in.
//!
//! Every command gets a fresh copy of the project (`crate::copies`), and that
//! copy's `target` directory is emptied with it, so Cargo recompiles the
//! mutated crate's whole dependency chain from scratch: with this repository's
//! `crate::scene`, nine crates per mutant. The fix is not to widen the copy but
//! to move the one directory Cargo writes outside the region `Slot::copy`
//! wipes, exactly where a Lua collection's `lua-coverage` scratch already sits
//! (`crate::slot`): kept by position, opened by absolute path, never by a name
//! the wipe skips. The copy itself stays a fresh copy, so a mutant's writes
//! still reach nothing a later command can see.
//!
//! Keeping the target directory is only sound because a copy keeps its files'
//! modification times: `std::fs::copy` preserves the last-write time on
//! Windows, so every crate that a mutant did not touch still looks older than
//! the last build and Cargo leaves it alone. The one file that must be forced
//! is the **previous** mutation: its restored bytes carry the original, older
//! time, so Cargo would call the crate fresh and hand the next command the
//! mutant's own object — a mutant judged on code that is not its own. Measured
//! on this repository: a source restored with its old time rebuilt 0 crates in
//! 0.25 s. `BuildCache` therefore remembers the last file a command mutated,
//! and the next command stamps it with the current time before running, which
//! is the one thing this module asks of the copy.
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// The name of the directory each worker's target directory occupies: one level
/// under the pool's scratch root, beside the `worker-<n>` directory that is
/// wiped, so no command's preparation can remove it.
pub const DIRECTORY: &str = "target-cache";

/// The note naming the source the last command mutated, and nothing else.
const PREVIOUS: &str = "previous-mutation";

/// The `Isolation:` line's companion: what a mutant's build reuses.
pub fn describe() -> &'static str {
    "a Cargo target directory per worker, kept outside each command's copy"
}

/// One worker's Cargo target directory, and the note that keeps it honest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildCache {
    directory: PathBuf,
}

impl BuildCache {
    /// The target directory at `directory`. The caller creates the directory and
    /// removes it with the pool; this type only names the position.
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// The `CARGO_TARGET_DIR` this worker's commands share.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Record the project-relative source `relative` that the command about to
    /// run mutates, so the command after it can force that crate.
    pub fn remember(&self, relative: &Path) -> io::Result<()> {
        fs::write(
            self.directory.join(PREVIOUS),
            relative.to_string_lossy().as_bytes(),
        )
    }

    /// Force the crate the previous command mutated to be rebuilt.
    ///
    /// The copy restored that file's original bytes with its original, older
    /// modification time, so `cargo`'s own check would call the crate fresh and
    /// reuse the object it built from the mutant. Stamping the file with the
    /// current time makes the check rebuild it from the bytes it now has.
    ///
    /// A file the previous command mutated and this one does not is forced
    /// exactly once; the note is dropped afterwards, because the rebuild Cargo
    /// then performs is what makes the target directory match the project again.
    pub fn force_previous(&self, project: &Path) -> io::Result<()> {
        let note = self.directory.join(PREVIOUS);
        let Ok(previous) = fs::read_to_string(&note) else {
            return Ok(());
        };
        fs::remove_file(&note)?;
        if previous.is_empty() {
            return Ok(());
        }
        let path = project.join(previous);
        // A source the project deleted or the copy skipped needs no forcing.
        if !path.is_file() {
            return Ok(());
        }
        let file = fs::OpenOptions::new().write(true).open(&path)?;
        file.set_modified(SystemTime::now())
    }

    /// What the last command mutated, for a test or a message.
    pub fn previous(&self) -> Option<PathBuf> {
        let note = fs::read_to_string(self.directory.join(PREVIOUS)).ok()?;
        (!note.is_empty()).then(|| PathBuf::from(note))
    }
}
