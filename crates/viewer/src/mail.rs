//! The production companion link (#142, #161): the viewer side of the
//! companion worker (start, poll, stop, `wake` with the delivery id). The
//! window's session owns the `to-viewer.edn` queue; a wake failure never
//! loses mail (contract §5). `:display` loads through
//! `viewer_session::display` (#17).

use std::path::Path;

use companion::CodingAgent;
use companion::worker::{self, Worker};

use crate::shell_session::{CompanionLink, LinkPoll, Stop};

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
    pub fn start(root: &Path, restart: bool, agent: Option<CodingAgent>) -> Companion {
        match worker::start(&root.to_string_lossy(), restart, agent) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
