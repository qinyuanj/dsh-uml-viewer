use std::{
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub const DIRECTORY: &str = "target-cache";

const PREVIOUS: &str = "previous-mutation";

pub fn describe() -> &'static str {
    "a Cargo target directory per worker, kept outside each command's copy"
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildCache {
    directory: PathBuf,
}

impl BuildCache {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn remember(&self, relative: &Path) -> io::Result<()> {
        fs::write(
            self.directory.join(PREVIOUS),
            relative.to_string_lossy().as_bytes(),
        )
    }

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
        if !path.is_file() {
            return Ok(());
        }
        let file = fs::OpenOptions::new().write(true).open(&path)?;
        file.set_modified(SystemTime::now())
    }

    pub fn previous(&self) -> Option<PathBuf> {
        let note = fs::read_to_string(self.directory.join(PREVIOUS)).ok()?;
        (!note.is_empty()).then(|| PathBuf::from(note))
    }
}
