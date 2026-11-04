//! Companion lifecycle orchestration (port of
//! `love/uml/adapters/companion_lifecycle.lua`). Prompt text, session naming
//! and ownership checks are pure; all process work stays in adapters.

use crate::agent::{CodingAgent, SessionIdentity};
use crate::herdr::{
    Config, ControllerSpec, Herdr, MAX_SOCKET_BYTES, ProgressFn, RunnerLauncher, Selected,
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
    config_root_in(&*host.variable)
}

fn config_root_in(var: &dyn Fn(&str) -> Option<String>) -> String {
    let root = var("XDG_CONFIG_HOME")
        .filter(|root| !root.is_empty())
        .unwrap_or_else(|| format!("{}/.config", var("HOME").unwrap_or_default()));
    root.trim_end_matches('/').to_owned()
}

/// `uml-viewer-<project>-<hash8>-<uuid8>`: the project name is the only part
/// that may shrink, so the socket path stays inside the measured limit.
pub fn session_name(root: &str, uuid: &str, config_root: &str) -> Result<String> {
    let base: String = root
        .trim_end_matches('/')
        .rsplit('/')
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
    let socket_prefix = format!("{config_root}/herdr/sessions/");
    let room =
        (MAX_SOCKET_BYTES.saturating_sub(socket_prefix.len())).saturating_sub("/herdr.sock".len());
    let room = room.min(MAX_NAME_BYTES);
    let fixed_length = fixed.len() - 2; // "%s" is a two-byte placeholder
    if room <= fixed_length {
        return Err(Failure::new(format!(
            "Herdr session name cannot fit: {socket_prefix}{}/herdr.sock exceeds the {MAX_SOCKET_BYTES}-byte Unix socket path; set a shorter XDG_CONFIG_HOME",
            fixed.replace("%s", "<project>")
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
    if agent.awaits_reported_session() {
        // The preflight passed, but ready needs the session Herdr reports
        // after launch (#195); stop before any ownership or resource.
        return Err(Failure::new(format!(
            "{} companion cannot start yet: its reported session is not awaited",
            agent.label()
        )));
    }
    let executable = options_in
        .executable
        .clone()
        .unwrap_or_else(|| agent.executable(host));
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

fn on_path(name: &str) -> Option<String> {
    std::env::var("PATH").ok().and_then(|path| {
        path.split(':')
            .map(|directory| format!("{directory}/{name}"))
            .find(|candidate| std::fs::File::open(candidate).is_ok())
    })
}

const HIDDEN_SIDEBAR: [(&str, &str); 2] = [
    ("sidebar_start_collapsed", "true"),
    ("sidebar_collapsed_mode", "\"hidden\""),
];

/// The person's Herdr config: `HERDR_CONFIG_PATH`, else under the config root.
fn user_config_path(var: &dyn Fn(&str) -> Option<String>) -> String {
    var("HERDR_CONFIG_PATH")
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| format!("{}/herdr/config.toml", config_root_in(var)))
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
    let base = std::fs::read_to_string(user_config_path(&var)).unwrap_or_default();
    let path = format!(
        "{}/.uml-viewer/herdr-attach.toml",
        root.trim_end_matches('/')
    );
    std::fs::write(&path, hidden_sidebar(&base)).ok()?;
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
    (check.exit == Some(0) && !check.timeout).then_some(path)
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
        let all = env(&[
            ("HERDR_CONFIG_PATH", "/c/herdr.toml"),
            ("XDG_CONFIG_HOME", "/x/"),
            ("HOME", "/h"),
        ]);
        assert_eq!("/c/herdr.toml", user_config_path(&all));
        let xdg = env(&[
            ("HERDR_CONFIG_PATH", ""),
            ("XDG_CONFIG_HOME", "/x/"),
            ("HOME", "/h"),
        ]);
        assert_eq!("/x/herdr/config.toml", user_config_path(&xdg));
        let home = env(&[("HOME", "/h")]);
        assert_eq!("/h/.config/herdr/config.toml", user_config_path(&home));
    }

    /// A typical `$HOME/.config` root; no test reads the process environment.
    const HOME_CONFIG: &str = "/home/person/.config";

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
        let host = Host::posix();
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
        let loaded = load(&root, &Host::posix()).unwrap().unwrap();
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
        let path = format!("{}:/definitely/empty", bin.display());
        let mut host = Host::posix();
        host.variable = std::rc::Rc::new(move |name: &str| (name == "PATH").then(|| path.clone()));
        let root = directory.to_string_lossy().into_owned();
        (directory, root, host)
    }

    #[test]
    fn a_codex_fresh_start_runs_its_preflight_and_then_stops_before_any_resource() {
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
        // A passing preflight still stops: the launch and the reported
        // session wait land in #195, so no ownership or Herdr resource exists.
        let (directory, root, host) = codex_project("codex-cli 0.166.0");
        let error = create(&root, &host, options()).err().unwrap();
        assert!(
            error
                .message
                .starts_with("Codex CLI companion cannot start yet"),
            "{}",
            error.message
        );
        assert!(!ownership::exists(&root));
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
