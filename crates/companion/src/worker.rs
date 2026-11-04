//! The companion worker (port of `love/uml/adapters/companion_worker.lua`).
//! One isolated process owns lifecycle side effects. The UI exchanges
//! generation-scoped files and never waits for subprocess stdout or cleanup.
//!
//! File protocol under `.uml-viewer/companion-worker-<token>.*`:
//! - `.request.edn`  `{:token :root :restart :base :viewer {:pid :start}}`,
//!   plus `:coding-agent :claude|:codex` when the launch named a provider
//! - `.command-<n>.edn` `{:token :operation :stop|:wake :extra ...}`
//! - `.event-<n>.edn` `{:token :phase :message}`, with phases `starting`,
//!   the progress stages, `ready`, `stopping`, `stopped` and `failed`.
//!
//! Every file is written to a temporary sibling, then renamed, through the
//! companion host. The viewer removes a generation's files once it settled
//! (terminal phase read, helper exited); a launch whose lock probe finds no
//! live helper first removes every older generation's files (#169).

use crate::agent::CodingAgent;
use crate::herdr::rules::{WakeVerdict, wake_verdict};
use crate::herdr::{Herdr, ProgressFn};
use crate::host::Host;
use crate::lifecycle;
use crate::mailbox::Mailbox;
use crate::posix;
use crate::record;
use crate::vocab::{Failure, Owner, Result};
use edn::Value;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// A companion that has not reached `ready` within this bound is cancelled;
/// ownership is retained (Lua `M.poll`).
pub const STARTUP_TIMEOUT_MS: i64 = 120_000;
const POLL_MS: i64 = 50;
/// How often delivered wakes are checked against the agent state; each check
/// is one Herdr snapshot.
const WATCH_MS: i64 = 1000;
const MAX_EVENTS_PER_POLL: usize = 32;

/// The unique token of one mail delivery. Herdr's wake contract requires the
/// literal prompt to carry it.
pub fn mail_token(id: i64) -> String {
    format!("mail-{id}")
}

fn write(host: &Host, path: &Path, value: &Value) -> Result<()> {
    // The destination is unique per generation, never replaced.
    (host.write)(path, edn::encode(value)?.as_bytes())
}

fn read(host: &Host, path: &Path) -> Option<Value> {
    let text = (host.read)(path).ok().flatten()?;
    edn::parse(&text).ok()
}

/// The per-project helper lock: held by a live helper for its whole run.
fn lock_path(root: &str) -> String {
    format!("{root}/.uml-viewer/companion-worker.lock")
}

/// The process identity without its name, as the worker and viewer compare
/// it (starttime only).
fn identity(host: &Host, pid: i32) -> Option<Owner> {
    (host.owner)(i64::from(pid)).map(|owner| Owner {
        name: String::new(),
        ..owner
    })
}

/// A started worker, as the viewer tracks it. It carries the companion host
/// it was started with; every later command and poll uses that host.
pub struct Worker {
    host: Host,
    pub base: String,
    pub token: String,
    pub owner: Owner,
    pub sequence: u64,
    pub seen: u64,
    pub phase: String,
    pub message: Option<String>,
    pub started: Option<i64>,
    pub timed_out: bool,
    pub exited: bool,
    /// This generation's protocol files were removed once it settled.
    pub reclaimed: bool,
}

/// Whether `rest`, the part of a file name after `<base>.`, names a protocol
/// file: `request.edn`, `command-<n>.edn` or `event-<n>.edn`.
fn protocol_suffix(rest: &str) -> bool {
    let numbered = |kind: &str| {
        rest.strip_prefix(kind)
            .and_then(|tail| tail.strip_suffix(".edn"))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    rest == "request.edn" || numbered("command-") || numbered("event-")
}

/// Remove the protocol files in `directory` whose names `claims` accepts.
/// Removal is best effort; a vanished file is not an error.
fn remove_protocol_files(host: &Host, directory: &Path, claims: impl Fn(&str) -> bool) {
    for name in (host.list)(directory) {
        if claims(&name) {
            (host.remove)(&directory.join(&name));
        }
    }
}

/// Remove this generation's protocol files: the request, every command and
/// every event under its base.
fn reclaim(worker: &mut Worker) {
    worker.reclaimed = true;
    let base = Path::new(&worker.base);
    let Some(stem) = base
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return;
    };
    let directory = base.parent().unwrap_or(Path::new(""));
    remove_protocol_files(&worker.host, directory, |name| {
        name.strip_prefix(stem.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(protocol_suffix)
    });
}

/// The launch-time reclaim of crashed or accumulated generations. Only a free
/// helper lock proves no helper is alive; it is released before the new
/// helper spawns, which takes it itself. A held or unopenable lock skips.
fn reclaim_stale(host: &Host, root: &str) {
    let Ok(Some(lock)) = posix::try_acquire(Path::new(&lock_path(root))) else {
        return;
    };
    let state = format!("{root}/.uml-viewer");
    remove_protocol_files(host, Path::new(&state), |name| {
        name.strip_prefix("companion-worker-")
            .and_then(|rest| rest.split_once('.'))
            .is_some_and(|(token, rest)| !token.is_empty() && protocol_suffix(rest))
    });
    posix::release(Some(lock));
}

/// The provider's short name for viewer status text: the recorded ownership
/// first, then the launch's explicit provider, else Claude.
pub fn agent_status_name(root: &str, agent: Option<CodingAgent>) -> &'static str {
    crate::ownership::read(root)
        .ok()
        .flatten()
        .filter(|record| *record != Value::Map(vec![]))
        .and_then(|record| crate::agent::identity_of(&record).ok())
        .map(|identity| identity.agent)
        .or(agent)
        .unwrap_or_default()
        .status_name()
}

/// The viewer-side start: write the request, then spawn the helper natively.
pub fn start(root: &str, restart: bool, agent: Option<CodingAgent>) -> Result<Worker> {
    let host = Host::posix();
    let token = (host.token)();
    let base = format!("{root}/.uml-viewer/companion-worker-{token}");
    let owner = identity(&host, posix::current_pid())
        .ok_or_else(|| Failure::new("viewer process identity is unavailable"))?;
    let mut fields = vec![
        ("token", Value::String(token.clone())),
        ("root", Value::String(root.to_owned())),
        ("restart", Value::Bool(restart)),
        ("base", Value::String(base.clone())),
        (
            "viewer",
            edn::record::record(vec![
                ("pid", Value::Integer(i64::from(owner.pid))),
                ("start", Value::String(owner.start.clone())),
            ]),
        ),
    ];
    fields.extend(agent.map(|agent| ("coding-agent", Value::keyword(agent.name()))));
    let request = edn::record::record(fields);
    reclaim_stale(&host, root);
    let request_path = format!("{base}.request.edn");
    write(&host, Path::new(&request_path), &request)?;
    let executable = std::env::current_exe()
        .map_err(|error| Failure::new(format!("cannot resolve the uml-viewer binary: {error}")))?;
    let argv = vec![
        std::ffi::OsString::from("companion-worker"),
        std::ffi::OsString::from(&request_path),
    ];
    let child = posix::spawn_native(executable.as_os_str(), &argv, Path::new(root))?;
    let started = Some((host.clock)());
    Ok(Worker {
        host,
        base,
        token,
        owner: child,
        sequence: 0,
        seen: 0,
        phase: "starting".to_owned(),
        message: None,
        started,
        timed_out: false,
        exited: false,
        reclaimed: false,
    })
}

/// Ask the helper to stop (`keep` retains the companion session).
pub fn command(worker: &mut Worker, operation: &str, keep: bool) -> Result<()> {
    worker.sequence += 1;
    let path = format!("{}.command-{}.edn", worker.base, worker.sequence);
    write(
        &worker.host,
        Path::new(&path),
        &edn::record::record(vec![
            ("token", Value::String(worker.token.clone())),
            ("operation", Value::keyword(operation)),
            (
                "extra",
                edn::record::record(vec![("keep", Value::Bool(keep))]),
            ),
        ]),
    )
}

/// Ask the helper to wake its pane for one mail delivery. The id is the
/// delivery token the wake contract acknowledges (contract §5); it must not
/// be the `:keep` shape of `stop`.
pub fn wake(worker: &mut Worker, id: i64) -> Result<()> {
    worker.sequence += 1;
    let path = format!("{}.command-{}.edn", worker.base, worker.sequence);
    write(
        &worker.host,
        Path::new(&path),
        &edn::record::record(vec![
            ("token", Value::String(worker.token.clone())),
            ("operation", Value::keyword("wake")),
            (
                "extra",
                edn::record::record(vec![("id", Value::Integer(id))]),
            ),
        ]),
    )
}

/// Consume up to 32 matching events, enforce the startup deadline once, and
/// notice a helper that exited without a final result. A stale generation
/// result is an error before the cursor advances. The poll that settles the
/// worker (terminal phase, helper exited) removes its protocol files once.
pub fn poll(worker: &mut Worker) -> Result<()> {
    let host = worker.host.clone();
    let start = worker.seen;
    while worker.seen - start < MAX_EVENTS_PER_POLL as u64 {
        let index = worker.seen + 1;
        let path = format!("{}.event-{index}.edn", worker.base);
        let Some(event) = read(&host, Path::new(&path)) else {
            break;
        };
        if record::get_name(&event, "token").as_deref() != Some(worker.token.as_str()) {
            return Err(Failure::new(
                "Companion helper result identity mismatch; ownership retained",
            ));
        }
        worker.seen = index;
        let phase = record::get_name(&event, "phase").unwrap_or_default();
        if !worker.timed_out || phase == "stopped" || phase == "failed" {
            worker.phase = phase.clone();
            worker.message = record::get_name(&event, "message");
            if phase == "ready" {
                worker.started = None;
            }
        }
    }
    let pending = worker.started.is_some()
        && !worker.timed_out
        && !matches!(worker.phase.as_str(), "ready" | "failed" | "stopped");
    if pending && (host.clock)() - worker.started.unwrap_or(0) > STARTUP_TIMEOUT_MS {
        worker.timed_out = true;
        worker.phase = "failed".to_owned();
        worker.message = Some(
            "Companion startup timed out; cancellation pending, ownership retained".to_owned(),
        );
        command(worker, "stop", false)?;
    }
    let observed = identity(&host, worker.owner.pid);
    let alive = observed.is_some_and(|observed| observed.start == worker.owner.start);
    if !alive {
        if !matches!(worker.phase.as_str(), "stopped" | "failed") {
            worker.phase = "failed".to_owned();
            worker.message = Some(
                "Companion helper exited without a final result; ownership retained".to_owned(),
            );
        }
        worker.exited = true;
    }
    // The moment the viewer's link settles: nothing will write or read this
    // generation again.
    if worker.exited && matches!(worker.phase.as_str(), "stopped" | "failed") && !worker.reclaimed {
        reclaim(worker);
    }
    Ok(())
}

/// The helper's generation state: one event journal, one command cursor and
/// the cancellation flags. The startup progress callback shares this state, so
/// a cancellation can land between create boundaries exactly as the Lua
/// worker's `progress` hook did.
struct Session {
    host: Host,
    base: String,
    token: String,
    sequence: u64,
    next_command: u64,
    stop: bool,
    keep: bool,
    pending: Vec<i64>,
    viewer_pid: i32,
    viewer_start: String,
}

impl Session {
    fn event(&mut self, phase: &str, message: Option<String>) -> Result<()> {
        self.sequence += 1;
        let mut entries = vec![
            ("token", Value::String(self.token.clone())),
            ("phase", Value::keyword(phase)),
        ];
        if let Some(message) = message {
            entries.push(("message", Value::String(message)));
        }
        write(
            &self.host,
            Path::new(&format!("{}.event-{}.edn", self.base, self.sequence)),
            &edn::record::record(entries),
        )
    }

    /// Drain every queued command, then notice a viewer that is gone.
    fn commands(&mut self) -> Result<()> {
        loop {
            let path = format!("{}.command-{}.edn", self.base, self.next_command);
            let Some(command) = read(&self.host, Path::new(&path)) else {
                break;
            };
            if record::get_name(&command, "token").as_deref() != Some(self.token.as_str()) {
                return Err(Failure::new("Helper command identity mismatch"));
            }
            self.next_command += 1;
            let extra = edn::record::get(&command, "extra")
                .cloned()
                .unwrap_or(Value::Nil);
            match record::get_name(&command, "operation").as_deref() {
                Some("stop") => {
                    self.stop = true;
                    self.keep = edn::record::is_true(&extra, "keep");
                }
                Some("wake") => {
                    if let Some(id) = record::get_i64(&extra, "id") {
                        self.pending.push(id);
                    }
                }
                _ => {}
            }
        }
        if self.viewer_pid < 1 {
            return Err(Failure::new(format!(
                "Invalid owned process PID: {}",
                self.viewer_pid
            )));
        }
        let viewer = identity(&self.host, self.viewer_pid);
        if !viewer.is_some_and(|viewer| viewer.start == self.viewer_start) {
            self.stop = true;
        }
        Ok(())
    }

    /// One create boundary: publish the stage, then honour a cancellation that
    /// arrived since the previous boundary.
    fn progress(&mut self, stage: &str) -> Result<()> {
        self.event(stage, None)?;
        self.commands()?;
        if self.stop {
            return Err(Failure::new("Companion startup cancelled"));
        }
        Ok(())
    }
}

/// The helper role body. `open_window` is `None` when the viewer asked for no
/// terminal attach (`UML_COMPANION_NO_ATTACH=1`).
pub fn run(request_path: &str, attach: bool, host: &Host) -> Result<i32> {
    let request = read(host, Path::new(request_path))
        .ok_or_else(|| Failure::new("Missing companion helper request"))?;
    let root = record::get_name(&request, "root").unwrap_or_default();
    let base = record::get_name(&request, "base").unwrap_or_default();
    let token = record::get_name(&request, "token").unwrap_or_default();
    let restart = edn::record::flag(&request, "restart");
    let viewer = edn::record::get(&request, "viewer")
        .cloned()
        .unwrap_or(Value::Nil);
    let session = Rc::new(RefCell::new(Session {
        host: host.clone(),
        base,
        token,
        sequence: 0,
        next_command: 1,
        stop: false,
        keep: false,
        pending: Vec::new(),
        viewer_pid: record::get_i64(&viewer, "pid").unwrap_or(0) as i32,
        viewer_start: record::get_name(&viewer, "start").unwrap_or_default(),
    }));
    let lock = posix::try_acquire(Path::new(&lock_path(&root)))?;
    if lock.is_none() {
        session.borrow_mut().event(
            "failed",
            Some("Another companion helper owns this project; ownership retained".to_owned()),
        )?;
        return Ok(1);
    }
    let progress: ProgressFn = {
        let session = session.clone();
        Rc::new(move |stage: &str| session.borrow_mut().progress(stage))
    };
    let outcome = requested_agent(&request)
        .and_then(|agent| run_body(&session, &root, restart, agent, attach, progress));
    let (stop, keep) = {
        let session = session.borrow();
        (session.stop, session.keep)
    };
    match outcome {
        Ok(()) => {}
        Err(error) => {
            if stop && !keep {
                // A completed record may be released only through the
                // verified cleanup contract; partial records remain.
                if lifecycle::required(&root, host).is_ok() {
                    match lifecycle::shutdown(&root, host) {
                        Ok(_) => {
                            session.borrow_mut().event(
                                "stopped",
                                Some("Startup cancelled; owned resources released".to_owned()),
                            )?;
                        }
                        Err(error) => {
                            session.borrow_mut().event(
                                "failed",
                                Some(format!("Startup cancelled: {error}; ownership retained")),
                            )?;
                        }
                    }
                } else {
                    session.borrow_mut().event(
                        "failed",
                        Some(format!("{error}; incomplete ownership retained")),
                    )?;
                }
            } else {
                // A start that released what it created, or never recorded
                // anything, retains nothing.
                let recorded = match crate::ownership::read_in(host, &root) {
                    Ok(None) => false,
                    Ok(Some(value)) => !record::is_empty_map(&value),
                    Err(_) => true,
                };
                let message = if recorded {
                    format!("{error}; ownership retained")
                } else {
                    error.to_string()
                };
                session.borrow_mut().event("failed", Some(message))?;
            }
            posix::release(lock);
            return Ok(1);
        }
    }
    posix::release(lock);
    Ok(0)
}

/// The provider the launch named, if any; the preflight already refused
/// anything else, so an unknown value here is a protocol fault.
fn requested_agent(request: &Value) -> Result<Option<CodingAgent>> {
    match edn::record::get(request, "coding-agent") {
        None => Ok(None),
        Some(Value::Keyword(name)) => CodingAgent::parse(name).map(Some),
        Some(_) => Err(crate::agent::unknown_provider()),
    }
}

/// The helper body. Startup stages arrive through the backend's progress
/// callback, so each stage is published at the boundary it names and a
/// cancellation can land between boundaries.
fn run_body(
    session: &Rc<RefCell<Session>>,
    root: &str,
    restart: bool,
    agent: Option<CodingAgent>,
    attach: bool,
    progress: ProgressFn,
) -> Result<()> {
    let host = session.borrow().host.clone();
    session.borrow_mut().event("starting", None)?;
    let (record_value, backend): (Value, Herdr) = if restart {
        lifecycle::restart(root, &host, agent)?
    } else {
        lifecycle::create(
            root,
            &host,
            lifecycle::CreateOptions {
                agent: agent.unwrap_or_default(),
                progress: Some(progress),
                ..lifecycle::CreateOptions::default()
            },
        )?
    };
    // A restart has no create boundaries, so drain once before publishing
    // ready; a cancellation or a vanished viewer still lands here.
    session.borrow_mut().commands()?;
    if session.borrow().stop {
        return Err(Failure::new("Companion startup cancelled"));
    }
    let ready = if !restart && attach {
        let name = record::get_name(&record_value, "session").unwrap_or_default();
        let opened = lifecycle::open_window(root, &name, &backend.herdr());
        if opened.terminal == "none" {
            Some(format!(
                "Companion started; attach manually: {}",
                opened.attach
            ))
        } else {
            None
        }
    } else {
        None
    };
    session.borrow_mut().event("ready", ready)?;
    let mailbox = Mailbox::outbox(Path::new(root));
    let pane = record::get_name(&record_value, "pane").unwrap_or_default();
    let owned = crate::agent::identity_of(&record_value)?.agent;
    let mut watched: Vec<(i64, i64)> = Vec::new();
    let mut watched_at = (host.clock)();
    loop {
        session.borrow_mut().commands()?;
        if !watched.is_empty() && (host.clock)() - watched_at >= WATCH_MS {
            watched_at = (host.clock)();
            // The state is read before the queue, so a mail popped by the
            // turn that just ended is never reported lost.
            match backend.agent_state(&pane) {
                Ok((status, seq)) => {
                    for id in settle(&mut watched, &status, seq, &|id| !mailbox.acknowledged(id)) {
                        session
                            .borrow_mut()
                            .event("ready", Some(lost_wake(owned, id)))?;
                    }
                }
                Err(error) => {
                    watched.clear();
                    session.borrow_mut().event(
                        "ready",
                        Some(format!("Companion wake check failed: {error}")),
                    )?;
                }
            }
        }
        let id = {
            let mut state = session.borrow_mut();
            if state.stop || state.pending.is_empty() {
                None
            } else {
                Some(state.pending.remove(0))
            }
        };
        let Some(id) = id else {
            if session.borrow().stop {
                break;
            }
            (session.borrow().host.sleep)(POLL_MS);
            continue;
        };
        let token = mail_token(id);
        let delivered = backend.wake(
            &pane,
            &["-l".to_owned(), format!("Mailbox {token}")],
            &token,
            &|token| mailbox.acknowledged_id(token),
        );
        match delivered {
            Ok(seq) => watched.push((id, seq)),
            Err(error) => session
                .borrow_mut()
                .event("ready", Some(format!("Companion wake failed: {error}")))?,
        }
    }
    session.borrow_mut().event("stopping", None)?;
    let keep = session.borrow().keep;
    if !keep {
        lifecycle::shutdown(root, &host)?;
    }
    session.borrow_mut().event("stopped", None)?;
    Ok(())
}

/// The status for a delivered wake whose turn ended with its mail queued.
fn lost_wake(agent: CodingAgent, id: i64) -> String {
    format!(
        "Companion wake lost: {} went idle with {} still queued",
        agent.status_name(),
        mail_token(id)
    )
}

/// Drop the delivered wakes whose mail left the queue or whose turn ended
/// with it still queued, and return the ids of the latter (contract §5).
fn settle(
    watched: &mut Vec<(i64, i64)>,
    status: &str,
    seq: i64,
    queued: &dyn Fn(i64) -> bool,
) -> Vec<i64> {
    let mut lost = Vec::new();
    watched.retain(
        |&(id, delivered)| match wake_verdict(delivered, status, seq, queued(id)) {
            WakeVerdict::Pending => true,
            WakeVerdict::Lost => {
                lost.push(id);
                false
            }
            WakeVerdict::Consumed => false,
        },
    );
    lost
}

impl Mailbox {
    /// Consumption proof by the literal token's mailbox id.
    fn acknowledged_id(&self, token: &str) -> bool {
        token
            .strip_prefix("mail-")
            .and_then(|id| id.parse::<i64>().ok())
            .is_some_and(|id| self.acknowledged(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[test]
    fn settling_keeps_pending_wakes_and_reports_only_lost_mail() {
        // mail 1 popped, mail 2 delivered at seq 4, mail 3 at seq 6.
        let mut watched = vec![(1, 2), (2, 4), (3, 6)];
        let lost = settle(&mut watched, "idle", 5, &|id| id != 1);
        assert_eq!(vec![2], lost);
        assert_eq!(vec![(3, 6)], watched);
        assert!(settle(&mut watched, "working", 7, &|_| true).is_empty());
        assert_eq!(vec![(3, 6)], watched);
    }

    #[test]
    fn a_lost_wake_names_the_provider_that_went_idle() {
        assert_eq!(
            "Companion wake lost: Claude went idle with mail-2 still queued",
            lost_wake(CodingAgent::Claude, 2)
        );
        assert_eq!(
            "Companion wake lost: Codex went idle with mail-3 still queued",
            lost_wake(CodingAgent::Codex, 3)
        );
    }

    #[test]
    fn names_a_mail_delivery_by_its_mailbox_id() {
        assert_eq!("mail-7", mail_token(7));
        assert_eq!("mail-12", mail_token(12));
    }

    struct Journal {
        files: Rc<RefCell<HashMap<String, Value>>>,
        events: Rc<RefCell<Vec<Value>>>,
    }

    fn host(
        journal: &Journal,
        owner: impl Fn(i32) -> Option<Owner> + 'static,
        now: Rc<RefCell<i64>>,
    ) -> Host {
        let files = journal.files.clone();
        let written = journal.events.clone();
        let write_files = journal.files.clone();
        let listed = journal.files.clone();
        let removed = journal.files.clone();
        Host {
            read: Rc::new(move |path| {
                let key = path.to_string_lossy().into_owned();
                match files.borrow().get(&key) {
                    Some(value) => edn::encode(value).map(Some).map_err(Failure::from),
                    None => Ok(None),
                }
            }),
            write: Rc::new(move |path, bytes| {
                let value = edn::parse(&String::from_utf8_lossy(bytes)).unwrap();
                written.borrow_mut().push(value.clone());
                // The helper journal is in-memory: a written command/event is
                // visible to the next read, exactly like the file protocol.
                write_files
                    .borrow_mut()
                    .insert(path.to_string_lossy().into_owned(), value);
                Ok(())
            }),
            // The journal is one flat directory.
            list: Rc::new(move |_| listed.borrow().keys().cloned().collect()),
            remove: Rc::new(move |path| {
                removed.borrow_mut().remove(&*path.to_string_lossy());
            }),
            owner: Rc::new(move |pid| owner(pid as i32)),
            clock: Rc::new(move || *now.borrow()),
            ..Host::fake()
        }
    }

    fn worker(host: Host) -> Worker {
        Worker {
            host,
            base: "journal".to_owned(),
            token: "generation".to_owned(),
            owner: Owner {
                pid: 2,
                start: "original".to_owned(),
                name: String::new(),
            },
            sequence: 0,
            seen: 0,
            phase: "starting".to_owned(),
            message: None,
            started: None,
            timed_out: false,
            exited: false,
            reclaimed: false,
        }
    }

    fn poll_with(events: Vec<Value>, observed: Option<Owner>) -> Worker {
        let journal = Journal {
            files: Rc::new(RefCell::new(HashMap::new())),
            events: Rc::new(RefCell::new(Vec::new())),
        };
        for (index, event) in events.into_iter().enumerate() {
            journal
                .files
                .borrow_mut()
                .insert(format!("journal.event-{}.edn", index + 1), event);
        }
        let now = Rc::new(RefCell::new(0i64));
        let host = host(&journal, move |_| observed.clone(), now);
        let mut worker = worker(host);
        poll(&mut worker).unwrap();
        worker
    }

    fn event(token: &str, phase: &str) -> Value {
        edn::record::record(vec![
            ("token", Value::String(token.to_owned())),
            ("phase", Value::keyword(phase)),
        ])
    }

    #[test]
    fn consumes_matching_startup_events_in_order() {
        let worker = poll_with(
            vec![event("generation", "server"), event("generation", "ready")],
            Some(Owner {
                pid: 2,
                start: "original".to_owned(),
                name: String::new(),
            }),
        );
        assert_eq!(2, worker.seen);
        assert_eq!("ready", worker.phase);
        assert!(!worker.exited);
    }

    #[test]
    fn rejects_stale_generation_results_before_advancing_the_cursor() {
        let journal = Journal {
            files: Rc::new(RefCell::new(HashMap::new())),
            events: Rc::new(RefCell::new(Vec::new())),
        };
        journal
            .files
            .borrow_mut()
            .insert("journal.event-1.edn".to_owned(), event("stale", "ready"));
        let now = Rc::new(RefCell::new(0i64));
        let host = host(
            &journal,
            |_| {
                Some(Owner {
                    pid: 2,
                    start: "original".to_owned(),
                    name: String::new(),
                })
            },
            now,
        );
        let mut worker = worker(host);
        let error = poll(&mut worker).unwrap_err();
        assert_eq!(
            "Companion helper result identity mismatch; ownership retained",
            error.message
        );
        assert_eq!(0, worker.seen);
        assert_eq!("starting", worker.phase);
    }

    #[test]
    fn reports_an_unknown_helper_exit_without_inventing_success() {
        let worker = poll_with(vec![], None);
        assert_eq!("failed", worker.phase);
        assert!(worker.exited);
        assert!(
            worker
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("without a final result")
        );
    }

    #[test]
    fn rejects_a_reused_helper_pid() {
        let worker = poll_with(
            vec![event("generation", "ready")],
            Some(Owner {
                pid: 2,
                start: "replacement".to_owned(),
                name: String::new(),
            }),
        );
        assert_eq!("failed", worker.phase);
        assert!(worker.exited);
    }

    #[test]
    fn preserves_a_published_failure_after_helper_exit() {
        let worker = poll_with(
            vec![edn::record::record(vec![
                ("token", Value::String("generation".to_owned())),
                ("phase", Value::keyword("failed")),
                (
                    "message",
                    Value::String("workspace failed; ownership retained".to_owned()),
                ),
            ])],
            None,
        );
        assert_eq!("failed", worker.phase);
        assert!(worker.exited);
        assert_eq!(
            Some("workspace failed; ownership retained".to_owned()),
            worker.message
        );
    }

    #[test]
    fn recognizes_a_confirmed_stop_after_helper_exit() {
        let worker = poll_with(vec![event("generation", "stopped")], None);
        assert_eq!("stopped", worker.phase);
        assert!(worker.exited);
    }

    fn alive() -> Option<Owner> {
        Some(Owner {
            pid: 2,
            start: "original".to_owned(),
            name: String::new(),
        })
    }

    /// A journal holding this generation's request, one command and the
    /// given events, beside another generation's files.
    fn journal_with(phases: &[&str]) -> Journal {
        let journal = Journal {
            files: Rc::new(RefCell::new(HashMap::new())),
            events: Rc::new(RefCell::new(Vec::new())),
        };
        let mut files = journal.files.borrow_mut();
        files.insert("journal.request.edn".to_owned(), Value::Nil);
        files.insert("journal.command-1.edn".to_owned(), Value::Nil);
        for (index, phase) in phases.iter().enumerate() {
            files.insert(
                format!("journal.event-{}.edn", index + 1),
                event("generation", phase),
            );
        }
        files.insert("other.event-1.edn".to_owned(), Value::Nil);
        files.insert("journal.lock".to_owned(), Value::Nil);
        drop(files);
        journal
    }

    fn names(journal: &Journal) -> Vec<String> {
        let mut names: Vec<String> = journal.files.borrow().keys().cloned().collect();
        names.sort();
        names
    }

    #[test]
    fn a_stopped_and_exited_generation_reclaims_its_protocol_files() {
        let journal = journal_with(&["ready", "stopping", "stopped"]);
        let host = host(&journal, |_| None, Rc::new(RefCell::new(0)));
        let mut worker = worker(host);
        poll(&mut worker).unwrap();
        assert_eq!("stopped", worker.phase);
        assert!(worker.exited);
        assert_eq!(vec!["journal.lock", "other.event-1.edn"], names(&journal));
    }

    #[test]
    fn a_failed_and_exited_generation_is_reclaimed_too() {
        let journal = journal_with(&["starting", "failed"]);
        let host = host(&journal, |_| None, Rc::new(RefCell::new(0)));
        let mut worker = worker(host);
        poll(&mut worker).unwrap();
        assert_eq!("failed", worker.phase);
        assert_eq!(vec!["journal.lock", "other.event-1.edn"], names(&journal));
    }

    #[test]
    fn an_unsettled_generation_keeps_its_protocol_files() {
        // Stop sent, `stopped` not yet published: the helper is alive.
        let journal = journal_with(&["ready", "stopping"]);
        let mut stopping = worker(host(&journal, |_| alive(), Rc::new(RefCell::new(0))));
        poll(&mut stopping).unwrap();
        assert_eq!(6, names(&journal).len());
        // `stopped` published, but the helper has not exited yet.
        let journal = journal_with(&["ready", "stopping", "stopped"]);
        let host = host(&journal, |_| alive(), Rc::new(RefCell::new(0)));
        let mut worker = worker(host);
        poll(&mut worker).unwrap();
        assert_eq!("stopped", worker.phase);
        assert_eq!(7, names(&journal).len());
    }

    #[test]
    fn a_settled_generation_is_reclaimed_only_once() {
        let journal = journal_with(&["stopped"]);
        let host = host(&journal, |_| None, Rc::new(RefCell::new(0)));
        let mut worker = worker(host);
        poll(&mut worker).unwrap();
        // A command written after settling (a late stop) stays for the next
        // launch's reclaim; a repeated poll does not delete again.
        journal
            .files
            .borrow_mut()
            .insert("journal.command-2.edn".to_owned(), Value::Nil);
        poll(&mut worker).unwrap();
        assert_eq!(
            vec!["journal.command-2.edn", "journal.lock", "other.event-1.edn"],
            names(&journal)
        );
    }

    fn state_dir(label: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "uml worker {label} {} {}",
            std::process::id(),
            crate::posix::monotonic_ms()
        ));
        std::fs::create_dir_all(directory.join(".uml-viewer")).unwrap();
        directory
    }

    /// Two old generations' protocol files beside files reclaim must keep.
    fn stale_files(state: &Path) -> Vec<&'static str> {
        let protocol = [
            "companion-worker-aaaa.request.edn",
            "companion-worker-aaaa.command-1.edn",
            "companion-worker-aaaa.event-1.edn",
            "companion-worker-aaaa.event-12.edn",
            "companion-worker-bbbb.request.edn",
            "companion-worker-bbbb.event-3.edn",
        ];
        let kept = vec![
            "companion-worker.lock",
            "companion.edn",
            "companion-worker-aaaa.event-x.edn",
            "companion-worker-.request.edn",
            "companion-worker-aaaa.notes.edn",
            "to-viewer.edn",
        ];
        for name in protocol.iter().chain(kept.iter()) {
            std::fs::write(state.join(name), "{}").unwrap();
        }
        kept
    }

    fn listing(state: &Path) -> Vec<String> {
        let mut names = (Host::posix().list)(state);
        names.sort();
        names
    }

    #[test]
    fn a_free_lock_lets_the_launch_reclaim_every_stale_generation() {
        let directory = state_dir("reclaim");
        let state = directory.join(".uml-viewer");
        let mut kept: Vec<String> = stale_files(&state).into_iter().map(str::to_owned).collect();
        kept.sort();
        reclaim_stale(&Host::posix(), &directory.to_string_lossy());
        assert_eq!(kept, listing(&state));
        // The probe released the lock, so the new helper can take it.
        let lock = posix::try_acquire(&state.join("companion-worker.lock")).unwrap();
        assert!(lock.is_some());
        posix::release(lock);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_held_lock_leaves_every_stale_generation_in_place() {
        let directory = state_dir("reclaim held");
        let state = directory.join(".uml-viewer");
        stale_files(&state);
        let before = listing(&state);
        let held = posix::try_acquire(&state.join("companion-worker.lock"))
            .unwrap()
            .expect("lock");
        reclaim_stale(&Host::posix(), &directory.to_string_lossy());
        assert_eq!(before, listing(&state));
        posix::release(Some(held));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn reclaim_without_a_state_directory_is_a_no_op() {
        let directory = state_dir("reclaim none");
        std::fs::remove_dir_all(&directory).unwrap();
        reclaim_stale(&Host::posix(), &directory.to_string_lossy());
        assert!(!directory.exists());
    }

    #[test]
    fn requests_cancellation_once_when_startup_exceeds_its_deadline() {
        let journal = Journal {
            files: Rc::new(RefCell::new(HashMap::new())),
            events: Rc::new(RefCell::new(Vec::new())),
        };
        let now = Rc::new(RefCell::new(120101i64));
        let host = host(
            &journal,
            |_| {
                Some(Owner {
                    pid: 2,
                    start: "original".to_owned(),
                    name: String::new(),
                })
            },
            now,
        );
        let mut worker = worker(host);
        worker.started = Some(100);
        poll(&mut worker).unwrap();
        poll(&mut worker).unwrap();
        assert!(worker.timed_out);
        assert_eq!("failed", worker.phase);
        assert!(
            worker
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("cancellation pending")
        );
        let cancels = journal
            .events
            .borrow()
            .iter()
            .filter(|event| record::get_name(event, "operation").as_deref() == Some("stop"))
            .count();
        assert_eq!(1, cancels);
        let extra = edn::record::get(journal.events.borrow().last().unwrap(), "extra")
            .cloned()
            .unwrap();
        assert_eq!(Some(&Value::Bool(false)), edn::record::get(&extra, "keep"));
    }

    #[test]
    fn ignores_late_ready_after_timeout_but_accepts_confirmed_cleanup() {
        let journal = Journal {
            files: Rc::new(RefCell::new(HashMap::new())),
            events: Rc::new(RefCell::new(Vec::new())),
        };
        journal.files.borrow_mut().insert(
            "journal.event-1.edn".to_owned(),
            event("generation", "ready"),
        );
        let now = Rc::new(RefCell::new(0i64));
        let host = host(
            &journal,
            |_| {
                Some(Owner {
                    pid: 2,
                    start: "original".to_owned(),
                    name: String::new(),
                })
            },
            now,
        );
        let mut worker = worker(host);
        worker.timed_out = true;
        worker.phase = "failed".to_owned();
        worker.message = Some("cancellation pending".to_owned());
        poll(&mut worker).unwrap();
        assert_eq!("failed", worker.phase);
        assert_eq!(Some("cancellation pending".to_owned()), worker.message);
        journal.files.borrow_mut().insert(
            "journal.event-2.edn".to_owned(),
            edn::record::record(vec![
                ("token", Value::String("generation".to_owned())),
                ("phase", Value::keyword("stopped")),
                (
                    "message",
                    Value::String("owned resources released".to_owned()),
                ),
            ]),
        );
        poll(&mut worker).unwrap();
        assert_eq!("stopped", worker.phase);
        assert_eq!(Some("owned resources released".to_owned()), worker.message);
    }

    #[test]
    fn disables_the_startup_deadline_after_ready() {
        let journal = Journal {
            files: Rc::new(RefCell::new(HashMap::new())),
            events: Rc::new(RefCell::new(Vec::new())),
        };
        journal.files.borrow_mut().insert(
            "journal.event-1.edn".to_owned(),
            event("generation", "ready"),
        );
        let now = Rc::new(RefCell::new(120101i64));
        let host = host(
            &journal,
            |_| {
                Some(Owner {
                    pid: 2,
                    start: "original".to_owned(),
                    name: String::new(),
                })
            },
            now,
        );
        let mut worker = worker(host);
        worker.started = Some(100);
        poll(&mut worker).unwrap();
        assert_eq!(None, worker.started);
        assert!(!worker.timed_out);
        journal.files.borrow_mut().insert(
            "journal.event-2.edn".to_owned(),
            event("generation", "stopping"),
        );
        poll(&mut worker).unwrap();
        assert_eq!("stopping", worker.phase);
        assert!(!worker.timed_out);
    }

    #[test]
    fn the_worker_file_protocol_writes_then_renames_every_event() {
        let directory = std::env::temp_dir().join(format!(
            "uml worker {} {}",
            std::process::id(),
            crate::posix::monotonic_ms()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("event-1.edn");
        let host = Host::posix();
        write(&host, &path, &event("generation", "ready")).unwrap();
        assert!(!directory.join("event-1.edn.tmp").exists());
        assert_eq!(
            Some("ready".to_owned()),
            record::get_name(&read(&host, &path).unwrap(), "phase")
        );
        assert!(read(&host, &directory.join("missing.edn")).is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    fn session_in(directory: &Path) -> Rc<RefCell<Session>> {
        let base = format!("{}/journal", directory.display());
        Rc::new(RefCell::new(Session {
            host: Host::posix(),
            base,
            token: "generation".to_owned(),
            sequence: 0,
            next_command: 1,
            stop: false,
            keep: false,
            pending: Vec::new(),
            viewer_pid: posix::current_pid(),
            viewer_start: identity(&Host::posix(), posix::current_pid())
                .expect("viewer identity")
                .start,
        }))
    }

    fn phases(directory: &Path) -> Vec<String> {
        let mut found: Vec<(u64, String)> = std::fs::read_dir(directory)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().to_string_lossy().into_owned();
                let index: u64 = name
                    .strip_prefix("journal.event-")?
                    .strip_suffix(".edn")?
                    .parse()
                    .ok()?;
                let value = read(&Host::posix(), &directory.join(&name))?;
                Some((index, record::get_name(&value, "phase")?))
            })
            .collect();
        found.sort();
        found.into_iter().map(|(_, phase)| phase).collect()
    }

    #[test]
    fn a_startup_stage_is_published_at_its_boundary_and_a_queued_stop_cancels() {
        let directory = std::env::temp_dir().join(format!(
            "uml worker progress {} {}",
            std::process::id(),
            crate::posix::monotonic_ms()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let session = session_in(&directory);
        session.clone().borrow_mut().progress("server").unwrap();
        // The command arrives while the workspace boundary is still being
        // created; it must land before the next stage runs.
        write(
            &Host::posix(),
            &directory.join("journal.command-1.edn"),
            &edn::record::record(vec![
                ("token", Value::String("generation".to_owned())),
                ("operation", Value::keyword("stop")),
                (
                    "extra",
                    edn::record::record(vec![("keep", Value::Bool(true))]),
                ),
            ]),
        )
        .unwrap();
        let error = session
            .clone()
            .borrow_mut()
            .progress("workspace")
            .unwrap_err();
        assert_eq!("Companion startup cancelled", error.message);
        assert!(session.borrow().stop);
        assert!(session.borrow().keep);
        assert_eq!(vec!["server", "workspace"], phases(&directory));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_wake_command_does_not_interrupt_the_startup_stages() {
        let directory = std::env::temp_dir().join(format!(
            "uml worker wake {} {}",
            std::process::id(),
            crate::posix::monotonic_ms()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let session = session_in(&directory);
        write(
            &Host::posix(),
            &directory.join("journal.command-1.edn"),
            &edn::record::record(vec![
                ("token", Value::String("generation".to_owned())),
                ("operation", Value::keyword("wake")),
                (
                    "extra",
                    edn::record::record(vec![("id", Value::Integer(4))]),
                ),
            ]),
        )
        .unwrap();
        session.clone().borrow_mut().progress("runner").unwrap();
        assert!(!session.borrow().stop);
        assert_eq!(vec![4i64], session.borrow().pending);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_request_carries_the_coding_agent_into_the_fresh_start() {
        let directory = std::env::temp_dir().join(format!(
            "uml worker provider {} {}",
            std::process::id(),
            crate::posix::monotonic_ms()
        ));
        std::fs::create_dir_all(directory.join(".uml-viewer")).unwrap();
        let bin = directory.join("bin");
        crate::preflight::fixtures::herdr(
            &bin,
            "0.9.5",
            22,
            crate::preflight::fixtures::CODEX_CURRENT,
        );
        let path = format!("{}:/definitely/empty", bin.display());
        let mut host = Host::posix();
        host.variable = Rc::new(move |name: &str| (name == "PATH").then(|| path.clone()));
        let root = directory.to_string_lossy().into_owned();
        for (generation, provider, expected) in [
            (
                "codex",
                Some(Value::keyword("codex")),
                "Codex CLI not found",
            ),
            (
                "pi",
                Some(Value::keyword("pi")),
                "unknown coding-agent provider",
            ),
        ] {
            let base = format!("{root}/.uml-viewer/companion-worker-{generation}");
            let request_path = format!("{base}.request.edn");
            let mut fields = vec![
                ("token", Value::String(generation.to_owned())),
                ("root", Value::String(root.clone())),
                ("base", Value::String(base.clone())),
                ("restart", Value::Bool(false)),
                (
                    "viewer",
                    edn::record::record(vec![
                        ("pid", Value::Integer(i64::from(posix::current_pid()))),
                        ("start", Value::String("start".to_owned())),
                    ]),
                ),
            ];
            fields.extend(provider.map(|value| ("coding-agent", value)));
            write(
                &host,
                Path::new(&request_path),
                &edn::record::record(fields),
            )
            .unwrap();
            assert_eq!(1, run(&request_path, false, &host).unwrap());
            let failed = (1..=3)
                .filter_map(|n| read(&host, Path::new(&format!("{base}.event-{n}.edn"))))
                .find(|event| record::get_name(event, "phase").as_deref() == Some("failed"))
                .expect("a failed event");
            let message = record::get_name(&failed, "message").unwrap_or_default();
            assert!(message.contains(expected), "{generation}: {message}");
            assert!(
                !crate::ownership::exists(&root),
                "{generation}: no ownership"
            );
            // Nothing was recorded, so nothing is claimed as retained.
            assert!(
                !message.ends_with("; ownership retained"),
                "{generation}: {message}"
            );
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_second_helper_for_the_same_project_is_refused() {
        let directory = std::env::temp_dir().join(format!(
            "uml worker lock {} {}",
            std::process::id(),
            crate::posix::monotonic_ms()
        ));
        std::fs::create_dir_all(directory.join(".uml-viewer")).unwrap();
        let root = directory.to_string_lossy().into_owned();
        let request_path = format!("{root}/.uml-viewer/companion-worker-gen.request.edn");
        write(
            &Host::posix(),
            Path::new(&request_path),
            &edn::record::record(vec![
                ("token", Value::String("gen".to_owned())),
                ("root", Value::String(root.clone())),
                (
                    "base",
                    Value::String(format!("{root}/.uml-viewer/companion-worker-gen")),
                ),
                ("restart", Value::Bool(false)),
                (
                    "viewer",
                    edn::record::record(vec![
                        ("pid", Value::Integer(i64::from(posix::current_pid()))),
                        ("start", Value::String("start".to_owned())),
                    ]),
                ),
            ]),
        )
        .unwrap();
        let held = posix::try_acquire(Path::new(&format!(
            "{root}/.uml-viewer/companion-worker.lock"
        )))
        .unwrap()
        .expect("lock");
        let code = run(&request_path, false, &Host::posix()).unwrap();
        assert_eq!(1, code);
        let event_path = format!("{root}/.uml-viewer/companion-worker-gen.event-1.edn");
        let event = read(&Host::posix(), Path::new(&event_path)).unwrap();
        assert_eq!(Some("failed".to_owned()), record::get_name(&event, "phase"));
        assert!(
            record::get_name(&event, "message")
                .unwrap_or_default()
                .contains("Another companion helper")
        );
        posix::release(Some(held));
        let _ = std::fs::remove_dir_all(&directory);
    }
}
