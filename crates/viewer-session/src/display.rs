//! How a displayed diagram loads (`:display`, from the companion's mail or
//! the plugin host's `display` action): exactly as a fresh document load,
//! with the metrics root derived from the diagram (`metrics::root`), not
//! from the project, so a diagram elsewhere still joins its own snapshots.
//! The path it records is the canonical one (`display_path`), because the
//! session state keeps that text and a restore must reproduce it exactly.

use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};

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

/// The filesystem path a display request names, in the one spelling a session
/// persists. A relative path joins the project root with `Path::join`, not a
/// formatted `/`: `std::fs` accepts `/` on Windows, so a formatted join is
/// stored as the view's path and only surfaces when a restore resolves the
/// same diagram under its native spelling. The result is canonicalized here,
/// once, so two spellings of one file (`./x.edn`, a `..` segment, a symlinked
/// or short-named root) cannot save two session states. A rooted or
/// drive/UNC-prefixed request names its own location by the Windows path
/// rules (`C:\x`, `\\srv\sh\x`, `\x` from the drive root) and is kept as
/// given: joining it would drop the base and silently resolve it relative.
fn display_path(base: &Path, path: &str) -> PathBuf {
    let given = Path::new(path);
    let named = match given.components().next() {
        Some(Component::Prefix(_) | Component::RootDir) => given.to_path_buf(),
        _ => base.join(given),
    };
    frame::project_files::canonical(&named)
}

impl MailBoundary for DisplayBoundary {
    fn resolve(&self, path: &str, root: Option<&str>) -> String {
        let base = match root {
            Some(root) => Path::new(root),
            None => self.root.as_path(),
        };
        display_path(base, path).to_string_lossy().into_owned()
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
        // A resolved path is a filesystem path, not a `/`-formatted string:
        // compare it as a path, and against the OS for the file itself, so the
        // assertion states "this file under the root" on both hosts.
        let resolved = boundary.resolve("second.edn", None);
        assert_eq!(
            std::fs::canonicalize(directory.join("second.edn")).unwrap(),
            std::fs::canonicalize(Path::new(&resolved)).unwrap(),
            "{resolved:?}"
        );
        assert_eq!(
            Path::new("/abs/x.edn"),
            Path::new(&boundary.resolve("/abs/x.edn", None))
        );
        // POSIX names: `\` is an ordinary character and `C:` no drive (on
        // Windows `C:/x.edn` is absolute and kept as it is).
        assert_eq!(
            Path::new(&root).join("odd\\name.edn"),
            Path::new(&boundary.resolve("odd\\name.edn", None))
        );
        #[cfg(unix)]
        assert_eq!(
            Path::new(&root).join("C:/x.edn"),
            Path::new(&boundary.resolve("C:/x.edn", None))
        );
        #[cfg(windows)]
        assert_eq!("C:/x.edn", boundary.resolve("C:/x.edn", None));
        // A state's own project root wins when it names one.
        assert_eq!(
            Path::new("/other").join("second.edn"),
            Path::new(&boundary.resolve("second.edn", Some("/other")))
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
        // The shell canonicalizes the project root once; a relative display
        // path must join and normalize under that same root on both hosts.
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

    /// The saved state is what a restart replays, so one displayed file has
    /// one persisted `diagram` path: two spellings of the same file must not
    /// save two states, and the path saved is the canonical one the restored
    /// session writes back (CI9 `windows-mutate`, fingerprint round trip).
    #[test]
    fn a_displayed_diagram_keeps_one_persisted_path_whichever_spelling_showed_it() {
        let directory = scratch("uml display persisted");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("second.edn"), "{:title \"Second\"}").unwrap();
        let root = frame::project_files::canonical(&directory);
        let mut session = crate::Session::open(
            root.clone(),
            root.join("diagram.edn"),
            crate::Start::Waiting,
        )
        .unwrap();
        let mut saved_with = |path: &str| {
            let mut command = Value::Map(vec![]);
            command.assoc(Value::keyword("op"), Value::keyword("display"));
            command.assoc(Value::keyword("path"), Value::String(path.to_owned()));
            session.apply_command(&command);
            session.saved_state()["diagram"].clone()
        };
        let absolute = root.join("second.edn");
        let plain = saved_with("second.edn");
        assert_eq!(
            plain,
            saved_with("./second.edn"),
            "`./` is the same file, so the saved state must be the same"
        );
        assert_eq!(
            plain,
            saved_with(&absolute.to_string_lossy()),
            "an absolute spelling is the same file"
        );
        assert_eq!(
            serde_json::Value::String(absolute.to_string_lossy().into_owned()),
            plain,
            "the persisted path is the canonical path a restore opens"
        );
        #[cfg(windows)]
        assert!(
            !plain.as_str().unwrap().contains('/'),
            "the persisted path is native, not a formatted join: {plain:?}"
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
