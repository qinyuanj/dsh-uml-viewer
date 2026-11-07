//! Pure Herdr ownership rules: pane shells, observed identity and cleanup
//! checkpoints (port of `love/uml/domain/herdr.lua`). The adapter owns every
//! external command.

use crate::agent::CodingAgent;
use crate::json::Json;
use crate::record;
use crate::text::shell_word;
use crate::vocab::{Failure, Result};
use edn::Value;

pub fn session_valid(session: &str) -> bool {
    let Some(rest) = session.strip_prefix("uml-viewer-") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The pane shells a command can be typed into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaneShell {
    Posix,
    /// Windows PowerShell 5.1 (Herdr's pane shell on Windows) or pwsh.
    PowerShell,
}

fn pane_shell(shell_name: &str) -> Result<PaneShell> {
    let name = shell_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    let name = name.strip_prefix('-').unwrap_or(&name);
    match name {
        "sh" | "bash" | "zsh" | "dash" => Ok(PaneShell::Posix),
        "powershell.exe" | "pwsh.exe" | "powershell" | "pwsh" => Ok(PaneShell::PowerShell),
        _ => Err(Failure::new(
            "Unsupported Herdr pane shell; command was not sent",
        )),
    }
}

/// Quote the literal argv for the pane's own shell: POSIX shells, or
/// PowerShell (the call operator with single-quoted words). Any other shell
/// refuses before the command is sent.
pub fn pane_command(command: &[String], shell_name: &str) -> Result<String> {
    match pane_shell(shell_name)? {
        PaneShell::Posix => Ok(command
            .iter()
            .map(|word| shell_word(word))
            .collect::<Vec<_>>()
            .join(" ")),
        PaneShell::PowerShell => {
            let words = command
                .iter()
                .map(|word| powershell_word(word))
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("& {}", words.join(" ")))
        }
    }
}

/// One literal PowerShell word: single quotes, with every quote character
/// PowerShell treats as one (`'` and U+2018..U+201B) doubled. Windows
/// PowerShell 5.1 rebuilds a native command line without escaping, so a word
/// it cannot pass through intact is refused instead: an empty word, an
/// embedded `"`, a control character, or whitespace with a trailing `\`.
fn powershell_word(word: &str) -> Result<String> {
    let unsafe_word = word.is_empty()
        || word.contains('"')
        || word.chars().any(char::is_control)
        || (word.contains(char::is_whitespace) && word.ends_with('\\'));
    if unsafe_word {
        return Err(Failure::new(format!(
            "Herdr PowerShell pane cannot pass the argument {word:?} intact; command was not sent"
        )));
    }
    let mut out = String::from("'");
    for c in word.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    Ok(out)
}

/// The terminal modes a previous agent may have left on: mouse tracking
/// first, then the alternate screen, bracketed paste, kitty keyboard flags
/// and the hidden cursor. Each entry follows an ESC.
const RESET_SEQUENCES: [&str; 10] = [
    "[?1003l", "[?1002l", "[?1006l", "[?1000l", "[?1049l", "[?2004l", "[<u", "[<u", "[<u", "[?25h",
];

/// The command typed into the pane's shell that writes the reset to the
/// pane's own terminal. A typed ESC byte is a key to the line editor and never
/// reaches the terminal, so the shell spells it: `\033` for printf on POSIX,
/// `[char]27` in PowerShell (which has no printf).
pub fn reset_command(shell_name: &str) -> Result<String> {
    Ok(match pane_shell(shell_name)? {
        PaneShell::Posix => format!(
            "printf {}",
            shell_word(&RESET_SEQUENCES.map(|s| format!("\\033{s}")).concat())
        ),
        PaneShell::PowerShell => format!(
            "[Console]::Write(('{}' -f [char]27))",
            RESET_SEQUENCES.map(|s| format!("{{0}}{s}")).concat()
        ),
    })
}

/// A directory as Herdr reports it, without the trailing separator Windows
/// Herdr adds in `workspace create` and `pane process-info` (not in `api
/// snapshot`).
fn directory_text(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed.ends_with(':') {
        path
    } else {
        trimmed
    }
}

/// Ownership is built on the identity the backend was constructed with, so
/// lifecycle fields such as the `--resume` recovery command survive the
/// commit.
pub fn ownership_base(
    identity: Option<&Value>,
    session: &str,
    cwd: &str,
    command: &[String],
) -> Value {
    let mut base = identity.cloned().unwrap_or_else(|| Value::Map(vec![]));
    edn::record::put(&mut base, "backend", Value::keyword("herdr"));
    edn::record::put(&mut base, "owner-role", Value::keyword("companion"));
    edn::record::put(&mut base, "session", Value::String(session.to_owned()));
    edn::record::put(&mut base, "cwd", Value::String(cwd.to_owned()));
    edn::record::put(
        &mut base,
        "command",
        Value::Vector(
            command
                .iter()
                .map(|arg| Value::String(arg.clone()))
                .collect(),
        ),
    );
    edn::record::put(&mut base, "provisional", Value::Bool(true));
    base
}

/// A copy without the provisional mark.
pub fn committed(record: &Value) -> Value {
    let mut owned = record.clone();
    edn::record::remove(&mut owned, "provisional");
    owned
}

const FIELDS: [&str; 10] = [
    "socket",
    "workspace",
    "pane",
    "terminal-id",
    "shell-pid",
    "owner-start",
    "server-pid",
    "server-start",
    "cwd",
    "command",
];

pub fn complete(identity: &Value) -> Result<()> {
    let session = record::get_name(identity, "session");
    if !session.as_deref().is_some_and(session_valid) {
        return Err(Failure::new(
            "Herdr requires an isolated uml-viewer named session",
        ));
    }
    if record::get_name(identity, "backend").as_deref() != Some("herdr")
        || record::get_name(identity, "owner-role").as_deref() != Some("companion")
    {
        return Err(Failure::new("Incomplete Herdr ownership"));
    }
    for field in FIELDS {
        if edn::record::get(identity, field).is_none() {
            return Err(Failure::new("Incomplete Herdr ownership"));
        }
    }
    Ok(())
}

/// Re-verify the full addressing chain against a live endpoint (Lua
/// `H.observe`): socket and session match the status, the snapshot holds
/// exactly one pane with the recorded ids, and the pane's process-info
/// reports the recorded shell pid and cwd.
pub fn observe(
    expected: &Value,
    status: &crate::json::Json,
    snapshot: &crate::json::Json,
    info: Option<&crate::json::Json>,
    canonical: &dyn Fn(&str) -> String,
) -> Result<()> {
    complete(expected)?;
    let session = record::get_name(expected, "session").unwrap_or_default();
    let socket = record::get_name(expected, "socket").unwrap_or_default();
    let status_session = status
        .get("session")
        .and_then(crate::json::Json::as_str)
        .unwrap_or_default();
    let status_socket = status
        .get("socket")
        .and_then(crate::json::Json::as_str)
        .unwrap_or_default();
    if status_session != session || status_socket != socket {
        return Err(Failure::new("Herdr socket changed"));
    }
    observe_snapshot(expected, snapshot)?;
    let shell_pid = record::get_i64(expected, "shell-pid");
    let pane = record::get_name(expected, "pane").unwrap_or_default();
    let cwd = record::get_name(expected, "cwd").unwrap_or_default();
    let info_ok = info.is_some_and(|info| {
        info.get("pane_id").and_then(crate::json::Json::as_str) == Some(pane.as_str())
            && info.get("shell_pid").and_then(crate::json::Json::as_i64) == shell_pid
    });
    let snapshot_pane = snapshot_pane(expected, snapshot);
    let canonical_ok = snapshot_pane
        .and_then(|pane| pane.get("cwd"))
        .and_then(crate::json::Json::as_str)
        .is_some_and(|observed| {
            directory_text(&canonical(&cwd)) == directory_text(&canonical(observed))
        });
    if !info_ok || !canonical_ok {
        return Err(Failure::new("Herdr pane identity changed"));
    }
    Ok(())
}

fn snapshot_pane<'a>(
    expected: &Value,
    snapshot: &'a crate::json::Json,
) -> Option<&'a crate::json::Json> {
    let pane_id = record::get_name(expected, "pane")?;
    let matches: Vec<&crate::json::Json> = snapshot
        .get("panes")?
        .array_items()?
        .iter()
        .filter(|pane| {
            pane.get("pane_id").and_then(crate::json::Json::as_str) == Some(pane_id.as_str())
        })
        .collect();
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

fn observe_snapshot(expected: &Value, snapshot: &crate::json::Json) -> Result<()> {
    use crate::json::Json;
    let version_ok = snapshot
        .get("version")
        .and_then(Json::as_str)
        .is_some_and(crate::herdr::select::herdr_version_supported);
    let protocol_ok = snapshot.get("protocol").and_then(Json::as_i64) == Some(22);
    if !version_ok
        || !protocol_ok
        || snapshot.get("panes").and_then(Json::array_items).is_none()
        || snapshot
            .get("workspaces")
            .and_then(Json::array_items)
            .is_none()
    {
        return Err(Failure::new("Invalid Herdr snapshot"));
    }
    let workspace = record::get_name(expected, "workspace").unwrap_or_default();
    let workspace_found = snapshot
        .get("workspaces")
        .and_then(Json::array_items)
        .is_some_and(|workspaces| {
            workspaces
                .iter()
                .any(|w| w.get("workspace_id").and_then(Json::as_str) == Some(workspace.as_str()))
        });
    let terminal = record::get_name(expected, "terminal-id").unwrap_or_default();
    let pane_ok = snapshot_pane(expected, snapshot).is_some_and(|pane| {
        pane.get("workspace_id").and_then(Json::as_str) == Some(workspace.as_str())
            && pane.get("terminal_id").and_then(Json::as_str) == Some(terminal.as_str())
    });
    if !workspace_found || !pane_ok {
        return Err(Failure::new("Herdr pane identity changed"));
    }
    Ok(())
}

/// Recovery may submit only when the pane foreground is exactly the recorded
/// shell.
pub fn foreground_allowed(info: Option<&crate::json::Json>, shell_pid: i64) -> Result<bool> {
    let foreground = info.and_then(|info| info.get("foreground_processes"));
    let Some(foreground) = foreground.and_then(crate::json::Json::array_items) else {
        return Err(Failure::new(
            "Recovery foreground is unknown or occupied; refusing replacement",
        ));
    };
    for entry in foreground {
        if entry.get("pid").and_then(crate::json::Json::as_i64) != Some(shell_pid) {
            return Err(Failure::new(
                "Recovery foreground is unknown or occupied; refusing replacement",
            ));
        }
    }
    Ok(true)
}

/// The inherited variables an isolated server must not see. A launch from
/// inside a Herdr pane carries that pane's `HERDR_*` context; Herdr 0.9.5's
/// headless bootstrap turns `HERDR_STARTUP_CWD` into an extra startup
/// workspace (contract §6), and the rest name a session this server is not.
/// `HERDR_CONFIG_PATH` is the person's own config choice, so it stays.
/// An inherited `CODEX_THREAD_ID` names the Codex session that launched
/// `./uml`; the Herdr Codex hook would drop the companion's own report.
pub fn inherited_pane_env(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| {
            (name.starts_with("HERDR_") && *name != "HERDR_CONFIG_PATH")
                || *name == "CODEX_THREAD_ID"
        })
        .cloned()
        .collect()
}

/// The session id Herdr reports for `agent` occupying the owned pane and
/// terminal: `None` while nothing is reported yet; a report from another
/// provider or integration, or one that is not a UUID id, is refused.
pub fn reported_session(
    snapshot: &Json,
    pane: &str,
    terminal: &str,
    agent: CodingAgent,
) -> Result<Option<String>> {
    let occupant = snapshot
        .get("agents")
        .and_then(Json::array_items)
        .and_then(|agents| {
            agents.iter().find(|entry| {
                entry.get("pane_id").and_then(Json::as_str) == Some(pane)
                    && entry.get("terminal_id").and_then(Json::as_str) == Some(terminal)
            })
        });
    let Some(occupant) = occupant else {
        return Ok(None);
    };
    let session = match occupant.get("agent_session") {
        None | Some(Json::Null) => return Ok(None),
        Some(session) => session,
    };
    let field = |name: &str| session.get(name).and_then(Json::as_str);
    if occupant.get("agent").and_then(Json::as_str) != Some(agent.name())
        || field("agent") != Some(agent.name())
        || field("source") != Some(&format!("herdr:{}", agent.name()))
        || field("kind") != Some("id")
    {
        return Err(Failure::new(format!(
            "Herdr reported a session that is not this {} companion's; refused",
            agent.diagnostic_name()
        )));
    }
    match field("value") {
        Some(id) if crate::agent::is_uuid(id) => Ok(Some(id.to_owned())),
        _ => Err(Failure::new(format!(
            "Herdr reported a {} session id that is not a UUID; refused",
            agent.diagnostic_name()
        ))),
    }
}

/// Ownership changed when any field but the cleanup checkpoint differs.
pub fn ownership_changed(identity: &Value, saved: &Value) -> bool {
    record::without_cleanup_phase(identity) != record::without_cleanup_phase(saved)
}

const PHASES: [&str; 5] = [
    "workspace-closing",
    "workspace-closed",
    "server-stopping",
    "server-stopped",
    "session-deleting",
];

pub fn cleanup_phase_valid(phase: Option<&str>) -> bool {
    match phase {
        None => true,
        Some(phase) => PHASES.contains(&phase),
    }
}

/// A runner event counts only for the current generation token.
#[cfg(test)]
pub fn runner_event<'a>(identity: &Value, event: &'a Value) -> Option<&'a Value> {
    let token = record::get_name(identity, "runner-token")?;
    if record::get_name(event, "token").as_deref() == Some(token.as_str()) {
        Some(event)
    } else {
        None
    }
}

/// Wake requires literal text, a unique token inside it and a consumption
/// acknowledgement callback.
pub fn wake_arguments(token: &str, keys: &[String]) -> Result<()> {
    if token.is_empty() || keys.len() != 2 || keys[0] != "-l" || !keys[1].contains(token) {
        return Err(Failure::new(
            "Herdr wake requires literal text, a unique token and consumption acknowledgement",
        ));
    }
    Ok(())
}

/// Where one delivered wake stands. Herdr folds a prompt queued during a
/// turn into the same `working` stretch, so an idle agent past the accepted
/// prompt's state sequence has ended the turn that saw the mail.
#[derive(Debug, PartialEq, Eq)]
pub enum WakeVerdict {
    Consumed,
    Pending,
    Lost,
}

pub fn wake_verdict(delivered_seq: i64, status: &str, seq: i64, queued: bool) -> WakeVerdict {
    if !queued {
        WakeVerdict::Consumed
    } else if matches!(status, "idle" | "done") && seq > delivered_seq {
        WakeVerdict::Lost
    } else {
        WakeVerdict::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Json;
    use std::collections::BTreeMap;

    fn owned() -> Value {
        edn::parse(
            r#"{:backend :herdr :owner-role :companion :session "uml-viewer-A"
                :socket "/x/herdr.sock" :workspace "w1" :pane "w1:p1" :terminal-id "term_A"
                :shell-pid 42 :owner-start "shell-start" :server-pid "41"
                :server-start "server-start" :cwd "/project" :command ["claude"]}"#,
        )
        .unwrap()
    }

    fn status() -> Json {
        Json::Object(BTreeMap::from([
            (
                "session".to_owned(),
                Json::String("uml-viewer-A".to_owned()),
            ),
            (
                "socket".to_owned(),
                Json::String("/x/herdr.sock".to_owned()),
            ),
        ]))
    }

    fn snapshot() -> Json {
        Json::Object(BTreeMap::from([
            ("version".to_owned(), Json::String("0.9.5".to_owned())),
            ("protocol".to_owned(), Json::Number(22.0)),
            (
                "panes".to_owned(),
                Json::Array(vec![Json::Object(BTreeMap::from([
                    ("pane_id".to_owned(), Json::String("w1:p1".to_owned())),
                    ("workspace_id".to_owned(), Json::String("w1".to_owned())),
                    ("terminal_id".to_owned(), Json::String("term_A".to_owned())),
                    ("cwd".to_owned(), Json::String("/project".to_owned())),
                ]))]),
            ),
            (
                "workspaces".to_owned(),
                Json::Array(vec![Json::Object(BTreeMap::from([(
                    "workspace_id".to_owned(),
                    Json::String("w1".to_owned()),
                )]))]),
            ),
        ]))
    }

    fn process_info() -> Json {
        Json::Object(BTreeMap::from([
            ("pane_id".to_owned(), Json::String("w1:p1".to_owned())),
            ("shell_pid".to_owned(), Json::Number(42.0)),
        ]))
    }

    #[test]
    fn session_names_must_carry_the_uml_viewer_prefix() {
        assert!(session_valid("uml-viewer-A"));
        assert!(session_valid("uml-viewer-proj-0000000f-abcdef12"));
        assert!(!session_valid("A"));
        assert!(!session_valid("uml-viewer-"));
        assert!(!session_valid("uml-viewer-a b"));
    }

    #[test]
    fn pane_commands_accept_login_shells_with_a_leading_dash() {
        for shell in ["-zsh", "-bash", "-sh", "/bin/-zsh"] {
            assert_eq!(
                "'echo' 'hi there'",
                pane_command(&["echo".into(), "hi there".into()], shell).unwrap(),
                "{shell}"
            );
        }
        assert!(pane_command(&["echo".into(), "hi".into()], "fish").is_err());
        assert!(pane_command(&["echo".into(), "hi".into()], "cmd.exe").is_err());
    }

    /// Herdr's Windows pane shell is Windows PowerShell 5.1. These forms were
    /// typed into a real pane (Herdr 0.9.5) and reached a child argv intact:
    /// spaces, CJK, `'`, `%COMSPEC%` and `$HOME` stay literal.
    #[test]
    fn a_powershell_pane_gets_the_call_operator_and_literal_single_quoted_words() {
        let argv: Vec<String> = [
            "C:\\Program Files\\uml\\uml-viewer.exe",
            "companion-runner",
            "C:\\p 中文 it's\\.uml-viewer/x.edn",
            "%COMSPEC% $HOME `t ‘q’",
        ]
        .map(str::to_owned)
        .to_vec();
        let expected = "& 'C:\\Program Files\\uml\\uml-viewer.exe' 'companion-runner' \
             'C:\\p 中文 it''s\\.uml-viewer/x.edn' '%COMSPEC% $HOME `t ‘‘q’’'";
        for shell in [
            "powershell.exe",
            "C:\\WINDOWS\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            "PWSH.EXE",
        ] {
            assert_eq!(expected, pane_command(&argv, shell).unwrap(), "{shell}");
        }
    }

    /// PowerShell 5.1 drops an empty argument and splits one with `"`, so
    /// such argv is refused before anything is typed.
    #[test]
    fn a_powershell_pane_refuses_words_it_cannot_pass_intact() {
        for word in ["", "x\"y", "a\nb", "C:\\a b\\"] {
            let error =
                pane_command(&["C:\\x.exe".into(), word.into()], "powershell.exe").unwrap_err();
            assert!(error.message.contains("command was not sent"), "{word:?}");
        }
        assert!(pane_command(&["C:\\x.exe".into(), "C:\\ab\\".into()], "powershell.exe").is_ok());
    }

    #[test]
    fn the_reset_is_spelled_for_the_pane_shell() {
        assert_eq!(
            r"printf '\033[?1003l\033[?1002l\033[?1006l\033[?1000l\033[?1049l\033[?2004l\033[<u\033[<u\033[<u\033[?25h'",
            reset_command("-bash").unwrap()
        );
        assert_eq!(
            "[Console]::Write(('{0}[?1003l{0}[?1002l{0}[?1006l{0}[?1000l{0}[?1049l{0}[?2004l{0}[<u{0}[<u{0}[<u{0}[?25h' -f [char]27))",
            reset_command("powershell.exe").unwrap()
        );
        assert!(reset_command("fish").is_err());
    }

    /// Windows Herdr reports a pane cwd with a trailing `\` in process-info
    /// and without it in the snapshot; both name the recorded directory.
    #[test]
    fn observe_accepts_the_pane_cwd_with_or_without_a_trailing_separator() {
        let mut owned = owned();
        edn::record::put(&mut owned, "cwd", Value::String("C:\\p q".to_owned()));
        let with_cwd = |cwd: &str| {
            let mut changed = snapshot();
            if let Json::Object(map) = &mut changed
                && let Some(Json::Array(panes)) = map.get_mut("panes")
                && let Json::Object(pane) = &mut panes[0]
            {
                pane.insert("cwd".to_owned(), Json::String(cwd.to_owned()));
            }
            observe(
                &owned,
                &status(),
                &changed,
                Some(&process_info()),
                &(|p| p.to_owned()),
            )
        };
        assert!(with_cwd("C:\\p q").is_ok());
        assert!(with_cwd("C:\\p q\\").is_ok());
        assert!(with_cwd("C:\\p").is_err());
        assert_eq!("C:\\", directory_text("C:\\"));
        assert_eq!("/", directory_text("/"));
    }

    #[test]
    fn ownership_base_builds_on_the_constructed_identity() {
        let identity = edn::parse(
            r#"{:recovery-command ["claude" "--resume" "u"] :claude-session "u" :provisional true}"#,
        )
        .unwrap();
        let base = ownership_base(
            Some(&identity),
            "uml-viewer-a",
            "/project",
            &["claude".into(), "--session-id".into(), "u".into()],
        );
        assert_eq!(Some("herdr".to_owned()), record::get_name(&base, "backend"));
        assert_eq!(
            Some("uml-viewer-a".to_owned()),
            record::get_name(&base, "session")
        );
        assert_eq!(
            vec!["claude", "--resume", "u"],
            record::string_vec(edn::record::seq(edn::record::get(
                &base,
                "recovery-command"
            )))
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
        );
        assert!(edn::record::is_true(&base, "provisional"));
    }

    #[test]
    fn committed_drops_the_provisional_mark() {
        let base = edn::record::record(vec![
            ("session", Value::String("s".to_owned())),
            ("provisional", Value::Bool(true)),
        ]);
        let owned = committed(&base);
        assert_eq!(None, edn::record::get(&owned, "provisional"));
        assert!(edn::record::is_true(&base, "provisional"));
    }

    #[test]
    fn observe_rechecks_the_full_addressing_chain() {
        let owned = owned();
        observe(
            &owned,
            &status(),
            &snapshot(),
            Some(&process_info()),
            &(|p| p.to_owned()),
        )
        .unwrap();
        let mut changed = snapshot();
        if let Json::Object(map) = &mut changed
            && let Some(Json::Array(panes)) = map.get_mut("panes")
        {
            panes[0] = Json::Object(BTreeMap::from([
                ("pane_id".to_owned(), Json::String("w1:p1".to_owned())),
                ("workspace_id".to_owned(), Json::String("w1".to_owned())),
                ("terminal_id".to_owned(), Json::String("term_B".to_owned())),
                ("cwd".to_owned(), Json::String("/project".to_owned())),
            ]));
        }
        let error = observe(
            &owned,
            &status(),
            &changed,
            Some(&process_info()),
            &(|p| p.to_owned()),
        )
        .unwrap_err();
        assert_eq!("Herdr pane identity changed", error.message);
    }

    #[test]
    fn observe_rejects_a_changed_socket() {
        let owned = owned();
        let other = Json::Object(BTreeMap::from([
            (
                "session".to_owned(),
                Json::String("uml-viewer-A".to_owned()),
            ),
            (
                "socket".to_owned(),
                Json::String("/other/herdr.sock".to_owned()),
            ),
        ]));
        let error = observe(
            &owned,
            &other,
            &snapshot(),
            Some(&process_info()),
            &(|p| p.to_owned()),
        )
        .unwrap_err();
        assert_eq!("Herdr socket changed", error.message);
    }

    #[test]
    fn foreground_must_be_exactly_the_recorded_shell() {
        let only_shell = Json::Object(BTreeMap::from([(
            "foreground_processes".to_owned(),
            Json::Array(vec![Json::Object(BTreeMap::from([(
                "pid".to_owned(),
                Json::Number(42.0),
            )]))]),
        )]));
        assert!(foreground_allowed(Some(&only_shell), 42).unwrap());
        let occupied = Json::Object(BTreeMap::from([(
            "foreground_processes".to_owned(),
            Json::Array(vec![
                Json::Object(BTreeMap::from([("pid".to_owned(), Json::Number(42.0))])),
                Json::Object(BTreeMap::from([("pid".to_owned(), Json::Number(7.0))])),
            ]),
        )]));
        assert!(foreground_allowed(Some(&occupied), 42).is_err());
        assert!(foreground_allowed(None, 42).is_err());
    }

    #[test]
    fn ownership_change_ignores_only_the_cleanup_checkpoint() {
        let owned = owned();
        let mut saved = owned.clone();
        edn::record::put(
            &mut saved,
            "cleanup-phase",
            Value::keyword("workspace-closed"),
        );
        assert!(!ownership_changed(&owned, &saved));
        edn::record::put(&mut saved, "pane", Value::String("w1:p2".to_owned()));
        assert!(ownership_changed(&owned, &saved));
    }

    #[test]
    fn cleanup_phases_are_a_fixed_grammar() {
        assert!(cleanup_phase_valid(None));
        for phase in PHASES {
            assert!(cleanup_phase_valid(Some(phase)), "{phase}");
        }
        assert!(!cleanup_phase_valid(Some("unknown")));
    }

    #[test]
    fn runner_events_match_only_the_current_generation_token() {
        let identity =
            edn::record::record(vec![("runner-token", Value::String("gen-2".to_owned()))]);
        let event = edn::record::record(vec![
            ("token", Value::String("gen-2".to_owned())),
            ("phase", Value::keyword("started")),
        ]);
        assert!(runner_event(&identity, &event).is_some());
        let stale = edn::record::record(vec![("token", Value::String("gen-1".to_owned()))]);
        assert!(runner_event(&identity, &stale).is_none());
    }

    #[test]
    fn wake_requires_literal_text_a_unique_token_and_acknowledgement() {
        assert!(wake_arguments("token-1", &["-l".into(), "mail token-1".into()]).is_ok());
        assert!(wake_arguments("", &["-l".into(), "mail".into()]).is_err());
        assert!(wake_arguments("token", &["mail token".into()]).is_err());
        assert!(wake_arguments("token", &["-p".into(), "mail token".into()]).is_err());
        assert!(wake_arguments("token", &["-l".into(), "mail other".into()]).is_err());
    }

    #[test]
    fn a_wake_is_lost_only_when_claude_ends_a_later_turn_with_the_mail_queued() {
        use WakeVerdict::*;
        assert_eq!(Consumed, wake_verdict(4, "idle", 5, false));
        assert_eq!(Consumed, wake_verdict(4, "working", 4, false));
        assert_eq!(Pending, wake_verdict(4, "working", 4, true));
        assert_eq!(Pending, wake_verdict(4, "blocked", 5, true));
        assert_eq!(Pending, wake_verdict(4, "unknown", 5, true));
        // Idle at the delivery sequence: the turn has not been observed yet.
        assert_eq!(Pending, wake_verdict(4, "idle", 4, true));
        assert_eq!(Lost, wake_verdict(4, "idle", 5, true));
        assert_eq!(Lost, wake_verdict(4, "done", 6, true));
    }
}
