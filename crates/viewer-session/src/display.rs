//! How a displayed diagram loads (`:display`, from the companion's mail or
//! the plugin host's `display` action): exactly as a fresh document load,
//! with the metrics root derived from the diagram (`metrics::root`), not
//! from the project, so a diagram elsewhere still joins its own snapshots.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use edn::Value;
use scene::document::{self, MailBoundary, ViewState};

use crate::metrics;

pub struct DisplayBoundary {
    root: PathBuf,
    loaded_digest: RefCell<Option<String>>,
}

impl DisplayBoundary {
    /// Relative display paths resolve against the project `root`.
    pub fn new(root: &Path) -> DisplayBoundary {
        DisplayBoundary {
            root: root.to_path_buf(),
            loaded_digest: RefCell::new(None),
        }
    }
}

/// Hash the same string buffer the IR parser consumed (without re-reading its path).
pub(crate) fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

impl DisplayBoundary {
    pub(crate) fn loaded_digest(&self) -> Option<String> {
        self.loaded_digest.borrow().clone()
    }
}

impl MailBoundary for DisplayBoundary {
    fn resolve(&self, path: &str, root: Option<&str>) -> String {
        if path.starts_with('/') || Path::new(path).is_absolute() {
            return path.to_owned();
        }
        let base = root
            .map(str::to_owned)
            .unwrap_or_else(|| self.root.to_string_lossy().into_owned());
        format!("{base}/{path}")
    }

    fn canonical(&self, path: &str) -> String {
        frame::project_files::canonical(Path::new(path))
            .to_string_lossy()
            .into_owned()
    }

    fn load(&self, path: &str) -> Option<ViewState> {
        let text = std::fs::read_to_string(path).ok()?;
        let doc = scene::ir::read_document(&text).ok()?;
        let mtime = frame::watch::mtime(Path::new(path)).ok();
        let base = metrics::root(Path::new(path));
        let (snapshots, stamp) = metrics::snapshot(&base);
        let mut state = document::new_state(
            doc,
            mtime.map(Value::String),
            None,
            Some(&frame::session::view_options()),
            &snapshots,
            Some(stamp),
        );
        state.path = Some(path.to_owned());
        state.project_root = Some(self.root.to_string_lossy().into_owned());
        *self.loaded_digest.borrow_mut() = Some(digest(&text));
        Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edn::record::get;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh temporary directory per call: process id plus an in-process
    /// counter, so parallel tests never share one.
    fn scratch(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("{label} {} {n}", std::process::id()))
    }

    #[test]
    fn a_display_path_resolves_against_the_project_root_unless_absolute() {
        let directory = scratch("uml mail");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("second.edn"), "{:title \"Second\"}").unwrap();
        let boundary = DisplayBoundary::new(&directory);
        let root = directory.to_string_lossy().into_owned();
        assert_eq!(
            format!("{root}/second.edn"),
            boundary.resolve("second.edn", None)
        );
        assert_eq!("/abs/x.edn", boundary.resolve("/abs/x.edn", None));
        // POSIX names: `\` is an ordinary character and `C:` no drive (on
        // Windows `C:/x.edn` is absolute and kept as it is).
        assert_eq!(
            format!("{root}/odd\\name.edn"),
            boundary.resolve("odd\\name.edn", None)
        );
        #[cfg(unix)]
        assert_eq!(
            format!("{root}/C:/x.edn"),
            boundary.resolve("C:/x.edn", None)
        );
        #[cfg(windows)]
        assert_eq!("C:/x.edn", boundary.resolve("C:/x.edn", None));
        // A state's own project root wins when it names one.
        assert_eq!(
            "/other/second.edn",
            boundary.resolve("second.edn", Some("/other"))
        );
        let loaded = boundary
            .load(&boundary.resolve("second.edn", None))
            .unwrap();
        assert_eq!(
            Some(&Value::String("Second".into())),
            get(loaded.document.as_ref().unwrap(), "title")
        );
        assert_eq!(
            Some(directory.to_string_lossy().into_owned()),
            loaded.project_root
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_display_path_loads_under_the_launch_canonical_project_root() {
        // The shell canonicalizes the project root once; on Windows a
        // verbatim `\\?\C:\…` root would not accept the `/` join.
        let directory = scratch("uml mail canonical");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("second.edn"), "{:title \"Second\"}").unwrap();
        let root = frame::project_files::canonical(&directory);
        assert!(!root.to_string_lossy().starts_with(r"\\?\"), "{root:?}");
        let boundary = DisplayBoundary::new(&root);
        let loaded = boundary.load(&boundary.resolve("second.edn", None));
        assert!(loaded.is_some(), "second.edn under {root:?}");
        let _ = std::fs::remove_dir_all(directory);
    }
}
