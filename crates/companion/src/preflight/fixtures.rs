//! Fake executables for the companion tests: Herdr, the Codex CLI and plain
//! children, each a declared table of replies. A reply answers the first
//! rule whose words open the argv; no matching rule exits 0 silently.
//!
//! POSIX renders the table as a `/bin/sh` script. Windows cannot run a
//! script as a native executable (and the companion refuses `.cmd`
//! wrappers), so there each fake is a copy of one small compiled program
//! that reads its table from `<name>.exe.fake` beside it.

use std::path::Path;

/// One reply: when the argv opens with `when`, wait `sleep_ms`, optionally
/// write the `report` variable and the working directory to the file named
/// by the first argument, print `out` and `err` (one line each) and exit.
#[derive(Clone, Default)]
pub(crate) struct Rule {
    when: Vec<String>,
    out: Vec<String>,
    err: Vec<String>,
    exit: i32,
    sleep_ms: u64,
    report: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct Fake {
    rules: Vec<Rule>,
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

impl Fake {
    pub(crate) fn new() -> Fake {
        Fake::default()
    }

    /// Reply to an argv opening with `when`.
    pub(crate) fn on(mut self, when: &[&str], out: &[&str], err: &[&str], exit: i32) -> Fake {
        self.rules.push(Rule {
            when: strings(when),
            out: strings(out),
            err: strings(err),
            exit,
            ..Rule::default()
        });
        self
    }

    /// Any argv: sleep, then exit.
    pub(crate) fn sleeping(mut self, sleep_ms: u64, exit: i32) -> Fake {
        self.rules.push(Rule {
            exit,
            sleep_ms,
            ..Rule::default()
        });
        self
    }

    /// Any argv: write `variable` and the working directory to the file
    /// named by the first argument, then exit 0.
    pub(crate) fn reporting(mut self, sleep_ms: u64, variable: &str) -> Fake {
        self.rules.push(Rule {
            sleep_ms,
            report: Some(variable.to_owned()),
            ..Rule::default()
        });
        self
    }

    /// Write the fake as `name` (`name.exe` on Windows) in `directory` and
    /// return the path to run.
    pub(crate) fn write(&self, directory: &Path, name: &str) -> String {
        std::fs::create_dir_all(directory).unwrap();
        platform::write(self, directory, name)
    }
}

/// A PATH holding `directory` and one directory that does not exist.
pub(crate) fn path_with(directory: &Path) -> String {
    let missing = if cfg!(windows) {
        "C:\\definitely\\empty"
    } else {
        "/definitely/empty"
    };
    crate::search::Search::native().path_list(&[&directory.to_string_lossy(), missing])
}

#[cfg(unix)]
mod platform {
    use super::Fake;
    use crate::text::shell_word;
    use std::path::Path;

    fn lines(words: &[String]) -> String {
        words
            .iter()
            .map(|word| shell_word(word))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn script(fake: &Fake) -> String {
        let mut body = String::from("#!/bin/sh\n");
        for rule in &fake.rules {
            let mut test = format!("[ \"$#\" -ge {} ]", rule.when.len());
            for (index, word) in rule.when.iter().enumerate() {
                test.push_str(&format!(
                    " && [ \"${}\" = {} ]",
                    index + 1,
                    shell_word(word)
                ));
            }
            body.push_str(&format!("if {test}; then\n"));
            if rule.sleep_ms > 0 {
                body.push_str(&format!("sleep {}\n", rule.sleep_ms as f64 / 1000.0));
            }
            if let Some(variable) = &rule.report {
                body.push_str(&format!(
                    "printf '%s\\n%s' \"${variable}\" \"$(pwd -P)\" > \"$1\"\n"
                ));
            }
            if !rule.out.is_empty() {
                body.push_str(&format!("printf '%s\\n' {}\n", lines(&rule.out)));
            }
            if !rule.err.is_empty() {
                body.push_str(&format!("printf '%s\\n' {} >&2\n", lines(&rule.err)));
            }
            body.push_str(&format!("exit {}\nfi\n", rule.exit));
        }
        body.push_str("exit 0\n");
        body
    }

    /// A child process writes the script: a write descriptor in this
    /// process would leak into a child forked by a concurrent test, and exec
    /// of a file open for writing fails ETXTBSY.
    pub(super) fn write(fake: &Fake, directory: &Path, name: &str) -> String {
        let path = directory.join(name);
        let status = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"",
                "sh",
            ])
            .arg(script(fake))
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success(), "fixture script {}", path.display());
        path.to_string_lossy().into_owned()
    }
}

#[cfg(windows)]
mod platform {
    use super::Fake;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    /// The fake program. It reads `<its own path>.fake`: one rule per line,
    /// fields split by U+001F, list items by U+001E.
    const SOURCE: &str = r#"
use std::io::Write;
fn list(field: &str) -> Vec<String> {
    if field.is_empty() { Vec::new() } else { field.split('\u{1e}').map(String::from).collect() }
}
fn main() {
    let exe = std::env::current_exe().expect("exe");
    let spec = std::fs::read_to_string(format!("{}.fake", exe.display())).expect("spec");
    let args: Vec<String> = std::env::args().skip(1).collect();
    for line in spec.split('\n').filter(|line| !line.is_empty()) {
        let f: Vec<&str> = line.split('\u{1f}').collect();
        let when = list(f[0]);
        if args.len() < when.len() || args[..when.len()] != when[..] { continue; }
        std::thread::sleep(std::time::Duration::from_millis(f[4].parse().unwrap()));
        if !f[5].is_empty() {
            let value = std::env::var(f[5]).unwrap_or_default();
            let cwd = std::env::current_dir().unwrap();
            std::fs::write(&args[0], format!("{}\n{}", value, cwd.display())).unwrap();
        }
        let mut out = std::io::stdout();
        for text in list(f[1]) { writeln!(out, "{text}").unwrap(); }
        out.flush().unwrap();
        for text in list(f[2]) { eprintln!("{text}"); }
        std::process::exit(f[3].parse().unwrap());
    }
}
"#;

    /// The compiled program, built once per source and shared by every test
    /// process through the temp directory.
    fn program() -> &'static PathBuf {
        static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
        PROGRAM.get_or_init(|| {
            let digest = SOURCE
                .bytes()
                .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
                });
            let directory = std::env::temp_dir().join("uml-companion-fakes");
            let program = directory.join(format!("fake-{digest:016x}.exe"));
            if program.is_file() {
                return program;
            }
            let work = directory.join(format!("build-{}", std::process::id()));
            std::fs::create_dir_all(&work).unwrap();
            let source = work.join("fake.rs");
            std::fs::write(&source, SOURCE).unwrap();
            let built = work.join("fake.exe");
            let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
            let status = std::process::Command::new(rustc)
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .args(["--edition", "2021", "-C", "debuginfo=0", "-o"])
                .arg(&built)
                .arg(&source)
                .status()
                .expect("rustc builds the fake program");
            assert!(status.success(), "rustc failed on the fake program");
            // Another test process may have won the race; either copy works.
            let _ = std::fs::rename(&built, &program);
            let _ = std::fs::remove_dir_all(&work);
            assert!(program.is_file(), "fake program {}", program.display());
            program
        })
    }

    fn spec(fake: &Fake) -> String {
        let list = |words: &[String]| words.join("\u{1e}");
        fake.rules
            .iter()
            .map(|rule| {
                [
                    list(&rule.when),
                    list(&rule.out),
                    list(&rule.err),
                    rule.exit.to_string(),
                    rule.sleep_ms.to_string(),
                    rule.report.clone().unwrap_or_default(),
                ]
                .join("\u{1f}")
                    + "\n"
            })
            .collect()
    }

    pub(super) fn write(fake: &Fake, directory: &Path, name: &str) -> String {
        let name = if name.to_ascii_lowercase().ends_with(".exe") {
            name.to_owned()
        } else {
            format!("{name}.exe")
        };
        let path = directory.join(&name);
        std::fs::write(directory.join(format!("{name}.fake")), spec(fake)).unwrap();
        std::fs::copy(program(), &path).unwrap();
        path.to_string_lossy().into_owned()
    }
}

/// A Herdr fixture: its release, bundled protocol and `integration status`
/// lines (the Claude line, then `codex` unless it is empty).
pub(crate) fn herdr(directory: &Path, release: &str, protocol: u32, codex: &str) -> String {
    let version = format!("herdr {release}");
    let schema =
        format!("{{\"$schema\":\"fixture\",\"protocol\":{protocol},\"schema_version\":1}}");
    let mut integration = vec!["claude: current (v11) (/h/claude.sh)"];
    if !codex.is_empty() {
        integration.push(codex);
    }
    Fake::new()
        .on(&["--version"], &[&version], &[], 0)
        .on(&["api"], &[&schema], &[], 0)
        .on(&["integration", "status"], &integration, &[], 0)
        .on(&["integration"], &[], &[], 2)
        .write(directory, "herdr")
}

pub(crate) const CODEX_CURRENT: &str = "codex: current (v10) (/h/codex.sh)";

/// What `codex --enable hooks features list` reports for hooks.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Hooks {
    Line(&'static str),
    Silent,
    Exit(i32),
}

pub(crate) const HOOKS_ENABLED: Hooks = Hooks::Line("hooks    stable    true");

/// A Codex CLI fixture: its `--version` line and `login status` exit, with
/// hooks enabled.
pub(crate) fn codex(directory: &Path, name: &str, version: &str, login: i32) -> String {
    codex_hooks(directory, name, version, login, HOOKS_ENABLED)
}

/// [`codex`] whose `--enable hooks features list` reports `hooks`.
pub(crate) fn codex_hooks(
    directory: &Path,
    name: &str,
    version: &str,
    login: i32,
    hooks: Hooks,
) -> String {
    let apps = "apps    stable    true";
    let plugin = "plugin_hooks    removed    false";
    let (features, exit) = match hooks {
        Hooks::Line(line) => (vec![apps, line, plugin], 0),
        Hooks::Silent => (vec![apps, plugin], 0),
        Hooks::Exit(code) => (vec![apps], code),
    };
    Fake::new()
        .on(&["--version"], &[version], &[], 0)
        .on(&["login", "status"], &[], &["Not logged in"], login)
        .on(&["login"], &[], &[], 2)
        .on(
            &["--enable", "hooks", "features", "list"],
            &features,
            &[],
            exit,
        )
        .on(&["--enable"], &[], &[], 2)
        .on(&[], &[], &[], 9)
        .write(directory, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(program: &str, args: &[&str]) -> crate::vocab::RunOut {
        let mut argv = vec![std::ffi::OsString::from(program)];
        argv.extend(args.iter().map(std::ffi::OsString::from));
        crate::posix::run(&argv, Some(30_000), &[])
    }

    #[test]
    fn a_fake_answers_the_first_rule_its_argv_opens_with() {
        let directory = std::env::temp_dir().join(format!("uml fake {}", std::process::id()));
        let fake = Fake::new()
            .on(&["a", "b"], &["ab 中文", "two"], &["err"], 3)
            .on(&["a"], &["just a"], &[], 0)
            .write(&directory, "tool");
        let both = run(&fake, &["a", "b", "c"]);
        assert_eq!(
            (Some(3), "ab 中文\ntwo", "err"),
            (both.exit, both.out.as_str(), both.err.as_str())
        );
        assert_eq!("just a", run(&fake, &["a", "x"]).out);
        let other = run(&fake, &["z"]);
        assert_eq!((Some(0), ""), (other.exit, other.out.as_str()));
        let _ = std::fs::remove_dir_all(&directory);
    }
}
