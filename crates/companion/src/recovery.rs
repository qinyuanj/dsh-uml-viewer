//! Durable project recovery controller boundary (port of
//! `love/uml/adapters/recovery.lua`). The viewer owns only immutable
//! references; this module is the sole journal writer when running detached.
//! Journals are EDN; runner `started`/`exit` side files are JSON.

use crate::host::Host;
use crate::posix;
use crate::record;
use crate::vocab::{Failure, Result};
use edn::Value;
use std::path::{Path, PathBuf};

fn default_runner_state_path(identity: &Value, generation: i64) -> String {
    let cwd = record::get_name(identity, "cwd").unwrap_or_else(|| ".".to_owned());
    let session = record::get_name(identity, "session").unwrap_or_default();
    format!("{cwd}/.uml-viewer/{session}-runner-{generation}.edn")
}

/// Read a journal, retrying transient sharing violations for up to two
/// seconds while the controller replaces the file. An absent file is `None`;
/// a malformed journal is an error, never retried (Lua `M.read_state`).
pub fn read_state(path: &Path, host: &Host) -> Result<Option<Value>> {
    let deadline = (host.clock)() + 2000;
    loop {
        match (host.read)(path) {
            Ok(Some(text)) => return edn::parse(&text).map(Some).map_err(Failure::from),
            Ok(None) => return Ok(None),
            Err(error) => {
                let message = error.message.to_lowercase();
                if message.contains("no such file") {
                    return Ok(None);
                }
                if (host.clock)() < deadline {
                    (host.sleep)(25);
                } else {
                    return Err(Failure::new(format!(
                        "Could not read recovery journal {}: {}",
                        path.display(),
                        error.message
                    )));
                }
            }
        }
    }
}

/// Only a positively present, well-formed runner file with a phase is a
/// record; malformed runner state is no acknowledgement, never an exit cause.
pub fn read_runner(state: &Value, host: &Host) -> Result<Option<Value>> {
    let Some(path) = record::get_name(state, "runner-state") else {
        return Ok(None);
    };
    let Ok(Some(text)) = (host.read)(Path::new(&path)) else {
        return Ok(None);
    };
    let Ok(value) = edn::parse(&text) else {
        return Ok(None);
    };
    if record::get_name(&value, "phase").is_some() {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

pub fn write_state(path: &Path, state: &Value, host: &Host) -> Result<()> {
    (host.write)(path, edn::encode(state)?.as_bytes())
}

pub fn status(identity: &Value, host: &Host) -> Result<Option<Value>> {
    read_state(Path::new(&rules::state_file(identity)), host)
}

pub fn save(identity: &Value, state: &Value, host: &Host) -> Result<()> {
    write_state(Path::new(&rules::state_file(identity)), state, host)
}

/// Register the journal for a fresh controller; an existing journal refuses
/// replacement.
pub fn register(identity: &Value, controller: &Value, host: &Host) -> Result<()> {
    if status(identity, host)?.is_some() {
        return Err(Failure::new(
            "Recovery journal already exists; refusing replacement",
        ));
    }
    save(
        identity,
        &rules::initial(identity, controller, (host.clock)()),
        host,
    )
}

/// The registered controller identity must still be exactly what the journal
/// recorded (unless the phase is already terminal).
pub fn verify(identity: &Value, host: &Host) -> Result<Value> {
    let state = status(identity, host)?
        .ok_or_else(|| Failure::new("Recovery controller identity is missing or changed"))?;
    rules::matching(identity, &state)?;
    let controller = edn::record::get(&state, "controller")
        .filter(|controller| record::is_map(controller))
        .ok_or_else(|| Failure::new("Recovery process identity is incomplete"))?;
    let pid = record::get_i64(controller, "pid")
        .filter(|pid| *pid >= 1)
        .ok_or_else(|| Failure::new("Recovery process identity is incomplete"))?;
    if record::get_name(controller, "start").is_none() {
        return Err(Failure::new("Recovery process identity is incomplete"));
    }
    let phase = record::get_name(&state, "phase").unwrap_or_default();
    if !rules::terminal(&phase) {
        let start = record::get_name(controller, "start").unwrap_or_default();
        let observed = (host.owner)(pid);
        let matches = observed
            .as_ref()
            .is_some_and(|observed| i64::from(observed.pid) == pid && observed.start == start);
        if !matches {
            return Err(Failure::new(
                "Recovery controller disappeared; refusing replacement",
            ));
        }
    }
    Ok(state)
}

/// Start and cancel share the OS gate lock (Lua `M.with_gate`).
pub fn with_gate<T>(identity: &Value, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let gate = posix::acquire(Path::new(&rules::gate_file(identity)))?;
    let result = action();
    posix::release(Some(gate));
    result
}

/// Durable cancel: verify, journal the cancel under the gate, then wait for
/// the controller to confirm by reaching a terminal phase and exiting.
pub fn cancel(identity: &Value, host: &Host) -> Result<()> {
    verify(identity, host)?;
    let reference = rules::reference(identity);
    let control_path = PathBuf::from(rules::control_file(identity));
    with_gate(identity, || {
        write_state(
            &control_path,
            &edn::record::record(vec![(
                "cancel",
                edn::record::get(&reference, "token")
                    .cloned()
                    .unwrap_or(Value::Nil),
            )]),
            host,
        )
    })?;
    let deadline = (host.clock)() + 10000;
    loop {
        let state = verify(identity, host)?;
        let controller = edn::record::get(&state, "controller").unwrap();
        let pid = record::get_i64(controller, "pid").unwrap_or(0);
        let start = record::get_name(controller, "start").unwrap_or_default();
        // A reused PID is not the registered controller: only the exact
        // original creation identity proves the controller is still running.
        let absent = match (host.owner)(pid) {
            None => true,
            Some(observed) => observed.start != start,
        };
        let phase = record::get_name(&state, "phase").unwrap_or_default();
        if rules::terminal(&phase) && absent {
            return Ok(());
        }
        if (host.clock)() < deadline {
            (host.sleep)(25);
        } else {
            return Err(Failure::new(format!(
                "Recovery cancellation was not acknowledged; resources retained (phase {}, controller {})",
                if phase.is_empty() { "unknown" } else { &phase },
                if absent { "absent" } else { "alive" }
            )));
        }
    }
}

/// Completed lifecycle journals retire; a live one refuses.
pub fn retire_completed(identity: &Value, host: &Host) -> Result<Option<()>> {
    let Some(existing) = status(identity, host)? else {
        return Ok(None);
    };
    let phase = record::get_name(&existing, "phase").unwrap_or_default();
    if !rules::terminal(&phase) {
        return Err(Failure::new("Recovery registration already exists"));
    }
    (host.remove)(Path::new(&rules::state_file(identity)));
    (host.remove)(Path::new(&rules::control_file(identity)));
    Ok(Some(()))
}

pub struct Step<'a> {
    pub identity: &'a Value,
    pub now: i64,
    pub host: &'a Host,
    pub launch: &'a dyn Fn(&Value) -> Result<()>,
    /// Whether Herdr reports the saved session in the pane yet; an error
    /// means it reports another one.
    pub verify: &'a dyn Fn(&Value) -> Result<bool>,
}

/// One controller step (Lua `M.step`): journal before the external side
/// effect; a persisted `:submitting` is never replayed after a rebind.
pub fn step(step: &Step) -> Result<Value> {
    let identity = step.identity;
    let host = step.host;
    let state = read_state(Path::new(&rules::state_file(identity)), host)?;
    let control = read_state(Path::new(&rules::control_file(identity)), host)?;
    let event = match &state {
        Some(state) => read_runner(state, host)?,
        None => None,
    };
    let runner_state_fn = |generation: i64| default_runner_state_path(identity, generation);
    let owner = host.owner.clone();
    let owner_ref = move |pid: i64| owner(pid);
    let token = host.token.clone();
    let token_ref = move || token();
    let decision = rules::decide(&rules::Context {
        identity,
        state: state
            .as_ref()
            .ok_or_else(|| Failure::new("Recovery controller identity is missing or changed"))?,
        control: control.as_ref(),
        event: event.as_ref(),
        now: step.now,
        owner: &owner_ref,
        token: &token_ref,
        runner_state: &runner_state_fn,
    })?;
    let state_path = PathBuf::from(rules::state_file(identity));
    if !decision.launch {
        let checked = if rules::session_unproven(&decision.state) {
            let check = (step.verify)(identity);
            rules::session_checked(identity, &decision.state, check, step.now)
        } else {
            decision.state.clone()
        };
        if decision.changed || checked != decision.state {
            write_state(&state_path, &checked, host)?;
        }
        return Ok(checked);
    }
    write_state(&state_path, &decision.state, host)?;
    let mut submission = identity.clone();
    if let Some(token) = edn::record::get(&decision.state, "runner-token") {
        edn::record::put(&mut submission, "runner-token", token.clone());
    }
    if let Some(path) = edn::record::get(&decision.state, "runner-state") {
        edn::record::put(&mut submission, "runner-state", path.clone());
    }
    let mut result = decision.state.clone();
    match (step.launch)(&submission) {
        Ok(()) => {
            edn::record::put(&mut result, "phase", Value::keyword("running"));
        }
        Err(error) => {
            edn::record::put(&mut result, "phase", Value::keyword("failed"));
            edn::record::put(
                &mut result,
                "reason",
                Value::String(format!("Recovery submission uncertain: {}", error.message)),
            );
        }
    }
    write_state(&state_path, &result, host)?;
    Ok(result)
}

/// Register a detached controller and wait for its journal (Lua `M.start`).
pub fn start(
    identity: &Value,
    host: &Host,
    spawn: &dyn Fn(&Value, &str) -> Result<()>,
) -> Result<Value> {
    retire_completed(identity, host)?;
    let manifest = rules::manifest_path(identity);
    write_state(Path::new(&manifest), identity, host)?;
    spawn(identity, &manifest)?;
    let deadline = (host.clock)() + 30000;
    loop {
        if status(identity, host)?.is_some() {
            return verify(identity, host);
        }
        if (host.clock)() < deadline {
            (host.sleep)(25);
        } else {
            return Err(Failure::new(
                "Detached recovery controller did not register; ownership retained",
            ));
        }
    }
}

/// The runner role body (Lua `M.run_child`): one generation lock, spawn the
/// companion command attached, publish started/exited records. Returns the
/// verified exit code, or `None` for an unknown exit.
pub fn run_child(identity: &Value, host: &Host) -> Result<Option<i32>> {
    let runner_state = record::get_name(identity, "runner-state")
        .ok_or_else(|| Failure::new("Runner manifest is missing its state path"))?;
    let generation_lock = posix::try_acquire(Path::new(&format!("{runner_state}.lock")))?;
    if generation_lock.is_none() {
        return Err(Failure::new("Runner generation already executing"));
    }
    let started_path = PathBuf::from(format!("{runner_state}.started.json"));
    let exit_path = PathBuf::from(format!("{runner_state}.exit.json"));
    (host.remove)(&started_path);
    (host.remove)(&exit_path);
    let command: Vec<std::ffi::OsString> =
        record::string_vec(edn::record::seq(edn::record::get(identity, "command")))
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect();
    if command.is_empty() {
        posix::release(generation_lock);
        return Err(Failure::new("Runner manifest has no command"));
    }
    let cwd = record::get_name(identity, "cwd").unwrap_or_else(|| ".".to_owned());
    let child = match posix::start_attached(
        &command,
        Path::new(&cwd),
        &["NO_COLOR"],
        &started_path,
        &exit_path,
    ) {
        Ok(child) => child,
        Err(error) => {
            posix::release(generation_lock);
            return Err(error);
        }
    };
    let deadline = (host.clock)() + 30000;
    let started = loop {
        let started = (host.read)(&started_path)
            .ok()
            .flatten()
            .and_then(|text| crate::json::decode(&text).ok());
        if let Some(started) = started {
            break started;
        }
        if (host.clock)() >= deadline {
            posix::release(generation_lock);
            return Err(Failure::new("Runner child did not report its identity"));
        }
        (host.sleep)(25);
    };
    let child_record = edn::record::record(vec![
        (
            "pid",
            Value::Integer(
                started
                    .get("pid")
                    .and_then(crate::json::Json::as_i64)
                    .unwrap_or(child.pid as i64),
            ),
        ),
        (
            "start",
            Value::String(
                started
                    .get("start")
                    .and_then(crate::json::Json::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ),
        ),
    ]);
    let token = edn::record::get(identity, "runner-token")
        .cloned()
        .unwrap_or(Value::Nil);
    write_state(
        Path::new(&runner_state),
        &edn::record::record(vec![
            ("token", token.clone()),
            ("phase", Value::keyword("started")),
            ("child", child_record.clone()),
        ]),
        host,
    )?;
    let exit = posix::await_exit(&child, Some(&exit_path));
    let mut exited = edn::record::record(vec![
        ("token", token),
        ("phase", Value::keyword("exited")),
        ("child", child_record),
        ("at", Value::Integer((host.clock)())),
    ]);
    if let Some(code) = exit {
        edn::record::put(&mut exited, "exit", Value::Integer(code as i64));
    }
    write_state(Path::new(&runner_state), &exited, host)?;
    posix::release(generation_lock);
    Ok(exit)
}

/// The detached controller loop (Lua `M.controller_loop`): register, then
/// step under the gate every 100 ms until a terminal phase. Returns the
/// terminal phase; step failures journal `:failed`.
pub fn controller_loop(
    identity: &Value,
    host: &Host,
    launch: &dyn Fn(&Value) -> Result<()>,
    verify: &dyn Fn(&Value) -> Result<bool>,
) -> Result<String> {
    let lock = posix::try_acquire(Path::new(&rules::lock_file(identity)))?;
    if lock.is_none() {
        return Err(Failure::new("Project recovery controller already running"));
    }
    let current = (host.owner)(i64::from(posix::current_pid()))
        .ok_or_else(|| Failure::new("Recovery process identity is incomplete"))?;
    let controller = edn::record::record(vec![
        ("pid", Value::Integer(i64::from(current.pid))),
        ("start", Value::String(current.start)),
    ]);
    register(identity, &controller, host)?;
    loop {
        let stepped = with_gate(identity, || {
            step(&Step {
                identity,
                now: (host.clock)(),
                host,
                launch,
                verify,
            })
        });
        let state = match stepped {
            Ok(state) => state,
            Err(error) => {
                let mut existing = status(identity, host)?.unwrap_or_else(|| Value::Map(vec![]));
                edn::record::put(&mut existing, "phase", Value::keyword("failed"));
                edn::record::put(&mut existing, "reason", Value::String(error.message));
                write_state(Path::new(&rules::state_file(identity)), &existing, host)?;
                existing
            }
        };
        let phase = record::get_name(&state, "phase").unwrap_or_default();
        if rules::terminal(&phase) {
            posix::release(lock);
            return Ok(phase);
        }
        (host.sleep)(100);
    }
}

pub(crate) mod rules;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::memory::Files;
    use crate::vocab::Owner;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// In-memory journal over the fake host: path → EDN text, exactly like
    /// the busted `fast_write` fixtures.
    struct Memory {
        files: Files,
    }

    impl Memory {
        fn new() -> Memory {
            Memory {
                files: Files::new(),
            }
        }

        fn insert(&self, path: &str, value: &Value) {
            self.files.put(path, &edn::encode(value).unwrap());
        }

        fn host(&self) -> Host {
            self.files.serve(Host {
                clock: Rc::new(|| 0),
                sleep: Rc::new(|_| {}),
                owner: Rc::new(|_| None),
                token: Rc::new(|| "mem-token".to_owned()),
                ..Host::fake()
            })
        }
    }

    fn identity(root: &str) -> Value {
        edn::parse(&format!(
            r#"{{:cwd "{root}" :session "uml-viewer-A" :runner-token "initial"
                :runner-state "{root}/.uml-viewer/runner.edn"
                :supervisor {{:token "controller"
                              :manifest "{root}/.uml-viewer/recovery-launch.edn"
                              :state-file "{root}/.uml-viewer/recovery.edn"
                              :control-file "{root}/.uml-viewer/recovery-control.edn"
                              :gate-file "{root}/.uml-viewer/recovery-gate.lock"
                              :lock-file "{root}/.uml-viewer/recovery-controller.lock"}}}}"#
        ))
        .unwrap()
    }

    fn controller() -> Value {
        edn::record::record(vec![
            ("pid", Value::Integer(10)),
            ("start", Value::String("start".to_owned())),
        ])
    }

    fn exited(token: &str, code: i64) -> Value {
        edn::record::record(vec![
            ("token", Value::String(token.to_owned())),
            ("phase", Value::keyword("exited")),
            (
                "child",
                edn::record::record(vec![
                    ("pid", Value::Integer(20)),
                    ("start", Value::String("child".to_owned())),
                ]),
            ),
            ("exit", Value::Integer(code)),
        ])
    }

    /// A provider whose session id it chooses up front has nothing to re-verify.
    fn no_session_check(_: &Value) -> Result<bool> {
        panic!("only a provider with a reported session is re-verified")
    }

    #[test]
    fn step_journals_a_backoff_then_submits_exactly_once_at_the_deadline() {
        let memory = Memory::new();
        let host = memory.host();
        let identity = identity("/r");
        register(&identity, &controller(), &host).unwrap();
        let launches = Rc::new(RefCell::new(Vec::new()));
        let launch_log = launches.clone();
        let launch = move |submission: &Value| {
            launch_log.borrow_mut().push(submission.clone());
            Ok(())
        };
        memory.insert("/r/.uml-viewer/runner.edn", &exited("initial", 37));
        let first = step(&Step {
            identity: &identity,
            now: 1000,
            host: &host,
            launch: &launch,
            verify: &no_session_check,
        })
        .unwrap();
        assert_eq!("backoff", record::get_name(&first, "phase").unwrap());
        assert_eq!(Some(2000), record::get_i64(&first, "deadline"));
        assert!(launches.borrow().is_empty());
        let early = step(&Step {
            identity: &identity,
            now: 1999,
            host: &host,
            launch: &launch,
            verify: &no_session_check,
        })
        .unwrap();
        assert!(launches.borrow().is_empty());
        assert_eq!("backoff", record::get_name(&early, "phase").unwrap());
        let submitted = step(&Step {
            identity: &identity,
            now: 2000,
            host: &host,
            launch: &launch,
            verify: &no_session_check,
        })
        .unwrap();
        assert_eq!(1, launches.borrow().len());
        assert_eq!(Some(1), record::get_i64(&submitted, "attempts"));
        assert_eq!(Some(1), record::get_i64(&submitted, "generation"));
        assert_eq!(
            Some("mem-token".to_owned()),
            record::get_name(&launches.borrow()[0], "runner-token")
        );
    }

    #[test]
    fn a_step_that_changes_nothing_does_not_rewrite_the_journal() {
        let memory = Memory::new();
        let writes = Rc::new(RefCell::new(0));
        let counted = writes.clone();
        let base = memory.host();
        let inner = base.write.clone();
        let host = Host {
            write: Rc::new(move |path, bytes| {
                *counted.borrow_mut() += 1;
                inner(path, bytes)
            }),
            ..base
        };
        let identity = identity("/r");
        register(&identity, &controller(), &host).unwrap();
        let launch = |_: &Value| Ok(());
        let baseline = *writes.borrow();
        step(&Step {
            identity: &identity,
            now: 0,
            host: &host,
            launch: &launch,
            verify: &no_session_check,
        })
        .unwrap();
        let first = *writes.borrow();
        step(&Step {
            identity: &identity,
            now: 50,
            host: &host,
            launch: &launch,
            verify: &no_session_check,
        })
        .unwrap();
        assert_eq!(first, *writes.borrow());
        assert_eq!(baseline, first);
    }

    #[test]
    fn an_uncertain_submission_fails_closed_and_never_replays() {
        let memory = Memory::new();
        let host = memory.host();
        let identity = identity("/r");
        register(&identity, &controller(), &host).unwrap();
        memory.insert("/r/.uml-viewer/runner.edn", &exited("initial", 37));
        let attempts = RefCell::new(0);
        let failing = |_: &Value| {
            *attempts.borrow_mut() += 1;
            Err(Failure::new("timeout"))
        };
        step(&Step {
            identity: &identity,
            now: 0,
            host: &host,
            launch: &failing,
            verify: &no_session_check,
        })
        .unwrap();
        let result = step(&Step {
            identity: &identity,
            now: 1000,
            host: &host,
            launch: &failing,
            verify: &no_session_check,
        })
        .unwrap();
        assert_eq!("failed", record::get_name(&result, "phase").unwrap());
        assert_eq!(Some(1), record::get_i64(&result, "attempts"));
        assert_eq!(
            Some("Recovery submission uncertain: timeout".to_owned()),
            record::get_name(&result, "reason")
        );
        let noop = |_: &Value| Ok(());
        step(&Step {
            identity: &identity,
            now: 999999,
            host: &host,
            launch: &noop,
            verify: &no_session_check,
        })
        .unwrap();
        assert_eq!(1, *attempts.borrow());
    }

    #[test]
    fn read_state_retries_transient_locks_but_not_malformed_journals() {
        let memory = Memory::new();
        let target = PathBuf::from("/r/state.edn");
        memory.files.put(&target, "{:phase :running}");
        let calls = Rc::new(RefCell::new(0));
        let counted = calls.clone();
        let base = memory.host();
        let inner = base.read.clone();
        let host = Host {
            read: Rc::new(move |path| {
                *counted.borrow_mut() += 1;
                if *counted.borrow() < 3 {
                    return Err(Failure::new("sharing violation"));
                }
                inner(path)
            }),
            ..base
        };
        let value = read_state(&target, &host).unwrap().unwrap();
        assert_eq!(
            Some("running".to_owned()),
            record::get_name(&value, "phase")
        );
        assert_eq!(3, *calls.borrow());
        memory.files.put(&target, "{:phase");
        assert!(read_state(&target, &host).is_err());
    }

    #[test]
    fn a_sharing_violation_outlasting_the_budget_fails() {
        let now = Rc::new(RefCell::new(0i64));
        let clock_now = now.clone();
        let sleep_now = now.clone();
        let host = Host {
            clock: Rc::new(move || *clock_now.borrow()),
            sleep: Rc::new(move |ms| *sleep_now.borrow_mut() += ms),
            read: Rc::new(|_| Err(Failure::new("sharing violation"))),
            ..Host::fake()
        };
        assert!(read_state(Path::new("/r/state.edn"), &host).is_err());
    }

    #[test]
    fn retire_completed_removes_terminal_journals_and_never_live_ones() {
        for phase in ["cancelled", "failed", "normal"] {
            let memory = Memory::new();
            let host = memory.host();
            let identity = identity("/r");
            register(&identity, &controller(), &host).unwrap();
            let mut state = status(&identity, &host).unwrap().unwrap();
            edn::record::put(&mut state, "phase", Value::keyword(phase));
            save(&identity, &state, &host).unwrap();
            assert!(retire_completed(&identity, &host).unwrap().is_some());
            assert!(status(&identity, &host).unwrap().is_none());
        }
        for phase in ["running", "backoff", "submitting"] {
            let memory = Memory::new();
            let host = memory.host();
            let identity = identity("/r");
            register(&identity, &controller(), &host).unwrap();
            let mut state = status(&identity, &host).unwrap().unwrap();
            edn::record::put(&mut state, "phase", Value::keyword(phase));
            save(&identity, &state, &host).unwrap();
            assert!(retire_completed(&identity, &host).is_err());
            assert_eq!(
                Some(phase.to_owned()),
                record::get_name(&status(&identity, &host).unwrap().unwrap(), "phase")
            );
        }
    }

    #[test]
    fn verify_accepts_only_the_exact_original_controller_identity() {
        let memory = Memory::new();
        let host = Host {
            owner: Rc::new(|pid: i64| {
                (pid == 10).then(|| Owner {
                    pid: 10,
                    start: "start".to_owned(),
                    name: "controller".to_owned(),
                })
            }),
            ..memory.host()
        };
        let identity = identity("/r");
        register(&identity, &controller(), &host).unwrap();
        assert_eq!(
            "running",
            record::get_name(&verify(&identity, &host).unwrap(), "phase").unwrap()
        );
        for replacement in [
            edn::record::record(vec![
                ("pid", Value::Integer(11)),
                ("start", Value::String("start".to_owned())),
            ]),
            edn::record::record(vec![
                ("pid", Value::Integer(10)),
                ("start", Value::String("other".to_owned())),
            ]),
            edn::record::record(vec![("start", Value::String("start".to_owned()))]),
        ] {
            let mut state = status(&identity, &host).unwrap().unwrap();
            edn::record::put(&mut state, "controller", replacement);
            save(&identity, &state, &host).unwrap();
            assert!(verify(&identity, &host).is_err());
        }
    }

    #[test]
    fn runner_state_must_carry_a_phase_to_count_as_acknowledgement() {
        let memory = Memory::new();
        let host = memory.host();
        let good = "/r/.uml-viewer/runner.edn";
        memory.files.put(good, "{:token \"t\" :phase :started}");
        let state = edn::record::record(vec![("runner-state", Value::String(good.to_owned()))]);
        assert!(read_runner(&state, &host).unwrap().is_some());
        memory.files.put(good, "broken EDN {{{");
        assert!(read_runner(&state, &host).unwrap().is_none());
        memory.files.put(good, "{:token \"t\"}");
        assert!(read_runner(&state, &host).unwrap().is_none());
    }

    fn tempfile() -> PathBuf {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "uml recovery test {} {}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join(".uml-viewer")).unwrap();
        directory
    }

    #[test]
    fn run_child_records_started_and_the_actual_exit() {
        let directory = tempfile();
        let root = directory.to_string_lossy().into_owned();
        let runner_state = format!("{root}/.uml-viewer/runner.edn");
        let identity = edn::parse(&format!(
            r#"{{:cwd "{root}" :command ["/bin/sh" "-c" "sleep 0.1; exit 7"]
                :runner-token "gen" :runner-state "{runner_state}"}}"#
        ))
        .unwrap();
        let exit = run_child(&identity, &Host::posix()).unwrap();
        assert_eq!(Some(7), exit);
        let text = std::fs::read_to_string(&runner_state).unwrap();
        let state = edn::parse(&text).unwrap();
        assert_eq!(Some("exited".to_owned()), record::get_name(&state, "phase"));
        assert_eq!(Some(7), record::get_i64(&state, "exit"));
        assert!(record::get_i64(&state, "at").is_some());
        let child = edn::record::get(&state, "child").unwrap();
        assert!(record::get_i64(child, "pid").unwrap_or(0) > 1);
        // A second run of the same generation now succeeds (lock released).
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn run_child_refuses_a_second_holder_of_the_generation_lock() {
        let directory = tempfile();
        let root = directory.to_string_lossy().into_owned();
        let runner_state = format!("{root}/.uml-viewer/runner.edn");
        let held = posix::try_acquire(Path::new(&format!("{runner_state}.lock")))
            .unwrap()
            .expect("lock");
        let identity = edn::parse(&format!(
            r#"{{:cwd "{root}" :command ["/bin/true"] :runner-token "gen"
                :runner-state "{runner_state}"}}"#
        ))
        .unwrap();
        let error = run_child(&identity, &Host::posix()).unwrap_err();
        assert_eq!("Runner generation already executing", error.message);
        posix::release(Some(held));
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A Codex companion's controller over the in-memory journal: a scripted
    /// runner, child liveness and Herdr's session report.
    struct Codex {
        memory: Memory,
        host: Host,
        identity: Value,
        alive: Rc<RefCell<bool>>,
        launches: RefCell<Vec<Value>>,
        report: RefCell<std::result::Result<bool, String>>,
        checks: RefCell<i64>,
    }

    const QUEUE: &str = "/r/.uml-viewer/to-agent.edn";
    const MAIL: &str = "{:next-id 3 :queue [{:id 1} {:id 2}]}";

    impl Codex {
        fn new() -> Codex {
            let memory = Memory::new();
            let alive = Rc::new(RefCell::new(false));
            let liveness = alive.clone();
            let host = Host {
                owner: Rc::new(move |pid: i64| {
                    (pid == 20 && *liveness.borrow()).then(|| Owner {
                        pid: 20,
                        start: "child".to_owned(),
                        name: "codex".to_owned(),
                    })
                }),
                ..memory.host()
            };
            let mut identity = identity("/r");
            edn::record::put(&mut identity, "coding-agent", Value::keyword("codex"));
            edn::record::put(
                &mut identity,
                "codex-session",
                Value::String("01a1a458-f60d-7561-aa06-d04df3bd14f7".to_owned()),
            );
            register(&identity, &controller(), &host).unwrap();
            memory.files.put(QUEUE, MAIL);
            Codex {
                memory,
                host,
                identity,
                alive,
                launches: RefCell::new(Vec::new()),
                report: RefCell::new(Ok(false)),
                checks: RefCell::new(0),
            }
        }

        fn at(&self, now: i64) -> Value {
            let launch = |submission: &Value| {
                self.launches.borrow_mut().push(submission.clone());
                Ok(())
            };
            let verify = |_: &Value| {
                *self.checks.borrow_mut() += 1;
                self.report.borrow().clone().map_err(Failure::new)
            };
            step(&Step {
                identity: &self.identity,
                now,
                host: &self.host,
                launch: &launch,
                verify: &verify,
            })
            .unwrap()
        }

        fn runner(&self, generation: i64) -> String {
            if generation == 0 {
                "/r/.uml-viewer/runner.edn".to_owned()
            } else {
                format!("/r/.uml-viewer/uml-viewer-A-runner-{generation}.edn")
            }
        }

        fn token(generation: i64) -> &'static str {
            if generation == 0 {
                "initial"
            } else {
                "mem-token"
            }
        }

        fn exits(&self, generation: i64, code: i64) {
            *self.alive.borrow_mut() = false;
            self.memory.insert(
                &self.runner(generation),
                &exited(Self::token(generation), code),
            );
        }

        fn starts(&self, generation: i64) {
            *self.alive.borrow_mut() = true;
            let mut started = exited(Self::token(generation), 0);
            edn::record::put(&mut started, "phase", Value::keyword("started"));
            edn::record::remove(&mut started, "exit");
            self.memory.insert(&self.runner(generation), &started);
        }

        /// The first abnormal exit relaunched at its 1 s deadline.
        fn relaunched(&self) -> Value {
            self.exits(0, 37);
            self.at(0);
            self.at(1000)
        }

        fn queue_is_preserved(&self) {
            assert_eq!(Some(MAIL.to_owned()), self.memory.files.get(QUEUE));
        }
    }

    fn phase_of(state: &Value) -> String {
        record::get_name(state, "phase").unwrap()
    }

    #[test]
    fn a_relaunched_codex_session_is_supervised_only_after_the_same_uuid_returns() {
        let codex = Codex::new();
        let submitted = codex.relaunched();
        assert_eq!("running", phase_of(&submitted));
        assert_eq!(1, codex.launches.borrow().len());
        // No child yet: nothing can have reported, so Herdr is not asked.
        codex.at(1100);
        assert_eq!(0, *codex.checks.borrow());
        codex.starts(1);
        assert!(edn::record::flag(&codex.at(1200), "session-check"));
        assert_eq!(1, *codex.checks.borrow());
        *codex.report.borrow_mut() = Ok(true);
        let proven = codex.at(1300);
        assert_eq!("running", phase_of(&proven));
        assert!(!edn::record::flag(&proven, "session-check"));
        let settled = *codex.checks.borrow();
        codex.at(1400);
        assert_eq!(settled, *codex.checks.borrow());
        assert_eq!(1, codex.launches.borrow().len());
        codex.queue_is_preserved();
    }

    #[test]
    fn a_different_session_after_relaunch_stops_recovery_for_good() {
        let codex = Codex::new();
        codex.relaunched();
        codex.starts(1);
        *codex.report.borrow_mut() = Err("Herdr reported another session".to_owned());
        let refused = codex.at(1200);
        assert_eq!("failed", phase_of(&refused));
        assert_eq!(
            Some("Recovery session check refused: Herdr reported another session".to_owned()),
            record::get_name(&refused, "reason")
        );
        codex.exits(1, 37);
        assert_eq!("failed", phase_of(&codex.at(99_999)));
        assert_eq!(1, codex.launches.borrow().len());
        codex.queue_is_preserved();
    }

    #[test]
    fn a_session_never_reported_after_relaunch_stops_recovery_at_the_deadline() {
        let codex = Codex::new();
        codex.relaunched();
        codex.starts(1);
        assert_eq!("running", phase_of(&codex.at(45_999)));
        let late = codex.at(46_000);
        assert_eq!("failed", phase_of(&late));
        assert_eq!(
            Some("Herdr reported no Codex CLI session within 45 s of recovery".to_owned()),
            record::get_name(&late, "reason")
        );
        assert_eq!(1, codex.launches.borrow().len());
        codex.queue_is_preserved();
    }

    #[test]
    fn codex_retries_stay_bounded_at_one_two_four_seconds_with_every_session_rechecked() {
        let codex = Codex::new();
        *codex.report.borrow_mut() = Ok(true);
        let mut generation = 0;
        for (at, deadline) in [(0, 1000), (1100, 3100), (3200, 7200)] {
            codex.exits(generation, 37);
            assert_eq!(Some(deadline), record::get_i64(&codex.at(at), "deadline"));
            assert_eq!("backoff", phase_of(&codex.at(deadline - 1)));
            generation += 1;
            let submitted = codex.at(deadline);
            assert_eq!(Some(generation), record::get_i64(&submitted, "attempts"));
            assert!(edn::record::flag(&submitted, "session-check"));
        }
        codex.exits(generation, 37);
        let exhausted = codex.at(7300);
        assert_eq!(
            Some("Recovery exhausted after three retries".to_owned()),
            record::get_name(&exhausted, "reason")
        );
        assert_eq!(3, codex.launches.borrow().len());
        codex.queue_is_preserved();
    }

    #[test]
    fn a_codex_child_exiting_before_its_session_returns_spends_the_budget_normally() {
        let codex = Codex::new();
        codex.relaunched();
        codex.starts(1);
        codex.at(1200);
        codex.exits(1, 37);
        let backoff = codex.at(1300);
        assert_eq!("backoff", phase_of(&backoff));
        assert_eq!(Some(3300), record::get_i64(&backoff, "deadline"));
    }

    #[test]
    fn sixty_seconds_of_codex_survival_resets_the_budget() {
        let codex = Codex::new();
        *codex.report.borrow_mut() = Ok(true);
        codex.relaunched();
        codex.starts(1);
        assert_eq!(Some(1), record::get_i64(&codex.at(1100), "attempts"));
        assert_eq!(Some(1), record::get_i64(&codex.at(61_099), "attempts"));
        assert_eq!(Some(0), record::get_i64(&codex.at(61_100), "attempts"));
    }

    #[test]
    fn normal_and_unknown_codex_exits_are_never_recovered() {
        let codex = Codex::new();
        codex.exits(0, 0);
        assert_eq!("normal", phase_of(&codex.at(0)));
        let codex = Codex::new();
        codex.memory.insert(
            "/r/.uml-viewer/runner.edn",
            &edn::record::record(vec![
                ("token", Value::String("initial".to_owned())),
                ("phase", Value::keyword("exited")),
            ]),
        );
        assert_eq!("failed", phase_of(&codex.at(0)));
        let codex = Codex::new();
        codex.exits(0, 37);
        codex
            .memory
            .insert("/r/.uml-viewer/runner.edn", &exited("other-generation", 37));
        assert_eq!("failed", phase_of(&codex.at(0)));
        assert!(codex.launches.borrow().is_empty());
        codex.queue_is_preserved();
    }

    #[test]
    fn closing_during_a_codex_backoff_cancels_the_due_retry() {
        let codex = Codex::new();
        codex.exits(0, 37);
        assert_eq!("backoff", phase_of(&codex.at(0)));
        codex.memory.insert(
            "/r/.uml-viewer/recovery-control.edn",
            &edn::record::record(vec![("cancel", Value::String("controller".to_owned()))]),
        );
        assert_eq!("cancelled", phase_of(&codex.at(1000)));
        assert!(codex.launches.borrow().is_empty());
        codex.queue_is_preserved();
    }

    #[test]
    fn a_claude_relaunch_journals_and_checks_no_session() {
        let memory = Memory::new();
        let host = memory.host();
        let identity = identity("/r");
        register(&identity, &controller(), &host).unwrap();
        memory.insert("/r/.uml-viewer/runner.edn", &exited("initial", 37));
        let launch = |_: &Value| Ok(());
        let go = |now: i64| {
            step(&Step {
                identity: &identity,
                now,
                host: &host,
                launch: &launch,
                verify: &no_session_check,
            })
            .unwrap()
        };
        go(0);
        assert_eq!(None, edn::record::get(&go(1000), "session-check"));
    }
}
