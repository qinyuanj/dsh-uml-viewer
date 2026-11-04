//! Ported from `love/spec/herdr_spec.lua`: the fake CLI answers the Herdr
//! protocol exactly and every call selects the recorded session.

use super::*;
use crate::host::memory::Files;
use crate::json::Json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

fn owned() -> Value {
    edn::parse(
        r#"{:backend :herdr :owner-role :companion :session "uml-viewer-A"
            :socket "/x/herdr.sock" :workspace "w1" :pane "w1:p1" :terminal-id "term_A"
            :shell-pid 42 :owner-start "shell-start" :server-pid "41"
            :server-start "server-start" :cwd "/nonexistent-uml-viewer-fixture/project"
            :command ["claude"]}"#,
    )
    .unwrap()
}

#[derive(Default)]
struct FakeState {
    stopped: bool,
    closed: bool,
    deleted: bool,
    ignore_close: bool,
    empty_sessions: bool,
    no_agent: bool,
    occupant: Option<String>,
    /// The `agent_session` Herdr reports for the occupant; absent until the
    /// provider's integration reports it.
    agent_session: Option<Json>,
    terminal: Option<String>,
    cwd: Option<String>,
    server_start: Option<String>,
    shell_start: Option<String>,
    fail: Option<(String, String)>,
    prompted: bool,
    agent_status: Option<String>,
    state_change_seq: Option<f64>,
    run_text: Option<String>,
    foreground: Option<Json>,
    foreground_absent: bool,
    reset_fail: bool,
}

type Calls = Rc<RefCell<Vec<Vec<String>>>>;
type Shared = Rc<RefCell<FakeState>>;

fn response(value: Json) -> RunOut {
    RunOut {
        exit: Some(0),
        out: crate::json::encode(&value),
        err: String::new(),
        timeout: false,
    }
}

fn object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn fake_cli(calls: Calls, state: Shared) -> impl Fn(&[String], Option<i64>) -> RunOut {
    move |argv, _| {
        calls.borrow_mut().push(argv.to_vec());
        let socket = "/x/herdr.sock".to_owned();
        assert_eq!("herdr", argv[0]);
        assert_eq!("--session", argv[1]);
        assert_eq!("uml-viewer-A", argv[2]);
        let op = (
            argv[3].as_str(),
            argv.get(4).map(String::as_str).unwrap_or(""),
        );
        {
            let state = state.borrow();
            if let Some(fail) = &state.fail
                && fail.0 == op.0
                && fail.1 == op.1
            {
                return RunOut {
                    exit: Some(1),
                    out: String::new(),
                    err: "injected failure".to_owned(),
                    timeout: false,
                };
            }
        }
        match op {
            ("status", "server") => {
                let stopped = state.borrow().stopped;
                response(object(vec![
                    ("session", Json::String("uml-viewer-A".to_owned())),
                    ("socket", Json::String(socket)),
                    ("running", Json::Bool(!stopped)),
                    ("version", Json::String("0.9.3".to_owned())),
                    ("protocol", Json::Number(22.0)),
                    ("compatible", Json::Bool(true)),
                    ("endpoint_compatible", Json::Bool(true)),
                ]))
            }
            ("api", "snapshot") => {
                let state = state.borrow();
                let panes = Json::Array(vec![object(vec![
                    ("pane_id", Json::String("w1:p1".to_owned())),
                    ("workspace_id", Json::String("w1".to_owned())),
                    (
                        "terminal_id",
                        Json::String(
                            state
                                .terminal
                                .clone()
                                .unwrap_or_else(|| "term_A".to_owned()),
                        ),
                    ),
                    (
                        "cwd",
                        Json::String(state.cwd.clone().unwrap_or_else(|| {
                            "/nonexistent-uml-viewer-fixture/project".to_owned()
                        })),
                    ),
                ])]);
                let workspaces = if state.closed {
                    Json::Array(vec![])
                } else {
                    Json::Array(vec![object(vec![(
                        "workspace_id",
                        Json::String("w1".to_owned()),
                    )])])
                };
                let agents = if state.no_agent {
                    Json::Array(vec![])
                } else {
                    Json::Array(vec![object(vec![
                        ("pane_id", Json::String("w1:p1".to_owned())),
                        ("terminal_id", Json::String("term_A".to_owned())),
                        (
                            "agent",
                            Json::String(state.occupant.clone().unwrap_or("claude".to_owned())),
                        ),
                        (
                            "agent_status",
                            Json::String(state.agent_status.clone().unwrap_or("idle".to_owned())),
                        ),
                        (
                            "state_change_seq",
                            Json::Number(state.state_change_seq.unwrap_or(1.0)),
                        ),
                        (
                            "agent_session",
                            state.agent_session.clone().unwrap_or(Json::Null),
                        ),
                    ])])
                };
                response(object(vec![(
                    "result",
                    object(vec![(
                        "snapshot",
                        object(vec![
                            ("version", Json::String("0.9.3".to_owned())),
                            ("protocol", Json::Number(22.0)),
                            ("workspaces", workspaces),
                            ("panes", panes),
                            ("agents", agents),
                        ]),
                    )]),
                )]))
            }
            ("pane", "process-info") => {
                let state = state.borrow();
                let foreground = if state.foreground_absent {
                    Json::Null
                } else if let Some(foreground) = &state.foreground {
                    foreground.clone()
                } else {
                    Json::Array(vec![object(vec![
                        ("pid", Json::Number(42.0)),
                        ("name", Json::String("bash".to_owned())),
                    ])])
                };
                response(object(vec![(
                    "result",
                    object(vec![(
                        "process_info",
                        object(vec![
                            ("pane_id", Json::String("w1:p1".to_owned())),
                            ("shell_pid", Json::Number(42.0)),
                            ("foreground_processes", foreground),
                        ]),
                    )]),
                )]))
            }
            ("workspace", "create") => response(object(vec![(
                "result",
                object(vec![
                    (
                        "workspace",
                        object(vec![("workspace_id", Json::String("w1".to_owned()))]),
                    ),
                    (
                        "root_pane",
                        object(vec![
                            ("pane_id", Json::String("w1:p1".to_owned())),
                            ("terminal_id", Json::String("term_A".to_owned())),
                        ]),
                    ),
                ]),
            )])),
            ("workspace", "close") => {
                let mut state = state.borrow_mut();
                if !state.ignore_close {
                    state.closed = true;
                }
                response(object(vec![(
                    "result",
                    object(vec![("type", Json::String("ok".to_owned()))]),
                )]))
            }
            ("session", "stop") => {
                state.borrow_mut().stopped = true;
                response(object(vec![
                    ("stopped", Json::Bool(true)),
                    (
                        "session",
                        object(vec![
                            ("name", Json::String("uml-viewer-A".to_owned())),
                            ("socket_path", Json::String(socket)),
                        ]),
                    ),
                ]))
            }
            ("session", "delete") => {
                state.borrow_mut().deleted = true;
                response(object(vec![
                    ("deleted", Json::Bool(true)),
                    (
                        "session",
                        object(vec![
                            ("name", Json::String("uml-viewer-A".to_owned())),
                            ("socket_path", Json::String(socket)),
                        ]),
                    ),
                ]))
            }
            ("session", "list") => {
                let state = state.borrow();
                let sessions = if state.empty_sessions {
                    Json::Array(vec![])
                } else if state.deleted {
                    Json::Array(vec![object(vec![(
                        "name",
                        Json::String("uml-viewer-B".to_owned()),
                    )])])
                } else {
                    Json::Array(vec![
                        object(vec![("name", Json::String("uml-viewer-A".to_owned()))]),
                        object(vec![("name", Json::String("uml-viewer-B".to_owned()))]),
                    ])
                };
                response(object(vec![("sessions", sessions)]))
            }
            ("agent", "prompt") => {
                state.borrow_mut().prompted = true;
                response(object(vec![(
                    "result",
                    object(vec![(
                        "agent",
                        object(vec![
                            ("agent_status", Json::String("working".to_owned())),
                            ("state_change_seq", Json::Number(4.0)),
                        ]),
                    )]),
                )]))
            }
            ("pane", "send-keys") => response(object(vec![(
                "result",
                object(vec![("type", Json::String("ok".to_owned()))]),
            )])),
            ("pane", "run") => {
                state.borrow_mut().run_text = argv.get(6).cloned();
                response(object(vec![(
                    "result",
                    object(vec![("type", Json::String("ok".to_owned()))]),
                )]))
            }
            // A fresh pane shows its prompt before the first `pane run` lands
            // (measured on 0.9.5); the adapter waits for this text, which the
            // CLI prints raw rather than as JSON.
            ("pane", "read") => RunOut {
                exit: Some(0),
                out: "❯ ".to_owned(),
                err: String::new(),
                timeout: false,
            },
            other => panic!("Unexpected CLI call: {other:?}"),
        }
    }
}

fn fake_owner(state: Shared) -> impl Fn(i64) -> Option<Owner> {
    move |pid| {
        let state = state.borrow();
        match pid {
            41 if !state.stopped => Some(Owner {
                pid: 41,
                start: state
                    .server_start
                    .clone()
                    .unwrap_or_else(|| "server-start".to_owned()),
                name: "herdr".to_owned(),
            }),
            42 => Some(Owner {
                pid: 42,
                start: state
                    .shell_start
                    .clone()
                    .unwrap_or_else(|| "shell-start".to_owned()),
                name: "bash".to_owned(),
            }),
            _ => None,
        }
    }
}

fn selected() -> Selected {
    Selected {
        path: "herdr".to_owned(),
        release: "0.9.3".to_owned(),
    }
}

fn config() -> Config {
    Config::plain(Some(selected()))
}

/// The fixture host over the fake CLI and process table: a fixed clock, no
/// sleeping, literal canonical paths and an in-memory file store. Every other
/// capability stays undeclared, so a test that reaches one fails by name.
fn fixture_host(
    run: impl Fn(&[String], Option<i64>) -> RunOut + 'static,
    owner: impl Fn(i64) -> Option<Owner> + 'static,
    files: &Files,
) -> Host {
    files.serve(Host {
        run: Rc::new(run),
        owner: Rc::new(owner),
        clock: Rc::new(|| 0),
        sleep: Rc::new(|_| {}),
        canonical: Rc::new(|path| path.to_owned()),
        ..Host::fake()
    })
}

fn backend(
    run: impl Fn(&[String], Option<i64>) -> RunOut + 'static,
    owner: impl Fn(i64) -> Option<Owner> + 'static,
) -> Herdr {
    Herdr::new(
        Some(owned()),
        fixture_host(run, owner, &Files::new()),
        config(),
    )
}

/// The fixture project and the companion executable inside it. Neither
/// exists on disk: the host answers for them.
const ROOT: &str = "/opt/fixture/project";
const EXECUTABLE: &str = "/opt/fixture/claude 中文";

/// Declare what `create` reads before the CLI: a `$HOME/.config` root, a
/// PATH, an openable `executable`, the inherited variable names and fresh
/// generation tokens.
fn creatable(backend: &mut Herdr, executable: &'static str) {
    backend.host.variable = Rc::new(|name| match name {
        "HOME" => Some("/home/person".to_owned()),
        "PATH" => Some("/usr/local/bin:/usr/bin".to_owned()),
        _ => None,
    });
    backend.host.openable = Rc::new(move |path| path == Path::new(executable));
    backend.host.variables = Rc::new(|| strings(&["PATH", "HOME"]));
    let serial = Rc::new(std::cell::Cell::new(0));
    backend.host.token = Rc::new(move || {
        serial.set(serial.get() + 1);
        format!("token-{}", serial.get())
    });
}

fn ops(calls: &Calls) -> Vec<String> {
    calls
        .borrow()
        .iter()
        .filter(|argv| argv[0] == "herdr")
        .map(|argv| format!("{} {}", argv[3], argv.get(4).cloned().unwrap_or_default()))
        .collect()
}

#[test]
fn binds_the_full_identity_via_explicit_session_selectors() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let backend = backend(fake_cli(calls.clone(), state.clone()), fake_owner(state));
    assert_eq!("w1:p1", backend.bind("uml-viewer-A", "w1:p1").unwrap());
    assert!(backend.probe("uml-viewer-A").unwrap());
}

#[test]
fn rejects_a_and_b_selectors_even_though_both_sessions_use_the_same_pane_id() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let backend = backend(fake_cli(calls.clone(), state.clone()), fake_owner(state));
    assert!(backend.bind("uml-viewer-B", "w1:p1").is_err());
    assert!(calls.borrow().is_empty());
}

#[test]
fn refuses_cleanup_when_terminal_shell_or_server_creation_identity_changes() {
    for change in ["terminal", "shell_start", "server_start"] {
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        {
            let mut state = state.borrow_mut();
            match change {
                "terminal" => state.terminal = Some("term_B".to_owned()),
                "shell_start" => state.shell_start = Some("reused".to_owned()),
                _ => state.server_start = Some("reused".to_owned()),
            }
        }
        let backend = backend(fake_cli(calls.clone(), state.clone()), fake_owner(state));
        assert!(backend.cleanup("uml-viewer-A").is_err(), "{change}");
        let seen = ops(&calls).join(",");
        assert!(!seen.contains("workspace close"), "{change}: {seen}");
        assert!(!seen.contains("session stop"), "{change}: {seen}");
    }
}

#[test]
fn closes_and_confirms_only_a_while_leaving_b_in_the_inventory() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let backend = backend(
        fake_cli(calls.clone(), state.clone()),
        fake_owner(state.clone()),
    );
    backend.cleanup("uml-viewer-A").unwrap();
    assert!(state.borrow().deleted);
    for argv in calls.borrow().iter() {
        if argv[0] == "herdr" {
            assert_eq!("uml-viewer-A", argv[2]);
        }
    }
}

#[test]
fn retains_ambiguous_cleanup_after_failure_or_a_false_successful_close() {
    for change in ["fail close", "ignore close", "fail stop"] {
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        {
            let mut state = state.borrow_mut();
            match change {
                "fail close" => state.fail = Some(("workspace".to_owned(), "close".to_owned())),
                "ignore close" => state.ignore_close = true,
                _ => state.fail = Some(("session".to_owned(), "stop".to_owned())),
            }
        }
        let backend = backend(
            fake_cli(calls.clone(), state.clone()),
            fake_owner(state.clone()),
        );
        assert!(backend.cleanup("uml-viewer-A").is_err(), "{change}");
        assert!(!state.borrow().deleted, "{change}");
    }
}

#[test]
fn never_treats_malformed_or_timed_out_probes_as_absence() {
    for result in [
        RunOut {
            exit: Some(0),
            out: "not JSON".to_owned(),
            err: String::new(),
            timeout: false,
        },
        RunOut {
            exit: None,
            out: String::new(),
            err: "timeout".to_owned(),
            timeout: true,
        },
    ] {
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        let answer = result.clone();
        let backend = backend(move |_, _| answer.clone(), fake_owner(state));
        assert!(backend.probe("uml-viewer-A").is_err());
    }
}

#[test]
fn wake_requires_a_real_claude_occupant() {
    for no_agent in [true, false] {
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let state: Shared = Rc::new(RefCell::new(FakeState {
            no_agent,
            ..FakeState::default()
        }));
        let backend = backend(fake_cli(calls, state.clone()), fake_owner(state.clone()));
        let result = backend.wake(
            "w1:p1",
            &["-l".to_owned(), "mail token-1".to_owned()],
            "token-1",
            &|_| false,
        );
        assert_eq!(no_agent, result.is_err());
        assert_eq!(!no_agent, state.borrow().prompted);
    }
}

/// The occupant must be the provider the ownership names; a project that
/// names none is Claude, and no other agent stands in for it.
#[test]
fn wake_requires_the_occupant_of_the_recorded_provider() {
    for (record, occupant, accepted) in [
        (owned(), "claude", true),
        (owned(), "codex", false),
        (codex_owned(), "codex", true),
        (codex_owned(), "claude", false),
    ] {
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let state: Shared = Rc::new(RefCell::new(FakeState {
            occupant: Some(occupant.to_owned()),
            agent_session: Some(reported(
                occupant,
                &format!("herdr:{occupant}"),
                "id",
                CODEX_SESSION,
            )),
            ..FakeState::default()
        }));
        let backend = Herdr::new(
            Some(record),
            fixture_host(
                fake_cli(calls, state.clone()),
                fake_owner(state.clone()),
                &Files::new(),
            ),
            config(),
        );
        let result = backend.wake(
            "w1:p1",
            &["-l".to_owned(), "mail token-1".to_owned()],
            "token-1",
            &|_| false,
        );
        assert_eq!(accepted, result.is_ok(), "{occupant}: {result:?}");
        assert_eq!(accepted, state.borrow().prompted, "{occupant}");
        if !accepted {
            let other = if occupant == "claude" {
                "Codex CLI"
            } else {
                "Claude Code"
            };
            assert_eq!(
                format!("No {other} agent occupies the owned Herdr pane"),
                result.unwrap_err().message
            );
        }
    }
}

/// A committed Codex ownership over the fixture pane.
fn codex_owned() -> Value {
    let mut record = owned();
    edn::record::put(&mut record, "coding-agent", Value::keyword("codex"));
    edn::record::put(
        &mut record,
        "codex-session",
        Value::String(CODEX_SESSION.to_owned()),
    );
    record
}

/// A Codex backend over the fake CLI with `session` as the occupant's
/// reported `agent_session`.
fn codex_backend(session: Option<Json>) -> (Herdr, Shared) {
    let state: Shared = Rc::new(RefCell::new(FakeState {
        occupant: Some("codex".to_owned()),
        agent_session: session,
        ..FakeState::default()
    }));
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let backend = Herdr::new(
        Some(codex_owned()),
        fixture_host(
            fake_cli(calls, state.clone()),
            fake_owner(state.clone()),
            &Files::new(),
        ),
        config(),
    );
    (backend, state)
}

/// After a recovery relaunch only the saved UUID in the owned pane counts.
#[test]
fn only_the_saved_codex_session_is_accepted_after_a_relaunch() {
    let check = |session: Option<Json>| {
        let (backend, _) = codex_backend(session);
        backend.resumed_session(&codex_owned())
    };
    let other = "01a1a458-f60d-7561-aa06-d04df3bd14f8";
    let same = reported("codex", "herdr:codex", "id", CODEX_SESSION);
    assert!(check(Some(same)).unwrap());
    assert!(!check(None).unwrap());
    for refused in [
        reported("codex", "herdr:codex", "id", other),
        reported("codex", "herdr:codex", "path", CODEX_SESSION),
        reported("codex", "herdr:claude", "id", CODEX_SESSION),
        reported("claude", "herdr:codex", "id", CODEX_SESSION),
        reported("codex", "herdr:codex", "id", "not-a-uuid"),
    ] {
        assert!(check(Some(refused)).is_err());
    }
}

#[test]
fn a_relaunched_pane_without_a_codex_occupant_is_not_proven_yet() {
    let (backend, state) = codex_backend(None);
    state.borrow_mut().no_agent = true;
    assert!(!backend.resumed_session(&codex_owned()).unwrap());
    let (backend, state) = codex_backend(None);
    state.borrow_mut().fail = Some(("api snapshot".to_owned(), "server gone".to_owned()));
    assert!(!backend.resumed_session(&codex_owned()).unwrap());
}

/// The Codex occupant must still be the saved session: a later session in
/// the same pane, or one Herdr no longer reports, is not this companion's.
#[test]
fn codex_wake_requires_the_saved_session_in_the_owned_pane() {
    let other = "01a1a458-f60d-7561-aa06-d04df3bd14f8";
    for (session, accepted) in [
        (
            Some(reported("codex", "herdr:codex", "id", CODEX_SESSION)),
            true,
        ),
        (Some(reported("codex", "herdr:codex", "id", other)), false),
        (None, false),
        (
            Some(reported("codex", "herdr:codex", "path", CODEX_SESSION)),
            false,
        ),
        (
            Some(reported("codex", "herdr:claude", "id", CODEX_SESSION)),
            false,
        ),
        (
            Some(reported("claude", "herdr:codex", "id", CODEX_SESSION)),
            false,
        ),
    ] {
        let (backend, state) = codex_backend(session.clone());
        let result = backend.wake(
            "w1:p1",
            &["-l".to_owned(), "mail token-1".to_owned()],
            "token-1",
            &|_| false,
        );
        assert_eq!(accepted, result.is_ok(), "{session:?}: {result:?}");
        assert_eq!(accepted, state.borrow().prompted, "{session:?}");
        assert_eq!(
            accepted,
            backend.agent_state("w1:p1").is_ok(),
            "{session:?}"
        );
        if !accepted {
            assert_eq!(
                "No Codex CLI agent occupies the owned Herdr pane with its saved session",
                result.unwrap_err().message
            );
        }
    }
}

/// Codex keeps the wake verdict contract: the accepted prompt's sequence is
/// the baseline; a popped mail is consumed, a turn that ended past it with
/// the mail queued is lost, and a working or blocked turn is still pending.
#[test]
fn codex_wake_verdicts_follow_the_owned_session_state() {
    use rules::{WakeVerdict, wake_verdict};
    for (status, seq, queued, verdict) in [
        ("idle", 5.0, false, WakeVerdict::Consumed),
        ("idle", 5.0, true, WakeVerdict::Lost),
        ("done", 5.0, true, WakeVerdict::Lost),
        ("idle", 4.0, true, WakeVerdict::Pending),
        ("working", 5.0, true, WakeVerdict::Pending),
        ("blocked", 6.0, true, WakeVerdict::Pending),
    ] {
        let (backend, state) =
            codex_backend(Some(reported("codex", "herdr:codex", "id", CODEX_SESSION)));
        let delivered = backend
            .wake(
                "w1:p1",
                &["-l".to_owned(), "Mailbox mail-1".to_owned()],
                "mail-1",
                &|_| false,
            )
            .unwrap();
        state.borrow_mut().agent_status = Some(status.to_owned());
        state.borrow_mut().state_change_seq = Some(seq);
        let (status_now, seq_now) = backend.agent_state("w1:p1").unwrap();
        assert_eq!(
            verdict,
            wake_verdict(delivered, &status_now, seq_now, queued),
            "{status} {seq} {queued}"
        );
    }
}

/// A viewer restart rebinds only the original Codex process: the saved
/// session must still occupy the pane, and nothing is launched or resumed.
#[test]
fn codex_bind_requires_the_original_session_and_launches_nothing() {
    let other = "01a1a458-f60d-7561-aa06-d04df3bd14f8";
    for (occupant, value, accepted) in [
        ("codex", CODEX_SESSION, true),
        ("codex", other, false),
        ("claude", CODEX_SESSION, false),
    ] {
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let state: Shared = Rc::new(RefCell::new(FakeState {
            occupant: Some(occupant.to_owned()),
            agent_session: Some(reported(
                occupant,
                &format!("herdr:{occupant}"),
                "id",
                value,
            )),
            ..FakeState::default()
        }));
        let backend = Herdr::new(
            Some(codex_owned()),
            fixture_host(
                fake_cli(calls.clone(), state.clone()),
                fake_owner(state.clone()),
                &Files::new(),
            ),
            config(),
        );
        let result = backend.bind("uml-viewer-A", "w1:p1");
        assert_eq!(accepted, result.is_ok(), "{occupant} {value}: {result:?}");
        let seen = ops(&calls);
        assert!(
            seen.iter()
                .all(|op| !op.starts_with("agent ") && op != "pane run" && op != "pane send-keys"),
            "{seen:?}"
        );
    }
}

/// Claude pops the mail on its own schedule, possibly after finishing the
/// turn it is in; the wake succeeds once Herdr accepts the prompt.
#[test]
fn wake_succeeds_when_the_prompt_is_accepted_while_the_mail_stays_queued() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let backend = backend(fake_cli(calls, state.clone()), fake_owner(state.clone()));
    backend
        .wake(
            "w1:p1",
            &["-l".to_owned(), "mail token-2".to_owned()],
            "token-2",
            &|_| false,
        )
        .unwrap();
    assert!(state.borrow().prompted);
}

/// Herdr reports a prompt that never starts a turn as stalled; the accepted
/// prompt's state sequence is the baseline the lost-mail check compares.
#[test]
fn wake_waits_for_the_turn_to_start_and_returns_its_state_sequence() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let backend = backend(fake_cli(calls.clone(), state.clone()), fake_owner(state));
    let seq = backend
        .wake(
            "w1:p1",
            &["-l".to_owned(), "mail token-4".to_owned()],
            "token-4",
            &|_| false,
        )
        .unwrap();
    assert_eq!(4, seq);
    let prompt = calls
        .borrow()
        .iter()
        .find(|argv| argv.get(3).map(String::as_str) == Some("agent"))
        .cloned()
        .unwrap();
    assert_eq!(
        &["--wait", "--until", "working", "--until", "blocked"],
        &prompt[7..12]
    );
}

#[test]
fn agent_state_reads_the_owned_claude_pane() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState {
        agent_status: Some("done".to_owned()),
        state_change_seq: Some(7.0),
        ..FakeState::default()
    }));
    let backend = backend(fake_cli(calls, state.clone()), fake_owner(state.clone()));
    assert_eq!(
        ("done".to_owned(), 7),
        backend.agent_state("w1:p1").unwrap()
    );
    state.borrow_mut().no_agent = true;
    assert!(backend.agent_state("w1:p1").is_err());
}

#[test]
fn wake_fails_when_herdr_rejects_the_prompt() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState {
        fail: Some(("agent".to_owned(), "prompt".to_owned())),
        ..FakeState::default()
    }));
    let backend = backend(fake_cli(calls, state.clone()), fake_owner(state));
    assert!(
        backend
            .wake(
                "w1:p1",
                &["-l".to_owned(), "mail token-3".to_owned()],
                "token-3",
                &|_| false,
            )
            .is_err()
    );
}

#[test]
fn wake_rejects_preexisting_acknowledgements_and_missing_tokens() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let backend = backend(fake_cli(calls, state.clone()), fake_owner(state.clone()));
    // No token embedded in the literal text.
    assert!(
        backend
            .wake("w1:p1", &["-l".to_owned(), "mail".to_owned()], "", &|_| {
                false
            })
            .is_err()
    );
    // Acknowledged before this delivery.
    assert!(
        backend
            .wake(
                "w1:p1",
                &["-l".to_owned(), "mail token".to_owned()],
                "token",
                &|_| true,
            )
            .is_err()
    );
    assert!(!state.borrow().prompted);
}

#[test]
fn stale_or_malformed_runner_state_is_ignored() {
    let path = "/opt/fixture/project/.uml-viewer/runner.edn";
    let files = Files::new();
    files.put(path, r#"{"token":"stale","phase":"started"}"#);
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let backend = Herdr::new(
        Some(owned()),
        fixture_host(fake_cli(calls, state.clone()), fake_owner(state), &files),
        config(),
    );
    let record = edn::record::record(vec![
        ("runner-state", Value::String(path.to_owned())),
        ("runner-token", Value::String("current".to_owned())),
    ]);
    assert!(backend.runner_state(&record).is_none());
    files.put(path, "broken JSON");
    assert!(backend.runner_state(&record).is_none());
    files.put(path, r#"{"token":"current","phase":"exited","exit":37}"#);
    let value = backend.runner_state(&record).unwrap();
    assert_eq!(Some(37), record::get_i64(&value, "exit"));
}

#[test]
fn socket_paths_over_the_measured_limit_are_refused_before_any_command() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let recorded = calls.clone();
    let run = move |argv: &[String], _: Option<i64>| {
        recorded.borrow_mut().push(argv.to_vec());
        RunOut {
            exit: Some(1),
            out: String::new(),
            err: "CLI reached".to_owned(),
            timeout: false,
        }
    };
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let mut backend = backend(run, fake_owner(state));
    creatable(&mut backend, EXECUTABLE);
    backend.identity = Some(edn::record::record(vec![(
        "provisional",
        Value::Bool(true),
    )]));
    backend.config.max_socket_bytes = 1;
    let error = backend
        .create("uml-viewer-fixture", ROOT, &["claude".to_owned()])
        .unwrap_err();
    assert!(
        error.message.contains("Unix socket limit"),
        "{}",
        error.message
    );
    assert!(calls.borrow().is_empty());
}

#[test]
fn pane_run_retries_only_canonical_mode_rejections_within_the_deadline() {
    // Other failures: a single attempt, no sleep.
    let attempts = Rc::new(RefCell::new(0));
    let sleeps = Rc::new(RefCell::new(0));
    let counted = attempts.clone();
    let slept = sleeps.clone();
    let mut first = backend(
        move |_, _| {
            *counted.borrow_mut() += 1;
            RunOut {
                exit: Some(1),
                out: String::new(),
                err: "pane_not_found".to_owned(),
                timeout: false,
            }
        },
        |_| None,
    );
    first.host.sleep = Rc::new(move |_| *slept.borrow_mut() += 1);
    assert!(first.pane_run("uml-viewer-A", "w1:p1", "text").is_err());
    assert_eq!(1, *attempts.borrow());
    assert_eq!(0, *sleeps.borrow());

    // Canonical mode: retried until the deadline is exhausted.
    let ticks = Rc::new(RefCell::new(vec![0i64, 5000, 20000]));
    let attempts = Rc::new(RefCell::new(0));
    let counted = attempts.clone();
    let clock_ticks = ticks.clone();
    let mut canonical_backend = backend(
        move |_, _| {
            *counted.borrow_mut() += 1;
            RunOut {
                exit: Some(1),
                out: String::new(),
                err: "pane_send_failed: input exceeds the canonical mode limit".to_owned(),
                timeout: false,
            }
        },
        |_| None,
    );
    canonical_backend.host.clock = Rc::new(move || {
        let mut ticks = clock_ticks.borrow_mut();
        if ticks.len() > 1 {
            ticks.remove(0)
        } else {
            ticks[0]
        }
    });
    assert!(
        canonical_backend
            .pane_run("uml-viewer-A", "w1:p1", "text")
            .is_err()
    );
    assert_eq!(2, *attempts.borrow());
}

#[test]
fn create_reads_the_recorded_provisional_as_a_lua_truthy_flag() {
    // herdr.lua:281 `if B.identity and not B.identity.provisional then fail`.
    // #154: `false`/`nil` is a settled ownership; `:yes`/`1` is provisional.
    let refused = |provisional: &str| {
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let state: Shared = Rc::new(RefCell::new(FakeState {
            empty_sessions: true,
            ..FakeState::default()
        }));
        let mut backend = backend(fake_cli(calls, state.clone()), fake_owner(state));
        creatable(&mut backend, "/bin/sh");
        // A provisional record proceeds to the server; stop there.
        backend.hooks.spawn_server = Some(Rc::new(|_, _, _, _| {
            Err(Failure::new("no server in this test"))
        }));
        backend.identity = Some(edn::parse(&format!("{{:session \"s\" {provisional}}}")).unwrap());
        backend
            .create("uml-viewer-A", ROOT, &["/bin/sh".to_owned()])
            .err()
            .is_some_and(|e| e.message.contains("existing Herdr ownership"))
    };
    for v in ["", ":provisional false", ":provisional nil"] {
        assert!(refused(v), "{v:?}");
    }
    for v in [":provisional true", ":provisional :yes", ":provisional 1"] {
        assert!(!refused(v), "{v:?}");
    }
}

#[test]
fn create_refuses_a_command_that_is_not_resolvable() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState {
        empty_sessions: true,
        ..FakeState::default()
    }));
    let mut backend = backend(fake_cli(calls, state.clone()), fake_owner(state));
    creatable(&mut backend, EXECUTABLE);
    backend.identity = Some(edn::record::record(vec![(
        "provisional",
        Value::Bool(true),
    )]));
    let error = backend
        .create(
            "uml-viewer-A",
            ROOT,
            &["definitely-not-on-path-106".to_owned()],
        )
        .unwrap_err();
    assert!(
        error.message.contains("not found on PATH"),
        "{}",
        error.message
    );
}

#[test]
fn a_command_without_argv_is_refused() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let mut backend = backend(fake_cli(calls, state.clone()), fake_owner(state));
    creatable(&mut backend, EXECUTABLE);
    backend.identity = Some(edn::record::record(vec![(
        "provisional",
        Value::Bool(true),
    )]));
    assert!(backend.create("uml-viewer-A", ROOT, &[]).is_err());
}

type Checkpoints = Rc<RefCell<Vec<Value>>>;

/// A create backend over the fixture host: every ownership checkpoint is
/// recorded in order, the server and runner are answered by hooks, and the
/// runner launcher is configured. `recovered` receives the ownership passed
/// to the recovery start hook.
fn create_backend(
    state: Shared,
    calls: Calls,
    files: &Files,
    checkpoints: Checkpoints,
    recovered: Rc<RefCell<Option<Value>>>,
) -> Herdr {
    let mut host = fixture_host(
        fake_cli(calls, state.clone()),
        fake_owner(state.clone()),
        files,
    );
    let inner = host.write.clone();
    host.write = Rc::new(move |path, bytes| {
        if path.ends_with("companion.edn") {
            checkpoints
                .borrow_mut()
                .push(edn::parse(&String::from_utf8_lossy(bytes)).unwrap());
        }
        inner(path, bytes)
    });
    let mut backend = Herdr::new(
        Some(edn::record::record(vec![(
            "provisional",
            Value::Bool(true),
        )])),
        host,
        Config {
            runner: Some(RunnerLauncher {
                argv: vec![
                    "love".to_owned(),
                    "/game".to_owned(),
                    "companion-runner".to_owned(),
                ],
            }),
            ..config()
        },
    );
    creatable(&mut backend, EXECUTABLE);
    backend.hooks = Hooks {
        spawn_server: Some(Rc::new(|_, _, _, _| {
            Ok(Some(Owner {
                pid: 41,
                start: "server-start".to_owned(),
                name: "herdr".to_owned(),
            }))
        })),
        runner_state: Some(Rc::new(|record| {
            Some(edn::record::record(vec![
                (
                    "token",
                    edn::record::get(record, "runner-token")
                        .cloned()
                        .unwrap_or(Value::Nil),
                ),
                ("phase", Value::keyword("started")),
            ]))
        })),
        start_recovery: Some(Rc::new(move |record, _| {
            *recovered.borrow_mut() = Some(record.clone());
            Ok(())
        })),
        cancel_recovery: None,
    };
    backend
}

fn create_state() -> Shared {
    Rc::new(RefCell::new(FakeState {
        empty_sessions: true,
        cwd: Some(ROOT.to_owned()),
        ..FakeState::default()
    }))
}

/// Full create flow with the fake CLI: provisional checkpoints land before
/// the runner is submitted, and the committed ownership keeps the original
/// command argv.
#[test]
fn create_persists_provisional_identity_before_run_and_returns_original_argv() {
    let state = create_state();
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let checkpoints: Checkpoints = Rc::new(RefCell::new(Vec::new()));
    let recovered: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let mut backend = create_backend(
        state.clone(),
        calls,
        &Files::new(),
        checkpoints.clone(),
        recovered.clone(),
    );
    let original = vec![
        EXECUTABLE.to_owned(),
        "space $ & ' 中文".to_owned(),
        String::new(),
    ];
    let identity = backend.create("uml-viewer-A", ROOT, &original).unwrap();
    assert_eq!(
        original,
        record::string_vec(edn::record::seq(edn::record::get(&identity, "command")))
    );
    assert_eq!(None, edn::record::get(&identity, "provisional"));
    assert_eq!(
        Some("term_A".to_owned()),
        record::get_name(&identity, "terminal-id")
    );
    assert!(record::get_name(&identity, "runner-token").is_some());
    let before_run = checkpoints
        .borrow()
        .iter()
        .any(|record| edn::record::is_true(record, "provisional"));
    assert!(before_run);
    for entry in checkpoints.borrow().iter() {
        if edn::record::get(entry, "pane").is_some()
            && edn::record::get(entry, "shell-pid").is_some()
        {
            assert_eq!(Some(42), record::get_i64(entry, "shell-pid"));
            assert_eq!(
                Some("shell-start".to_owned()),
                record::get_name(entry, "owner-start")
            );
        }
    }
    assert!(state.borrow().run_text.is_some());
    let recovered = recovered.borrow().clone().expect("recovery registered");
    assert_eq!(None, edn::record::get(&recovered, "provisional"));
}

#[test]
fn create_waits_for_a_server_bootstrap_beyond_the_ordinary_bound() {
    let state = create_state();
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let run_state = state.clone();
    let run_calls = calls.clone();
    let slow = move |argv: &[String], timeout_ms: Option<i64>| {
        if argv.get(3).map(String::as_str) == Some("workspace")
            && argv.get(4).map(String::as_str) == Some("create")
            && timeout_ms.is_none_or(|timeout| timeout <= 12000)
        {
            return RunOut {
                exit: None,
                out: String::new(),
                err: "process timed out".to_owned(),
                timeout: true,
            };
        }
        fake_cli(run_calls.clone(), run_state.clone())(argv, timeout_ms)
    };
    let checkpoints: Checkpoints = Rc::new(RefCell::new(Vec::new()));
    let recovered: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let mut backend = create_backend(state, calls, &Files::new(), checkpoints, recovered);
    backend.host.run = Rc::new(slow);
    let identity = backend
        .create("uml-viewer-A", ROOT, &[EXECUTABLE.to_owned()])
        .unwrap();
    assert_eq!(
        Some("w1:p1".to_owned()),
        record::get_name(&identity, "pane")
    );
}

/// Launched from inside a Herdr pane, the companion inherits that pane's
/// `HERDR_*` context. The isolated server must not: Herdr 0.9.5 turns an
/// inherited `HERDR_STARTUP_CWD` into an extra startup workspace.
/// The person's own `HERDR_CONFIG_PATH` is not pane context and stays.
#[test]
fn create_starts_the_server_without_the_inherited_herdr_pane_context() {
    let state = create_state();
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let checkpoints: Checkpoints = Rc::new(RefCell::new(Vec::new()));
    let recovered: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let mut backend = create_backend(state, calls, &Files::new(), checkpoints, recovered);
    backend.host.variables = Rc::new(|| {
        strings(&[
            "PATH",
            "HERDR_STARTUP_CWD",
            "HERDR_PANE_ID",
            "HERDR_SOCKET_PATH",
            "HERDR_CONFIG_PATH",
            "XDG_CONFIG_HOME",
        ])
    });
    let removed: Rc<RefCell<Option<Vec<String>>>> = Rc::new(RefCell::new(None));
    let seen = removed.clone();
    backend.hooks.spawn_server = Some(Rc::new(move |_, _, _, remove| {
        *seen.borrow_mut() = Some(remove.to_vec());
        Ok(Some(Owner {
            pid: 41,
            start: "server-start".to_owned(),
            name: "herdr".to_owned(),
        }))
    }));
    backend
        .create("uml-viewer-A", ROOT, &[EXECUTABLE.to_owned()])
        .unwrap();
    assert_eq!(
        Some(strings(&[
            "HERDR_STARTUP_CWD",
            "HERDR_PANE_ID",
            "HERDR_SOCKET_PATH"
        ])),
        removed.borrow().clone()
    );
}

/// Launched from inside a Codex session, the companion inherits its
/// `CODEX_THREAD_ID`; the Herdr Codex hook then suppresses the companion's
/// own session report. The server, and so every pane, starts without it.
#[test]
fn create_starts_the_server_without_an_inherited_codex_thread() {
    let state = create_state();
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let checkpoints: Checkpoints = Rc::new(RefCell::new(Vec::new()));
    let recovered: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let mut backend = create_backend(state, calls, &Files::new(), checkpoints, recovered);
    backend.host.variables = Rc::new(|| strings(&["PATH", "CODEX_THREAD_ID", "CODEX_HOME"]));
    let removed: Rc<RefCell<Option<Vec<String>>>> = Rc::new(RefCell::new(None));
    let seen = removed.clone();
    backend.hooks.spawn_server = Some(Rc::new(move |_, _, _, remove| {
        *seen.borrow_mut() = Some(remove.to_vec());
        Ok(Some(Owner {
            pid: 41,
            start: "server-start".to_owned(),
            name: "herdr".to_owned(),
        }))
    }));
    backend
        .create("uml-viewer-A", ROOT, &[EXECUTABLE.to_owned()])
        .unwrap();
    assert_eq!(
        Some(strings(&["CODEX_THREAD_ID"])),
        removed.borrow().clone()
    );
}

/// The recovery controller fixture: pid 77, started at `ctl`.
fn controller_owner() -> Owner {
    Owner {
        pid: 77,
        start: "ctl".to_owned(),
        name: "uml-viewer".to_owned(),
    }
}

/// One detached spawn as observed: executable, argv, cwd and stdout log.
type Spawned = (String, Vec<String>, String, Option<String>);

/// Without a start hook, create registers the durable controller through the
/// backend's own host: the manifest is journalled first, then the configured
/// controller is spawned detached beside the recovery log, and create
/// returns only once that controller's journal verifies.
#[test]
fn create_starts_the_durable_recovery_controller_through_its_host() {
    let state = create_state();
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let files = Files::new();
    let checkpoints: Checkpoints = Rc::new(RefCell::new(Vec::new()));
    let recovered: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let mut backend = create_backend(state.clone(), calls, &files, checkpoints, recovered);
    backend.hooks.start_recovery = None;
    backend.config.recovery = Some(ControllerSpec {
        executable: "/opt/fixture/uml-viewer".to_owned(),
        argv: vec!["companion-controller".to_owned()],
    });
    let owner_state = state.clone();
    backend.host.owner = Rc::new(move |pid| {
        if pid == 77 {
            return Some(controller_owner());
        }
        fake_owner(owner_state.clone())(pid)
    });
    // The spawned controller registers its journal from the manifest it was
    // handed, exactly as `companion-controller` does.
    let spawned: Rc<RefCell<Vec<Spawned>>> = Rc::new(RefCell::new(Vec::new()));
    let seen = spawned.clone();
    let journal_host = backend.host.clone();
    let journal_files = files.clone();
    backend.host.spawn_detached = Rc::new(move |request: &Detached| {
        seen.borrow_mut().push((
            request.executable.to_owned(),
            request.argv.to_vec(),
            request.cwd.to_owned(),
            request.stdout.map(str::to_owned),
        ));
        let manifest = journal_files
            .get(request.argv.last().unwrap())
            .expect("manifest journalled before the spawn");
        let identity = edn::parse(&manifest).unwrap();
        let controller = edn::record::record(vec![
            ("pid", Value::Integer(77)),
            ("start", Value::String("ctl".to_owned())),
        ]);
        recovery::register(&identity, &controller, &journal_host)?;
        Ok(Some(controller_owner()))
    });
    let owned = backend
        .create("uml-viewer-A", ROOT, &[EXECUTABLE.to_owned()])
        .unwrap();
    let manifest = format!("{ROOT}/.uml-viewer/recovery-launch.edn");
    assert_eq!(
        vec![(
            "/opt/fixture/uml-viewer".to_owned(),
            strings(&["companion-controller", &manifest]),
            ROOT.to_owned(),
            Some(format!("{ROOT}/.uml-viewer/recovery.log")),
        )],
        spawned.borrow().clone()
    );
    let journal = edn::parse(
        &files
            .get(format!("{ROOT}/.uml-viewer/recovery.edn"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        Some("running".to_owned()),
        record::get_name(&journal, "phase")
    );
    assert_eq!(
        record::get_name(&owned, "session"),
        record::get_name(&journal, "session")
    );
}

#[test]
fn create_without_a_controller_launcher_never_spawns_recovery() {
    let state = create_state();
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let files = Files::new();
    let checkpoints: Checkpoints = Rc::new(RefCell::new(Vec::new()));
    let recovered: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let mut backend = create_backend(state, calls, &files, checkpoints, recovered);
    backend.hooks.start_recovery = None;
    let error = backend
        .create("uml-viewer-A", ROOT, &[EXECUTABLE.to_owned()])
        .unwrap_err();
    assert_eq!("A detached controller launcher is required", error.message);
    assert!(
        files
            .get(format!("{ROOT}/.uml-viewer/recovery.edn"))
            .is_none()
    );
}

const CODEX_SESSION: &str = "01a1a458-f60d-7561-aa06-d04df3bd14f7";

/// The `agent_session` the Herdr Codex integration reports.
fn reported(agent: &str, source: &str, kind: &str, value: &str) -> Json {
    object(vec![
        ("agent", Json::String(agent.to_owned())),
        ("source", Json::String(source.to_owned())),
        ("kind", Json::String(kind.to_owned())),
        ("value", Json::String(value.to_owned())),
    ])
}

/// A create backend whose provisional ownership names Codex, with Herdr
/// reporting `occupant` and `session` for the owned pane.
fn codex_create(
    occupant: &str,
    session: Option<Json>,
) -> (
    Herdr,
    Shared,
    Calls,
    Checkpoints,
    Rc<RefCell<Option<Value>>>,
) {
    codex_create_in(occupant, session, &Files::new())
}

fn codex_create_in(
    occupant: &str,
    session: Option<Json>,
    files: &Files,
) -> (
    Herdr,
    Shared,
    Calls,
    Checkpoints,
    Rc<RefCell<Option<Value>>>,
) {
    let state = create_state();
    state.borrow_mut().occupant = Some(occupant.to_owned());
    state.borrow_mut().agent_session = session;
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let checkpoints: Checkpoints = Rc::new(RefCell::new(Vec::new()));
    let recovered: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let mut backend = create_backend(
        state.clone(),
        calls.clone(),
        files,
        checkpoints.clone(),
        recovered.clone(),
    );
    backend.identity = Some(edn::record::record(vec![
        ("coding-agent", Value::keyword("codex")),
        ("provisional", Value::Bool(true)),
    ]));
    (backend, state, calls, checkpoints, recovered)
}

fn codex_fresh() -> Vec<String> {
    crate::agent::CodingAgent::Codex.fresh_command(EXECUTABLE, ROOT, None)
}

/// Fresh Codex has no session id until Herdr reports it; ownership commits
/// and recovery starts only with that id and its explicit resume.
#[test]
fn a_fresh_codex_commits_only_the_session_herdr_reports() {
    let (mut backend, _, _, checkpoints, recovered) = codex_create(
        "codex",
        Some(reported("codex", "herdr:codex", "id", CODEX_SESSION)),
    );
    let owned = backend
        .create("uml-viewer-A", ROOT, &codex_fresh())
        .unwrap();
    let resume = crate::agent::CodingAgent::Codex.recovery_command(EXECUTABLE, ROOT, CODEX_SESSION);
    for record in [
        &owned,
        recovered.borrow().as_ref().expect("recovery started"),
    ] {
        assert_eq!(
            Some(CODEX_SESSION.to_owned()),
            record::get_name(record, "codex-session")
        );
        assert_eq!(None, edn::record::get(record, "claude-session"));
        assert_eq!(
            resume,
            record::string_vec(edn::record::seq(edn::record::get(
                record,
                "recovery-command"
            )))
        );
    }
    // Every committed checkpoint already carries the reported identity.
    for record in checkpoints.borrow().iter() {
        if !edn::record::is_true(record, "provisional") {
            assert!(record::get_name(record, "codex-session").is_some());
        }
    }
}

/// A refused start ran exactly one runner submission, started no recovery,
/// and released this start's workspace and server before clearing ownership.
fn assert_released(
    error: &Failure,
    state: &Shared,
    calls: &Calls,
    checkpoints: &Checkpoints,
    recovered: &Rc<RefCell<Option<Value>>>,
) {
    assert!(
        error
            .message
            .ends_with("this start's Herdr session was released"),
        "{}",
        error.message
    );
    let runs = ops(calls).iter().filter(|op| *op == "pane run").count();
    assert_eq!(1, runs, "no fallback launch");
    assert!(recovered.borrow().is_none(), "no recovery started");
    assert!(state.borrow().closed && state.borrow().stopped);
    assert_eq!(Some(&Value::Map(vec![])), checkpoints.borrow().last());
    for record in checkpoints.borrow().iter() {
        assert_eq!(None, edn::record::get(record, "claude-session"));
        assert_eq!(None, edn::record::get(record, "codex-session"));
    }
}

#[test]
fn a_fresh_codex_refuses_a_foreign_or_malformed_session_and_releases_its_start() {
    for (occupant, session, cause) in [
        (
            "claude",
            reported("claude", "herdr:claude", "id", CODEX_SESSION),
            "not this Codex CLI companion's",
        ),
        (
            "codex",
            reported("claude", "herdr:claude", "id", CODEX_SESSION),
            "not this Codex CLI companion's",
        ),
        (
            "codex",
            reported("codex", "herdr:codex", "path", "/tmp/rollout.jsonl"),
            "not this Codex CLI companion's",
        ),
        (
            "codex",
            reported("codex", "herdr:codex", "id", "latest"),
            "not a UUID",
        ),
    ] {
        let (mut backend, state, calls, checkpoints, recovered) =
            codex_create(occupant, Some(session));
        let error = backend
            .create("uml-viewer-A", ROOT, &codex_fresh())
            .unwrap_err();
        assert!(error.message.contains(cause), "{}", error.message);
        assert_released(&error, &state, &calls, &checkpoints, &recovered);
    }
}

#[test]
fn a_fresh_codex_whose_session_is_never_reported_times_out_and_releases_its_start() {
    let (mut backend, state, calls, checkpoints, recovered) = codex_create("codex", None);
    let now = Rc::new(std::cell::Cell::new(0));
    let clock = now.clone();
    backend.host.clock = Rc::new(move || clock.get());
    backend.host.sleep = Rc::new(move |ms| now.set(now.get() + ms));
    let error = backend
        .create("uml-viewer-A", ROOT, &codex_fresh())
        .unwrap_err();
    assert!(
        error
            .message
            .starts_with("Herdr reported no Codex CLI session within 45 s"),
        "{}",
        error.message
    );
    // Hook trust is not visible to the preflight, so the timeout names it.
    assert!(
        error.message.contains("likely an untrusted Codex hook")
            && error.message.contains("trust Herdr's SessionStart hook"),
        "{}",
        error.message
    );
    assert_released(&error, &state, &calls, &checkpoints, &recovered);
}

/// A refused start also removes the runner files it created (#195): its
/// state, launch manifest and generation lock. Another session's are kept.
#[test]
fn a_refused_fresh_codex_removes_only_its_own_runner_files() {
    let files = Files::new();
    let runner = format!("{ROOT}/.uml-viewer/uml-viewer-A-runner.edn");
    let other = format!("{ROOT}/.uml-viewer/uml-viewer-B-runner.edn");
    files.put(&runner, "{:token \"t\" :phase :started}");
    files.put(format!("{runner}.lock"), "");
    files.put(&other, "{:token \"b\" :phase :started}");
    let (mut backend, state, calls, checkpoints, recovered) = codex_create_in(
        "codex",
        Some(reported("codex", "herdr:codex", "id", "latest")),
        &files,
    );
    let error = backend
        .create("uml-viewer-A", ROOT, &codex_fresh())
        .unwrap_err();
    assert_released(&error, &state, &calls, &checkpoints, &recovered);
    for leftover in [
        runner.clone(),
        format!("{runner}.launch.edn"),
        format!("{runner}.lock"),
    ] {
        assert_eq!(None, files.get(&leftover), "{leftover}");
    }
    assert!(
        files.get(&other).is_some(),
        "another session's runner state"
    );
}

#[test]
fn cleanup_resumes_after_workspace_closure_only_under_the_same_server_identity() {
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState {
        fail: Some(("session".to_owned(), "stop".to_owned())),
        ..FakeState::default()
    }));
    let files = Files::new();
    let backend = Herdr::new(
        Some(owned()),
        fixture_host(
            fake_cli(calls.clone(), state.clone()),
            fake_owner(state.clone()),
            &files,
        ),
        config(),
    );
    assert!(backend.cleanup("uml-viewer-A").is_err());
    let cwd = record::get_name(&owned(), "cwd").unwrap();
    let checkpoint = files
        .get(crate::ownership::path(&cwd))
        .map(|text| edn::parse(&text).unwrap());
    assert_eq!(
        Some("server-stopping".to_owned()),
        checkpoint
            .as_ref()
            .and_then(|record| record::get_name(record, "cleanup-phase"))
    );
    // The stop failure cleared, the journaled checkpoint resumes the cleanup
    // and the workspace close is not repeated.
    state.borrow_mut().fail = None;
    backend.cleanup("uml-viewer-A").unwrap();
    let closes = ops(&calls)
        .iter()
        .filter(|op| *op == "workspace close")
        .count();
    assert_eq!(1, closes);
}

#[test]
fn cleanup_cancels_recovery_before_destroying_any_resource() {
    let mut record = owned();
    edn::record::put(
        &mut record,
        "supervisor",
        edn::record::record(vec![("token", Value::String("controller".to_owned()))]),
    );
    let order: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let order_run = order.clone();
    let order_cancel = order.clone();
    let calls: Calls = Rc::new(RefCell::new(Vec::new()));
    let state: Shared = Rc::new(RefCell::new(FakeState::default()));
    let run_state = state.clone();
    let mut backend = Herdr::new(
        Some(record.clone()),
        fixture_host(
            move |argv: &[String], timeout: Option<i64>| {
                order_run.borrow_mut().push(format!(
                    "{} {}",
                    argv[3],
                    argv.get(4).cloned().unwrap_or_default()
                ));
                fake_cli(calls.clone(), run_state.clone())(argv, timeout)
            },
            fake_owner(state.clone()),
            &Files::new(),
        ),
        config(),
    );
    backend.hooks.cancel_recovery = Some(Rc::new(move |_| {
        order_cancel.borrow_mut().push("cancel".to_owned());
        Ok(())
    }));
    backend.cleanup("uml-viewer-A").unwrap();
    assert_eq!("cancel", order.borrow()[0]);

    let mut backend = Herdr::new(
        Some(record),
        fixture_host(
            |_: &[String], _: Option<i64>| panic!("must not execute"),
            |_| None,
            &Files::new(),
        ),
        config(),
    );
    backend.hooks.cancel_recovery = Some(Rc::new(|_| Err(Failure::new("cannot confirm release"))));
    assert!(backend.cleanup("uml-viewer-A").is_err());
}

/// The durable recovery journal of [`owned`] with a supervisor. Journals live
/// in the host's file store; only the start/cancel gate is a real lock file,
/// because the gate is the POSIX `flock` of ADR 0007.
mod supervised {
    use super::*;
    use std::cell::Cell;

    pub(super) struct Supervised {
        pub record: Value,
        pub files: Files,
        gate: std::path::PathBuf,
    }

    impl Drop for Supervised {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.gate);
        }
    }

    pub(super) fn supervised() -> Supervised {
        static SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let gate = std::env::temp_dir().join(format!(
            "uml herdr gate {} {}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&gate).unwrap();
        let mut record = owned();
        edn::record::put(
            &mut record,
            "supervisor",
            edn::record::record(vec![
                ("token", Value::String("controller".to_owned())),
                (
                    "manifest",
                    Value::String("/s/recovery-launch.edn".to_owned()),
                ),
                ("state-file", Value::String("/s/recovery.edn".to_owned())),
                (
                    "control-file",
                    Value::String("/s/recovery-control.edn".to_owned()),
                ),
                (
                    "gate-file",
                    Value::String(
                        gate.join("recovery-gate.lock")
                            .to_string_lossy()
                            .into_owned(),
                    ),
                ),
                (
                    "lock-file",
                    Value::String("/s/recovery-controller.lock".to_owned()),
                ),
            ]),
        );
        Supervised {
            record,
            files: Files::new(),
            gate,
        }
    }

    /// A fake host over `supervised` whose process table also holds the
    /// registered controller while `alive`; the controller is registered.
    fn host_over(
        supervised: &Supervised,
        state: Shared,
        calls: Calls,
        alive: Rc<Cell<bool>>,
    ) -> Host {
        let owner_state = state.clone();
        let host = fixture_host(
            fake_cli(calls, state),
            move |pid| {
                if pid == 77 {
                    return alive.get().then(controller_owner);
                }
                fake_owner(owner_state.clone())(pid)
            },
            &supervised.files,
        );
        let controller = edn::record::record(vec![
            ("pid", Value::Integer(77)),
            ("start", Value::String("ctl".to_owned())),
        ]);
        recovery::register(&supervised.record, &controller, &host).unwrap();
        host
    }

    /// A backend over `supervised` (see [`host_over`]).
    fn backend_over(
        supervised: &Supervised,
        state: Shared,
        calls: Calls,
        alive: Rc<Cell<bool>>,
    ) -> Herdr {
        let host = host_over(supervised, state, calls, alive);
        Herdr::new(Some(supervised.record.clone()), host, config())
    }

    #[test]
    fn the_supervisor_verify_checks_the_registered_controller_through_the_host() {
        let supervised = supervised();
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let alive = Rc::new(Cell::new(true));
        let host = host_over(&supervised, state, calls, alive.clone());
        assert!(recovery::verify(&supervised.record, &host).is_ok());
        // A vanished controller is never adopted or replaced.
        alive.set(false);
        let error = recovery::verify(&supervised.record, &host).unwrap_err();
        assert_eq!(
            "Recovery controller disappeared; refusing replacement",
            error.message
        );
    }

    /// Contract §2/§6: the cancel is journalled and acknowledged by the
    /// controller before the first destructive command.
    #[test]
    fn cleanup_cancels_the_durable_controller_before_releasing_resources() {
        let supervised = supervised();
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let alive = Rc::new(Cell::new(true));
        let mut backend = backend_over(&supervised, state.clone(), calls.clone(), alive.clone());
        // The controller acknowledges a journalled cancel: terminal phase,
        // then exit.
        let order: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let inner = backend.host.write.clone();
        let files = supervised.files.clone();
        let cancelled = order.clone();
        backend.host.write = Rc::new(move |path, bytes| {
            inner(path, bytes)?;
            if path == Path::new("/s/recovery-control.edn") {
                cancelled.borrow_mut().push("cancel".to_owned());
                let mut journal = edn::parse(&files.get("/s/recovery.edn").unwrap()).unwrap();
                edn::record::put(&mut journal, "phase", Value::keyword("cancelled"));
                inner(
                    Path::new("/s/recovery.edn"),
                    edn::encode(&journal).unwrap().as_bytes(),
                )?;
                alive.set(false);
            }
            Ok(())
        });
        let inner_run = backend.host.run.clone();
        let destroyed = order.clone();
        backend.host.run = Rc::new(move |argv, timeout| {
            destroyed.borrow_mut().push(format!(
                "{} {}",
                argv[3],
                argv.get(4).cloned().unwrap_or_default()
            ));
            inner_run(argv, timeout)
        });
        backend.cleanup("uml-viewer-A").unwrap();
        let order = order.borrow();
        let cancel = order.iter().position(|op| op == "cancel").unwrap();
        let close = order.iter().position(|op| op == "workspace close").unwrap();
        assert!(cancel < close, "{order:?}");
        assert!(state.borrow().deleted);
        let control =
            edn::parse(&supervised.files.get("/s/recovery-control.edn").unwrap()).unwrap();
        assert_eq!(
            Some("controller".to_owned()),
            record::get_name(&control, "cancel")
        );
    }

    #[test]
    fn an_unacknowledged_cancel_retains_every_resource() {
        let supervised = supervised();
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let mut backend = backend_over(
            &supervised,
            state.clone(),
            calls.clone(),
            Rc::new(Cell::new(true)),
        );
        let now = Rc::new(Cell::new(0i64));
        let clock = now.clone();
        backend.host.clock = Rc::new(move || clock.get());
        backend.host.sleep = Rc::new(move |ms| now.set(now.get() + ms));
        let error = backend.cleanup("uml-viewer-A").unwrap_err();
        assert!(
            error
                .message
                .starts_with("Recovery cancellation was not acknowledged; resources retained"),
            "{}",
            error.message
        );
        let seen = ops(&calls).join(",");
        assert!(!seen.contains("workspace close"), "{seen}");
        assert!(!seen.contains("session stop"), "{seen}");
        assert!(!state.borrow().deleted);
    }
}

/// Relaunch: the reset sequence runs before ctrl+c before the submission, a
/// failed reset stops the relaunch, and an occupied foreground refuses.
mod relaunch {
    use super::*;

    type RunOverride = Rc<dyn Fn(&[String], Option<i64>) -> RunOut>;

    fn identity() -> Value {
        let mut identity = owned();
        edn::record::put(
            &mut identity,
            "runner-token",
            Value::String("gen-token".to_owned()),
        );
        edn::record::put(
            &mut identity,
            "runner-state",
            Value::String(
                "/nonexistent-uml-viewer-fixture/project/.uml-viewer/runner.edn".to_owned(),
            ),
        );
        identity
    }

    fn launch_with(state: Shared, run_override: Option<RunOverride>) -> Result<(Calls, Value)> {
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let run_calls = calls.clone();
        let run_state = state.clone();
        let identity = identity();
        let run = run_override.unwrap_or_else(|| {
            Rc::new(move |argv: &[String], timeout: Option<i64>| {
                let text = argv.get(6).cloned().unwrap_or_default();
                let is_pane_run = argv.get(3).map(String::as_str) == Some("pane")
                    && argv.get(4).map(String::as_str) == Some("run");
                if is_pane_run && text.contains("printf") && run_state.borrow().reset_fail {
                    return RunOut {
                        exit: Some(4),
                        out: String::new(),
                        err: "nonexistent directory".to_owned(),
                        timeout: false,
                    };
                }
                if is_pane_run {
                    run_state.borrow_mut().run_text = Some(text.clone());
                }
                fake_cli(run_calls.clone(), run_state.clone())(argv, timeout)
            })
        });
        let mut host = fixture_host(
            move |argv: &[String], timeout: Option<i64>| run(argv, timeout),
            fake_owner(state.clone()),
            &Files::new(),
        );
        // The recorded `claude` resolves on PATH.
        host.variable = Rc::new(|name| (name == "PATH").then(|| "/usr/local/bin".to_owned()));
        host.openable = Rc::new(|path| path == Path::new("/usr/local/bin/claude"));
        let mut backend = Herdr::new(
            Some(identity.clone()),
            host,
            Config::controller_relaunch(
                Some(selected()),
                RunnerLauncher {
                    argv: vec![
                        "love".to_owned(),
                        "/game".to_owned(),
                        "companion-runner".to_owned(),
                    ],
                },
            ),
        );
        backend.relaunch(&identity)?;
        Ok((calls, identity))
    }

    /// The fixture project's Codex ownership over the relaunch identity.
    fn codex_identity(recovery: Option<Value>) -> Value {
        let mut record = identity();
        edn::record::put(&mut record, "coding-agent", Value::keyword("codex"));
        edn::record::put(
            &mut record,
            "codex-session",
            Value::String(CODEX_SESSION.to_owned()),
        );
        edn::record::remove(&mut record, "recovery-command");
        if let Some(recovery) = recovery {
            edn::record::put(&mut record, "recovery-command", recovery);
        }
        record
    }

    fn codex_resume() -> Vec<String> {
        crate::agent::CodingAgent::Codex.recovery_command(
            "/usr/local/bin/codex",
            &record::get_name(&identity(), "cwd").unwrap(),
            CODEX_SESSION,
        )
    }

    /// Relaunch `identity` over the fake CLI: the calls, and the manifest the
    /// submitted runner would read.
    fn codex_relaunch(identity: &Value) -> (Result<()>, Calls, Files) {
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let files = Files::new();
        let mut host = fixture_host(
            fake_cli(calls.clone(), state.clone()),
            fake_owner(state),
            &files,
        );
        host.openable = Rc::new(|path| path == Path::new("/usr/local/bin/codex"));
        let mut backend = Herdr::new(
            Some(identity.clone()),
            host,
            Config::controller_relaunch(
                Some(selected()),
                RunnerLauncher {
                    argv: vec!["uml-viewer".to_owned(), "companion-runner".to_owned()],
                },
            ),
        );
        (backend.relaunch(identity), calls, files)
    }

    #[test]
    fn a_codex_relaunch_submits_the_explicit_resume_in_the_fixed_project() {
        let resume = codex_resume();
        let identity = codex_identity(Some(Value::Vector(
            resume.iter().cloned().map(Value::String).collect(),
        )));
        let (result, _, files) = codex_relaunch(&identity);
        result.unwrap();
        let state = record::get_name(&identity, "runner-state").unwrap();
        let manifest =
            edn::parse(&files.get(format!("{state}.launch.edn")).expect("manifest")).unwrap();
        assert_eq!(
            resume,
            record::string_vec(edn::record::seq(edn::record::get(&manifest, "command")))
        );
        assert_eq!(
            record::get_name(&identity, "cwd"),
            record::get_name(&manifest, "cwd")
        );
        let id = resume.iter().position(|item| item == CODEX_SESSION);
        assert_eq!(Some(resume.len() - 2), id);
        assert!(
            !resume
                .iter()
                .any(|item| item == "--last" || item == "--all")
        );
    }

    #[test]
    fn a_codex_relaunch_without_a_valid_recovery_command_leaves_the_pane_untouched() {
        for recovery in [
            None,
            Some(Value::Nil),
            Some(Value::Vector(vec![])),
            Some(Value::String("codex resume --last".to_owned())),
        ] {
            let (result, calls, _) = codex_relaunch(&codex_identity(recovery));
            let error = result.unwrap_err();
            assert!(error.message.contains("Codex CLI"), "{}", error.message);
            assert!(calls.borrow().is_empty(), "{:?}", calls.borrow());
        }
    }

    #[test]
    fn resets_killed_tui_terminal_modes_and_the_input_line_before_submitting() {
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        let (calls, _) = launch_with(state.clone(), None).unwrap();
        let mut reset_index = None;
        let mut keys_index = None;
        let mut run_index = None;
        for (index, argv) in calls.borrow().iter().enumerate() {
            let op = format!("{} {}", argv[3], argv.get(4).cloned().unwrap_or_default());
            let text = argv.get(6).cloned().unwrap_or_default();
            if op == "pane run" && text.contains("printf") {
                reset_index = Some(index);
            } else if op == "pane send-keys" {
                keys_index = Some(index);
            } else if op == "pane run" && text.contains("companion-runner") {
                run_index = Some(index);
            }
        }
        let (reset, keys, run) = (
            reset_index.expect("reset"),
            keys_index.expect("keys"),
            run_index.expect("run"),
        );
        assert!(reset < keys && keys < run);
        let script = calls.borrow()[reset].join(" ");
        for mode in [
            "[?1049l", "[?2004l", "[<u", "[?1000l", "[?1002l", "[?1003l", "[?1006l",
        ] {
            assert!(
                script.contains(&format!("\\033{mode}")),
                "reset must clear {mode}"
            );
        }
        // `pane run` types its text into the shell. A raw ESC there is a key
        // the line editor swallows, so the modes a SIGTERM'd Codex left on
        // (kitty keyboard flags) stayed set and the ctrl+c that follows
        // arrived as CSI-u junk in front of the runner command (#199).
        assert!(
            script.chars().all(|c| c == ' ' || c.is_ascii_graphic()),
            "{script:?}"
        );
        let text = state.borrow().run_text.clone().expect("runner submitted");
        assert!(text.contains("'companion-runner'"), "{text}");
    }

    #[test]
    fn fails_closed_without_submitting_when_the_terminal_reset_fails() {
        let state: Shared = Rc::new(RefCell::new(FakeState {
            reset_fail: true,
            ..FakeState::default()
        }));
        assert!(launch_with(state.clone(), None).is_err());
        let text = state.borrow().run_text.clone();
        assert!(
            text.is_none()
                || !text
                    .as_deref()
                    .unwrap_or_default()
                    .contains("companion-runner"),
            "the replacement command must never be submitted"
        );
    }

    #[test]
    fn refuses_submission_when_the_pane_foreground_is_unknown_or_occupied() {
        for foreground in [
            None,
            Some(Json::Array(vec![
                object(vec![
                    ("pid", Json::Number(42.0)),
                    ("name", Json::String("bash".to_owned())),
                ]),
                object(vec![
                    ("pid", Json::Number(7.0)),
                    ("name", Json::String("claude".to_owned())),
                ]),
            ])),
        ] {
            let state: Shared = Rc::new(RefCell::new(FakeState {
                foreground_absent: foreground.is_none(),
                foreground,
                ..FakeState::default()
            }));
            assert!(launch_with(state.clone(), None).is_err());
            let text = state.borrow().run_text.clone();
            assert!(
                text.is_none()
                    || !text
                        .as_deref()
                        .unwrap_or_default()
                        .contains("companion-runner"),
                "the replacement command must never be submitted"
            );
        }
    }

    #[test]
    fn retries_only_canonical_mode_rejections_of_the_submitted_command() {
        let state: Shared = Rc::new(RefCell::new(FakeState::default()));
        let attempts = Rc::new(RefCell::new(0));
        let counted = attempts.clone();
        let run_state = state.clone();
        let calls: Calls = Rc::new(RefCell::new(Vec::new()));
        let run_calls = calls.clone();
        let run: RunOverride = Rc::new(move |argv, timeout| {
            let op = (
                argv.get(3).map(String::as_str),
                argv.get(4).map(String::as_str),
            );
            let text = argv.get(6).cloned().unwrap_or_default();
            if op == (Some("pane"), Some("run")) {
                if text.contains("printf") {
                    return response(object(vec![(
                        "result",
                        object(vec![("type", Json::String("ok".to_owned()))]),
                    )]));
                }
                *counted.borrow_mut() += 1;
                if *counted.borrow() < 3 {
                    return RunOut {
                        exit: Some(1),
                        out: String::new(),
                        err: "pane_send_failed: input exceeds the canonical mode limit".to_owned(),
                        timeout: false,
                    };
                }
                run_state.borrow_mut().run_text = Some(text);
                return response(object(vec![(
                    "result",
                    object(vec![("type", Json::String("ok".to_owned()))]),
                )]));
            }
            fake_cli(run_calls.clone(), run_state.clone())(argv, timeout)
        });
        launch_with(state, Some(run)).unwrap();
        assert_eq!(3, *attempts.borrow());
    }
}
