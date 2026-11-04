//! The coding-agent 提供方 (provider) a companion runs: a closed set of
//! Claude Code and Codex CLI. This module is the single mapping point from a
//! provider and its session identity to the fresh and recovery argv and to
//! the ownership fields; Herdr stays the only companion backend.

use crate::host::Host;
use crate::prompt;
use crate::vocab::{Failure, Result};
use edn::Value;

/// The provider a companion record names and the session it runs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SessionIdentity {
    pub agent: CodingAgent,
    pub session: Option<String>,
}

/// Read the provider and its session from a companion record. A record
/// without `:coding-agent` predates providers and is Claude; it is only read,
/// never rewritten. Any other provider value is refused, and a committed
/// Codex record must name a UUID session in its own field.
impl SessionIdentity {
    /// The ownership fields this identity contributes: the provider (only
    /// when it is not Claude, so Claude records keep their pre-provider
    /// shape), the session in the provider's own field, and the explicit
    /// recovery command. Without a session there is nothing to resume.
    pub fn ownership_fields(&self, executable: &str, root: &str) -> Vec<(&'static str, Value)> {
        let mut fields = Vec::new();
        if self.agent != CodingAgent::Claude {
            fields.push(("coding-agent", Value::keyword(self.agent.name())));
        }
        if let Some(session) = &self.session {
            fields.push((
                "recovery-command",
                Value::Vector(
                    self.agent
                        .recovery_command(executable, root, session)
                        .into_iter()
                        .map(Value::String)
                        .collect(),
                ),
            ));
            fields.push((self.agent.session_key(), Value::String(session.clone())));
        }
        fields
    }
}

pub fn identity_of(record: &Value) -> Result<SessionIdentity> {
    let agent = match edn::record::get(record, "coding-agent") {
        None | Some(Value::Nil) => CodingAgent::Claude,
        Some(Value::Keyword(name)) => CodingAgent::parse(name)?,
        Some(_) => return Err(unknown_provider()),
    };
    let session = crate::record::get_name(record, agent.session_key());
    if agent.awaits_reported_session() {
        let provisional = edn::record::get(record, "provisional") == Some(&Value::Bool(true));
        match &session {
            Some(id) if is_uuid(id) => {}
            None if provisional => {}
            _ => {
                return Err(Failure::new(format!(
                    "{} companion ownership has no verifiable session; retained",
                    agent.label()
                )));
            }
        }
    }
    Ok(SessionIdentity { agent, session })
}

/// The argv a recovery relaunch may run. Claude keeps its recorded rule: a
/// `:recovery-command` vector wins, even an empty one. Codex only ever runs
/// its explicit resume of the saved session in the project; anything else
/// fails closed and never falls back on the fresh `:command`.
pub fn relaunch_command(record: &Value) -> Result<Vec<String>> {
    let owned = identity_of(record)?;
    let recorded = edn::record::get(record, "recovery-command");
    if !owned.agent.awaits_reported_session() {
        return Ok(crate::record::string_vec(match recorded {
            Some(Value::Vector(items)) => items,
            _ => edn::record::seq(edn::record::get(record, "command")),
        }));
    }
    let root = crate::record::get_name(record, "cwd");
    match recorded {
        Some(Value::Vector(items))
            if explicit_resume(items, owned.session.as_deref(), root.as_deref()) =>
        {
            Ok(crate::record::string_vec(items))
        }
        _ => Err(Failure::new(format!(
            "{} ownership has no valid recovery command for its saved session; recovery refused",
            owned.agent.label()
        ))),
    }
}

/// `[exe resume <options> <session> <prompt>]`, with the project as `-C` and
/// neither of the selectors that would let Codex pick a session itself.
fn explicit_resume(items: &[Value], session: Option<&str>, root: Option<&str>) -> bool {
    let argv = crate::record::string_vec(items);
    let (Some(session), Some(root)) = (session, root) else {
        return false;
    };
    let [executable, resume, options @ .., id, prompt] = &argv[..] else {
        return false;
    };
    argv.len() == items.len()
        && !executable.is_empty()
        && resume == "resume"
        && id == session
        && !prompt.is_empty()
        && options
            .windows(2)
            .any(|pair| pair[0] == "-C" && pair[1] == root)
        && !options
            .iter()
            .any(|option| option == "--last" || option == "--all")
}

fn unknown_provider() -> Failure {
    Failure::new("Companion ownership names an unknown coding-agent provider; retained")
}

/// RFC 4122 text form, any version: Codex reports UUIDv7 session ids.
pub(crate) fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
            group.len() == length && group.bytes().all(|b| b.is_ascii_hexdigit())
        })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CodingAgent {
    /// A project that names no provider runs Claude Code.
    #[default]
    Claude,
    Codex,
}

impl CodingAgent {
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "claude" => Ok(CodingAgent::Claude),
            "codex" => Ok(CodingAgent::Codex),
            _ => Err(unknown_provider()),
        }
    }

    /// The provider's name: the ownership keyword and the Herdr occupant.
    pub fn name(self) -> &'static str {
        match self {
            CodingAgent::Claude => "claude",
            CodingAgent::Codex => "codex",
        }
    }

    /// The product name used in status and error text.
    pub fn label(self) -> &'static str {
        match self {
            CodingAgent::Claude => "Claude Code",
            CodingAgent::Codex => "Codex CLI",
        }
    }

    /// The short name used in running status text.
    pub fn noun(self) -> &'static str {
        match self {
            CodingAgent::Claude => "Claude",
            CodingAgent::Codex => "Codex",
        }
    }

    /// The provider executable: its `*_BIN` override when set, otherwise the
    /// bare name resolved on PATH.
    pub fn executable(self, host: &Host) -> String {
        let (variable, name) = match self {
            CodingAgent::Claude => ("CLAUDE_BIN", "claude"),
            CodingAgent::Codex => ("CODEX_BIN", "codex"),
        };
        (host.variable)(variable)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| name.to_owned())
    }

    /// The ownership field that holds this provider's session id.
    pub fn session_key(self) -> &'static str {
        match self {
            CodingAgent::Claude => "claude-session",
            CodingAgent::Codex => "codex-session",
        }
    }

    /// Whether the session id is reported by Herdr after launch instead of
    /// being chosen up front. Codex's TUI cannot be handed a fresh id.
    pub fn awaits_reported_session(self) -> bool {
        self == CodingAgent::Codex
    }

    /// The shared business rules with this provider's recovery sentence.
    pub fn standing_rules(self) -> String {
        let recovery = match self {
            CodingAgent::Claude => prompt::CLAUDE_RECOVERY_RULE,
            CodingAgent::Codex => prompt::CODEX_RECOVERY_RULE,
        };
        [prompt::SHARED_RULES, recovery, prompt::RULES_TAIL].concat()
    }

    /// Fresh argv for a provider. Claude takes the session id chosen up
    /// front; Codex has none until Herdr reports it.
    pub fn fresh_command(self, executable: &str, root: &str, session: Option<&str>) -> Vec<String> {
        match self {
            CodingAgent::Claude => self.claude_command(
                executable,
                "--session-id",
                session.unwrap_or_default(),
                prompt::LAUNCH_PROMPT,
            ),
            CodingAgent::Codex => {
                let mut argv = vec![executable.to_owned()];
                argv.extend(self.codex_options(root));
                argv.push(prompt::LAUNCH_PROMPT.to_owned());
                argv
            }
        }
    }

    /// Recovery argv for a known session id: the only producer of a resume.
    /// Codex resumes that id explicitly, never the latest or a picked one.
    pub fn recovery_command(self, executable: &str, root: &str, session: &str) -> Vec<String> {
        match self {
            CodingAgent::Claude => {
                self.claude_command(executable, "--resume", session, prompt::RECOVERY_PROMPT)
            }
            CodingAgent::Codex => {
                let mut argv = vec![executable.to_owned(), "resume".to_owned()];
                argv.extend(self.codex_options(root));
                argv.push(session.to_owned());
                argv.push(prompt::RECOVERY_PROMPT.to_owned());
                argv
            }
        }
    }

    fn claude_command(
        self,
        executable: &str,
        flag: &str,
        session: &str,
        instruction: &str,
    ) -> Vec<String> {
        vec![
            executable.to_owned(),
            "--dangerously-skip-permissions".to_owned(),
            flag.to_owned(),
            session.to_owned(),
            "--append-system-prompt".to_owned(),
            self.standing_rules(),
            instruction.to_owned(),
        ]
    }

    /// Unattended edits and commands are granted by this process's own
    /// arguments; hooks (Herdr's session report) and no shared daemon do not
    /// rest on the person's global Codex config. Trusting the project root
    /// is an override for this invocation only: Codex saves nothing, and an
    /// untrusted git project would otherwise wait on its folder-trust screen.
    fn codex_options(self, root: &str) -> Vec<String> {
        vec![
            "--dangerously-bypass-approvals-and-sandbox".to_owned(),
            "--enable".to_owned(),
            "hooks".to_owned(),
            "--no-daemon".to_owned(),
            "-C".to_owned(),
            root.to_owned(),
            "-c".to_owned(),
            format!(
                "developer_instructions={}",
                toml_basic_string(&self.standing_rules())
            ),
            "-c".to_owned(),
            format!(
                "projects={{{}={{trust_level=\"trusted\"}}}}",
                toml_basic_string(root)
            ),
        ]
    }
}

/// A TOML basic string, so Codex parses the `-c` value as text instead of
/// falling back on its raw-string guess.
fn toml_basic_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a over the bytes: a stable fingerprint of the prompt text the
    /// Claude argv carried before the provider model existed.
    fn fingerprint(text: &str) -> (usize, u64) {
        let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
        (text.len(), hash)
    }

    const CLAUDE_RULES: (usize, u64) = (6344, 0x9836_a4a5_25f1_d37f);
    const LAUNCH: (usize, u64) = (182, 0x6944_8897_816f_41f1);
    const RECOVERY: (usize, u64) = (585, 0x88ed_aa98_bc8a_0f60);

    fn identity(text: &str) -> Result<SessionIdentity> {
        identity_of(&edn::parse(text).unwrap())
    }

    const UUID: &str = "01a1a458-f60d-7561-aa06-d04df3bd14f7";

    #[test]
    fn ownership_without_a_provider_is_claude_with_its_recorded_session() {
        assert_eq!(
            SessionIdentity {
                agent: CodingAgent::Claude,
                session: Some("u-1".to_owned())
            },
            identity(r#"{:backend :herdr :claude-session "u-1"}"#).unwrap()
        );
        assert_eq!(
            SessionIdentity {
                agent: CodingAgent::Claude,
                session: None
            },
            identity(r#"{:backend :herdr}"#).unwrap()
        );
        assert_eq!(
            CodingAgent::Claude,
            identity(r#"{:coding-agent :claude :claude-session "u-1"}"#)
                .unwrap()
                .agent
        );
    }

    #[test]
    fn a_codex_identity_reads_its_own_session_field() {
        assert_eq!(
            SessionIdentity {
                agent: CodingAgent::Codex,
                session: Some(UUID.to_owned())
            },
            identity(&format!(
                r#"{{:coding-agent :codex :codex-session "{UUID}" :claude-session "other"}}"#
            ))
            .unwrap()
        );
        // Before Herdr reports the session, only a provisional record has none.
        assert_eq!(
            None,
            identity(r#"{:coding-agent :codex :provisional true}"#)
                .unwrap()
                .session
        );
    }

    #[test]
    fn unknown_providers_and_unverifiable_codex_sessions_are_refused() {
        for text in [
            r#"{:coding-agent :pi}"#.to_owned(),
            r#"{:coding-agent "codex" :codex-session "x"}"#.to_owned(),
            r#"{:coding-agent :codex}"#.to_owned(),
            r#"{:coding-agent :codex :codex-session "not-a-uuid"}"#.to_owned(),
            format!(r#"{{:coding-agent :codex :claude-session "{UUID}"}}"#),
        ] {
            assert!(identity(&text).is_err(), "{text}");
        }
    }

    #[test]
    fn providers_share_the_business_rules_and_differ_only_in_the_recovery_sentence() {
        let claude = CodingAgent::Claude.standing_rules();
        let codex = CodingAgent::Codex.standing_rules();
        let sentence =
            "Recovery resumes this Claude session and drains queued mail without fresh scanning.\n";
        let (head, tail) = claude.split_once(sentence).unwrap();
        assert_eq!("Do not commit or push unless asked.\n", tail);
        assert!(codex.starts_with(head));
        assert!(codex.ends_with(tail));
        assert_eq!(
            "Recovery resumes this Codex session by its recorded id and drains queued mail without fresh scanning.\n",
            &codex[head.len()..codex.len() - tail.len()]
        );
    }

    #[test]
    fn codex_runs_unattended_in_the_project_with_explicit_instructions() {
        let fresh = CodingAgent::Codex.fresh_command("/opt/codex", "/p q", None);
        let options = [
            "--dangerously-bypass-approvals-and-sandbox",
            "--enable",
            "hooks",
            "--no-daemon",
            "-C",
            "/p q",
            "-c",
        ];
        assert_eq!("/opt/codex", fresh[0]);
        assert_eq!(options, fresh[1..8]);
        assert_eq!(12, fresh.len());
        let instructions = &fresh[8];
        assert!(
            instructions.starts_with(
                "developer_instructions=\"You are the UML-viewer companion. The current"
            )
        );
        assert!(instructions.ends_with("asked.\\n\""));
        assert!(!instructions.contains('\n'));
        assert_eq!(
            ["-c", r#"projects={"/p q"={trust_level="trusted"}}"#],
            fresh[9..11]
        );
        assert_eq!(LAUNCH, fingerprint(&fresh[11]));

        let recovery = CodingAgent::Codex.recovery_command("/opt/codex", "/p q", UUID);
        assert_eq!(["/opt/codex", "resume"], recovery[..2]);
        assert_eq!(options, recovery[2..9]);
        assert_eq!(fresh[8..11], recovery[9..12]);
        assert_eq!(UUID, recovery[12]);
        assert_eq!(RECOVERY, fingerprint(&recovery[13]));
        assert_eq!(14, recovery.len());
    }

    /// A first start in a git project otherwise stops on Codex's "Trust this
    /// folder?" screen and never reports its session (#199). The trust is
    /// this invocation's override for the project root only; nothing is saved.
    #[test]
    fn codex_trusts_only_the_project_root_for_its_own_invocation() {
        let trust = r#"projects={"/p \"q\".r"={trust_level="trusted"}}"#;
        let trusted = |argv: &[String]| argv.windows(2).any(|pair| pair == ["-c", trust]);
        assert!(trusted(&CodingAgent::Codex.fresh_command(
            "/opt/codex",
            "/p \"q\".r",
            None
        )));
        assert!(trusted(&CodingAgent::Codex.recovery_command(
            "/opt/codex",
            "/p \"q\".r",
            UUID
        )));
    }

    #[test]
    fn instructions_are_a_toml_basic_string() {
        assert_eq!(
            r#""a \"q\" \\ b\nc\t\u0001""#,
            toml_basic_string("a \"q\" \\ b\nc\t\u{1}")
        );
    }

    #[test]
    fn each_provider_prefers_its_own_bin_override() {
        let with = |expected: &'static str, value: Option<&'static str>| Host {
            variable: std::rc::Rc::new(move |name| {
                assert_eq!(expected, name);
                value.map(str::to_owned)
            }),
            ..Host::fake()
        };
        let claude = CodingAgent::Claude;
        assert_eq!(
            "/opt/claude",
            claude.executable(&with("CLAUDE_BIN", Some("/opt/claude")))
        );
        assert_eq!("claude", claude.executable(&with("CLAUDE_BIN", Some(""))));
        assert_eq!("claude", claude.executable(&with("CLAUDE_BIN", None)));
        let codex = CodingAgent::Codex;
        assert_eq!(
            "/opt/codex",
            codex.executable(&with("CODEX_BIN", Some("/opt/codex")))
        );
        assert_eq!("codex", codex.executable(&with("CODEX_BIN", None)));
    }

    #[test]
    fn a_reported_codex_session_yields_its_field_and_explicit_resume() {
        let fields = SessionIdentity {
            agent: CodingAgent::Codex,
            session: Some(UUID.to_owned()),
        }
        .ownership_fields("/opt/codex", "/p");
        let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            ["coding-agent", "recovery-command", "codex-session"],
            names[..]
        );
        let Value::Vector(resume) = &fields[1].1 else {
            panic!("recovery command is a vector")
        };
        assert_eq!(Value::String("resume".to_owned()), resume[1]);
        assert_eq!(Value::String(UUID.to_owned()), resume[resume.len() - 2]);
        assert!(!resume.contains(&Value::String("--last".to_owned())));
    }

    fn codex_record(recovery: &str) -> Value {
        edn::parse(&format!(
            r#"{{:coding-agent :codex :codex-session "{UUID}" :cwd "/p q"
                :command ["/opt/codex" "fresh"] {recovery}}}"#
        ))
        .unwrap()
    }

    fn edn_vector(argv: &[String]) -> String {
        edn::encode(&Value::Vector(
            argv.iter().cloned().map(Value::String).collect(),
        ))
        .unwrap()
    }

    #[test]
    fn a_codex_relaunch_runs_only_the_explicit_resume_of_its_saved_session() {
        let resume = CodingAgent::Codex.recovery_command("/opt/codex", "/p q", UUID);
        let record = codex_record(&format!(":recovery-command {}", edn_vector(&resume)));
        assert_eq!(resume, relaunch_command(&record).unwrap());
    }

    #[test]
    fn a_codex_relaunch_refuses_any_other_recovery_command_and_never_runs_fresh() {
        let good = CodingAgent::Codex.recovery_command("/opt/codex", "/p q", UUID);
        let edited = |edit: &dyn Fn(&mut Vec<String>)| {
            let mut argv = good.clone();
            edit(&mut argv);
            format!(":recovery-command {}", edn_vector(&argv))
        };
        let other = "01a1a458-f60d-7561-aa06-d04df3bd14f8";
        for recovery in [
            String::new(),
            ":recovery-command nil".to_owned(),
            ":recovery-command []".to_owned(),
            ":recovery-command \"/opt/codex resume\"".to_owned(),
            ":recovery-command [\"/opt/codex\" 1]".to_owned(),
            edited(&|argv| argv[0].clear()),
            edited(&|argv| argv[1] = "fork".to_owned()),
            edited(&|argv| argv[12] = other.to_owned()),
            edited(&|argv| argv[12] = "--last".to_owned()),
            edited(&|argv| argv.insert(2, "--last".to_owned())),
            edited(&|argv| argv.insert(2, "--all".to_owned())),
            edited(&|argv| argv[7] = "/elsewhere".to_owned()),
            edited(&|argv| {
                argv.remove(7);
                argv.remove(6);
            }),
            edited(&|argv| argv[13].clear()),
            // A picker: `codex resume` with the id dropped.
            edited(&|argv| {
                argv.remove(12);
            }),
        ] {
            let error = relaunch_command(&codex_record(&recovery))
                .expect_err(&format!("accepted {recovery}"));
            assert!(error.message.contains("Codex CLI"), "{}", error.message);
        }
    }

    #[test]
    fn a_claude_relaunch_keeps_its_recorded_command_rule() {
        let relaunch = |text: &str| relaunch_command(&edn::parse(text).unwrap()).unwrap();
        assert_eq!(
            ["claude", "--resume", "u-1"],
            relaunch(
                r#"{:command ["claude" "fresh"] :recovery-command ["claude" "--resume" "u-1"]}"#
            )[..]
        );
        assert_eq!(
            ["claude", "fresh"],
            relaunch(r#"{:command ["claude" "fresh"]}"#)[..]
        );
        assert!(relaunch(r#"{:command ["claude" "fresh"] :recovery-command []}"#).is_empty());
    }

    #[test]
    fn claude_fresh_and_recovery_argv_are_unchanged() {
        let fresh = CodingAgent::Claude.fresh_command("/opt/claude", "/p", Some("u-1"));
        assert_eq!(
            [
                "/opt/claude",
                "--dangerously-skip-permissions",
                "--session-id",
                "u-1",
                "--append-system-prompt"
            ],
            fresh[..5]
        );
        assert_eq!(7, fresh.len());
        assert_eq!(CLAUDE_RULES, fingerprint(&fresh[5]));
        assert_eq!(LAUNCH, fingerprint(&fresh[6]));
        let recovery = CodingAgent::Claude.recovery_command("/opt/claude", "/p", "u-1");
        assert_eq!(
            [
                "/opt/claude",
                "--dangerously-skip-permissions",
                "--resume",
                "u-1",
                "--append-system-prompt"
            ],
            recovery[..5]
        );
        assert_eq!(7, recovery.len());
        assert_eq!(CLAUDE_RULES, fingerprint(&recovery[5]));
        assert_eq!(RECOVERY, fingerprint(&recovery[6]));
    }
}
