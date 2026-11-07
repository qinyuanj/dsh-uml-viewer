//! Companion lifecycle orchestration (port of
//! `love/uml/adapters/companion_lifecycle.lua`). Prompt text, session naming
//! and ownership checks are pure; all process work stays in adapters.

use crate::agent::{CodingAgent, SessionIdentity};
use crate::herdr::{
    Config, ControllerSpec, Herdr, ProgressFn, RunnerLauncher, Selected, SocketLayout,
};
use crate::host::Host;
use crate::ownership;
use crate::posix;
use crate::preflight::rules;
use crate::record;
use crate::recovery;
use crate::vocab::{Failure, Result, RunOut};
use edn::Value;

pub const MAX_NAME_BYTES: usize = 64;

pub fn config_root(host: &Host) -> String {
    SocketLayout::NATIVE.config_root(&*host.variable)
}

/// `uml-viewer-<project>-<hash8>-<uuid8>`: the project name is the only part
/// that may shrink, so the socket path stays inside the measured limit.
pub fn session_name(root: &str, uuid: &str, config_root: &str) -> Result<String> {
    session_name_in(SocketLayout::NATIVE, root, uuid, config_root)
}

/// [`session_name`] for a declared platform's socket layout.
pub fn session_name_in(
    layout: SocketLayout,
    root: &str,
    uuid: &str,
    config_root: &str,
) -> Result<String> {
    let separators = layout.separators();
    let max_socket = layout.max_bytes();
    let base: String = root
        .trim_end_matches(separators)
        .rsplit(separators)
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("project")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let hash = root.bytes().fold(0u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(u32::from(byte))
    });
    let uuid_digits: String = uuid.replace('-', "").chars().take(8).collect();
    let fixed = format!("uml-viewer-%s-{hash:08x}-{uuid_digits}");
    // The socket path around the session name, measured with an empty name.
    let frame = layout.socket(config_root, "").len();
    let room = max_socket.saturating_sub(frame).min(MAX_NAME_BYTES);
    let fixed_length = fixed.len() - 2; // "%s" is a two-byte placeholder
    if room <= fixed_length {
        return Err(Failure::new(format!(
            "Herdr session name cannot fit: {} exceeds the {max_socket}-byte Unix socket path; set a shorter XDG_CONFIG_HOME",
            layout.socket(config_root, &fixed.replace("%s", "<project>"))
        )));
    }
    let name_budget = room - fixed_length;
    Ok(fixed.replace("%s", &base.chars().take(name_budget).collect::<String>()))
}

/// Load and verify this project's complete Herdr ownership. Missing, legacy,
/// foreign or incomplete records are refused, never adopted.
pub fn load(root: &str, host: &Host) -> Result<Option<Value>> {
    let Some(value) = ownership::read_in(host, root)? else {
        return Ok(None);
    };
    if record::is_empty_map(&value) {
        return Ok(None);
    }
    rules::recorded_backend(Some(&value))?;
    crate::herdr::rules::complete(&value)?;
    crate::agent::identity_of(&value)?;
    let cwd = record::get_name(&value, "cwd").unwrap_or_default();
    if (host.canonical)(&cwd) != (host.canonical)(root) {
        return Err(Failure::new(
            "Companion ownership is missing, ambiguous or belongs to another project; retained",
        ));
    }
    Ok(Some(value))
}

pub fn required(root: &str, host: &Host) -> Result<Value> {
    load(root, host)?.ok_or_else(|| {
        Failure::new("Companion ownership is missing, ambiguous or unreadable; retained")
    })
}

pub(crate) fn current_exe() -> Result<String> {
    std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| Failure::new(format!("cannot resolve the uml-viewer binary: {error}")))
}

/// The role argv for this binary: `[exe, role, manifest]`.
fn role_launcher(role: &str) -> Result<Vec<String>> {
    Ok(vec![current_exe()?, role.to_owned()])
}

/// This binary's runner role, as the pane launches it.
pub(crate) fn runner_launcher() -> Result<RunnerLauncher> {
    Ok(RunnerLauncher {
        argv: role_launcher("companion-runner")?,
    })
}

/// Read-only backend selection used before any resource is created.
pub fn select(root: &str, args: &[String]) -> Result<Selected> {
    crate::preflight::select(root, args, false)
}

/// The launch preflight for `agent`, in the host's environment: the
/// recorded provider must match it and its own requirements must hold.
fn launch_preflight(
    root: &str,
    host: &Host,
    extra: &[&str],
    agent: CodingAgent,
) -> Result<(Selected, CodingAgent)> {
    let mut args = vec!["--backend", "herdr", "--coding-agent", agent.name()];
    args.extend(extra);
    let args: Vec<String> = args.into_iter().map(str::to_owned).collect();
    crate::preflight::launch_in(root, &args, &|name| (host.variable)(name))
}

/// The lifecycle backend configuration: the selected Herdr, the binary's own
/// runner and controller roles, and the worker's progress callback.
fn config(selected: Selected, progress: Option<ProgressFn>) -> Result<Config> {
    Ok(Config::lifecycle(
        selected,
        progress,
        runner_launcher()?,
        ControllerSpec {
            executable: current_exe()?,
            argv: vec!["companion-controller".to_owned()],
        },
    ))
}

/// The provisional ownership a fresh start writes before any resource is
/// created: the Herdr session it will own, the provider's fresh argv, and
/// whatever its session identity already allows. `uuid` names the Herdr
/// session; Claude also runs under it, while Codex's own id arrives later.
fn provisional_record(
    agent: CodingAgent,
    executable: &str,
    canonical: &str,
    session: &str,
    uuid: &str,
) -> Value {
    let identity = SessionIdentity {
        agent,
        session: (!agent.awaits_reported_session()).then(|| uuid.to_owned()),
    };
    let command = agent.fresh_command(executable, canonical, identity.session.as_deref());
    let mut fields = vec![
        ("backend", Value::keyword("herdr")),
        ("owner-role", Value::keyword("companion")),
        ("cwd", Value::String(canonical.to_owned())),
        ("session", Value::String(session.to_owned())),
        (
            "command",
            Value::Vector(command.into_iter().map(Value::String).collect()),
        ),
    ];
    fields.extend(identity.ownership_fields(executable, canonical));
    fields.push(("provisional", Value::Bool(true)));
    edn::record::record(fields)
}

pub fn create(root: &str, host: &Host, options_in: CreateOptions) -> Result<(Value, Herdr)> {
    let agent = options_in.agent;
    if load(root, host)?.is_some() {
        return Err(Failure::new(
            "Companion ownership already recorded; use --restart or resolve it before fresh start",
        ));
    }
    let (selected, _) = launch_preflight(root, host, &[], agent)?;
    // Codex runs as the executable its preflight checked, so the fresh and
    // recovery argv name that file rather than a later PATH lookup.
    let executable = match options_in.executable.clone() {
        Some(executable) => executable,
        None => agent.executable(&|name| (host.variable)(name))?,
    };
    let uuid = options_in.uuid.clone().unwrap_or_else(|| (host.token)());
    let session = session_name(root, &uuid, &config_root(host))?;
    let canonical = (host.canonical)(root);
    let provisional = provisional_record(agent, &executable, &canonical, &session, &uuid);
    let command = record::string_vec(edn::record::seq(edn::record::get(&provisional, "command")));
    ownership::write(host, root, &provisional)?;
    // The backend builds every later record on this identity, so the committed
    // ownership keeps the recovery command and the provider's session id.
    let mut backend = Herdr::new(
        Some(provisional),
        host.clone(),
        config(selected, options_in.progress)?,
    );
    let owned = backend.create(&session, &canonical, &command)?;
    backend.identity = Some(owned.clone());
    Ok((owned, backend))
}

#[derive(Default)]
pub struct CreateOptions {
    pub agent: CodingAgent,
    /// The provider executable; its `*_BIN` override or PATH name otherwise.
    pub executable: Option<String>,
    pub uuid: Option<String>,
    /// Called at each create boundary. The worker publishes a progress event
    /// there and lets a cancellation land between boundaries, so it may
    /// refuse and stop the startup.
    pub progress: Option<ProgressFn>,
}

/// Rebind the recorded companion. An explicit provider may only confirm the
/// recorded one; without one the recorded provider is reused.
pub fn restart(root: &str, host: &Host, requested: Option<CodingAgent>) -> Result<(Value, Herdr)> {
    let plain = load(root, host)?
        .ok_or_else(|| Failure::new("Companion ownership is missing; cannot restart"))?;
    let recorded = crate::agent::identity_of(&plain)?.agent;
    let (selected, _) =
        launch_preflight(root, host, &["--restart"], requested.unwrap_or(recorded))?;
    let backend = Herdr::new(Some(plain.clone()), host.clone(), config(selected, None)?);
    let session = record::get_name(&plain, "session").unwrap_or_default();
    let pane = record::get_name(&plain, "pane").unwrap_or_default();
    backend.probe(&session)?;
    backend.bind(&session, &pane)?;
    recovery::verify(&plain, host)?;
    Ok((plain, backend))
}

/// Cancel recovery first, then release only what this project verifiably
/// owns; ownership is cleared only after confirmed cleanup.
pub fn shutdown(root: &str, host: &Host) -> Result<bool> {
    let Some(plain) = load(root, host)? else {
        return Ok(false);
    };
    let selected = select(root, &["--backend".to_owned(), "herdr".to_owned()])?;
    let backend = Herdr::new(Some(plain.clone()), host.clone(), config(selected, None)?);
    let session = record::get_name(&plain, "session").unwrap_or_default();
    backend.cleanup(&session)?;
    ownership::write(host, root, &Value::Map(vec![]))?;
    Ok(true)
}

pub struct Opened {
    pub terminal: String,
    pub attach: String,
}

#[cfg(not(windows))]
fn on_path(name: &str) -> Option<String> {
    std::env::var("PATH").ok().and_then(|path| {
        path.split(':')
            .map(|directory| format!("{directory}/{name}"))
            .find(|candidate| std::fs::File::open(candidate).is_ok())
    })
}

/// `wt.exe` is an app execution alias under `WindowsApps`: a reparse point
/// that CreateProcess runs but that cannot be opened for reading, so its
/// presence is the probe.
#[cfg(windows)]
fn on_path(name: &str) -> Option<String> {
    let search = crate::search::Search::native();
    search
        .candidates(&std::env::var("PATH").unwrap_or_default(), name)
        .into_iter()
        .flatten()
        .find(|candidate| std::fs::symlink_metadata(candidate).is_ok())
}

const HIDDEN_SIDEBAR: [(&str, &str); 2] = [
    ("sidebar_start_collapsed", "true"),
    ("sidebar_collapsed_mode", "\"hidden\""),
];

/// The person's Herdr config: `HERDR_CONFIG_PATH`, else under the config root.
fn user_config_path(layout: SocketLayout, var: &dyn Fn(&str) -> Option<String>) -> String {
    let s = layout.separators()[0];
    var("HERDR_CONFIG_PATH")
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| format!("{}{s}herdr{s}config.toml", layout.config_root(var)))
}

fn table_name(header: &str) -> String {
    let inner = header
        .trim_start_matches('[')
        .split(']')
        .next()
        .unwrap_or("");
    inner
        .split('.')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(".")
}

/// `base` with the sidebar hidden: any existing sidebar collapse keys give
/// way to a hidden pair at the top of `[ui]`, which is appended when absent.
/// Line-based, so a config it cannot follow is caught by `config check`.
fn hidden_sidebar(base: &str) -> String {
    let settings = HIDDEN_SIDEBAR.map(|(key, value)| format!("{key} = {value}"));
    let mut out: Vec<String> = Vec::new();
    let mut table = String::new();
    let mut placed = false;
    for line in base.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            table = table_name(trimmed);
            out.push(line.to_owned());
            if table == "ui" && !placed {
                out.extend(settings.iter().cloned());
                placed = true;
            }
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.split('.').map(str::trim).collect::<Vec<_>>().join(".");
            let full = if table.is_empty() {
                key
            } else {
                format!("{table}.{key}")
            };
            if HIDDEN_SIDEBAR
                .iter()
                .any(|(name, _)| full == format!("ui.{name}"))
            {
                continue;
            }
        }
        out.push(line.to_owned());
    }
    if !placed {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push("[ui]".to_owned());
        out.extend(settings);
    }
    out.join("\n") + "\n"
}

/// The attach client, not the server, reads the sidebar settings (measured on
/// Herdr 0.9.5), so the companion's client gets the person's config with the
/// sidebar hidden. A config Herdr rejects falls back to theirs unchanged.
fn attach_config(root: &str, herdr: &str) -> Option<String> {
    let var = |name: &str| std::env::var(name).ok();
    let base =
        std::fs::read_to_string(user_config_path(SocketLayout::NATIVE, &var)).unwrap_or_default();
    let path = attach_config_path(root);
    std::fs::write(&path, hidden_sidebar(&base)).ok()?;
    config_accepted(herdr, &path).then_some(path)
}

#[cfg(not(windows))]
fn attach_config_path(root: &str) -> String {
    format!(
        "{}/.uml-viewer/herdr-attach.toml",
        root.trim_end_matches('/')
    )
}

#[cfg(windows)]
fn attach_config_path(root: &str) -> String {
    std::path::Path::new(root)
        .join(".uml-viewer")
        .join("herdr-attach.toml")
        .to_string_lossy()
        .into_owned()
}

/// Whether `herdr config check` accepts `path` as its config.
#[cfg(not(windows))]
fn config_accepted(herdr: &str, path: &str) -> bool {
    let argv: Vec<std::ffi::OsString> = [
        "/usr/bin/env",
        &format!("HERDR_CONFIG_PATH={path}"),
        herdr,
        "config",
        "check",
    ]
    .iter()
    .map(std::ffi::OsString::from)
    .collect();
    let check = posix::run(&argv, Some(10000), &[]);
    check.exit == Some(0) && !check.timeout
}

/// Windows has no `env`: carry the override on the contained child. The
/// platform boundary owns the deadline and cleanup of the whole process tree.
#[cfg(windows)]
fn config_accepted(herdr: &str, path: &str) -> bool {
    let argv = [herdr, "config", "check"].map(std::ffi::OsString::from);
    let check = posix::run_with_env(&argv, Some(10000), &[], &[("HERDR_CONFIG_PATH", path)]);
    check.exit == Some(0) && !check.timeout
}

/// The cmd.exe line a native Windows Terminal tab runs, and whether it can
/// be opened. Windows Terminal starts a tab from its own environment, not
/// the caller's, so the config root and the attach config travel as `set`
/// assignments. cmd expands `%` even inside quotes, a `"` ends the quoting
/// and Windows Terminal splits its command line at `;`: an attach config
/// with one of those is left out (the tab reads the person's own config), and
/// a config root or Herdr path with one cannot be carried, so the window is
/// not opened and the line is only reported.
fn windows_attach(
    config_root: Option<&str>,
    attach_config: Option<&str>,
    herdr: &str,
    session: &str,
) -> (String, bool) {
    let carried =
        |text: &str| !text.contains(['%', '"', ';']) && !text.chars().any(char::is_control);
    let mut line = String::new();
    let mut openable = carried(herdr) && carried(session);
    if let Some(root) = config_root.filter(|root| !root.is_empty()) {
        openable &= carried(root);
        line.push_str(&format!("set \"XDG_CONFIG_HOME={root}\"&& "));
    }
    if let Some(path) = attach_config.filter(|path| carried(path)) {
        line.push_str(&format!("set \"HERDR_CONFIG_PATH={path}\"&& "));
    }
    line.push_str(&format!("\"{herdr}\" session attach {session}"));
    (line, openable)
}

/// The argv that opens a new Windows Terminal window running `line`.
/// Windows Terminal quotes a command-line word that holds a space when it
/// rebuilds the tab's command line, and `/s` makes cmd strip exactly those
/// outer quotes, so `line` reaches cmd unchanged.
fn windows_terminal_argv(terminal: &str, session: &str, line: &str) -> Vec<String> {
    [
        terminal, "-w", "new", "new-tab", "--title", session, "cmd.exe", "/d", "/s", "/c", line,
    ]
    .map(str::to_owned)
    .to_vec()
}

fn windows_attach_environment(
    var: &dyn Fn(&str) -> Option<String>,
    config: Option<&str>,
    herdr: &str,
    session: &str,
) -> (String, bool) {
    // Terminal tabs inherit Terminal's environment, which can have a stale
    // XDG value or a different APPDATA. Carry the server's resolved root.
    let config_root = SocketLayout::Windows.config_root(var);
    windows_attach(Some(&config_root), config, herdr, session)
}

fn open_windows_terminal(session: &str, herdr: &str, config: Option<&str>) -> Opened {
    let (line, openable) =
        windows_attach_environment(&|name| std::env::var(name).ok(), config, herdr, session);
    let terminal = if openable { on_path("wt.exe") } else { None };
    let Some(terminal) = terminal else {
        return Opened {
            terminal: "none".to_owned(),
            attach: line,
        };
    };
    let argv: Vec<std::ffi::OsString> = windows_terminal_argv(&terminal, session, &line)
        .iter()
        .map(std::ffi::OsString::from)
        .collect();
    let result: RunOut = posix::run(&argv, Some(10000), &[]);
    let started = result.exit == Some(0) && !result.timeout;
    Opened {
        terminal: if started { "windows-terminal" } else { "none" }.to_owned(),
        attach: line,
    }
}

/// The `/usr/bin/env` prefix that carries the config root and the attach
/// config into the new tab; empty when neither applies. Windows Terminal
/// splits its command line at `;` even inside quotes, so an attach config
/// under such a project path is left out and the tab reads the person's own.
fn attach_env(config_root: Option<String>, attach_config: Option<&str>) -> Vec<String> {
    let mut assignments: Vec<String> = config_root
        .filter(|root| !root.is_empty())
        .map(|root| format!("XDG_CONFIG_HOME={root}"))
        .into_iter()
        .collect();
    assignments.extend(
        attach_config
            .filter(|path| !path.contains(';'))
            .map(|path| format!("HERDR_CONFIG_PATH={path}")),
    );
    if assignments.is_empty() {
        return assignments;
    }
    assignments.insert(0, "/usr/bin/env".to_owned());
    assignments
}

/// WSL reaches the host's Windows Terminal through interop; the new tab enters
/// the same distribution and attaches with the selected Herdr's absolute path.
/// Elsewhere, or when the terminal does not start, the caller reports the
/// command a person can run (ADR 0007).
pub fn open_window(root: &str, session: &str, herdr: &str) -> Opened {
    let config = attach_config(root, herdr);
    if cfg!(windows) {
        return open_windows_terminal(session, herdr, config.as_deref());
    }
    let attach = format!(
        "{}{} session attach {}",
        config
            .as_ref()
            .map(|path| format!("HERDR_CONFIG_PATH={} ", crate::text::shell_word(path)))
            .unwrap_or_default(),
        crate::text::shell_word(herdr),
        crate::text::shell_word(session)
    );
    if cfg!(target_os = "macos") {
        return open_mac_terminal(attach);
    }
    let distro = std::env::var("WSL_DISTRO_NAME").unwrap_or_default();
    let terminal = if distro.is_empty() {
        None
    } else {
        on_path("wt.exe")
    };
    let Some(terminal) = terminal else {
        return Opened {
            terminal: "none".to_owned(),
            attach,
        };
    };
    let mut argv: Vec<String> = vec![
        terminal,
        "-w".to_owned(),
        "new".to_owned(),
        "new-tab".to_owned(),
        "--title".to_owned(),
        session.to_owned(),
        "wsl.exe".to_owned(),
        "-d".to_owned(),
        distro,
        "--exec".to_owned(),
    ];
    argv.extend(attach_env(
        std::env::var("XDG_CONFIG_HOME").ok(),
        config.as_deref(),
    ));
    for word in [herdr, "session", "attach", session] {
        argv.push(word.to_owned());
    }
    let os_argv: Vec<std::ffi::OsString> = argv.iter().map(std::ffi::OsString::from).collect();
    let result: RunOut = posix::run(&os_argv, Some(10000), &[]);
    let started = result.exit == Some(0) && !result.timeout;
    Opened {
        terminal: if started { "windows-terminal" } else { "none" }.to_owned(),
        attach,
    }
}

/// macOS opens a Terminal.app window through AppleScript; the attach line
/// runs in the person's own login shell, so only the config root is carried.
fn open_mac_terminal(attach: String) -> Opened {
    let line = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|root| !root.is_empty())
        .map(|root| {
            format!(
                "XDG_CONFIG_HOME={} {attach}",
                crate::text::shell_word(&root)
            )
        })
        .unwrap_or_else(|| attach.clone());
    let argv: Vec<std::ffi::OsString> = [
        "/usr/bin/osascript".to_owned(),
        "-e".to_owned(),
        format!(
            "tell application \"Terminal\" to do script \"{}\"",
            apple_script_text(&line)
        ),
        "-e".to_owned(),
        "tell application \"Terminal\" to activate".to_owned(),
    ]
    .iter()
    .map(std::ffi::OsString::from)
    .collect();
    let result: RunOut = posix::run(&argv, Some(10000), &[]);
    let started = result.exit == Some(0) && !result.timeout;
    Opened {
        terminal: if started { "terminal-app" } else { "none" }.to_owned(),
        attach,
    }
}

/// Body of an AppleScript string literal: backslash and double quote escaped.
fn apple_script_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_config_check_timeout_reaps_its_grandchild_and_keeps_a_bystander() {
        let directory =
            std::env::temp_dir().join(format!("uml config check 空格 {}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("check.rs");
        let executable = directory.join("herdr.exe");
        std::fs::write(
            &source,
            r#"
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["grandchild"] {
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }
    assert_eq!(args, ["config", "check"]);
    let path = std::env::var("HERDR_CONFIG_PATH").unwrap();
    let mode = std::fs::read_to_string(&path).unwrap();
    if mode == "accept" { return; }
    if mode == "refuse" { std::process::exit(2); }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("grandchild").spawn().unwrap();
    std::fs::write(format!("{path}.pid"), child.id().to_string()).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(30));
}
"#,
        )
        .unwrap();
        let built = std::process::Command::new("rustc")
            .args(["--crate-name", "config_check_fixture", "-o"])
            .arg(&executable)
            .arg(&source)
            .status()
            .unwrap();
        assert!(built.success());
        let config = directory.join("attach 丁 & config.toml");
        let herdr = executable.to_str().unwrap();
        let path = config.to_str().unwrap();
        for (mode, accepted) in [("accept", true), ("refuse", false)] {
            std::fs::write(&config, mode).unwrap();
            assert_eq!(accepted, config_accepted(herdr, path));
        }
        let mut bystander = std::process::Command::new(&executable)
            .arg("grandchild")
            .spawn()
            .unwrap();
        std::fs::write(&config, "hang").unwrap();
        let started = std::time::Instant::now();
        let accepted = config_accepted(herdr, path);
        let elapsed = started.elapsed();
        let pid: i32 = std::fs::read_to_string(format!("{path}.pid"))
            .unwrap()
            .parse()
            .unwrap();
        let deadline = posix::monotonic_ms() + 2000;
        while posix::identity(pid).unwrap().is_some() && posix::monotonic_ms() < deadline {
            posix::sleep(20);
        }
        let survived = posix::identity(pid).unwrap().is_some();
        let bystander_alive = bystander.try_wait().unwrap().is_none();
        bystander.kill().unwrap();
        bystander.wait().unwrap();
        if survived {
            // Clean up only the fixture child on the red run.
            let _ = std::process::Command::new("taskkill.exe")
                .args(["/F", "/PID", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        let _ = std::fs::remove_dir_all(&directory);
        assert!(!accepted);
        assert!(elapsed < std::time::Duration::from_secs(15), "{elapsed:?}");
        assert!(bystander_alive, "bystander was touched");
        assert!(!survived, "config-check grandchild survived its deadline");
    }

    #[test]
    fn apple_script_text_escapes_quotes_and_backslashes_only() {
        assert_eq!(
            r#"'/a b/herdr' attach 'x\"y\\z' $w"#,
            apple_script_text(r#"'/a b/herdr' attach 'x"y\z' $w"#)
        );
    }

    #[test]
    fn the_attach_config_hides_the_sidebar_and_keeps_every_other_setting() {
        let base = "onboarding = false\n[ui.toast]\ndelivery = \"herdr\"\n\n[ui]\nsidebar_start_collapsed = false\nstatus_indicators = \"symbols\"\nsidebar_collapsed_mode = \"compact\"\n[theme]\nname = \"x\"\n";
        assert_eq!(
            "onboarding = false\n[ui.toast]\ndelivery = \"herdr\"\n\n[ui]\nsidebar_start_collapsed = true\nsidebar_collapsed_mode = \"hidden\"\nstatus_indicators = \"symbols\"\n[theme]\nname = \"x\"\n",
            hidden_sidebar(base)
        );
        assert_eq!(
            "ui.sidebar_width = 30\n[theme]\nname = \"x\"\n\n[ui]\nsidebar_start_collapsed = true\nsidebar_collapsed_mode = \"hidden\"\n",
            hidden_sidebar(
                "ui.sidebar_width = 30\nui.sidebar_start_collapsed=false\n[theme]\nname = \"x\"\n"
            )
        );
        assert_eq!(
            "[ui]\nsidebar_start_collapsed = true\nsidebar_collapsed_mode = \"hidden\"\n",
            hidden_sidebar("")
        );
    }

    #[test]
    fn the_new_tab_receives_the_config_root_and_the_attach_config() {
        assert_eq!(Vec::<String>::new(), attach_env(None, None));
        assert_eq!(Vec::<String>::new(), attach_env(Some(String::new()), None));
        assert_eq!(
            vec![
                "/usr/bin/env".to_owned(),
                "XDG_CONFIG_HOME=/x".to_owned(),
                "HERDR_CONFIG_PATH=/p/a.toml".to_owned()
            ],
            attach_env(Some("/x".to_owned()), Some("/p/a.toml"))
        );
        assert_eq!(
            vec![
                "/usr/bin/env".to_owned(),
                "HERDR_CONFIG_PATH=/p/a.toml".to_owned()
            ],
            attach_env(None, Some("/p/a.toml"))
        );
        assert_eq!(
            vec!["/usr/bin/env".to_owned(), "XDG_CONFIG_HOME=/x".to_owned()],
            attach_env(Some("/x".to_owned()), Some("/p;q/a.toml"))
        );
        assert_eq!(Vec::<String>::new(), attach_env(None, Some("/p;q/a.toml")));
    }

    #[test]
    fn the_person_s_herdr_config_follows_the_override_then_the_config_root() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        let posix = SocketLayout::Posix;
        let all = env(&[
            ("HERDR_CONFIG_PATH", "/c/herdr.toml"),
            ("XDG_CONFIG_HOME", "/x/"),
            ("HOME", "/h"),
        ]);
        assert_eq!("/c/herdr.toml", user_config_path(posix, &all));
        let xdg = env(&[
            ("HERDR_CONFIG_PATH", ""),
            ("XDG_CONFIG_HOME", "/x/"),
            ("HOME", "/h"),
        ]);
        assert_eq!("/x/herdr/config.toml", user_config_path(posix, &xdg));
        let home = env(&[("HOME", "/h")]);
        assert_eq!(
            "/h/.config/herdr/config.toml",
            user_config_path(posix, &home)
        );
        // Windows: %APPDATA% stands in for $HOME/.config (probed on 0.9.5).
        let windows = SocketLayout::Windows;
        let appdata = env(&[
            ("APPDATA", "C:\\Users\\p\\AppData\\Roaming"),
            ("HOME", "/h"),
        ]);
        assert_eq!(
            "C:\\Users\\p\\AppData\\Roaming\\herdr\\config.toml",
            user_config_path(windows, &appdata)
        );
        let xdg = env(&[("XDG_CONFIG_HOME", "D:\\x\\"), ("APPDATA", "C:\\a")]);
        assert_eq!("D:\\x\\herdr\\config.toml", user_config_path(windows, &xdg));
    }

    /// A typical `$HOME/.config` root; no test reads the process environment.
    const HOME_CONFIG: &str = "/home/person/.config";

    fn session_name(root: &str, uuid: &str, config_root: &str) -> Result<String> {
        session_name_in(SocketLayout::Posix, root, uuid, config_root)
    }

    const MAX_SOCKET_BYTES: usize = SocketLayout::Posix.max_bytes();

    /// The Windows socket is `<root>\herdr\sessions\<name>\herdr.sock`, with
    /// 243 bytes measured to work and 244 to fail on Herdr 0.9.5.
    #[test]
    fn a_windows_session_name_fits_the_windows_socket_budget() {
        let layout = SocketLayout::Windows;
        let root = "C:\\Users\\p\\AppData\\Roaming";
        let name = session_name_in(layout, "C:\\work\\my proj\\", "abcdef12-rest", root).unwrap();
        assert!(name.starts_with("uml-viewer-my-proj-"), "{name}");
        assert!(crate::herdr::rules::session_valid(&name), "{name}");
        let tight = format!("C:\\{}", "x".repeat(170));
        let name = session_name_in(layout, "C:\\work\\project", "abcdef12-rest", &tight).unwrap();
        let socket = layout.socket(&tight, &name);
        assert!(socket.len() <= 243, "{socket} ({} bytes)", socket.len());
        assert_eq!(
            format!("{tight}\\herdr\\sessions\\{name}\\herdr.sock"),
            socket
        );
        let error = session_name_in(
            layout,
            "C:\\p",
            "abcdef12-rest",
            &format!("C:\\{}", "x".repeat(220)),
        )
        .unwrap_err();
        assert!(error.message.contains("243-byte"), "{}", error.message);
    }

    #[test]
    fn a_windows_tab_overrides_stale_terminal_roots_with_the_callers_resolved_root() {
        for xdg in [None, Some(""), Some("D:\\caller xdg")] {
            let var = |name: &str| match name {
                "XDG_CONFIG_HOME" => xdg.map(str::to_owned),
                "APPDATA" => Some("C:\\custom roaming".to_owned()),
                _ => None,
            };
            let (line, openable) = windows_attach_environment(&var, None, "C:\\herdr.exe", "s");
            let root = if xdg == Some("D:\\caller xdg") {
                "D:\\caller xdg"
            } else {
                "C:\\custom roaming"
            };
            assert_eq!(
                format!("set \"XDG_CONFIG_HOME={root}\"&& \"C:\\herdr.exe\" session attach s"),
                line
            );
            assert!(openable);
            // Even a Terminal carrying XDG_CONFIG_HOME=D:\\stale will execute
            // the caller's assignment before Herdr resolves the session socket.
            assert!(
                windows_terminal_argv("wt.exe", "s", &line)
                    .last()
                    .unwrap()
                    .starts_with("set \"XDG_CONFIG_HOME=")
            );
        }
    }

    #[test]
    fn a_windows_terminal_tab_carries_the_config_root_and_attach_config_as_set_lines() {
        let herdr = "C:\\Program Files\\herdr\\herdr.exe";
        assert_eq!(
            (
                "\"C:\\Program Files\\herdr\\herdr.exe\" session attach uml-viewer-a".to_owned(),
                true
            ),
            windows_attach(None, None, herdr, "uml-viewer-a")
        );
        assert_eq!(
            (
                "set \"XDG_CONFIG_HOME=C:\\x y&z\"&& set \"HERDR_CONFIG_PATH=C:\\p\\a.toml\"&& \"C:\\Program Files\\herdr\\herdr.exe\" session attach uml-viewer-a".to_owned(),
                true
            ),
            windows_attach(
                Some("C:\\x y&z"),
                Some("C:\\p\\a.toml"),
                herdr,
                "uml-viewer-a"
            )
        );
        // An attach config cmd or Windows Terminal would re-parse is left out.
        for config in ["C:\\p;q\\a.toml", "C:\\%p%\\a.toml"] {
            assert_eq!(
                windows_attach(None, None, herdr, "uml-viewer-a"),
                windows_attach(None, Some(config), herdr, "uml-viewer-a")
            );
        }
        // A config root or Herdr path that cannot be carried is only reported.
        assert!(!windows_attach(Some("C:\\a;b"), None, herdr, "uml-viewer-a").1);
        assert!(!windows_attach(None, None, "C:\\%x%\\herdr.exe", "uml-viewer-a").1);
        assert_eq!(
            [
                "wt.exe",
                "-w",
                "new",
                "new-tab",
                "--title",
                "uml-viewer-a",
                "cmd.exe",
                "/d",
                "/s",
                "/c",
                "line"
            ],
            windows_terminal_argv("wt.exe", "uml-viewer-a", "line")[..]
        );
    }

    #[test]
    fn names_a_fresh_session_uniquely_per_launch_within_the_grammar() {
        let name = session_name("/projects/some project", "abcdef12-rest", HOME_CONFIG).unwrap();
        assert!(name.len() <= 64);
        assert!(crate::herdr::rules::session_valid(&name), "{name}");
        let long = session_name(
            &format!("/projects/{}", "x".repeat(200)),
            "abcdef12-rest",
            HOME_CONFIG,
        )
        .unwrap();
        assert!(long.len() <= 64);
        let stripped: String = long.trim_start_matches("uml-viewer-").to_owned();
        assert!(stripped.ends_with("-abcdef12"), "{long}");
        // Two roots whose names share a prefix must not collide.
        let a = session_name("/projects/alpha-one", "abcdef12-rest", HOME_CONFIG).unwrap();
        let b = session_name("/projects/alpha-two", "abcdef12-rest", HOME_CONFIG).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn shrinks_the_name_so_the_socket_path_stays_inside_the_measured_limit() {
        let config_root = "/tmp/uml-restart-xdg-o7gbqk3s/config";
        let name = session_name("/projects/some project", "abcdef12-rest", config_root).unwrap();
        let socket = format!("{config_root}/herdr/sessions/{name}/herdr.sock");
        assert!(
            socket.len() <= MAX_SOCKET_BYTES,
            "{socket} ({} bytes)",
            socket.len()
        );
        assert!(crate::herdr::rules::session_valid(&name), "{name}");
        assert!(name.len() <= MAX_NAME_BYTES);
        let other = session_name("/projects/other", "abcdef12-rest", config_root).unwrap();
        assert_ne!(name, other);
        assert!(other.ends_with("abcdef12"));
    }

    #[test]
    fn reports_the_real_cause_when_no_name_can_fit_the_config_root() {
        let error = session_name(
            "/projects/some project",
            "abcdef12-rest",
            &format!("/tmp/{}", "x".repeat(120)),
        )
        .unwrap_err();
        assert!(error.message.contains("XDG_CONFIG_HOME"));
        assert!(error.message.contains("Unix socket path"));
    }

    #[test]
    fn load_retains_missing_foreign_and_incomplete_ownership() {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "uml lifecycle {} {}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join(".uml-viewer")).unwrap();
        let root = directory.to_string_lossy().into_owned();
        let host = Host::native();
        assert!(load(&root, &host).unwrap().is_none());
        let cases = [
            format!(r#"{{:backend :unknown :cwd "{root}" :session "s" :pane "%1"}}"#),
            format!(r#"{{:cwd "{root}" :session "s" :pane "%1"}}"#),
            format!(
                r#"{{:backend :herdr :owner-role :launcher :cwd "{root}" :session "s" :pane "%1"}}"#
            ),
            "[:invalid]".to_owned(),
            complete_claude_record(&root).replacen('{', "{:coding-agent :pi ", 1),
            complete_claude_record(&root).replacen('{', "{:coding-agent :codex ", 1),
        ];
        for text in cases {
            std::fs::write(directory.join(".uml-viewer/companion.edn"), &text).unwrap();
            assert!(load(&root, &host).is_err(), "{text}");
            assert_eq!(text, ownership::bytes(&root).unwrap(), "bytes are retained");
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A committed Claude ownership as written before coding-agent providers.
    fn complete_claude_record(root: &str) -> String {
        let root = root.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            r#"{{:backend :herdr :claude-session "u-1" :command ["claude" "--session-id" "u-1"] :cwd "{root}" :owner-role :companion :pane "%1" :recovery-command ["claude" "--resume" "u-1"] :server-pid 2 :server-start "s" :session "uml-viewer-a" :shell-pid 3 :owner-start "o" :socket "/x.sock" :terminal-id "t" :workspace "w"}}"#
        )
    }

    fn keys(record: &Value) -> Vec<String> {
        let Value::Map(entries) = record else {
            panic!("not a map")
        };
        let mut keys: Vec<String> = entries
            .iter()
            .map(|(key, _)| edn::record::text(key).unwrap().to_owned())
            .collect();
        keys.sort();
        keys
    }

    fn strings(record: &Value, key: &str) -> Vec<String> {
        record::string_vec(edn::record::seq(edn::record::get(record, key)))
    }

    #[test]
    fn a_fresh_claude_record_keeps_today_s_fields_and_names_no_provider() {
        let record = provisional_record(
            CodingAgent::Claude,
            "/opt/claude",
            "/p",
            "uml-viewer-a",
            "u-1",
        );
        assert_eq!(
            [
                "backend",
                "claude-session",
                "command",
                "cwd",
                "owner-role",
                "provisional",
                "recovery-command",
                "session"
            ],
            keys(&record)[..]
        );
        assert_eq!(
            Some("u-1".to_owned()),
            record::get_name(&record, "claude-session")
        );
        assert_eq!(
            [
                "/opt/claude",
                "--dangerously-skip-permissions",
                "--session-id",
                "u-1"
            ],
            strings(&record, "command")[..4]
        );
        assert_eq!(
            [
                "/opt/claude",
                "--dangerously-skip-permissions",
                "--resume",
                "u-1"
            ],
            strings(&record, "recovery-command")[..4]
        );
    }

    #[test]
    fn a_fresh_codex_record_names_its_provider_and_waits_for_its_session() {
        let record = provisional_record(
            CodingAgent::Codex,
            "/opt/codex",
            "/p",
            "uml-viewer-a",
            "u-1",
        );
        assert_eq!(
            [
                "backend",
                "coding-agent",
                "command",
                "cwd",
                "owner-role",
                "provisional",
                "session"
            ],
            keys(&record)[..]
        );
        assert_eq!(
            Some(&Value::keyword("codex")),
            edn::record::get(&record, "coding-agent")
        );
        assert_eq!(
            ["/opt/codex", "--dangerously-bypass-approvals-and-sandbox"],
            strings(&record, "command")[..2]
        );
    }

    #[test]
    fn an_old_claude_ownership_loads_as_claude_without_being_rewritten() {
        let directory =
            std::env::temp_dir().join(format!("uml lifecycle claude {}", std::process::id()));
        std::fs::create_dir_all(directory.join(".uml-viewer")).unwrap();
        let root = directory.to_string_lossy().into_owned();
        let text = complete_claude_record(&root);
        std::fs::write(directory.join(".uml-viewer/companion.edn"), &text).unwrap();
        let loaded = load(&root, &Host::native()).unwrap().unwrap();
        assert_eq!(
            crate::agent::SessionIdentity {
                agent: CodingAgent::Claude,
                session: Some("u-1".to_owned())
            },
            crate::agent::identity_of(&loaded).unwrap()
        );
        assert_eq!(text, ownership::bytes(&root).unwrap(), "bytes are retained");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A project directory with fixture Herdr and Codex executables, and a
    /// host whose environment names only them.
    fn codex_project(codex: &str) -> (std::path::PathBuf, String, Host) {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "uml lifecycle codex {} {}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join(".uml-viewer")).unwrap();
        let bin = directory.join("bin");
        crate::preflight::fixtures::herdr(
            &bin,
            "0.9.5",
            22,
            crate::preflight::fixtures::CODEX_CURRENT,
        );
        crate::preflight::fixtures::codex(&bin, "codex", codex, 0);
        let path = crate::preflight::fixtures::path_with(&bin);
        let mut host = Host::native();
        host.variable = std::rc::Rc::new(move |name: &str| (name == "PATH").then(|| path.clone()));
        let root = directory.to_string_lossy().into_owned();
        (directory, root, host)
    }

    #[test]
    fn a_codex_fresh_start_runs_its_preflight_and_then_launches_the_resolved_codex() {
        let options = || CreateOptions {
            agent: CodingAgent::Codex,
            ..CreateOptions::default()
        };
        let (directory, root, host) = codex_project("codex-cli 0.165.0");
        let error = create(&root, &host, options()).err().unwrap();
        assert!(
            error.message.contains("Codex CLI 0.165.0 at"),
            "{}",
            error.message
        );
        assert!(!ownership::exists(&root));
        let _ = std::fs::remove_dir_all(&directory);
        // A passing preflight records the provisional Codex ownership with
        // the executable the preflight resolved on PATH, then reaches Herdr
        // (the fixture Herdr answers no session inventory).
        let (directory, root, host) = codex_project("codex-cli 0.166.0");
        let error = create(&root, &host, options()).err().unwrap();
        assert_eq!("Malformed Herdr JSON", error.message);
        let recorded = edn::parse(&ownership::bytes(&root).unwrap()).unwrap();
        assert_eq!(
            Some(&Value::keyword("codex")),
            edn::record::get(&recorded, "coding-agent")
        );
        assert_eq!(
            directory
                .join("bin")
                .join(if cfg!(windows) { "codex.exe" } else { "codex" })
                .to_string_lossy(),
            strings(&recorded, "command")[0]
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_role_argv_names_this_binary_and_the_role() {
        let argv = role_launcher("companion-runner").unwrap();
        assert_eq!(2, argv.len());
        assert!(
            argv[0].ends_with("companion")
                || argv[0].contains("uml-viewer")
                || std::path::Path::new(&argv[0]).is_absolute()
        );
        assert_eq!("companion-runner", argv[1]);
    }
}
