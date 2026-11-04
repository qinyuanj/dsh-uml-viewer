//! Read-only companion backend preflight (port of
//! `love/uml/adapters/companion_preflight.lua` and the `companion-preflight`
//! role). Never creates a resource; selection is pure and the adapter only
//! supplies PATH candidates, executable probing, ownership bytes and commands.

use crate::agent::CodingAgent;
use crate::herdr::select::{self, Probe, Selected};
use crate::ownership;
use crate::posix;
use crate::vocab::{Failure, Result, RunOut};

/// PATH candidates, in PATH order, deduplicated by the candidate path.
fn candidates(path: &str) -> Vec<String> {
    let probe = |candidate: &str| {
        std::fs::File::open(candidate).ok().map(|_| Probe {
            executable: true,
            canonical: candidate.to_owned(),
        })
    };
    select::herdr_candidates(path, &probe)
}

fn run_herdr(argv: &[String], timeout_ms: Option<i64>) -> RunOut {
    let os_argv: Vec<std::ffi::OsString> = argv.iter().map(std::ffi::OsString::from).collect();
    posix::run(&os_argv, timeout_ms, &[])
}

/// The full read-only preflight: ownership, requested backend, platform, then
/// Herdr selection. Returns the selected Herdr.
pub fn select(root: &str, args: &[String], restart: bool) -> Result<Selected> {
    let path = std::env::var("PATH").unwrap_or_default();
    select_on_path(root, args, restart, &path)
}

/// Selection against an explicit PATH; the role passes the process PATH and
/// tests pass a fixture directory, so no test mutates process state.
pub fn select_on_path(root: &str, args: &[String], restart: bool, path: &str) -> Result<Selected> {
    select_agent(root, args, restart, path).map(|(selected, _)| selected)
}

/// The launch preflight: Herdr selection, then the provider this launch runs
/// and that provider's own requirements. Fresh and restart entries and the
/// lifecycle create and restart run it; shutdown and recovery do not, so a
/// missing Codex never blocks closing what is already owned.
pub fn launch(root: &str, args: &[String]) -> Result<(Selected, CodingAgent)> {
    launch_in(root, args, &|name| std::env::var(name).ok())
}

/// [`launch`] against an explicit environment (`PATH`, `CODEX_BIN`).
pub fn launch_in(
    root: &str,
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Selected, CodingAgent)> {
    companion_scope(root, args, env)?;
    let path = env("PATH").unwrap_or_default();
    let (selected, agent) = select_agent(root, args, false, &path)?;
    agent.launch_ready(&selected, env)?;
    Ok((selected, agent))
}

/// The companion runner names its project in [`COMPANION_ROOT`]. A lifecycle
/// entry run from inside a companion never starts a viewer or companion, and
/// restarts only that companion's own project (after `:quit-for-restart`).
fn companion_scope(
    root: &str,
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<()> {
    let Some(owner) = env(COMPANION_ROOT).filter(|owner| !owner.is_empty()) else {
        return Ok(());
    };
    let canonical = |path: &str| posix::canonical(path).unwrap_or_else(|_| path.to_owned());
    if args.iter().any(|arg| arg == "--restart") && canonical(&owner) == canonical(root) {
        return Ok(());
    }
    Err(Failure::new(format!(
        "Refusing to run the viewer lifecycle inside the companion for {owner:?}: \
         a companion never starts a viewer or companion, and restarts only its own \
         project with ./uml --restart after :quit-for-restart"
    )))
}

/// Set by the companion runner on its agent child to the canonical project root.
pub const COMPANION_ROOT: &str = "UML_COMPANION_ROOT";

/// Codex CLI requirements, each refused with its real cause: the executable
/// (`CODEX_BIN` taken literally, else `codex` on PATH), its version floor,
/// a usable login, hooks enabled for the companion's invocation, and a
/// selected Herdr whose release and Codex integration report the session. Nothing is installed, logged in or configured.
pub(crate) fn codex_ready(selected: &Selected, env: &dyn Fn(&str) -> Option<String>) -> Result<()> {
    let executable = CodingAgent::Codex.executable(env)?;
    let run = |argv: &[&str]| {
        let argv: Vec<std::ffi::OsString> = argv.iter().map(std::ffi::OsString::from).collect();
        posix::run(&argv, Some(CHECK_TIMEOUT_MS), &[])
    };
    let version = run(&[&executable, "--version"]);
    rules::codex_version(&executable, &version)?;
    rules::codex_herdr(selected, &run(&[&selected.path, "integration", "status"]))?;
    rules::codex_login(&executable, &run(&[&executable, "login", "status"]))?;
    rules::codex_hooks(
        &executable,
        &run(&[&executable, "--enable", "hooks", "features", "list"]),
    )
}

const CHECK_TIMEOUT_MS: i64 = 15_000;

fn select_agent(
    root: &str,
    args: &[String],
    restart: bool,
    path: &str,
) -> Result<(Selected, CodingAgent)> {
    let (_, requested) = rules::selection_args(args)?;
    let agent = rules::coding_agent_arg(args)?;
    let record = ownership::read(root)?;
    let canonical = |path: &str| posix::canonical(path).unwrap_or_else(|_| path.to_owned());
    rules::preflight_checks(
        record.as_ref(),
        &rules::PreflightOptions {
            root,
            requested,
            restart,
            platform: posix::platform_support(),
            canonical: &canonical,
        },
    )?;
    let agent = rules::provider(record.as_ref(), agent)?;
    let candidates = candidates(path);
    Ok((select::select_herdr(&candidates, &run_herdr)?, agent))
}

/// The `companion-preflight` role: stdout is exactly `herdr`, stderr carries
/// the selected Herdr, exit 0; failures are one `uml: ...` line and exit 1.
pub fn run(args: &[std::ffi::OsString]) -> std::process::ExitCode {
    let root = args
        .first()
        .map(|root| root.to_string_lossy().into_owned())
        .unwrap_or_default();
    let rest: Vec<String> = args[1..]
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    match launch(&root, &rest) {
        Ok((selected, agent)) => {
            println!("herdr");
            eprintln!(
                "uml: selected Herdr {} at {} for {}",
                selected.release,
                selected.path,
                agent.diagnostic_name()
            );
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("uml: {}", error.message);
            std::process::ExitCode::FAILURE
        }
    }
}

pub(crate) mod rules;

#[cfg(test)]
pub(crate) mod fixtures {
    pub(crate) fn script(directory: &std::path::Path, name: &str, body: &str) -> String {
        std::fs::create_dir_all(directory).unwrap();
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// A Herdr fixture: its release, bundled protocol and integration lines.
    pub(crate) fn herdr(directory: &std::path::Path, release: &str, protocol: u32, codex: &str) {
        script(
            directory,
            "herdr",
            &format!(
                "case \"$1\" in\n--version) echo 'herdr {release}' ;;\n\
                 api) echo '{{\"$schema\":\"fixture\",\"protocol\":{protocol},\"schema_version\":1}}' ;;\n\
                 integration) [ \"$2\" = status ] || exit 2\n  echo 'claude: current (v11) (/h/claude.sh)'\n  {codex}\n  ;;\nesac\n"
            ),
        );
    }

    pub(crate) const CODEX_CURRENT: &str = "echo 'codex: current (v10) (/h/codex.sh)'";

    pub(crate) const HOOKS_ENABLED: &str = "echo 'hooks    stable    true'";

    /// A Codex CLI fixture: its `--version` line and `login status` exit,
    /// with hooks enabled.
    pub(crate) fn codex(
        directory: &std::path::Path,
        name: &str,
        version: &str,
        login: u8,
    ) -> String {
        codex_hooks(directory, name, version, login, HOOKS_ENABLED)
    }

    /// [`codex`] whose `--enable hooks features list` runs `hooks`.
    pub(crate) fn codex_hooks(
        directory: &std::path::Path,
        name: &str,
        version: &str,
        login: u8,
        hooks: &str,
    ) -> String {
        script(
            directory,
            name,
            &format!(
                "case \"$1\" in\n--version) echo '{version}' ;;\n\
                 login) [ \"$2\" = status ] || exit 2; echo 'Not logged in' >&2; exit {login} ;;\n\
                 --enable) [ \"$2 $3 $4\" = 'hooks features list' ] || exit 2\n  \
                 echo 'apps    stable    true'\n  {hooks}\n  \
                 echo 'plugin_hooks    removed    false' ;;\n\
                 *) exit 9 ;;\nesac\n"
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{CODEX_CURRENT, codex as codex_fixture, herdr as herdr_fixture};
    use super::*;

    fn temp_root() -> String {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "uml preflight {} {}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join(".uml-viewer")).unwrap();
        directory.to_string_lossy().into_owned()
    }

    fn write_record(root: &str, text: &str) {
        std::fs::write(
            std::path::Path::new(root).join(".uml-viewer/companion.edn"),
            text,
        )
        .unwrap();
    }

    #[test]
    fn a_fresh_launch_selects_herdr_without_creating_ownership() {
        let root = temp_root();
        // PATH points at a fixture directory with a compatible herdr.
        let bin = std::path::Path::new(&root).join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let herdr = bin.join("herdr");
        std::fs::write(
            &herdr,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'herdr 0.9.4'; exit 0; fi\n"
                .to_owned()
                + "echo '{\"$schema\":\"fixture\",\"protocol\":22,\"schema_version\":1}'\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&herdr).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&herdr, permissions).unwrap();
        let path = format!("{}:/definitely/empty", bin.display());
        let selected = select_on_path(
            &root,
            &["--backend".into(), "herdr".into(), "d.edn".into()],
            false,
            &path,
        )
        .unwrap();
        assert!(selected.path.ends_with("/bin/herdr"));
        assert_eq!("0.9.4", selected.release);
        assert!(!ownership::exists(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn an_unknown_duplicate_or_missing_coding_agent_is_refused() {
        let root = temp_root();
        for values in [
            vec!["--coding-agent", "gemini"],
            vec!["--coding-agent", "Codex"],
            vec!["--coding-agent"],
            vec!["--coding-agent", "claude", "--coding-agent", "claude"],
            vec![
                "--coding-agent",
                "codex",
                "d.edn",
                "--coding-agent",
                "claude",
            ],
        ] {
            let error = select(&root, &args(&values), false).unwrap_err();
            assert!(
                error.message.contains("--coding-agent claude|codex"),
                "{values:?}: {}",
                error.message
            );
        }
        assert!(!ownership::exists(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn environment(
        bin: &std::path::Path,
        codex_bin: Option<String>,
    ) -> impl Fn(&str) -> Option<String> {
        let path = format!("{}:/definitely/empty", bin.display());
        move |name| match name {
            "PATH" => Some(path.clone()),
            "CODEX_BIN" => codex_bin.clone(),
            _ => None,
        }
    }

    #[test]
    fn a_current_codex_launch_and_a_claude_launch_on_herdr_093_pass_preflight() {
        let root = temp_root();
        let bin = std::path::Path::new(&root).join("bin");
        herdr_fixture(&bin, "0.9.5", 22, CODEX_CURRENT);
        codex_fixture(&bin, "codex", "codex-cli 0.166.0", 0);
        let (selected, agent) = launch_in(
            &root,
            &args(&["--coding-agent", "codex"]),
            &environment(&bin, None),
        )
        .unwrap();
        assert_eq!(
            ("0.9.5", CodingAgent::Codex),
            (selected.release.as_str(), agent)
        );
        herdr_fixture(
            &bin,
            "0.9.3",
            22,
            "echo 'codex: outdated (v9) (/h/codex.sh)'",
        );
        std::fs::remove_file(bin.join("codex")).unwrap();
        for values in [vec![], vec!["--coding-agent", "claude"]] {
            let (selected, agent) =
                launch_in(&root, &args(&values), &environment(&bin, None)).unwrap();
            assert_eq!(
                ("0.9.3", CodingAgent::Claude),
                (selected.release.as_str(), agent)
            );
        }
        assert!(!ownership::exists(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// name, Herdr (release, protocol, integration), Codex (version, login), cause.
    type Case = (
        &'static str,
        (&'static str, u32, &'static str),
        Option<(&'static str, u8)>,
        &'static str,
    );

    #[test]
    fn each_unmet_codex_requirement_is_refused_with_its_cause_before_any_resource() {
        let current = ("0.9.5", 22, CODEX_CURRENT);
        let cases: [Case; 9] = [
            (
                "missing",
                current,
                None,
                "Codex CLI not found: \"codex\" is not on PATH",
            ),
            (
                "old",
                current,
                Some(("codex-cli 0.165.9", 0)),
                "Codex CLI 0.165.9 at /",
            ),
            (
                "prerelease",
                current,
                Some(("codex-cli 0.167.0-alpha.1", 0)),
                "Codex CLI 0.167.0-alpha.1 at",
            ),
            (
                "garbled",
                current,
                Some(("Codex 1", 0)),
                "did not report a version",
            ),
            (
                "logged out",
                current,
                Some(("codex-cli 0.166.0", 1)),
                "no usable login (`login status` exited 1: \"Not logged in\")",
            ),
            (
                "herdr 0.9.4",
                ("0.9.4", 22, CODEX_CURRENT),
                Some(("codex-cli 0.166.0", 0)),
                "need Herdr 0.9.5 or newer (protocol 22); the selected Herdr 0.9.4",
            ),
            (
                "protocol 21",
                ("0.9.5", 21, CODEX_CURRENT),
                Some(("codex-cli 0.166.0", 0)),
                "bundled schema is not protocol 22",
            ),
            (
                "outdated",
                ("0.9.5", 22, "echo 'codex: outdated (v9) (/h/codex.sh)'"),
                Some(("codex-cli 0.166.0", 0)),
                "Herdr Codex integration is outdated, not current",
            ),
            (
                "absent",
                ("0.9.5", 22, ":"),
                Some(("codex-cli 0.166.0", 0)),
                "reported no Codex integration",
            ),
        ];
        for (name, (release, protocol, integration), codex, expected) in cases {
            let root = temp_root();
            let bin = std::path::Path::new(&root).join("bin");
            herdr_fixture(&bin, release, protocol, integration);
            if let Some((version, login)) = codex {
                codex_fixture(&bin, "codex", version, login);
            }
            let error = launch_in(
                &root,
                &args(&["--coding-agent", "codex"]),
                &environment(&bin, None),
            )
            .unwrap_err();
            assert!(
                error.message.contains(expected),
                "{name}: {}",
                error.message
            );
            assert!(!error.message.contains('\n'), "{name}: one line");
            assert!(!ownership::exists(&root), "{name}: no ownership");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// Herdr only learns the session through Codex's SessionStart hook, so
    /// hooks the companion's own invocation would run disabled (a managed
    /// requirement, say) are refused here, not as a 45 s identity timeout.
    #[test]
    fn codex_hooks_that_stay_disabled_for_the_companion_are_refused_before_any_resource() {
        for (hooks, expected) in [
            (
                "echo 'hooks    stable    false'",
                "Codex hooks stay disabled for the companion",
            ),
            (":", "did not report its hooks feature"),
            ("exit 3", "did not report its hooks feature"),
        ] {
            let root = temp_root();
            let bin = std::path::Path::new(&root).join("bin");
            herdr_fixture(&bin, "0.9.5", 22, CODEX_CURRENT);
            super::fixtures::codex_hooks(&bin, "codex", "codex-cli 0.166.0", 0, hooks);
            let error = launch_in(
                &root,
                &args(&["--coding-agent", "codex"]),
                &environment(&bin, None),
            )
            .unwrap_err();
            assert!(
                error.message.contains(expected),
                "{hooks}: {}",
                error.message
            );
            assert!(
                error.message.contains("--enable hooks features list"),
                "{}",
                error.message
            );
            assert!(!error.message.contains('\n'), "one line");
            assert!(!ownership::exists(&root), "no ownership");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn codex_bin_is_run_as_its_literal_path_and_overrides_path() {
        let root = temp_root();
        let bin = std::path::Path::new(&root).join("bin");
        herdr_fixture(&bin, "0.9.5", 22, CODEX_CURRENT);
        // A current codex on PATH must not stand in for the named one.
        codex_fixture(&bin, "codex", "codex-cli 0.166.0", 0);
        let odd = std::path::Path::new(&root).join("目录 $(x) ;'");
        let named = codex_fixture(&odd, "co dex*", "codex-cli 0.100.0", 0);
        let error = launch_in(
            &root,
            &args(&["--coding-agent", "codex"]),
            &environment(&bin, Some(named.clone())),
        )
        .unwrap_err();
        assert!(
            error.message.contains(&format!(
                "Codex CLI 0.100.0 at {named} is not a supported release (X.Y.Z >= 0.166.0)"
            )),
            "{}",
            error.message
        );
        let missing = format!("{named}-missing");
        let error = launch_in(
            &root,
            &args(&["--coding-agent", "codex"]),
            &environment(&bin, Some(missing.clone())),
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains(&format!("CODEX_BIN {missing:?} is not an executable"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #199: a Codex companion probing for the entry ran a bare `./uml` that
    /// resolved to another directory and started a second viewer with a
    /// Claude companion. Inside a companion (the runner names its project in
    /// `UML_COMPANION_ROOT`) no fresh start passes, and a restart passes
    /// only for that companion's own project.
    #[test]
    fn a_companion_may_restart_only_its_own_viewer_and_never_start_one() {
        let root = temp_root();
        let other = temp_root();
        let bin = std::path::Path::new(&root).join("bin");
        herdr_fixture(&bin, "0.9.5", 22, CODEX_CURRENT);
        let inside = |marker: &str| {
            let path = format!("{}:/definitely/empty", bin.display());
            let marker = marker.to_owned();
            move |name: &str| match name {
                "PATH" => Some(path.clone()),
                "UML_COMPANION_ROOT" => Some(marker.clone()),
                _ => None,
            }
        };
        for (project, values) in [
            (&root, vec![]),
            (&other, vec![]),
            (&other, vec!["--restart"]),
        ] {
            let error = launch_in(project, &args(&values), &inside(&root)).unwrap_err();
            assert!(
                error.message.contains("inside the companion"),
                "{}",
                error.message
            );
        }
        write_record(&root, &complete_record(&root, ""));
        assert!(launch_in(&root, &args(&["--restart"]), &inside(&root)).is_ok());
        assert!(launch_in(&root, &args(&["--restart"]), &inside(&format!("{root}/"))).is_ok());
        assert!(launch_in(&other, &args(&[]), &inside("")).is_ok());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn recorded_codex_ownership_runs_the_codex_preflight_without_an_option() {
        let root = temp_root();
        write_record(
            &root,
            &complete_record(
                &root,
                &format!(r#":coding-agent :codex :codex-session "{CODEX_SESSION}""#),
            ),
        );
        let bin = std::path::Path::new(&root).join("bin");
        herdr_fixture(&bin, "0.9.4", 22, CODEX_CURRENT);
        codex_fixture(&bin, "codex", "codex-cli 0.166.0", 0);
        for values in [
            vec!["--restart"],
            vec!["--restart", "--coding-agent", "codex"],
        ] {
            let error = launch_in(&root, &args(&values), &environment(&bin, None)).unwrap_err();
            assert!(
                error.message.contains("the selected Herdr 0.9.4"),
                "{}",
                error.message
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    const CODEX_SESSION: &str = "01a1a458-f60d-7561-aa06-d04df3bd14f7";

    fn complete_record(root: &str, provider: &str) -> String {
        format!(
            r#"{{:backend :herdr :owner-role :companion :cwd "{root}" :session "uml-viewer-A"
                :pane "w1:p1" :socket "/s/herdr.sock" :workspace "w1" :terminal-id "term_A"
                :shell-pid 42 :owner-start "ss" :server-pid 41 :server-start "rs" {provider}}}"#
        )
    }

    #[test]
    fn an_explicit_provider_that_conflicts_with_ownership_is_refused_before_selection() {
        let codex = format!(r#":coding-agent :codex :codex-session "{CODEX_SESSION}""#);
        for (recorded, requested, label) in [
            ("", "codex", "Claude Code"),
            (codex.as_str(), "claude", "Codex CLI"),
        ] {
            for restart in [vec!["--restart"], vec![]] {
                let root = temp_root();
                write_record(&root, &complete_record(&root, recorded));
                let before = ownership::bytes(&root);
                let mut values = restart.clone();
                values.extend(["--coding-agent", requested]);
                // An empty PATH would refuse selection: the conflict comes first.
                let error = select_on_path(&root, &args(&values), false, "").unwrap_err();
                assert!(
                    error.message.contains(&format!("records {label}"))
                        && error
                            .message
                            .contains(&format!("--coding-agent {requested}")),
                    "{}",
                    error.message
                );
                assert_eq!(before, ownership::bytes(&root), "bytes are retained");
                let _ = std::fs::remove_dir_all(&root);
            }
        }
    }

    #[test]
    fn legacy_unmarked_unknown_and_incomplete_ownership_is_refused_before_selection() {
        let patterns = [
            r#"{:backend :tmux :cwd "%s" :session "saved" :pane "%1"}"#,
            r#"{:cwd "%s" :session "saved" :pane "%1"}"#,
            r#"{:backend :unknown :cwd "%s" :session "saved" :pane "%1"}"#,
            r#"{:backend :herdr :owner-role :companion :cwd "%s" :session "saved" :pane "%1"}"#,
            r#"{:backend :herdr :owner-role :launcher :cwd "%s" :session "saved" :pane "%1"}"#,
            r#"{:backend :herdr :owner-role :companion :cwd "%s" :session "saved" :pane "%1" :provisional true}"#,
        ];
        for pattern in patterns {
            let root = temp_root();
            write_record(&root, &pattern.replace("%s", &root));
            let before = ownership::bytes(&root);
            let error = select(&root, &[], false).unwrap_err();
            assert!(!error.message.is_empty());
            assert_eq!(before, ownership::bytes(&root), "bytes are retained");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn corrupt_ownership_is_retained_and_a_missing_restart_is_refused() {
        let root = temp_root();
        write_record(&root, "{:backend :herdr");
        let error = select(&root, &[], false).unwrap_err();
        assert!(error.message.contains("corrupt or unreadable"));
        std::fs::remove_file(std::path::Path::new(&root).join(".uml-viewer/companion.edn"))
            .unwrap();
        let missing = select(&root, &["--restart".into()], true).unwrap_err();
        assert!(missing.message.contains("ownership is missing"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_incompatible_path_candidate_is_reported() {
        let root = temp_root();
        let bin = std::path::Path::new(&root).join("a");
        std::fs::create_dir_all(&bin).unwrap();
        let herdr = bin.join("herdr");
        std::fs::write(&herdr, "#!/bin/sh\necho 'herdr 0.9.2'\n").unwrap();
        let mut permissions = std::fs::metadata(&herdr).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&herdr, permissions).unwrap();
        let path = format!("{}:/definitely/empty", bin.display());
        let error = select_on_path(&root, &[], false, &path).unwrap_err();
        assert!(
            error.message.contains("Candidates on PATH"),
            "{}",
            error.message
        );
        assert!(
            error
                .message
                .contains("(unknown: not a herdr X.Y.Z >= 0.9.3)")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_platform_decision_is_the_delivered_wsl_linux_target() {
        // On the delivered host this is Ok; elsewhere the refusal reason is
        // the ADR 0007 text. Either way it must not be empty.
        if let Err(reason) = posix::platform_support() {
            assert!(
                reason.contains("supported platform")
                    || reason.contains("Unsupported companion platform")
            );
        }
    }

    #[test]
    fn failure_messages_are_single_lines_for_the_entry() {
        let error = crate::vocab::Failure::new("one line");
        assert!(!error.message.contains('\n'));
    }
}
