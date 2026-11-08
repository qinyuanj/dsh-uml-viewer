//! `uml-viewer quality <crap|mutate|dry>` (qinyuanj/dsh-uml-viewer#15).
//!
//! ADR 0001 with its 2026-11-08 appendix: the dispatcher reads
//! `uml-quality.json`, runs the `standardTools` route (the built-in `crap`,
//! `mutate` and `dry` subcommands) or an `operations` override, and prints one
//! version-1 result record as a single JSON line on **stdout**. The contract is
//! documented in docs/quality-dispatch.md; the record is `ToolResultRecord` of
//! packages/contracts plus `reason`. Nothing writes
//! `.uml-viewer/quality-result.json`: the record is the invocation's return
//! value, so it cannot be stale. The tool's own report goes to stderr.

mod config;
mod redirect;
mod standard;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use config::{Config, Operation};

const USAGE: &str = "usage: uml-viewer quality <crap|mutate|dry> [--root <path>] [args]

Run the project's configured quality tool (uml-quality.json) and print the
version-1 result record as one JSON line on stdout; the tool's own report
goes to stderr. --root must directly follow the operation; every later
argument goes to the tool verbatim (mutate: target files or --changed, all
in one call).

  --root <path>   Project root holding uml-quality.json (default: .).

Exit status: the tool's real exit code (128+n after signal n); 1 for a
configuration, launch or unported-language refusal (an error record with
validResult false); 2 for a usage error (no record).
";

/// A usage error: no record, exit 2.
const USAGE_EXIT: u8 = 2;

pub fn run(args: &[OsString]) -> ExitCode {
    let args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    if args
        .iter()
        .take(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let (operation, root, forward) = match parse(&args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprint!("uml-viewer quality: {message}\n\n{USAGE}");
            return ExitCode::from(USAGE_EXIT);
        }
    };
    let root = quality_core::paths::resolve(&root);
    let record = match Config::read(&root)
        .map_err(|message| Refusal::config(operation, message))
        .and_then(|config| dispatch(&root, operation, &config, &forward))
    {
        Ok(outcome) => outcome.record(&root, operation, None),
        Err(refusal) => {
            eprintln!("uml-viewer quality: {}", refusal.message);
            let outcome = Outcome {
                exit_code: refusal.exit_code,
                valid_result: false,
                crap_scope: refusal.crap_scope,
                configured: refusal.configured,
                tool: refusal.tool,
            };
            outcome.record(&root, operation, Some(refusal.message))
        }
    };
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{}", record.json());
    let _ = stdout.flush();
    ExitCode::from(record.exit_code)
}

fn parse(args: &[String]) -> Result<(Operation, PathBuf, Vec<String>), String> {
    let Some(first) = args.first() else {
        return Err("missing operation".to_string());
    };
    let operation = Operation::parse(first).ok_or_else(|| format!("unknown operation {first}"))?;
    // Only the dispatcher's own leading --root selects the project. Anything
    // after the first forwarded argument belongs to the tool verbatim (an
    // operations command may itself take --root).
    let (root, forwarded_at) = if args.get(1).is_some_and(|arg| arg == "--root") {
        let value = args
            .get(2)
            .filter(|value| !value.is_empty())
            .ok_or("--root requires a value")?;
        (PathBuf::from(value), 3)
    } else {
        (PathBuf::from("."), 1)
    };
    Ok((operation, root, args[forwarded_at..].to_vec()))
}

/// A refusal before any tool ran: an explicit error record, never an empty
/// success.
pub(crate) struct Refusal {
    pub message: String,
    pub exit_code: u8,
    pub crap_scope: Option<&'static str>,
    pub configured: bool,
    pub tool: String,
}

impl Refusal {
    fn config(operation: Operation, message: String) -> Refusal {
        Refusal {
            message,
            exit_code: 1,
            crap_scope: None,
            configured: false,
            tool: operation.tool().0.to_string(),
        }
    }
}

/// What a completed run produced.
pub(crate) struct Outcome {
    pub exit_code: u8,
    pub valid_result: bool,
    pub crap_scope: Option<&'static str>,
    pub configured: bool,
    pub tool: String,
}

impl Outcome {
    fn record(self, root: &Path, operation: Operation, reason: Option<String>) -> Record {
        Record {
            operation: operation.name(),
            project_root: root.to_string_lossy().into_owned(),
            exit_code: self.exit_code,
            valid_result: self.valid_result,
            crap_scope: self.crap_scope,
            configured: self.configured,
            tool: self.tool,
            reason,
        }
    }
}

/// The version-1 result record (`ToolResultRecord` in packages/contracts).
struct Record {
    operation: &'static str,
    project_root: String,
    exit_code: u8,
    valid_result: bool,
    crap_scope: Option<&'static str>,
    configured: bool,
    tool: String,
    /// Why no tool ran; null after a completed run.
    reason: Option<String>,
}

impl Record {
    fn json(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert("version".into(), 1.into());
        map.insert("operation".into(), self.operation.into());
        map.insert("projectRoot".into(), self.project_root.clone().into());
        map.insert("exitCode".into(), self.exit_code.into());
        map.insert("validResult".into(), self.valid_result.into());
        map.insert("crapScope".into(), self.crap_scope.into());
        map.insert("configured".into(), self.configured.into());
        map.insert("tool".into(), self.tool.clone().into());
        map.insert("reason".into(), self.reason.clone().into());
        serde_json::Value::Object(map).to_string()
    }
}

fn dispatch(
    root: &Path,
    operation: Operation,
    config: &Config,
    forward: &[String],
) -> Result<Outcome, Refusal> {
    match config.override_for(operation) {
        Some(declared) => override_command(root, declared, forward),
        None => standard::run(root, operation, config.standard(), forward),
    }
}

/// A declared operation runs its own argv at the project root, no shell:
/// fixed argv, then the caller's arguments verbatim when `forwardArgs` is
/// `append`. Its stdout is folded into stderr so the record stays alone.
fn override_command(
    root: &Path,
    declared: &config::Override,
    forward: &[String],
) -> Result<Outcome, Refusal> {
    let mut argv = declared.argv.clone();
    if declared.forward_args {
        argv.extend(forward.iter().cloned());
    }
    let refuse = |message: String| Refusal {
        message,
        exit_code: 1,
        crap_scope: declared.crap_scope,
        configured: true,
        tool: "configured".to_string(),
    };
    if let Some(message) = script_wrapper(&argv[0]) {
        return Err(refuse(message));
    }
    let program = quality_core::process::Program::Argv(&argv);
    let status = redirect::stdout_to_stderr(|| quality_core::process::run(&program, root))
        .map_err(|error| refuse(format!("cannot launch {}: {error}", argv[0])))?;
    let exit_code = native_exit(status);
    Ok(Outcome {
        exit_code,
        valid_result: declared.valid_exit_codes.contains(&exit_code),
        crap_scope: declared.crap_scope,
        configured: true,
        tool: "configured".to_string(),
    })
}

/// A child's exit as an entry status, as `uml_posix.native_exit` reports it.
/// `raw` is `quality_core::process::run`'s status: on POSIX a signal `n` is
/// `-n` and becomes `128+n`; on Windows it is the 32-bit exit code, and an
/// NTSTATUS above 255 becomes `128+signal` for the known ones, else 255,
/// named on stderr.
fn native_exit(raw: i32) -> u8 {
    if cfg!(unix) {
        return if raw < 0 {
            (128 - raw).clamp(0, 255) as u8
        } else {
            raw.clamp(0, 255) as u8
        };
    }
    let code = raw as u32;
    if code <= 255 {
        return code as u8;
    }
    let signal = match code {
        0xC000_013A => Some(2),
        0xC000_001D => Some(4),
        0xC000_0094 => Some(8),
        0xC000_0005 | 0xC000_00FD => Some(11),
        0xC000_0409 => Some(6),
        _ => None,
    };
    let mapped = signal.map_or(255, |n| 128 + n);
    eprintln!("uml: child ended with NTSTATUS 0x{code:08X}; reported as exit {mapped}.");
    mapped
}

/// Script and shim entries are refused: a shell boundary cannot preserve
/// arbitrary argv (ADR 0001). On POSIX `.exe` is not native either.
fn script_wrapper(program: &str) -> Option<String> {
    let lower = program.to_lowercase();
    let wrappers: &[&str] = if cfg!(windows) {
        &[".ps1", ".bat", ".cmd"]
    } else {
        &[".ps1", ".exe", ".bat", ".cmd"]
    };
    wrappers
        .iter()
        .any(|suffix| lower.ends_with(suffix))
        .then(|| {
            if cfg!(windows) {
                "a Windows quality command needs a native .exe entry; .ps1/.bat/.cmd are refused"
                    .to_string()
            } else {
                "a POSIX quality command needs a native executable entry".to_string()
            }
        })
}
