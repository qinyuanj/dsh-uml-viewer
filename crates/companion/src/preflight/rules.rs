//! Pure read-only preflight rules (port of `love/uml/domain/companion.lua`):
//! the backend a recorded ownership names, the `--backend` argument, and the
//! validation that runs before any Herdr command or resource creation.

use crate::agent::CodingAgent;
use crate::herdr::select;
use crate::record;
use crate::vocab::{Failure, Result, RunOut};
use edn::Value;

pub const LEGACY_GUIDANCE: &str = "tmux/psmux companions are retired. Ownership and resources were retained; no takeover or cleanup was attempted. Finish the old viewer/session using the old release, verify that this project's legacy resources have exited, then archive .uml-viewer/companion.edn and start ./uml fresh with Herdr. Do not use --restart to migrate; retain session.edn and mailbox queues.";

/// The backend named by a present ownership record. Legacy (tmux/psmux) or
/// unmarked records are retained and refused.
pub fn recorded_backend(record: Option<&Value>) -> Result<Option<String>> {
    let Some(record) = record else {
        return Ok(None);
    };
    if record::is_empty_map(record) {
        return Ok(None);
    }
    if !record::is_map(record) {
        return Ok(None);
    }
    let backend = record::get_name(record, "backend");
    match backend.as_deref() {
        None | Some("tmux") | Some("psmux") => {
            Err(Failure::with_reason(LEGACY_GUIDANCE, "legacy-ownership"))
        }
        Some(backend) => Ok(Some(backend.to_owned())),
    }
}

/// Removes only the exact lowercase `--backend herdr` pair; all other argv is
/// retained literally (an uppercase `--BACKEND` is not an option).
pub fn selection_args(args: &[String]) -> Result<(Vec<String>, Option<String>)> {
    let mut forwarded = Vec::new();
    let mut selected: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--backend" {
            let value = args.get(index + 1).map(String::as_str);
            if matches!(value, Some("tmux") | Some("psmux")) {
                return Err(Failure::with_reason(LEGACY_GUIDANCE, "retired-backend"));
            }
            if selected.is_some() || value != Some("herdr") {
                return Err(Failure::new(
                    "Unknown or duplicate companion backend; use --backend herdr",
                ));
            }
            selected = Some("herdr".to_owned());
            index += 2;
        } else {
            forwarded.push(arg.clone());
            index += 1;
        }
    }
    Ok((forwarded, selected))
}

/// The provider an explicit `--coding-agent` names. Only `claude` and
/// `codex` are accepted, at most once; absence leaves the choice to the
/// recorded ownership, or Claude for a fresh start.
pub fn coding_agent_arg(args: &[String]) -> Result<Option<CodingAgent>> {
    let refused = || {
        Failure::new("Unknown or duplicate coding-agent provider; use --coding-agent claude|codex")
    };
    let mut selected = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--coding-agent" {
            let agent = match args.get(index + 1).map(String::as_str) {
                Some("claude") => CodingAgent::Claude,
                Some("codex") => CodingAgent::Codex,
                _ => return Err(refused()),
            };
            if selected.replace(agent).is_some() {
                return Err(refused());
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(selected)
}

/// The provider this launch runs: the recorded one, which an explicit choice
/// may only confirm, or the explicit choice (Claude by default) when nothing
/// is recorded. Switching requires a normal close and a fresh start, so a
/// conflict is refused before anything is replaced or duplicated.
pub fn provider(record: Option<&Value>, requested: Option<CodingAgent>) -> Result<CodingAgent> {
    let recorded = match record {
        Some(record) if record::is_map(record) && !record::is_empty_map(record) => {
            Some(crate::agent::identity_of(record)?.agent)
        }
        _ => None,
    };
    match (recorded, requested) {
        (Some(recorded), Some(requested)) if recorded != requested => Err(Failure::new(format!(
            "Companion ownership records {}; --coding-agent {} conflicts with it. Nothing was replaced; close the companion normally, then start fresh with the other provider",
            recorded.label(),
            requested.name()
        ))),
        (recorded, requested) => Ok(recorded.or(requested).unwrap_or_default()),
    }
}

pub const CODEX_GUIDANCE: &str = "Install and log in to Codex CLI yourself, or set CODEX_BIN to its executable; nothing is installed, logged in or configured automatically, and the launch does not fall back to Claude Code.";

/// The oldest Codex CLI release with end-to-end evidence: the real WSL2
/// acceptance (#199) passed with 0.166.0.
pub const CODEX_FLOOR: [u64; 3] = [0, 166, 0];
/// Codex companions need Herdr's Codex occupant and `agent_session` report.
pub const CODEX_HERDR_FLOOR: [u64; 3] = [0, 9, 5];

fn release(floor: [u64; 3]) -> String {
    floor.map(|part| part.to_string()).join(".")
}

/// `codex --version` must print `codex-cli X.Y.Z` at or above the floor.
pub fn codex_version(executable: &str, out: &RunOut) -> Result<()> {
    let line = out.out.trim();
    let version = line
        .strip_prefix("codex-cli ")
        .filter(|_| out.exit == Some(0));
    match version {
        Some(version) if select::release_at_least(version, CODEX_FLOOR) => Ok(()),
        Some(version) => Err(Failure::new(format!(
            "Codex CLI {version} at {executable} is not a supported release (X.Y.Z >= {}). {CODEX_GUIDANCE}",
            release(CODEX_FLOOR)
        ))),
        None => Err(Failure::new(format!(
            "Codex CLI at {executable} did not report a version (`--version` exited {}: {:?}). {CODEX_GUIDANCE}",
            exit_text(out),
            first_line(out)
        ))),
    }
}

/// The selected Herdr must be at least 0.9.5 (its protocol 22 is already a
/// selection requirement) and report its Codex integration as current.
/// `herdr integration status` is text only: one `name: state (...)` line.
pub fn codex_herdr(selected: &select::Selected, status: &RunOut) -> Result<()> {
    if !select::release_at_least(&selected.release, CODEX_HERDR_FLOOR) {
        return Err(Failure::new(format!(
            "Codex CLI companions need Herdr {} or newer (protocol 22); the selected Herdr {} at {} is older. Claude Code projects keep the 0.9.3 floor. {}",
            release(CODEX_HERDR_FLOOR),
            selected.release,
            selected.path,
            select::HERDR_INSTALL
        )));
    }
    let state = status
        .out
        .lines()
        .find_map(|line| line.trim().strip_prefix("codex: "))
        .map(|rest| {
            rest.split(" (")
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned()
        });
    match state.as_deref() {
        Some("current") if status.exit == Some(0) => Ok(()),
        Some(state) => Err(Failure::new(format!(
            "Herdr Codex integration is {state}, not current; run `{} integration install codex` yourself. Codex hooks must also be enabled and trusted so Herdr receives the session.",
            selected.path
        ))),
        None => Err(Failure::new(format!(
            "Herdr at {} reported no Codex integration (`integration status` exited {}); run `{} integration install codex` yourself.",
            selected.path,
            exit_text(status),
            selected.path
        ))),
    }
}

/// `codex login status` must succeed: an unusable login is reported, never
/// repaired.
pub fn codex_login(executable: &str, out: &RunOut) -> Result<()> {
    if out.exit == Some(0) {
        return Ok(());
    }
    Err(Failure::new(format!(
        "Codex CLI at {executable} has no usable login (`login status` exited {}: {:?}). {CODEX_GUIDANCE}",
        exit_text(out),
        first_line(out)
    )))
}

fn exit_text(out: &RunOut) -> String {
    match (out.exit, out.timeout) {
        (_, true) => "after a timeout".to_owned(),
        (Some(code), _) => code.to_string(),
        (None, _) => "without a status".to_owned(),
    }
}

fn first_line(out: &RunOut) -> String {
    [&out.out, &out.err]
        .iter()
        .flat_map(|text| text.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

pub struct PreflightOptions<'a> {
    pub root: &'a str,
    pub requested: Option<String>,
    pub restart: bool,
    /// The delivered-platform decision: `Ok(())` supported, `Err(reason)`.
    pub platform: std::result::Result<(), String>,
    pub canonical: &'a dyn Fn(&str) -> String,
}

/// Read-only validation before any Herdr command or resource creation.
pub fn preflight_checks(record: Option<&Value>, opts: &PreflightOptions) -> Result<String> {
    if let Some(record) = record
        && !record::is_map(record)
    {
        return Err(Failure::new(
            "Present companion ownership is corrupt or unreadable; file retained",
        ));
    }
    let recorded = recorded_backend(record)?;
    if let Some(recorded) = &recorded
        && recorded != "herdr"
    {
        return Err(Failure::new(
            "Unknown recorded companion backend; ownership retained",
        ));
    }
    if let Some(record) = record
        && record::is_map(record)
        && !record::is_empty_map(record)
    {
        let complete = [
            "socket",
            "workspace",
            "terminal-id",
            "shell-pid",
            "owner-start",
            "server-pid",
            "server-start",
        ]
        .iter()
        .all(|key| edn::record::get(record, key).is_some());
        if !complete
            || record::get_name(record, "session").is_none_or(|session| session.is_empty())
            || record::get_name(record, "pane").is_none_or(|pane| pane.is_empty())
            || edn::record::get(record, "cwd").is_none()
            || record::get_name(record, "owner-role").as_deref() != Some("companion")
            || edn::record::is_true(record, "provisional")
        {
            return Err(Failure::new(
                "Companion ownership is incomplete, provisional or belongs to another project; file retained",
            ));
        }
        let cwd = record::get_name(record, "cwd").unwrap_or_default();
        if (opts.canonical)(opts.root) != (opts.canonical)(&cwd) {
            return Err(Failure::new(
                "Companion ownership is incomplete, provisional or belongs to another project; file retained",
            ));
        }
    }
    if let Some(requested) = &opts.requested
        && requested != "herdr"
    {
        return Err(Failure::new(LEGACY_GUIDANCE));
    }
    if opts.restart && recorded.is_none() {
        return Err(Failure::new(
            "Companion ownership is missing; cannot restart",
        ));
    }
    if let Err(reason) = &opts.platform {
        return Err(Failure::new(format!(
            "{reason}; ownership and queues are retained."
        )));
    }
    Ok(recorded.unwrap_or_else(|| "herdr".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_guidance_is_ported_verbatim() {
        assert!(LEGACY_GUIDANCE.contains("Do not use --restart to migrate"));
    }

    #[test]
    fn selection_removes_only_the_exact_lowercase_backend_option() {
        let (args, backend) = selection_args(&[
            "--BACKEND".into(),
            "custom.edn".into(),
            "--backend".into(),
            "herdr".into(),
            "--restart".into(),
        ])
        .unwrap();
        assert_eq!(vec!["--BACKEND", "custom.edn", "--restart"], args);
        assert_eq!(Some("herdr".to_owned()), backend);
    }

    #[test]
    fn selection_refuses_retired_unknown_and_duplicate_backends() {
        for args in [vec!["--backend", "tmux"], vec!["--backend", "psmux"]] {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let error = selection_args(&args).unwrap_err();
            assert_eq!(Some("retired-backend"), error.reason);
        }
        for args in [
            vec!["--backend", "invalid"],
            vec!["--backend"],
            vec!["--backend", "herdr", "--backend", "herdr"],
        ] {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let error = selection_args(&args).unwrap_err();
            assert!(
                error
                    .message
                    .contains("Unknown or duplicate companion backend")
            );
        }
    }

    fn preflight(record: Option<&Value>, requested: Option<&str>, restart: bool) -> Result<String> {
        preflight_checks(
            record,
            &PreflightOptions {
                root: "/project",
                requested: requested.map(str::to_owned),
                restart,
                platform: Ok(()),
                canonical: &(|path| path.to_owned()),
            },
        )
    }

    #[test]
    fn preflight_accepts_fresh_launch_and_complete_ownership() {
        assert_eq!("herdr", preflight(None, None, false).unwrap());
        let record = edn::parse(
            r#"{:backend :herdr :owner-role :companion :cwd "/project" :session "uml-viewer-A"
                :pane "w1:p1" :socket "/s/herdr.sock" :workspace "w1" :terminal-id "term_A"
                :shell-pid 42 :owner-start "ss" :server-pid 41 :server-start "rs"}"#,
        )
        .unwrap();
        assert_eq!("herdr", preflight(Some(&record), None, false).unwrap());
    }

    #[test]
    fn preflight_refuses_legacy_and_unmarked_ownership_with_guidance() {
        for backend in [Some(":tmux"), Some(":psmux"), None] {
            for restart in [false, true] {
                let text = format!(
                    "{{{} :cwd \"/project\" :session \"saved\" :pane \"%1\"}}",
                    backend.map(|b| format!(":backend {b}")).unwrap_or_default()
                );
                let record = edn::parse(&text).unwrap();
                let error = preflight(Some(&record), None, restart).unwrap_err();
                assert_eq!(Some("legacy-ownership"), error.reason);
                assert!(error.message.contains("old release"));
                assert!(error.message.contains("Do not use --restart to migrate"));
            }
        }
    }

    #[test]
    fn preflight_refuses_unknown_backends_retired_requests_and_unsupported_platforms() {
        let record = edn::parse(r#"{:backend :other :owner-role :companion}"#).unwrap();
        let error = preflight(Some(&record), None, false).unwrap_err();
        assert!(error.message.contains("Unknown recorded companion backend"));
        for requested in ["tmux", "psmux", "unknown"] {
            let error = preflight(None, Some(requested), false).unwrap_err();
            assert!(error.message.contains("companions are retired"));
        }
        let refused = preflight_checks(
            None,
            &PreflightOptions {
                root: "/project",
                requested: None,
                restart: false,
                platform: Err("Host refused".to_owned()),
                canonical: &(|path| path.to_owned()),
            },
        )
        .unwrap_err();
        assert_eq!(
            "Host refused; ownership and queues are retained.",
            refused.message
        );
    }

    #[test]
    fn preflight_rejects_missing_restart_corrupt_and_incomplete_or_foreign_records() {
        let missing = preflight(None, None, true).unwrap_err();
        assert!(missing.message.contains("ownership is missing"));
        let corrupt = edn::parse("[:invalid]").unwrap();
        let error = preflight(Some(&corrupt), None, false).unwrap_err();
        assert!(error.message.contains("corrupt or unreadable"));
        let valid = r#"{:backend :herdr :owner-role :companion :cwd "/project" :session "saved"
            :pane "%1" :socket "/s" :workspace "w1" :terminal-id "t" :shell-pid 1
            :owner-start "o" :server-pid 2 :server-start "r"}"#;
        for changed in [
            valid.replace(":owner-role :companion", ":owner-role :launcher"),
            valid.replace(":session \"saved\"", ":session \"\""),
            valid.replace(":cwd \"/project\"", ":cwd \"/other\""),
            valid.replace("}", ":provisional true}"),
            valid.replace(":terminal-id \"t\" ", ""),
        ] {
            let record = edn::parse(&changed).unwrap();
            let error = preflight(Some(&record), None, true).unwrap_err();
            assert!(
                error
                    .message
                    .contains("incomplete, provisional or belongs to another project"),
                "{changed}: {}",
                error.message
            );
        }
    }
}
