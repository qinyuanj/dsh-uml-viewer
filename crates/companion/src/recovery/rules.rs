//! Pure durable recovery policy (port of `love/uml/domain/recovery.lua`). The
//! controller is the only journal writer; retries are bounded and only a
//! verified abnormal child exit qualifies.

use crate::record;
use crate::vocab::{Failure, Owner, Result};
use edn::Value;

pub const BACKOFF_MS: [i64; 3] = [1000, 2000, 4000];
pub const RESET_MS: i64 = 60000;
pub const GONE_GRACE_MS: i64 = 2000;
pub const SESSION_WAIT_MS: i64 = 45000;

/// The immutable supervisor reference the viewer persists (Lua
/// `R.reference`): a present `:supervisor` on the identity wins, otherwise
/// the conventional paths under the project root.
pub fn reference(identity: &Value) -> Value {
    if let Some(supervisor) = edn::record::get(identity, "supervisor") {
        return supervisor.clone();
    }
    let cwd = record::get_name(identity, "cwd").unwrap_or_else(|| ".".to_owned());
    let root = format!("{cwd}/.uml-viewer");
    edn::record::record(vec![
        ("token", Value::String("controller".to_owned())),
        (
            "manifest",
            Value::String(format!("{root}/recovery-launch.edn")),
        ),
        ("state-file", Value::String(format!("{root}/recovery.edn"))),
        (
            "control-file",
            Value::String(format!("{root}/recovery-control.edn")),
        ),
        (
            "gate-file",
            Value::String(format!("{root}/recovery-gate.lock")),
        ),
        (
            "lock-file",
            Value::String(format!("{root}/recovery-controller.lock")),
        ),
    ])
}

fn supervisor_path(identity: &Value, key: &str) -> String {
    record::get_name(&reference(identity), key).unwrap_or_default()
}

pub fn manifest_path(identity: &Value) -> String {
    supervisor_path(identity, "manifest")
}

pub fn state_file(identity: &Value) -> String {
    supervisor_path(identity, "state-file")
}

pub fn control_file(identity: &Value) -> String {
    supervisor_path(identity, "control-file")
}

pub fn gate_file(identity: &Value) -> String {
    supervisor_path(identity, "gate-file")
}

pub fn lock_file(identity: &Value) -> String {
    supervisor_path(identity, "lock-file")
}

/// The journal must belong to this identity: token, cwd and session all
/// match, or the controller refuses to reason about it.
pub fn matching<'a>(identity: &Value, state: &'a Value) -> Result<&'a Value> {
    let reference = reference(identity);
    let token = record::get_name(&reference, "token");
    let ok = record::get_name(state, "token") == token
        && record::get_name(state, "cwd") == record::get_name(identity, "cwd")
        && record::get_name(state, "session") == record::get_name(identity, "session");
    if !ok {
        return Err(Failure::new(
            "Recovery controller identity is missing or changed",
        ));
    }
    Ok(state)
}

pub fn terminal(phase: &str) -> bool {
    matches!(phase, "failed" | "cancelled" | "normal")
}

pub fn initial(identity: &Value, controller: &Value, now: i64) -> Value {
    let reference = reference(identity);
    let mut state = edn::record::record(vec![
        (
            "token",
            edn::record::get(&reference, "token")
                .cloned()
                .unwrap_or(Value::Nil),
        ),
        (
            "session",
            edn::record::get(identity, "session")
                .cloned()
                .unwrap_or(Value::Nil),
        ),
        (
            "cwd",
            edn::record::get(identity, "cwd")
                .cloned()
                .unwrap_or(Value::Nil),
        ),
        ("controller", controller.clone()),
        ("phase", Value::keyword("running")),
        ("generation", Value::Integer(0)),
        ("attempts", Value::Integer(0)),
        ("submitted-at", Value::Integer(now)),
    ]);
    if let Some(token) = edn::record::get(identity, "runner-token") {
        edn::record::put(&mut state, "runner-token", token.clone());
    }
    if let Some(path) = edn::record::get(identity, "runner-state") {
        edn::record::put(&mut state, "runner-state", path.clone());
    }
    state
}

fn same_child(a: &Value, b: &Value) -> bool {
    record::is_map(a)
        && record::is_map(b)
        && edn::record::get(a, "pid") == edn::record::get(b, "pid")
        && edn::record::get(a, "start") == edn::record::get(b, "start")
}

fn child_present(child: Option<&Value>, owner: &dyn Fn(i64) -> Option<Owner>) -> bool {
    let Some(child) = child else { return false };
    let Some(pid) = record::get_i64(child, "pid") else {
        return false;
    };
    let Ok(observed) = owner_as_result(owner, pid) else {
        return false;
    };
    match observed {
        Some(observed) => {
            i64::from(observed.pid) == pid
                && Some(observed.start.as_str()) == record::get_name(child, "start").as_deref()
        }
        None => false,
    }
}

fn owner_as_result(owner: &dyn Fn(i64) -> Option<Owner>, pid: i64) -> Result<Option<Owner>> {
    if pid < 1 || pid > i32::MAX as i64 {
        return Ok(None);
    }
    Ok(owner(pid))
}

fn child_absent(child: Option<&Value>, owner: &dyn Fn(i64) -> Option<Owner>) -> bool {
    let Some(child) = child else { return false };
    let Some(pid) = record::get_i64(child, "pid") else {
        return false;
    };
    if pid < 1 {
        return false;
    }
    if record::get_name(child, "start").is_none() {
        return false;
    }
    matches!(owner_as_result(owner, pid), Ok(None))
}

fn stopped(state: &Value, reason: &str) -> Value {
    let mut result = state.clone();
    edn::record::put(&mut result, "phase", Value::keyword("failed"));
    edn::record::put(&mut result, "reason", Value::String(reason.to_owned()));
    result
}

pub struct Decision {
    pub state: Value,
    pub changed: bool,
    pub launch: bool,
}

pub struct Context<'a> {
    pub identity: &'a Value,
    pub state: &'a Value,
    pub control: Option<&'a Value>,
    pub event: Option<&'a Value>,
    pub now: i64,
    pub owner: &'a dyn Fn(i64) -> Option<Owner>,
    pub token: &'a dyn Fn() -> String,
    pub runner_state: &'a dyn Fn(i64) -> String,
}

fn phase(state: &Value) -> String {
    record::get_name(state, "phase").unwrap_or_default()
}

fn get_i64(state: &Value, key: &str) -> Option<i64> {
    record::get_i64(state, key)
}

/// One controller tick (Lua `R.decide`). The durable cancel beats any other
/// transition; a persisted `:submitting` is never replayed.
pub fn decide(ctx: &Context) -> Result<Decision> {
    let state = ctx.state;
    let state = matching(ctx.identity, state)?;
    let reference = reference(ctx.identity);
    let now = ctx.now;
    if let Some(control) = ctx.control
        && record::get_name(control, "cancel") == record::get_name(&reference, "token")
    {
        let mut result = state.clone();
        edn::record::put(&mut result, "phase", Value::keyword("cancelled"));
        edn::record::put(
            &mut result,
            "reason",
            Value::String("Viewer closed; recovery revoked".to_owned()),
        );
        return Ok(Decision {
            state: result,
            changed: true,
            launch: false,
        });
    }
    if terminal(&phase(state)) {
        return Ok(Decision {
            state: state.clone(),
            changed: false,
            launch: false,
        });
    }
    if phase(state) == "backoff" {
        if now < get_i64(state, "deadline").unwrap_or(now) {
            return Ok(Decision {
                state: state.clone(),
                changed: false,
                launch: false,
            });
        }
        let generation = get_i64(state, "generation").unwrap_or(0) + 1;
        let mut pending = state.clone();
        edn::record::put(&mut pending, "phase", Value::keyword("submitting"));
        edn::record::put(&mut pending, "generation", Value::Integer(generation));
        edn::record::put(
            &mut pending,
            "attempts",
            Value::Integer(get_i64(state, "attempts").unwrap_or(0) + 1),
        );
        edn::record::put(&mut pending, "runner-token", Value::String((ctx.token)()));
        edn::record::put(
            &mut pending,
            "runner-state",
            Value::String((ctx.runner_state)(generation)),
        );
        edn::record::put(&mut pending, "submitted-at", Value::Integer(now));
        edn::record::remove(&mut pending, "deadline");
        edn::record::remove(&mut pending, "uptime-since");
        edn::record::remove(&mut pending, "child");
        if crate::agent::identity_of(ctx.identity)?
            .agent
            .awaits_reported_session()
        {
            edn::record::put(&mut pending, "session-check", Value::Bool(true));
        }
        return Ok(Decision {
            state: pending,
            changed: true,
            launch: true,
        });
    }
    if phase(state) == "submitting" {
        return Ok(Decision {
            state: stopped(
                state,
                "Unacknowledged recovery submission; refusing duplicate",
            ),
            changed: true,
            launch: false,
        });
    }
    let Some(event) = ctx.event else {
        if now - get_i64(state, "submitted-at").unwrap_or(now) > 30000 {
            return Ok(Decision {
                state: stopped(state, "Runner acknowledgement missing; exit cause unknown"),
                changed: true,
                launch: false,
            });
        }
        return Ok(Decision {
            state: state.clone(),
            changed: false,
            launch: false,
        });
    };
    if record::get_name(state, "runner-token") != record::get_name(event, "token") {
        return Ok(Decision {
            state: stopped(state, "Runner generation changed; exit cause unknown"),
            changed: true,
            launch: false,
        });
    }
    match record::get_name(event, "phase").as_deref() {
        Some("started") => {
            let child = edn::record::get(event, "child");
            if child_present(child, ctx.owner) {
                let mut result = state.clone();
                let since = get_i64(state, "uptime-since").unwrap_or(now);
                edn::record::put(&mut result, "child", child.cloned().unwrap_or(Value::Nil));
                edn::record::put(&mut result, "uptime-since", Value::Integer(since));
                edn::record::remove(&mut result, "child-gone-since");
                if now - since >= RESET_MS {
                    edn::record::put(&mut result, "attempts", Value::Integer(0));
                }
                let changed = result != *state;
                return Ok(Decision {
                    state: result,
                    changed,
                    launch: false,
                });
            }
            let gone_since = get_i64(state, "child-gone-since").unwrap_or(now);
            if now - gone_since > GONE_GRACE_MS {
                return Ok(Decision {
                    state: stopped(
                        state,
                        "Runner child disappeared without a matching exit; recovery unsupported",
                    ),
                    changed: true,
                    launch: false,
                });
            }
            let mut result = state.clone();
            edn::record::put(&mut result, "child-gone-since", Value::Integer(gone_since));
            let changed = result != *state;
            Ok(Decision {
                state: result,
                changed,
                launch: false,
            })
        }
        Some("exited") => {
            let exit = get_i64(event, "exit");
            let child = edn::record::get(event, "child");
            let valid = exit.is_some()
                && child_absent(child, ctx.owner)
                && (edn::record::get(state, "child").is_none()
                    || same_child(
                        child.unwrap_or(&Value::Nil),
                        edn::record::get(state, "child").unwrap(),
                    ));
            if !valid {
                return Ok(Decision {
                    state: stopped(state, "Unverified child exit; recovery unsupported"),
                    changed: true,
                    launch: false,
                });
            }
            let exit = exit.unwrap_or(0);
            if exit == 0 {
                let mut result = state.clone();
                edn::record::put(&mut result, "phase", Value::keyword("normal"));
                edn::record::put(&mut result, "child", child.cloned().unwrap_or(Value::Nil));
                edn::record::put(&mut result, "exit", Value::Integer(0));
                edn::record::put(
                    &mut result,
                    "reason",
                    Value::String("Companion exited normally".to_owned()),
                );
                return Ok(Decision {
                    state: result,
                    changed: true,
                    launch: false,
                });
            }
            let attempts = get_i64(state, "attempts").unwrap_or(0);
            if attempts >= BACKOFF_MS.len() as i64 {
                let mut result = stopped(state, "Recovery exhausted after three retries");
                edn::record::put(&mut result, "exit", Value::Integer(exit));
                return Ok(Decision {
                    state: result,
                    changed: true,
                    launch: false,
                });
            }
            let mut result = state.clone();
            edn::record::put(&mut result, "phase", Value::keyword("backoff"));
            edn::record::put(&mut result, "child", child.cloned().unwrap_or(Value::Nil));
            edn::record::put(&mut result, "exit", Value::Integer(exit));
            edn::record::put(
                &mut result,
                "deadline",
                Value::Integer(now + BACKOFF_MS[attempts as usize]),
            );
            Ok(Decision {
                state: result,
                changed: true,
                launch: false,
            })
        }
        _ => Ok(Decision {
            state: stopped(state, "Runner exit cause unknown; recovery unsupported"),
            changed: true,
            launch: false,
        }),
    }
}

/// A relaunched session whose id Herdr reports stays unproven until the
/// report names the saved session. Only a started child can have reported.
pub fn session_unproven(state: &Value) -> bool {
    phase(state) == "running"
        && edn::record::flag(state, "session-check")
        && edn::record::get(state, "child").is_some()
}

/// Apply one report check: proven clears the mark, another session stops
/// recovery at once, and silence stops it at the deadline.
pub fn session_checked(identity: &Value, state: &Value, check: Result<bool>, now: i64) -> Value {
    match check {
        Ok(true) => {
            let mut result = state.clone();
            edn::record::remove(&mut result, "session-check");
            result
        }
        Ok(false) if now - get_i64(state, "submitted-at").unwrap_or(now) < SESSION_WAIT_MS => {
            state.clone()
        }
        Ok(false) => stopped(
            state,
            &format!(
                "Herdr reported no {} session within {} s of recovery",
                crate::agent::identity_of(identity)
                    .map(|owned| owned.agent.label())
                    .unwrap_or("coding-agent"),
                SESSION_WAIT_MS / 1000
            ),
        ),
        Err(error) => stopped(
            state,
            &format!("Recovery session check refused: {}", error.message),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Value {
        edn::parse(
            r#"{:cwd "/r" :session "uml-viewer-A" :runner-token "initial"
                :runner-state "/r/.uml-viewer/runner.edn"
                :supervisor {:token "controller"
                             :manifest "/r/.uml-viewer/recovery-launch.edn"
                             :state-file "/r/.uml-viewer/recovery.edn"
                             :control-file "/r/.uml-viewer/recovery-control.edn"
                             :gate-file "/r/.uml-viewer/recovery-gate.lock"
                             :lock-file "/r/.uml-viewer/recovery-controller.lock"}}"#,
        )
        .unwrap()
    }

    fn registered(identity: &Value) -> Value {
        initial(
            identity,
            &edn::record::record(vec![
                ("pid", Value::Integer(10)),
                ("start", Value::String("start".to_owned())),
            ]),
            0,
        )
    }

    fn exit_event(token: &str, code: i64) -> Value {
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

    fn started_event(token: &str) -> Value {
        edn::record::record(vec![
            ("token", Value::String(token.to_owned())),
            ("phase", Value::keyword("started")),
            (
                "child",
                edn::record::record(vec![
                    ("pid", Value::Integer(20)),
                    ("start", Value::String("child".to_owned())),
                ]),
            ),
        ])
    }

    fn no_owner(_: i64) -> Option<Owner> {
        None
    }

    fn ctx<'a>(
        identity: &'a Value,
        state: &'a Value,
        event: Option<&'a Value>,
        now: i64,
        owner: &'a dyn Fn(i64) -> Option<Owner>,
    ) -> Context<'a> {
        Context {
            identity,
            state,
            control: None,
            event,
            now,
            owner,
            token: &|| "new-token".to_owned(),
            runner_state: &|generation| {
                format!("/r/.uml-viewer/uml-viewer-A-runner-{generation}.edn")
            },
        }
    }

    #[test]
    fn a_verified_abnormal_exit_earns_a_finite_first_retry_deadline() {
        let identity = identity();
        let state = registered(&identity);
        let event = exit_event("initial", 37);
        let decision = decide(&ctx(&identity, &state, Some(&event), 1000, &no_owner)).unwrap();
        assert_eq!("backoff", phase(&decision.state));
        assert_eq!(Some(2000), get_i64(&decision.state, "deadline"));
        assert_eq!(Some(0), get_i64(&decision.state, "attempts"));
        assert!(!decision.launch);
        // Before the deadline nothing happens; at it, a new generation is
        // submitted with a fresh runner token.
        let pending = decide(&ctx(&identity, &decision.state, None, 1999, &no_owner)).unwrap();
        assert!(!pending.launch);
        let submit = decide(&ctx(&identity, &decision.state, None, 2000, &no_owner)).unwrap();
        assert!(submit.launch);
        assert_eq!(Some(1), get_i64(&submit.state, "attempts"));
        assert_eq!(Some(1), get_i64(&submit.state, "generation"));
        assert_eq!(
            Some("new-token".to_owned()),
            record::get_name(&submit.state, "runner-token")
        );
    }

    #[test]
    fn retries_exhaust_at_exactly_three_with_one_two_four_second_budgets() {
        let identity = identity();
        let mut state = registered(&identity);
        for (at, deadline, attempts) in [(0, 1000, 1), (1100, 3100, 2), (3200, 7200, 3)] {
            let event = exit_event(&record::get_name(&state, "runner-token").unwrap(), 37);
            let decision = decide(&ctx(&identity, &state, Some(&event), at, &no_owner)).unwrap();
            assert_eq!(Some(deadline), get_i64(&decision.state, "deadline"));
            let early = decide(&ctx(
                &identity,
                &decision.state,
                None,
                deadline - 1,
                &no_owner,
            ))
            .unwrap();
            assert!(!early.launch);
            let submit =
                decide(&ctx(&identity, &decision.state, None, deadline, &no_owner)).unwrap();
            assert!(submit.launch);
            assert_eq!(Some(attempts), get_i64(&submit.state, "attempts"));
            // A successful submission returns to running.
            let mut running = submit.state.clone();
            edn::record::put(&mut running, "phase", Value::keyword("running"));
            state = running;
        }
        let event = exit_event(&record::get_name(&state, "runner-token").unwrap(), 37);
        let decision = decide(&ctx(&identity, &state, Some(&event), 7300, &no_owner)).unwrap();
        assert_eq!("failed", phase(&decision.state));
        assert_eq!(
            Some("Recovery exhausted after three retries".to_owned()),
            record::get_name(&decision.state, "reason")
        );
        let after = decide(&ctx(&identity, &decision.state, None, 100000, &no_owner)).unwrap();
        assert!(!after.launch);
    }

    #[test]
    fn the_budget_resets_only_after_sixty_seconds_of_verified_uptime() {
        let identity = identity();
        let state = registered(&identity);
        let event = exit_event("initial", 37);
        let backoff = decide(&ctx(&identity, &state, Some(&event), 0, &no_owner)).unwrap();
        let submit = decide(&ctx(&identity, &backoff.state, None, 1000, &no_owner)).unwrap();
        let mut running = submit.state.clone();
        edn::record::put(&mut running, "phase", Value::keyword("running"));
        let token = record::get_name(&running, "runner-token").unwrap();
        let alive = |pid: i64| {
            (pid == 20).then(|| Owner {
                pid: 20,
                start: "child".to_owned(),
                name: "child".to_owned(),
            })
        };
        let event = started_event(&token);
        let at_1100 = decide(&ctx(&identity, &running, Some(&event), 1100, &alive)).unwrap();
        let at_61099 =
            decide(&ctx(&identity, &at_1100.state, Some(&event), 61099, &alive)).unwrap();
        assert_eq!(Some(1), get_i64(&at_61099.state, "attempts"));
        let at_61100 = decide(&ctx(
            &identity,
            &at_61099.state,
            Some(&event),
            61100,
            &alive,
        ))
        .unwrap();
        assert_eq!(Some(0), get_i64(&at_61100.state, "attempts"));
        let exit = exit_event(&token, 37);
        let next = decide(&ctx(
            &identity,
            &at_61100.state,
            Some(&exit),
            61200,
            &no_owner,
        ))
        .unwrap();
        assert_eq!(Some(62200), get_i64(&next.state, "deadline"));
    }

    #[test]
    fn normal_stale_or_unverifiable_exits_are_never_recovered() {
        for event in [exit_event("initial", 0), exit_event("stale", 37), {
            let mut e = exit_event("initial", 37);
            edn::record::remove(&mut e, "child");
            e
        }] {
            let identity = identity();
            let state = registered(&identity);
            let decision = decide(&ctx(&identity, &state, Some(&event), 0, &no_owner)).unwrap();
            let name = phase(&decision.state);
            assert!(name == "normal" || name == "failed", "{name}");
            let later = decide(&ctx(&identity, &decision.state, None, 99999, &no_owner)).unwrap();
            assert!(!later.launch);
        }
    }

    #[test]
    fn a_bounded_grace_lets_the_runner_record_the_exit_first() {
        let identity = identity();
        let state = registered(&identity);
        let event = started_event("initial");
        let first = decide(&ctx(&identity, &state, Some(&event), 0, &no_owner)).unwrap();
        assert_eq!("running", phase(&first.state));
        assert_eq!(Some(0), get_i64(&first.state, "child-gone-since"));
        let exit = exit_event("initial", 37);
        let next = decide(&ctx(&identity, &first.state, Some(&exit), 100, &no_owner)).unwrap();
        assert_eq!("backoff", phase(&next.state));
        assert_eq!(Some(1100), get_i64(&next.state, "deadline"));
    }

    #[test]
    fn the_grace_window_is_bounded_and_fail_closed() {
        let identity = identity();
        let state = registered(&identity);
        let event = started_event("initial");
        let first = decide(&ctx(&identity, &state, Some(&event), 1000, &no_owner)).unwrap();
        let at_2999 = decide(&ctx(&identity, &first.state, Some(&event), 2999, &no_owner)).unwrap();
        let at_3000 = decide(&ctx(
            &identity,
            &at_2999.state,
            Some(&event),
            3000,
            &no_owner,
        ))
        .unwrap();
        assert_eq!("running", phase(&at_3000.state));
        let at_3001 = decide(&ctx(
            &identity,
            &at_3000.state,
            Some(&event),
            3001,
            &no_owner,
        ))
        .unwrap();
        assert_eq!("failed", phase(&at_3001.state));
        assert_eq!(
            Some(
                "Runner child disappeared without a matching exit; recovery unsupported".to_owned()
            ),
            record::get_name(&at_3001.state, "reason")
        );
    }

    #[test]
    fn the_grace_marker_clears_when_the_child_is_verified_alive_again() {
        let identity = identity();
        let state = registered(&identity);
        let event = started_event("initial");
        let first = decide(&ctx(&identity, &state, Some(&event), 0, &no_owner)).unwrap();
        assert_eq!(Some(0), get_i64(&first.state, "child-gone-since"));
        let alive = |pid: i64| {
            (pid == 20).then(|| Owner {
                pid: 20,
                start: "child".to_owned(),
                name: "child".to_owned(),
            })
        };
        let next = decide(&ctx(&identity, &first.state, Some(&event), 100, &alive)).unwrap();
        assert_eq!(None, edn::record::get(&next.state, "child-gone-since"));
        assert_eq!("running", phase(&next.state));
    }

    #[test]
    fn a_durable_cancel_beats_a_due_retry() {
        let identity = identity();
        let state = registered(&identity);
        let event = exit_event("initial", 37);
        let backoff = decide(&ctx(&identity, &state, Some(&event), 0, &no_owner)).unwrap();
        let control = edn::record::record(vec![("cancel", Value::String("controller".to_owned()))]);
        let mut context = ctx(&identity, &backoff.state, None, 1000, &no_owner);
        context.control = Some(&control);
        let decision = decide(&context).unwrap();
        assert_eq!("cancelled", phase(&decision.state));
        assert!(!decision.launch);
    }

    #[test]
    fn an_unacknowledged_submission_is_never_replayed() {
        let identity = identity();
        let state = registered(&identity);
        let event = exit_event("initial", 37);
        let backoff = decide(&ctx(&identity, &state, Some(&event), 0, &no_owner)).unwrap();
        let submit = decide(&ctx(&identity, &backoff.state, None, 1000, &no_owner)).unwrap();
        assert!(submit.launch);
        // The submission is persisted as :submitting; a later tick must not
        // relaunch, it stops instead.
        let again = decide(&ctx(&identity, &submit.state, None, 999999, &no_owner)).unwrap();
        assert!(!again.launch);
        assert_eq!("failed", phase(&again.state));
        assert_eq!(
            Some("Unacknowledged recovery submission; refusing duplicate".to_owned()),
            record::get_name(&again.state, "reason")
        );
    }

    #[test]
    fn a_missing_runner_acknowledgement_stops_after_thirty_seconds() {
        let identity = identity();
        let state = registered(&identity);
        let early = decide(&ctx(&identity, &state, None, 29999, &no_owner)).unwrap();
        assert_eq!("running", phase(&early.state));
        let late = decide(&ctx(&identity, &state, None, 30001, &no_owner)).unwrap();
        assert_eq!("failed", phase(&late.state));
        assert_eq!(
            Some("Runner acknowledgement missing; exit cause unknown".to_owned()),
            record::get_name(&late.state, "reason")
        );
    }

    #[test]
    fn matching_rejects_a_foreign_or_missing_journal_identity() {
        let identity = identity();
        let state = registered(&identity);
        assert!(matching(&identity, &state).is_ok());
        let mut foreign = state.clone();
        edn::record::put(
            &mut foreign,
            "session",
            Value::String("uml-viewer-B".to_owned()),
        );
        assert!(matching(&identity, &foreign).is_err());
    }
}
