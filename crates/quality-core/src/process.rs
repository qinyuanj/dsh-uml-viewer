//! The quality tools' one process boundary: coverage and test commands,
//! with the terminal's streams, as argv (no shell) or as a host-shell
//! command string (`/bin/sh -c`, `cmd.exe /c`).
//! qinyuanj/dsh-uml-viewer#3 adds the Windows kill-on-close Job Object here.

#[cfg(windows)]
#[path = "process_windows.rs"]
mod process_windows;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
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

#[cfg(not(windows))]
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
    let status = execute(program, cwd, false)?.status;
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

/// Run with captured stdout and stderr, preserving their bytes.
pub fn output(program: &Program, cwd: &Path) -> io::Result<std::process::Output> {
    execute(program, cwd, true)
}

/// The captured process boundary: the same job/process-group containment as
/// [`output`], retained as a named alias for callers with captured children.
pub fn capture(program: &Program, cwd: &Path) -> io::Result<std::process::Output> {
    execute(program, cwd, true)
}

#[cfg(windows)]
fn execute(program: &Program, cwd: &Path, capture: bool) -> io::Result<std::process::Output> {
    process_windows::execute(program, cwd, capture)
}

#[cfg(unix)]
#[allow(clippy::disallowed_methods)] // the quality tools' one process boundary
fn execute(program: &Program, cwd: &Path, capture: bool) -> io::Result<std::process::Output> {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Stdio};
    struct Group(Child);
    impl Drop for Group {
        fn drop(&mut self) {
            // The group is created in the child before exec. Keep its identity
            // until wait returns; descendants can still hold captured pipes.
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.wait();
        }
    }
    fn reader<T: Read + Send + 'static>(
        pipe: Option<T>,
    ) -> Option<std::thread::JoinHandle<io::Result<Vec<u8>>>> {
        pipe.map(|mut pipe| {
            std::thread::spawn(move || {
                let mut out = Vec::new();
                pipe.read_to_end(&mut out)?;
                Ok(out)
            })
        })
    }
    fn finish(reader: Option<std::thread::JoinHandle<io::Result<Vec<u8>>>>) -> io::Result<Vec<u8>> {
        reader.map_or_else(
            || Ok(Vec::new()),
            |reader| {
                reader
                    .join()
                    .map_err(|_| io::Error::other("process output reader panicked"))?
            },
        )
    }
    let mut command = command(program, cwd)?;
    command.process_group(0);
    if capture {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }
    let mut group = Group(command.spawn()?);
    let stdout = reader(group.0.stdout.take());
    let stderr = reader(group.0.stderr.take());
    let status = group.0.wait();
    drop(group);
    let out = finish(stdout);
    let err = finish(stderr);
    Ok(std::process::Output {
        status: status?,
        stdout: out?,
        stderr: err?,
    })
}

#[cfg(not(any(windows, unix)))]
fn execute(program: &Program, cwd: &Path, capture: bool) -> io::Result<std::process::Output> {
    if capture {
        command(program, cwd)?.output()
    } else {
        command(program, cwd)?
            .status()
            .map(|status| std::process::Output {
                status,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
    }
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
