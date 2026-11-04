//! The production companion link (#142, #161): the viewer side of the
//! companion worker (start, poll, stop, `wake` with the delivery id), and the
//! boundary `:display` mail loads diagrams through. The viewer session owns
//! the `to-viewer.edn` queue; a wake failure never loses mail (contract §5).

use std::path::{Path, PathBuf};

use companion::worker::{self, Worker};
use edn::Value;
use scene::document::{self, MailBoundary, ViewState};

use crate::viewer_session::{CompanionLink, LinkPoll, Stop};

/// The real worker behind the session's companion link. A launch without
/// `--backend`, or whose helper did not start, has no worker: mail is still
/// consumed, and a wake leaves the delivery to the durable queue.
pub struct Companion {
    worker: Option<Worker>,
    /// The launch diagnosis line (the Lua `mail_status`).
    status: Option<String>,
}

fn worker_error(error: companion::Failure) -> String {
    error.message
}

impl Companion {
    /// A backend launch: start the helper. The session built its view and
    /// mail cursor before calling this, so a `:display` written during
    /// startup is consumed rather than skipped (the Lua `Mailbox.last_id`
    /// capture).
    pub fn start(root: &Path, restart: bool, coding_agent: Option<&str>) -> Companion {
        match worker::start(&root.to_string_lossy(), restart, coding_agent) {
            Ok(started) => Companion {
                worker: Some(started),
                status: Some("Companion: starting".into()),
            },
            // The queue is authoritative; a viewer without a helper still
            // consumes mail, and the launch keeps the project's identity.
            Err(error) => Companion {
                worker: None,
                status: Some(format!("Companion: {}", worker_error(error))),
            },
        }
    }

    /// The mail queue without a helper process.
    pub fn mail_only() -> Companion {
        Companion {
            worker: None,
            status: None,
        }
    }
}

impl CompanionLink for Companion {
    fn launch_status(&self) -> Option<String> {
        self.status.clone()
    }

    /// Consume up to 32 worker events and report a phase change, and
    /// whether the worker reached `ready` on this poll.
    fn poll(&mut self) -> LinkPoll {
        let Some(worker) = &mut self.worker else {
            return LinkPoll::default();
        };
        let before = (worker.seen, worker.phase.clone());
        if let Err(error) = worker::poll(worker) {
            worker.phase = "failed".into();
            worker.message = Some(worker_error(error));
        }
        let changed =
            before.0 != worker.seen || worker.phase == "failed" || before.1 != worker.phase;
        let status = changed.then(|| match &worker.message {
            Some(message) => format!("Companion: {message}"),
            None => format!("Companion: {}", worker.phase),
        });
        LinkPoll {
            ready: changed && worker.phase == "ready" && before.1 != "ready",
            status,
        }
    }

    /// Ask the helper to stop, keeping the companion session or releasing it.
    fn stop(&mut self, stop: Stop) -> Result<(), String> {
        match &mut self.worker {
            Some(worker) => {
                worker::command(worker, "stop", stop == Stop::KeepCompanion).map_err(worker_error)
            }
            None => Ok(()),
        }
    }

    /// `Worker.command(companion,'wake',{id=cmd.id})`: the delivery id is the
    /// token the wake contract acknowledges. Without a helper the durable
    /// queue already holds the mail.
    fn wake(&mut self, id: i64) -> Result<(), String> {
        match &mut self.worker {
            Some(worker) => worker::wake(worker, id).map_err(worker_error),
            None => Ok(()),
        }
    }

    /// `true` once the helper settled (stopped or failed) and exited.
    fn settled(&self) -> bool {
        self.worker
            .as_ref()
            .is_none_or(|w| matches!(w.phase.as_str(), "stopped" | "failed") && w.exited)
    }
}

/// Loads a `:display` path exactly as a fresh document load would: the
/// metrics root is derived from the diagram (`metrics.root`), not from the
/// project, so a diagram elsewhere still joins its own snapshots.
pub struct DisplayBoundary {
    root: PathBuf,
}

impl DisplayBoundary {
    /// Relative `:display` paths resolve against the project `root`.
    pub fn new(root: &Path) -> DisplayBoundary {
        DisplayBoundary {
            root: root.to_path_buf(),
        }
    }
}

impl MailBoundary for DisplayBoundary {
    fn resolve(&self, path: &str, root: Option<&str>) -> String {
        if path.starts_with('/') {
            return path.to_owned();
        }
        let base = root
            .map(str::to_owned)
            .unwrap_or_else(|| self.root.to_string_lossy().into_owned());
        format!("{base}/{path}")
    }

    fn canonical(&self, path: &str) -> String {
        std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_owned())
    }

    fn load(&self, path: &str) -> Option<ViewState> {
        let text = std::fs::read_to_string(path).ok()?;
        let doc = scene::ir::read_document(&text).ok()?;
        let mtime = crate::watch::mtime(Path::new(path)).ok();
        let base = crate::metrics::root(Path::new(path));
        let (snapshots, stamp) = crate::metrics::snapshot(&base);
        let mut state = document::new_state(
            doc,
            mtime.map(Value::String),
            None,
            Some(&crate::session::view_options()),
            &snapshots,
            Some(stamp),
        );
        state.path = Some(path.to_owned());
        state.project_root = Some(self.root.to_string_lossy().into_owned());
        Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edn::record::get;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A fresh temporary directory per call: process id plus an in-process
    /// counter, so parallel tests never share one.
    fn scratch(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
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
        // POSIX names: `\` is an ordinary character and `C:` no drive.
        assert_eq!(
            format!("{root}/odd\\name.edn"),
            boundary.resolve("odd\\name.edn", None)
        );
        assert_eq!(
            format!("{root}/C:/x.edn"),
            boundary.resolve("C:/x.edn", None)
        );
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
    fn a_mail_only_link_wakes_stops_and_is_settled_without_a_helper() {
        let mut link = Companion::mail_only();
        assert_eq!(None, link.launch_status());
        assert!(link.wake(4).is_ok());
        assert!(link.settled());
        assert!(link.stop(Stop::ReleaseCompanion).is_ok());
        assert_eq!(LinkPoll::default(), link.poll());
    }
}
