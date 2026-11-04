//! Herdr session backend (port of `love/uml/adapters/herdr.lua`). Every CLI
//! request selects the recorded session; the selected executable is fixed for
//! this process. Ownership is EDN on disk and EDN maps in memory.

use crate::host::{Detached, Host};
use crate::json::Json;
use crate::record;
use crate::recovery;
use crate::text::shell_word;
use crate::vocab::{Failure, Owner, Result, RunOut};
use edn::Value;
use std::path::Path;
use std::rc::Rc;

/// Herdr derives its API socket from XDG_CONFIG_HOME + session name. Measured
/// against Herdr 0.9.5: the server starts at 101 bytes but workspace creation
/// fails from 102 bytes, and that failure surfaces as an opaque closed-channel
/// error. The generated name therefore shrinks to whatever the config root
/// leaves, instead of assuming a fixed project-name budget.
pub const MAX_SOCKET_BYTES: usize = 101;

/// The first `workspace create` waits out the server bootstrap; WSL PATH
/// entries under /mnt/c stretch it past the ordinary command bound (contract
/// §6).
const BOOTSTRAP_MS: i64 = 60000;

/// A dropped `pane run` is indistinguishable from a slow runner start, so
/// the submission is retried a bounded number of times before failing.
const ACK_ATTEMPTS: u32 = 3;
const ACK_WAIT_MS: i64 = 10000;

/// How long a fresh start waits for Herdr to report a provider's session
/// (Codex's SessionStart hook), within the worker's 120 s startup bound.
const IDENTITY_WAIT_MS: i64 = 45000;

pub use select::Selected;

/// How the runner role is launched inside the pane: the argv the pane's shell
/// receives after quoting (the manifest path is appended per generation).
#[derive(Clone)]
pub struct RunnerLauncher {
    pub argv: Vec<String>,
}

/// How the detached controller is spawned (`spawn_detached` prepends the
/// executable).
#[derive(Clone)]
pub struct ControllerSpec {
    pub executable: String,
    pub argv: Vec<String>,
}

/// Server spawn hook: (executable, argv, cwd, inherited variables to remove).
type ServerSpawn = Rc<dyn Fn(&str, &[String], &str, &[String]) -> Result<Option<Owner>>>;
type RunnerStateFn = Rc<dyn Fn(&Value) -> Option<Value>>;
type StartRecovery = Rc<dyn Fn(&Value, Option<&ControllerSpec>) -> Result<()>>;
type CancelRecovery = Rc<dyn Fn(&Value) -> Result<()>>;
/// Reports one create/relaunch stage. The worker uses it to publish a
/// progress event and to let a cancellation land between boundaries, so it
/// may refuse and stop the startup.
pub type ProgressFn = Rc<dyn Fn(&str) -> Result<()>>;
type SelectFn = Rc<dyn Fn() -> Result<Selected>>;

/// What one backend is configured to do, apart from its host: the selected
/// Herdr, the runner and recovery controller launchers, the measured socket
/// bound, the progress callback and a deferred selection.
#[derive(Clone)]
pub struct Config {
    /// The selected Herdr for this process; every CLI call uses it.
    pub selected: Option<Selected>,
    pub runner: Option<RunnerLauncher>,
    pub recovery: Option<ControllerSpec>,
    pub max_socket_bytes: usize,
    pub progress: Option<ProgressFn>,
    pub select: Option<SelectFn>,
}

impl Config {
    fn plain(selected: Option<Selected>) -> Config {
        Config {
            selected,
            runner: None,
            recovery: None,
            max_socket_bytes: MAX_SOCKET_BYTES,
            progress: None,
            select: None,
        }
    }

    /// The detached controller's relaunch backend: the selected Herdr and the
    /// runner role only. It carries no recovery controller launcher, exactly
    /// as before #160; whether that is intended is a separate question.
    pub fn controller_relaunch(selected: Option<Selected>, runner: RunnerLauncher) -> Config {
        Config {
            runner: Some(runner),
            ..Config::plain(selected)
        }
    }

    /// The `companion-smoke` create backend: runner and recovery controller.
    pub fn smoke(selected: Selected, runner: RunnerLauncher, recovery: ControllerSpec) -> Config {
        Config {
            runner: Some(runner),
            recovery: Some(recovery),
            ..Config::plain(Some(selected))
        }
    }

    /// A cleanup-only backend: the selected Herdr and nothing else.
    pub fn backend(selected: Selected) -> Config {
        Config::plain(Some(selected))
    }

    /// The lifecycle backend (create, restart, shutdown): runner, recovery
    /// controller and the worker's progress callback.
    pub fn lifecycle(
        selected: Selected,
        progress: Option<ProgressFn>,
        runner: RunnerLauncher,
        recovery: ControllerSpec,
    ) -> Config {
        Config {
            runner: Some(runner),
            recovery: Some(recovery),
            progress,
            ..Config::plain(Some(selected))
        }
    }
}

/// Test hooks over whole backend steps, kept from before #160 (server spawn,
/// runner state, recovery start and cancel). Production leaves them all
/// absent; an absent hook runs the real step on the backend's host.
#[derive(Clone, Default)]
pub struct Hooks {
    pub spawn_server: Option<ServerSpawn>,
    pub runner_state: Option<RunnerStateFn>,
    pub start_recovery: Option<StartRecovery>,
    pub cancel_recovery: Option<CancelRecovery>,
}

pub struct Herdr {
    pub identity: Option<Value>,
    /// Every capability, including the one recovery runs on.
    pub host: Host,
    pub config: Config,
    pub hooks: Hooks,
    selected: Option<Selected>,
}

fn strings(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|s| (*s).to_owned()).collect()
}

impl Herdr {
    pub fn new(identity: Option<Value>, host: Host, config: Config) -> Herdr {
        let selected = config.selected.clone();
        Herdr {
            identity,
            host,
            config,
            hooks: Hooks::default(),
            selected,
        }
    }

    pub fn herdr(&self) -> String {
        self.selected
            .as_ref()
            .map(|selected| selected.path.clone())
            .unwrap_or_else(|| "herdr".to_owned())
    }

    fn selected(&mut self) -> Result<Selected> {
        if let Some(selected) = &self.selected {
            return Ok(selected.clone());
        }
        if let Some(select) = &self.config.select {
            self.selected = Some(select()?);
            return Ok(self.selected.clone().unwrap());
        }
        Err(Failure::new("Herdr executable was not selected"))
    }

    fn fail<T>(&self, message: impl Into<String>) -> Result<T> {
        Err(Failure::new(message.into()))
    }

    fn checked(&self, argv: &[String], timeout_ms: Option<i64>) -> Result<RunOut> {
        let result = (self.host.run)(argv, timeout_ms);
        if result.timeout || result.exit != Some(0) {
            return self.fail(format!(
                "Companion command failed: {}: {}",
                argv[0], result.err
            ));
        }
        Ok(result)
    }

    fn parse(&self, result: &RunOut) -> Result<Json> {
        let value =
            crate::json::decode(&result.out).map_err(|_| Failure::new("Malformed Herdr JSON"))?;
        if !matches!(value, Json::Object(_)) {
            return self.fail("Malformed Herdr JSON");
        }
        if let Some(error) = value.get("error")
            && !matches!(error, Json::Null)
        {
            return self.fail("Herdr API error");
        }
        Ok(value)
    }

    fn cli_within(&self, timeout_ms: Option<i64>, session: &str, args: &[&str]) -> Result<Json> {
        let mut argv = vec![self.herdr(), "--session".to_owned(), session.to_owned()];
        argv.extend(args.iter().map(|s| (*s).to_owned()));
        self.parse(&self.checked(&argv, timeout_ms)?)
    }

    pub fn cli(&self, session: &str, args: &[&str]) -> Result<Json> {
        self.cli_within(None, session, args)
    }

    pub fn session_check(&self, session: &str) -> Result<()> {
        if !rules::session_valid(session) {
            return self.fail("Herdr requires an isolated uml-viewer named session");
        }
        Ok(())
    }

    pub fn status(&self, session: &str) -> Result<Json> {
        let value = self.cli(session, &["status", "server", "--json"])?;
        let socket = value.get("socket").and_then(Json::as_str);
        let valid = value.get("session").and_then(Json::as_str) == Some(session)
            && socket.is_some_and(|socket| socket.starts_with('/'))
            && matches!(value.get("running"), Some(Json::Bool(_)));
        if !valid {
            return self.fail("Ambiguous Herdr endpoint");
        }
        Ok(value)
    }

    pub fn compatible(&self, status: &Json) -> Result<()> {
        let ok = status.get("running") == Some(&Json::Bool(true))
            && status
                .get("version")
                .and_then(Json::as_str)
                .is_some_and(select::herdr_version_supported)
            && status.get("protocol").and_then(Json::as_i64) == Some(22)
            && status.get("compatible") == Some(&Json::Bool(true))
            && status.get("endpoint_compatible") == Some(&Json::Bool(true));
        if !ok {
            return self.fail("Herdr endpoint is not a verified >=0.9.3/protocol 22 server");
        }
        Ok(())
    }

    pub fn snapshot(&self, session: &str) -> Result<Json> {
        let data = self.cli(session, &["api", "snapshot"])?;
        let snapshot = data.get("result").and_then(|r| r.get("snapshot"));
        let valid = snapshot.is_some_and(|snapshot| {
            snapshot
                .get("version")
                .and_then(Json::as_str)
                .is_some_and(select::herdr_version_supported)
                && snapshot.get("protocol").and_then(Json::as_i64) == Some(22)
                && snapshot.get("panes").and_then(Json::array_items).is_some()
                && snapshot
                    .get("workspaces")
                    .and_then(Json::array_items)
                    .is_some()
        });
        match snapshot {
            Some(snapshot) if valid => Ok(snapshot.clone()),
            _ => self.fail("Invalid Herdr snapshot"),
        }
    }

    /// `/proc` identity of an owned process; the recorded pid is opaque text
    /// or an integer and must be a plain positive decimal.
    pub fn process_owner(&self, pid: &Value) -> Result<Option<Owner>> {
        let text = match pid {
            Value::Integer(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => String::new(),
        };
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) || text.starts_with('0') {
            return self.fail("Invalid owned process PID");
        }
        let pid: i64 = text
            .parse()
            .map_err(|_| Failure::new("Invalid owned process PID"))?;
        Ok((self.host.owner)(pid))
    }

    pub fn owner_matches(&self, pid: &Value, start: &str) -> Result<Owner> {
        let observed = self.process_owner(pid)?;
        match observed {
            Some(observed) if observed.start == start => Ok(observed),
            _ => self.fail("Herdr process creation identity changed"),
        }
    }

    pub fn poll<T>(
        &self,
        timeout_ms: i64,
        interval_ms: i64,
        probe: &dyn Fn() -> Option<T>,
        message: &str,
    ) -> Result<T> {
        let deadline = (self.host.clock)() + timeout_ms;
        loop {
            if let Some(value) = probe() {
                return Ok(value);
            }
            if (self.host.clock)() < deadline {
                (self.host.sleep)(interval_ms);
            } else {
                return self.fail(message);
            }
        }
    }

    pub fn launch_server(&self, session: &str, cwd: &str) -> Result<Owner> {
        // Server diagnostics belong beside the project's other lifecycle
        // journals; without them a failed start is an unexplained timeout.
        let remove = rules::inherited_pane_env(&(self.host.variables)());
        let owner = match &self.hooks.spawn_server {
            Some(spawn) => spawn(
                &self.herdr(),
                &strings(&["--session", session, "server"]),
                cwd,
                &remove,
            )?,
            None => {
                let log = Path::new(cwd)
                    .join(".uml-viewer/herdr-server.log")
                    .to_string_lossy()
                    .into_owned();
                let err = format!("{log}.err");
                (self.host.spawn_detached)(&Detached {
                    executable: &self.herdr(),
                    argv: &strings(&["--session", session, "server"]),
                    cwd,
                    stdout: Some(&log),
                    stderr: Some(&err),
                    remove: &remove,
                })?
            }
        };
        owner.ok_or_else(|| Failure::new("Detached Herdr server exited"))
    }

    pub fn observe(&self, expected: &Value, session: &str, pane: &str) -> Result<()> {
        rules::complete(expected)?;
        if record::get_name(expected, "session").as_deref() != Some(session)
            || record::get_name(expected, "pane").as_deref() != Some(pane)
        {
            return self.fail("Herdr selector conflicts with ownership");
        }
        let status = self.status(session)?;
        self.compatible(&status)?;
        if status.get("socket").and_then(Json::as_str)
            != record::get_name(expected, "socket").as_deref()
        {
            return self.fail("Herdr socket changed");
        }
        let server_pid = edn::record::get(expected, "server-pid")
            .cloned()
            .unwrap_or(Value::Nil);
        let server_start = record::get_name(expected, "server-start").unwrap_or_default();
        self.owner_matches(&server_pid, &server_start)?;
        let snapshot = self.snapshot(session)?;
        let info = self.cli(session, &["pane", "process-info", "--pane", pane])?;
        let process_info = info.get("result").and_then(|r| r.get("process_info"));
        rules::observe(
            expected,
            &status,
            &snapshot,
            process_info,
            &*self.host.canonical,
        )?;
        let shell_pid = edn::record::get(expected, "shell-pid")
            .cloned()
            .unwrap_or(Value::Nil);
        let owner_start = record::get_name(expected, "owner-start").unwrap_or_default();
        self.owner_matches(&shell_pid, &owner_start)?;
        Ok(())
    }

    /// The runner's state file, parsed as EDN (or JSON for older records);
    /// only the current generation token counts.
    pub fn runner_state(&self, record: &Value) -> Option<Value> {
        if let Some(injected) = &self.hooks.runner_state {
            return injected(record);
        }
        let path = record::get_name(record, "runner-state")?;
        let text = (self.host.read)(Path::new(&path)).ok().flatten()?;
        let value = edn::parse(&text)
            .ok()
            .or_else(|| crate::json::decode(&text).ok().map(json_to_edn))?;
        if record::get_name(&value, "token") != record::get_name(record, "runner-token") {
            return None;
        }
        record::get_name(&value, "phase")?;
        Some(value)
    }

    /// The runner must exec the agent directly so its recorded identity is
    /// the agent's own process, never a shell that re-parses argv. Script
    /// wrappers are refused up front: an unreadable or PATH-missing
    /// executable cannot prove its child identity.
    pub fn require_direct_command(&self, command: &[String]) -> Result<String> {
        let first = command.first().cloned().unwrap_or_default();
        if first.is_empty() {
            return self.fail("Herdr command must be a nonempty argv vector");
        }
        let mut resolved = first.clone();
        if !first.contains('/') {
            let path = (self.host.variable)("PATH").unwrap_or_default();
            for directory in path.split(':') {
                let candidate = format!("{directory}/{first}");
                if (self.host.openable)(Path::new(&candidate)) {
                    resolved = candidate;
                    break;
                }
            }
        }
        if !(self.host.openable)(Path::new(&resolved)) {
            return self.fail(format!("Companion executable not found on PATH: {first}"));
        }
        Ok(resolved)
    }

    pub fn runner_command(&self, record: &Value, command: &[String]) -> Result<Vec<String>> {
        let first = self.require_direct_command(command)?;
        let command = if first != command[0] {
            let mut resolved = command.to_vec();
            resolved[0] = first;
            resolved
        } else {
            command.to_vec()
        };
        let state_path = record::get_name(record, "runner-state")
            .ok_or_else(|| Failure::new("Herdr ownership has no runner state path"))?;
        let manifest = format!("{state_path}.launch.edn");
        let mut copy = record.clone();
        edn::record::put(
            &mut copy,
            "command",
            Value::Vector(
                command
                    .iter()
                    .map(|arg| Value::String(arg.clone()))
                    .collect(),
            ),
        );
        recovery::write_state(Path::new(&manifest), &copy, &self.host)?;
        let runner = self
            .config
            .runner
            .as_ref()
            .ok_or_else(|| Failure::new("Herdr runner launcher is not configured"))?;
        let mut argv = runner.argv.clone();
        argv.push(manifest);
        Ok(argv)
    }

    /// `pane run` retries only canonical-mode rejections, within the ordinary
    /// command bound.
    pub fn pane_run(&self, session: &str, pane: &str, text: &str) -> Result<()> {
        let argv = vec![
            self.herdr(),
            "--session".to_owned(),
            session.to_owned(),
            "pane".to_owned(),
            "run".to_owned(),
            pane.to_owned(),
            text.to_owned(),
        ];
        let deadline = (self.host.clock)() + 10000;
        loop {
            let result = (self.host.run)(&argv, None);
            if result.exit == Some(0) && !result.timeout {
                return Ok(());
            }
            let output = format!("{}{}", result.out, result.err);
            if (self.host.clock)() < deadline && output.contains("canonical mode") {
                (self.host.sleep)(100);
            } else {
                return self.fail("Herdr pane run failed");
            }
        }
    }

    /// Reset what a previous agent left enabled. Mouse tracking modes go
    /// first: a pane still reporting SGR mouse events delivers them to the
    /// replacement agent as keystrokes. On POSIX the sequence is written to
    /// the pane's own terminal by running printf inside that pane. `pane run`
    /// types the text, so ESC is spelled `\033` for printf to expand: a typed
    /// ESC byte is a key to the line editor and never reaches the terminal.
    pub fn terminal_reset(&self, record: &Value) -> Result<()> {
        const RESET_FORMAT: &str = r"\033[?1003l\033[?1002l\033[?1006l\033[?1000l\033[?1049l\033[?2004l\033[<u\033[<u\033[<u\033[?25h";
        let session = record::get_name(record, "session").unwrap_or_default();
        let pane = record::get_name(record, "pane").unwrap_or_default();
        self.pane_run(
            &session,
            &pane,
            &format!("printf {}", shell_word(RESET_FORMAT)),
        )?;
        (self.host.sleep)(250);
        self.checked(
            &[
                self.herdr(),
                "--session".to_owned(),
                session.clone(),
                "pane".to_owned(),
                "send-keys".to_owned(),
                pane,
                "ctrl+c".to_owned(),
            ],
            None,
        )?;
        (self.host.sleep)(150);
        Ok(())
    }

    /// Recovery relaunch: re-verify, reset the pane, submit the runner with
    /// the explicit recovery command on the same pane. A recovery command the
    /// provider refuses stops here, before the pane is touched.
    pub fn relaunch(&mut self, record: &Value) -> Result<()> {
        let _ = self.selected()?;
        let command = crate::agent::relaunch_command(record)?;
        let session = record::get_name(record, "session").unwrap_or_default();
        let pane = record::get_name(record, "pane").unwrap_or_default();
        self.observe(record, &session, &pane)?;
        let info = self.cli(&session, &["pane", "process-info", "--pane", &pane])?;
        let process_info = info.get("result").and_then(|r| r.get("process_info"));
        let shell_pid = record::get_i64(record, "shell-pid").unwrap_or(0);
        rules::foreground_allowed(process_info, shell_pid)?;
        self.terminal_reset(record)?;
        let shell = self
            .process_owner(
                &edn::record::get(record, "shell-pid")
                    .cloned()
                    .unwrap_or(Value::Nil),
            )?
            .ok_or_else(|| Failure::new("Herdr process creation identity changed"))?;
        let argv = self.runner_command(record, &command)?;
        self.pane_run(&session, &pane, &rules::pane_command(&argv, &shell.name)?)
    }

    /// Whether the pane reports the saved session after a recovery relaunch.
    /// `false` while nothing is reported (or Herdr cannot say yet); a report
    /// of anything else is refused, not waited on.
    pub fn resumed_session(&self, record: &Value) -> Result<bool> {
        let owned = crate::agent::identity_of(record)?;
        let session = record::get_name(record, "session").unwrap_or_default();
        let pane = record::get_name(record, "pane").unwrap_or_default();
        let terminal = record::get_name(record, "terminal-id").unwrap_or_default();
        let Ok(snapshot) = self.snapshot(&session) else {
            return Ok(false);
        };
        match rules::reported_session(&snapshot, &pane, &terminal, owned.agent)? {
            None => Ok(false),
            Some(reported) if Some(&reported) == owned.session.as_ref() => Ok(true),
            Some(_) => self.fail(format!(
                "Herdr reported another {} session in the owned pane; its saved session did not resume",
                owned.agent.diagnostic_name()
            )),
        }
    }

    fn cancel_recovery(&self, record: &Value) -> Result<()> {
        if let Some(cancel) = &self.hooks.cancel_recovery {
            return cancel(record);
        }
        recovery::cancel(record, &self.host)
    }

    /// Herdr derives its API socket from XDG_CONFIG_HOME and the session
    /// name. Measured against 0.9.5: a 101-byte path works and 102 fails, and
    /// the failure surfaces only later as a closed-channel error on workspace
    /// create, so refuse up front with the actual cause.
    pub fn socket_check(&self, session: &str) -> Result<()> {
        let variable = &self.host.variable;
        let root = variable("XDG_CONFIG_HOME")
            .filter(|root| !root.is_empty())
            .unwrap_or_else(|| format!("{}/.config", variable("HOME").unwrap_or_default()));
        let path = format!(
            "{}/herdr/sessions/{session}/herdr.sock",
            root.trim_end_matches('/')
        );
        let limit = self.config.max_socket_bytes;
        if path.len() > limit {
            return self.fail(format!(
                "Herdr socket path exceeds the Unix socket limit ({} > {limit}): {path}; set a shorter XDG_CONFIG_HOME or project path",
                path.len()
            ));
        }
        Ok(())
    }

    /// A fresh pane's shell is not ready for input the moment `workspace
    /// create` returns: the first `pane run` is discarded while the shell
    /// boots (measured on Herdr 0.9.5; it fails silently with exit 0). Wait
    /// for the pane's first output, bounded, before the first submission.
    /// `pane read` prints terminal text, not JSON, so it bypasses `cli`.
    fn await_pane_ready(&self, session: &str, pane: &str) -> Result<()> {
        let argv = vec![
            self.herdr(),
            "--session".to_owned(),
            session.to_owned(),
            "pane".to_owned(),
            "read".to_owned(),
            pane.to_owned(),
        ];
        self.poll(
            15000,
            100,
            &|| {
                let result = (self.host.run)(&argv, None);
                (result.exit == Some(0) && !result.timeout && !result.out.trim().is_empty())
                    .then_some(())
            },
            "Herdr pane shell never became ready",
        )?;
        Ok(())
    }

    pub fn create(&mut self, session: &str, cwd: &str, command: &[String]) -> Result<Value> {
        self.session_check(session)?;
        self.socket_check(session)?;
        if command.is_empty() {
            return self.fail("Herdr command must be a nonempty argv vector");
        }
        let resolved = self.require_direct_command(command)?;
        let command = if resolved != command[0] {
            let mut command = command.to_vec();
            command[0] = resolved;
            command
        } else {
            command.to_vec()
        };
        if self
            .identity
            .as_ref()
            .is_some_and(|identity| !edn::record::flag(identity, "provisional"))
        {
            return self.fail("Cannot create over existing Herdr ownership");
        }
        let sessions = self.cli(session, &["session", "list", "--json"])?;
        let inventory = sessions
            .get("sessions")
            .and_then(Json::array_items)
            .ok_or_else(|| Failure::new("Ambiguous session inventory"))?;
        for entry in inventory {
            if entry.get("name").and_then(Json::as_str) == Some(session) {
                return self.fail("Herdr named session already exists");
            }
        }
        let canonical_cwd = (self.host.canonical)(cwd);
        let mut base =
            rules::ownership_base(self.identity.as_ref(), session, &canonical_cwd, &command);
        crate::ownership::write(&self.host, &canonical_cwd, &base)?;
        self.progress("server")?;
        let owner = self.launch_server(session, &canonical_cwd)?;
        edn::record::put(
            &mut base,
            "server-pid",
            Value::String(owner.pid.to_string()),
        );
        edn::record::put(
            &mut base,
            "server-start",
            Value::String(owner.start.clone()),
        );
        crate::ownership::write(&self.host, &canonical_cwd, &base)?;
        let status = self.poll(
            5000,
            50,
            &|| match self.status(session) {
                Ok(value) if value.get("running") == Some(&Json::Bool(true)) => Some(value),
                _ => None,
            },
            "Herdr server did not become reachable",
        )?;
        self.compatible(&status)?;
        edn::record::put(
            &mut base,
            "socket",
            Value::String(
                status
                    .get("socket")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ),
        );
        crate::ownership::write(&self.host, &canonical_cwd, &base)?;
        self.progress("workspace")?;
        let created = self.cli_within(
            Some(BOOTSTRAP_MS),
            session,
            &["workspace", "create", "--cwd", &canonical_cwd, "--no-focus"],
        )?;
        let result = created
            .get("result")
            .ok_or_else(|| Failure::new("Malformed Herdr JSON"))?;
        let pane = result
            .get("root_pane")
            .ok_or_else(|| Failure::new("Malformed Herdr JSON"))?;
        let workspace = result
            .get("workspace")
            .and_then(|w| w.get("workspace_id"))
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        edn::record::put(&mut base, "workspace", Value::String(workspace));
        edn::record::put(
            &mut base,
            "pane",
            Value::String(
                pane.get("pane_id")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ),
        );
        edn::record::put(
            &mut base,
            "terminal-id",
            Value::String(
                pane.get("terminal_id")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ),
        );
        crate::ownership::write(&self.host, &canonical_cwd, &base)?;
        let pane_id = record::get_name(&base, "pane").unwrap_or_default();
        let info = self.cli(session, &["pane", "process-info", "--pane", &pane_id])?;
        let process_info = info
            .get("result")
            .and_then(|r| r.get("process_info"))
            .cloned()
            .ok_or_else(|| Failure::new("Malformed Herdr JSON"))?;
        let shell_pid = process_info
            .get("shell_pid")
            .and_then(Json::as_i64)
            .ok_or_else(|| Failure::new("Malformed Herdr JSON"))?;
        let shell = self
            .process_owner(&Value::Integer(shell_pid))?
            .ok_or_else(|| Failure::new("Herdr process creation identity changed"))?;
        edn::record::put(&mut base, "shell-pid", Value::Integer(shell_pid));
        edn::record::put(&mut base, "owner-start", Value::String(shell.start.clone()));
        edn::record::put(
            &mut base,
            "runner-token",
            Value::String((self.host.token)()),
        );
        let root = format!("{canonical_cwd}/.uml-viewer/");
        let runner_state = format!("{root}{session}-runner.edn");
        edn::record::put(&mut base, "runner-state", Value::String(runner_state));
        edn::record::put(
            &mut base,
            "supervisor",
            edn::record::record(vec![
                ("token", Value::String((self.host.token)())),
                (
                    "manifest",
                    Value::String(format!("{root}recovery-launch.edn")),
                ),
                ("state-file", Value::String(format!("{root}recovery.edn"))),
                (
                    "control-file",
                    Value::String(format!("{root}recovery-control.edn")),
                ),
                (
                    "gate-file",
                    Value::String(format!("{root}recovery-gate.lock")),
                ),
                (
                    "lock-file",
                    Value::String(format!("{root}recovery-controller.lock")),
                ),
            ]),
        );
        crate::ownership::write(&self.host, &canonical_cwd, &base)?;
        self.observe(&base, session, &pane_id)?;
        self.progress("runner")?;
        self.await_pane_ready(session, &pane_id)?;
        let argv = self.runner_command(&base, &command)?;
        let submission = rules::pane_command(&argv, &shell.name)?;
        // Herdr 0.9.5 can still swallow the submission after the pane has
        // printed its first output, and it reports success when it does.
        // Resubmit while no runner state appears: the runner takes a
        // generation lock, so a duplicate simply exits and cannot run twice.
        let mut acknowledged = None;
        for attempt in 0..ACK_ATTEMPTS {
            self.pane_run(session, &pane_id, &submission)?;
            match self.poll(
                ACK_WAIT_MS,
                100,
                &|| self.runner_state(&base),
                "Herdr runner did not acknowledge startup",
            ) {
                Ok(state) => {
                    acknowledged = Some(state);
                    break;
                }
                Err(error) => {
                    if attempt + 1 == ACK_ATTEMPTS {
                        return Err(error);
                    }
                }
            }
        }
        let state = acknowledged.ok_or_else(|| Failure::new("No runner acknowledgement"))?;
        let phase = record::get_name(&state, "phase").unwrap_or_default();
        let clean_exit = phase == "exited" && record::get_i64(&state, "exit") == Some(0);
        if phase != "started" && !clean_exit {
            return self.fail("Herdr runner failed during startup");
        }
        self.observe(&base, session, &pane_id)?;
        let agent = crate::agent::identity_of(&base)?.agent;
        if agent.awaits_reported_session()
            && let Err(cause) = self.await_reported_session(&mut base, agent, &pane_id, &command[0])
        {
            return Err(self.release_fresh(&base, cause));
        }
        let owned = rules::committed(&base);
        crate::ownership::write(&self.host, &canonical_cwd, &owned)?;
        self.progress("recovery")?;
        let start_recovery = self.hooks.start_recovery.clone();
        match start_recovery {
            Some(start) => start(&owned, self.config.recovery.as_ref())?,
            None => {
                recovery::start(&owned, &self.host, &|identity, manifest| {
                    let controller = self.config.recovery.as_ref().ok_or_else(|| {
                        Failure::new("A detached controller launcher is required")
                    })?;
                    let mut argv = controller.argv.clone();
                    argv.push(manifest.to_owned());
                    let log = crate::recovery::rules::state_file(identity)
                        .replace("recovery.edn", "recovery.log");
                    let err = format!("{log}.err");
                    (self.host.spawn_detached)(&Detached {
                        executable: &controller.executable,
                        argv: &argv,
                        cwd: &canonical_cwd,
                        stdout: Some(&log),
                        stderr: Some(&err),
                        remove: &[],
                    })?;
                    Ok(())
                })?;
            }
        }
        self.progress("committed")?;
        self.identity = Some(owned.clone());
        Ok(owned)
    }

    /// Wait for Herdr to report the session `agent` runs in the owned pane,
    /// then record it with its explicit recovery command. Nothing else
    /// stands in for it: no other provider, latest or new session.
    fn await_reported_session(
        &self,
        base: &mut Value,
        agent: crate::agent::CodingAgent,
        pane: &str,
        executable: &str,
    ) -> Result<()> {
        let session = record::get_name(base, "session").unwrap_or_default();
        let terminal = record::get_name(base, "terminal-id").unwrap_or_default();
        let cwd = record::get_name(base, "cwd").unwrap_or_default();
        let reported = self.poll(
            IDENTITY_WAIT_MS,
            250,
            &|| match self
                .snapshot(&session)
                .and_then(|snapshot| rules::reported_session(&snapshot, pane, &terminal, agent))
            {
                Ok(None) => None,
                outcome => Some(outcome),
            },
            &format!(
                "Herdr reported no {} session within {} s{}; start refused",
                agent.diagnostic_name(),
                IDENTITY_WAIT_MS / 1000,
                agent.unreported_session_cause()
            ),
        )??;
        let identity = crate::agent::SessionIdentity {
            agent,
            session: reported,
        };
        for (key, value) in identity.ownership_fields(executable, &cwd) {
            edn::record::put(base, key, value);
        }
        crate::ownership::write(&self.host, &cwd, base)
    }

    /// A fresh start that cannot prove its session releases what it created:
    /// its own Herdr session, runner files and ownership. No recovery
    /// controller runs yet, so there is nothing to cancel first. A failed
    /// release retains the ownership and the files it still describes.
    fn release_fresh(&mut self, base: &Value, cause: Failure) -> Failure {
        let mut record = base.clone();
        edn::record::remove(&mut record, "supervisor");
        let session = record::get_name(&record, "session").unwrap_or_default();
        let cwd = record::get_name(&record, "cwd").unwrap_or_default();
        self.identity = Some(record.clone());
        let released = crate::ownership::write(&self.host, &cwd, &record)
            .and_then(|()| self.cleanup(&session))
            .map(|()| {
                if let Some(state) = record::get_name(&record, "runner-state") {
                    for suffix in ["", ".launch.edn", ".lock"] {
                        (self.host.remove)(Path::new(&format!("{state}{suffix}")));
                    }
                }
            })
            .and_then(|()| crate::ownership::write(&self.host, &cwd, &Value::Map(vec![])));
        Failure::new(match released {
            Ok(()) => format!("{}; this start's Herdr session was released", cause.message),
            Err(error) => format!(
                "{}; releasing this start's Herdr session failed: {}",
                cause.message, error.message
            ),
        })
    }

    pub fn probe(&self, session: &str) -> Result<bool> {
        let identity = self
            .identity
            .as_ref()
            .ok_or_else(|| Failure::new("Incomplete Herdr ownership"))?
            .clone();
        let pane = record::get_name(&identity, "pane").unwrap_or_default();
        self.observe(&identity, session, &pane)?;
        Ok(true)
    }

    /// Rebind the owned pane. A provider whose session Herdr reports is
    /// rebound only while that original session still occupies it; nothing
    /// is launched or resumed in its place.
    pub fn bind(&self, session: &str, pane: &str) -> Result<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or_else(|| Failure::new("Incomplete Herdr ownership"))?
            .clone();
        self.observe(&identity, session, pane)?;
        if crate::agent::identity_of(&identity)?
            .agent
            .awaits_reported_session()
        {
            self.owned_agent(&identity, session, pane)?;
        }
        Ok(pane.to_owned())
    }

    /// Wake delivers literal text carrying this delivery's unique token and
    /// returns once the owned agent is working on it, with the state sequence
    /// Herdr reported then. The agent pops the mail on its own schedule; the durable
    /// queue keeps it until then (contract §5).
    pub fn wake(
        &self,
        pane: &str,
        keys: &[String],
        token: &str,
        acknowledged: &dyn Fn(&str) -> bool,
    ) -> Result<i64> {
        rules::wake_arguments(token, keys)?;
        let identity = self
            .identity
            .as_ref()
            .ok_or_else(|| Failure::new("Incomplete Herdr ownership"))?
            .clone();
        let session = record::get_name(&identity, "session").unwrap_or_default();
        self.observe(&identity, &session, pane)?;
        if acknowledged(token) {
            return self.fail("Herdr wake acknowledgement predates this delivery");
        }
        self.owned_agent(&identity, &session, pane)?;
        // Herdr returns agent_prompt_stalled when the prompt starts no turn.
        let accepted = self.cli(
            &session,
            &[
                "agent", "prompt", pane, &keys[1], "--wait", "--until", "working", "--until",
                "blocked",
            ],
        )?;
        let seq = accepted
            .get("result")
            .and_then(|result| result.get("agent"))
            .and_then(|agent| agent.get("state_change_seq"))
            .and_then(Json::as_i64)
            .ok_or_else(|| Failure::new("Herdr prompt result has no state sequence"))?;
        self.observe(&identity, &session, pane)?;
        Ok(seq)
    }

    /// The owned pane's agent status and state sequence.
    pub fn agent_state(&self, pane: &str) -> Result<(String, i64)> {
        let identity = self
            .identity
            .as_ref()
            .ok_or_else(|| Failure::new("Incomplete Herdr ownership"))?
            .clone();
        let session = record::get_name(&identity, "session").unwrap_or_default();
        let agent = self.owned_agent(&identity, &session, pane)?;
        match (
            agent.get("agent_status").and_then(Json::as_str),
            agent.get("state_change_seq").and_then(Json::as_i64),
        ) {
            (Some(status), Some(seq)) => Ok((status.to_owned(), seq)),
            _ => self.fail("Herdr agent state is incomplete"),
        }
    }

    /// The agent occupying the owned pane, which must be the provider the
    /// ownership names; no other agent is accepted in its place. A provider
    /// whose session Herdr reports must still report the saved one: another
    /// session started later in the same pane is not this companion's.
    fn owned_agent(&self, identity: &Value, session: &str, pane: &str) -> Result<Json> {
        let owned = crate::agent::identity_of(identity)?;
        let agent = owned.agent;
        let snapshot = self.snapshot(session)?;
        let terminal_id = record::get_name(identity, "terminal-id").unwrap_or_default();
        let occupant = snapshot
            .get("agents")
            .and_then(Json::array_items)
            .and_then(|agents| {
                agents.iter().find(|entry| {
                    entry.get("pane_id").and_then(Json::as_str) == Some(pane)
                        && entry.get("terminal_id").and_then(Json::as_str)
                            == Some(terminal_id.as_str())
                        && entry.get("agent").and_then(Json::as_str) == Some(agent.name())
                })
            })
            .cloned()
            .ok_or_else(|| {
                Failure::new(format!(
                    "No {} agent occupies the owned Herdr pane",
                    agent.diagnostic_name()
                ))
            })?;
        if agent.awaits_reported_session()
            && !matches!(
                rules::reported_session(&snapshot, pane, &terminal_id, agent),
                Ok(Some(reported)) if Some(&reported) == owned.session.as_ref()
            )
        {
            return self.fail(format!(
                "No {} agent occupies the owned Herdr pane with its saved session",
                agent.diagnostic_name()
            ));
        }
        Ok(occupant)
    }

    /// Cleanup: cancel recovery first, then destroy only the recorded
    /// resources, checkpointing before each destructive step so an interrupted
    /// cleanup resumes where it stopped (contract §2, §6).
    pub fn cleanup(&self, session: &str) -> Result<()> {
        let record = self
            .identity
            .as_ref()
            .ok_or_else(|| Failure::new("Incomplete Herdr ownership"))?
            .clone();
        rules::complete(&record)?;
        if session != record::get_name(&record, "session").unwrap_or_default() {
            return self.fail("Cleanup selector conflicts with ownership");
        }
        if edn::record::get(&record, "supervisor").is_some() {
            self.cancel_recovery(&record)?;
        }
        let cwd = record::get_name(&record, "cwd").unwrap_or_default();
        let saved = crate::ownership::read_in(&self.host, &cwd)?;
        if let Some(saved) = &saved
            && !record::is_empty_map(saved)
            && rules::ownership_changed(&record, saved)
        {
            return self.fail("Cleanup ownership changed");
        }
        let checkpoint = |record: &Value, phase: &str| -> Result<()> {
            let mut value = record.clone();
            edn::record::put(&mut value, "cleanup-phase", Value::keyword(phase));
            crate::ownership::write(&self.host, &cwd, &value)
        };
        let mut phase = saved
            .as_ref()
            .and_then(|saved| record::get_name(saved, "cleanup-phase"))
            .or_else(|| record::get_name(&record, "cleanup-phase"));
        let mut endpoint = self.status(session)?;
        if endpoint.get("socket").and_then(Json::as_str)
            != record::get_name(&record, "socket").as_deref()
        {
            return self.fail("Cleanup socket changed");
        }
        if phase.is_none() {
            self.observe(
                &record,
                session,
                &record::get_name(&record, "pane").unwrap_or_default(),
            )?;
            checkpoint(&record, "workspace-closing")?;
            phase = Some("workspace-closing".to_owned());
        }
        if matches!(
            phase.as_deref(),
            Some("workspace-closing") | Some("workspace-closed")
        ) {
            self.compatible(&endpoint)?;
            let server_pid = edn::record::get(&record, "server-pid")
                .cloned()
                .unwrap_or(Value::Nil);
            let server_start = record::get_name(&record, "server-start").unwrap_or_default();
            self.owner_matches(&server_pid, &server_start)?;
            let snapshot = self.snapshot(session)?;
            let workspace = record::get_name(&record, "workspace").unwrap_or_default();
            let present = snapshot
                .get("workspaces")
                .and_then(Json::array_items)
                .is_some_and(|workspaces| {
                    workspaces.iter().any(|w| {
                        w.get("workspace_id").and_then(Json::as_str) == Some(workspace.as_str())
                    })
                });
            if present {
                self.observe(
                    &record,
                    session,
                    &record::get_name(&record, "pane").unwrap_or_default(),
                )?;
                self.cli(session, &["workspace", "close", &workspace])?;
            }
            let snapshot = self.snapshot(session)?;
            let remains = snapshot
                .get("workspaces")
                .and_then(Json::array_items)
                .is_some_and(|workspaces| {
                    workspaces.iter().any(|w| {
                        w.get("workspace_id").and_then(Json::as_str) == Some(workspace.as_str())
                    })
                });
            if remains {
                return self.fail("Herdr workspace remains after close");
            }
            checkpoint(&record, "workspace-closed")?;
            phase = Some("workspace-closed".to_owned());
        }
        if !rules::cleanup_phase_valid(phase.as_deref()) {
            return self.fail("Unknown cleanup phase");
        }
        if !matches!(
            phase.as_deref(),
            Some("server-stopped") | Some("session-deleting")
        ) {
            checkpoint(&record, "server-stopping")?;
            endpoint = self.status(session)?;
            if endpoint.get("socket").and_then(Json::as_str)
                != record::get_name(&record, "socket").as_deref()
            {
                return self.fail("Cleanup socket changed");
            }
            let server_pid = edn::record::get(&record, "server-pid")
                .cloned()
                .unwrap_or(Value::Nil);
            let server_start = record::get_name(&record, "server-start").unwrap_or_default();
            if endpoint.get("running") == Some(&Json::Bool(true)) {
                self.compatible(&endpoint)?;
                self.owner_matches(&server_pid, &server_start)?;
                let stopped = self.cli(session, &["session", "stop", session, "--json"])?;
                let confirmed = stopped.get("stopped") == Some(&Json::Bool(true))
                    && stopped
                        .get("session")
                        .and_then(|s| s.get("name"))
                        .and_then(Json::as_str)
                        == Some(session)
                    && stopped
                        .get("session")
                        .and_then(|s| s.get("socket_path"))
                        .and_then(Json::as_str)
                        == record::get_name(&record, "socket").as_deref();
                if !confirmed {
                    return self.fail("Herdr session stop was not confirmed");
                }
            } else if self.process_owner(&server_pid)?.is_some() {
                return self.fail("Stopped endpoint has a live or reused server PID");
            }
            self.poll(
                3000,
                50,
                &|| match self.process_owner(&server_pid) {
                    Ok(None) => Some(()),
                    _ => None,
                },
                "Herdr server process remains after stop",
            )?;
            checkpoint(&record, "server-stopped")?;
        }
        endpoint = self.status(session)?;
        let server_pid = edn::record::get(&record, "server-pid")
            .cloned()
            .unwrap_or(Value::Nil);
        if !(endpoint.get("running") == Some(&Json::Bool(false))
            && endpoint.get("socket").and_then(Json::as_str)
                == record::get_name(&record, "socket").as_deref()
            && self.process_owner(&server_pid)?.is_none())
        {
            return self.fail("Herdr endpoint remains after stop");
        }
        checkpoint(&record, "session-deleting")?;
        let sessions = self.cli(session, &["session", "list", "--json"])?;
        let inventory = sessions
            .get("sessions")
            .and_then(Json::array_items)
            .ok_or_else(|| Failure::new("Ambiguous cleanup inventory"))?;
        for entry in inventory {
            if entry.get("name").and_then(Json::as_str) == Some(session) {
                let deleted = self.cli(session, &["session", "delete", session, "--json"])?;
                let confirmed = deleted.get("deleted") == Some(&Json::Bool(true))
                    && deleted
                        .get("session")
                        .and_then(|s| s.get("name"))
                        .and_then(Json::as_str)
                        == Some(session)
                    && deleted
                        .get("session")
                        .and_then(|s| s.get("socket_path"))
                        .and_then(Json::as_str)
                        == record::get_name(&record, "socket").as_deref();
                if !confirmed {
                    return self.fail("Herdr session deletion was not confirmed");
                }
                break;
            }
        }
        let sessions = self.cli(session, &["session", "list", "--json"])?;
        let inventory = sessions
            .get("sessions")
            .and_then(Json::array_items)
            .ok_or_else(|| Failure::new("Ambiguous cleanup inventory"))?;
        for entry in inventory {
            if entry.get("name").and_then(Json::as_str) == Some(session) {
                return self.fail("Herdr session remains after deletion");
            }
        }
        Ok(())
    }

    fn progress(&self, stage: &str) -> Result<()> {
        match &self.config.progress {
            Some(progress) => progress(stage),
            None => Ok(()),
        }
    }
}

/// Older runner side files were JSON; convert what the journal readers need.
fn json_to_edn(value: Json) -> Value {
    match value {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Bool(b),
        Json::Number(n) if n.fract() == 0.0 && n.abs() <= 9.007_199_254_740_991e15 => {
            Value::Integer(n as i64)
        }
        Json::Number(n) => Value::Float(n),
        Json::String(s) => Value::String(s),
        Json::Array(items) => Value::Vector(items.into_iter().map(json_to_edn).collect()),
        Json::Object(map) => Value::Map(
            map.into_iter()
                .map(|(key, value)| (Value::keyword(&key.replace('_', "-")), json_to_edn(value)))
                .collect(),
        ),
    }
}

pub(crate) mod rules;
pub(crate) mod select;

#[cfg(test)]
mod tests;
