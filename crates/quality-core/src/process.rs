//! The quality tools' one process boundary: coverage and test commands,
//! with the terminal's streams, as argv (no shell) or as a host-shell
//! command string (`/bin/sh -c`, `cmd.exe /c`).
//! qinyuanj/dsh-uml-viewer#3 adds the Windows kill-on-close Job Object here.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What to run.
#[derive(Clone, Debug)]
pub enum Program<'a> {
    Argv(&'a [String]),
    Shell(&'a str),
}

impl Program<'_> {
    /// The command as crapper echoes it: the string itself, or
    /// `shlex.join(argv)`.
    pub fn display(&self) -> String {
        match self {
            Program::Shell(text) => text.to_string(),
            Program::Argv(argv) => argv
                .iter()
                .map(|arg| shlex_quote(arg))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

/// `shlex.quote`.
pub fn shlex_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    let safe = arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if safe {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\"'\"'"))
    }
}

#[allow(clippy::disallowed_methods)] // the quality tools' one process boundary
fn command(program: &Program, cwd: &Path) -> io::Result<Command> {
    let mut command = match program {
        Program::Argv(argv) => {
            let (first, rest) = argv
                .split_first()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
            let mut command = Command::new(first);
            command.args(rest);
            command
        }
        Program::Shell(text) => shell(text),
    };
    command.current_dir(cwd);
    Ok(command)
}

#[cfg(windows)]
#[allow(clippy::disallowed_methods)]
fn shell(text: &str) -> Command {
    use std::os::windows::process::CommandExt;
    // subprocess with shell=True: `{COMSPEC} /c "{command}"`.
    let comspec = std::env::var_os("COMSPEC").unwrap_or_else(|| OsString::from("cmd.exe"));
    let mut command = Command::new(comspec);
    command.raw_arg(format!("/c \"{text}\""));
    command
}

#[cfg(not(windows))]
#[allow(clippy::disallowed_methods)]
fn shell(text: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(text);
    command
}

/// Run `program` in `cwd` with inherited streams; its exit status (a
/// signal `n` as `-n`). An error means it did not start.
pub fn run(program: &Program, cwd: &Path) -> io::Result<i32> {
    let status = command(program, cwd)?.status()?;
    if let Some(code) = status.code() {
        return Ok(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Ok(-signal);
        }
    }
    Ok(1)
}

/// `shutil.which`: the first `name` on PATH (with PATHEXT on Windows).
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let extensions: Vec<String> = if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut list: Vec<String> = pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect();
        let lower = name.to_lowercase();
        if list.iter().any(|ext| lower.ends_with(&ext.to_lowercase())) {
            list = vec![String::new()];
        }
        list
    } else {
        vec![String::new()]
    };
    for directory in std::env::split_paths(&path) {
        for extension in &extensions {
            let candidate = directory.join(format!("{name}{extension}"));
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
