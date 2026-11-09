//! Every command, including coverage and baseline, gets a disposable overlay.
//! Mounting over the canonical root in the child namespace preserves absolute
//! paths and sibling Cargo dependencies without exposing writes to the parent.
#[cfg(target_os = "linux")]
use crate::interrupts;
#[cfg(target_os = "linux")]
use crate::scratch;
use crate::scratch::Root;
#[cfg(target_os = "linux")]
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    time::Instant,
};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

/// A command that did not produce a result.
pub enum Failure {
    /// A stop signal arrived; the caller's interrupt path cleans up.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Interrupted,
    /// This host cannot give the command its isolation: never run weaker.
    /// The original reports this with exit 1, before any test result counts.
    #[cfg_attr(not(windows), allow(dead_code))]
    Unavailable(String),
    /// The view could not be entered, or the command could not be launched.
    Message(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Interrupted => formatter.write_str("interrupted"),
            Failure::Unavailable(message) | Failure::Message(message) => {
                formatter.write_str(message)
            }
        }
    }
}

pub struct View {
    pub directory: PathBuf,
    // A slot borrows its pool's root, so only the pool removes the tree.
    _root: Option<Root>,
}
pub struct Completed {
    pub code: i32,
    pub elapsed: Duration,
    pub output: String,
}

/// A command's own overlay layers: a fresh view of the project every time.
#[cfg(target_os = "linux")]
fn reset_layers(directory: &Path) -> io::Result<()> {
    for name in ["upper", "work", "mutation", "output"] {
        scratch::force_remove(&directory.join(name))?;
    }
    for name in ["upper", "work"] {
        scratch::private_directory(&directory.join(name))?;
    }
    Ok(())
}

impl View {
    pub fn new(root: &Path) -> io::Result<Self> {
        let scratch = Root::new(root)?;
        let directory = scratch.path().to_path_buf();
        Ok(Self {
            directory,
            _root: Some(scratch),
        })
    }

    /// A view of a worker slot whose scratch root another owner removes.
    pub fn reused(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            _root: None,
        }
    }

    #[cfg(target_os = "linux")]
    // The Linux namespace view launcher is the quality tools' own adapter for
    // this one command kind: it owns PR_SET_PDEATHSIG, the per-command process
    // group and the file-redirected streams that the generic capture API in
    // quality_core::process cannot express.
    #[allow(clippy::disallowed_methods)]
    pub fn run(
        &self,
        root: &Path,
        cwd: &Path,
        command: &str,
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        self.run_argv(
            root,
            cwd,
            &["/bin/sh".into(), "-c".into(), command.into()],
            mutation,
            timeout,
        )
    }

    /// Run an argument vector as-is, without a shell: a spec file name with a
    /// space or a quote reaches the command as one argument, like the
    /// original's argv-list commands.
    #[cfg(target_os = "linux")]
    // The Linux namespace view launcher is the quality tools' own adapter for
    // this one command kind: it owns PR_SET_PDEATHSIG, the per-command process
    // group and the file-redirected streams that the generic capture API in
    // quality_core::process cannot express.
    #[allow(clippy::disallowed_methods)]
    pub fn run_argv(
        &self,
        root: &Path,
        cwd: &Path,
        argv: &[String],
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        use std::os::unix::process::CommandExt;
        if root
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(|c| b",:\\".contains(c))
        {
            return Err(Failure::Message(
                "overlayfs cannot represent this project path (comma, colon or backslash)".into(),
            ));
        }
        // A worker slot is reused, so its previous command's view is discarded
        // before this one: layer bytes never reach the next mutant.
        reset_layers(&self.directory).map_err(message)?;
        let mutation_path = self.directory.join("mutation");
        let destination = if let Some((path, source)) = mutation {
            scratch::private_file(&mutation_path)
                .map_err(message)?
                .write_all(source.as_bytes())
                .map_err(message)?;
            path.to_path_buf()
        } else {
            PathBuf::new()
        };
        // All substitutions are positional arguments, never shell source.
        const SCRIPT: &str = r#"set -eu
root=$1
layers=$2
cwd=$3
mutation=$4
destination=$5
shift 5
mount --make-rprivate /
mount -t overlay overlay -o "userxattr,lowerdir=$root,upperdir=$layers/upper,workdir=$layers/work" "$root"
if [ -n "$destination" ]; then cp -- "$mutation" "$destination"; fi
cd -- "$cwd"
export MUTATOR_VIEW="$layers"
# Keep build and test writes out of the caller's home and caches, but keep the
# installed toolchains reachable through their own homes.
toolchain_home=$HOME
CARGO_HOME=${CARGO_HOME:-$toolchain_home/.cargo}
RUSTUP_HOME=${RUSTUP_HOME:-$toolchain_home/.rustup}
export CARGO_HOME RUSTUP_HOME
home="$root/target/mutator-home"
mkdir -p "$home/tmp" "$home/.cache"
export HOME="$home" XDG_CACHE_HOME="$home/.cache" TMPDIR="$home/tmp"
export CARGO_TARGET_DIR="$root/target/mutator-build"
exec "$@"
"#;
        let output_path = self.directory.join("output");
        let output = scratch::private_file(&output_path).map_err(message)?;
        let start = Instant::now();
        let mut view = Command::new("unshare");
        view.args([
            "--user",
            "--map-root-user",
            "--mount",
            "--pid",
            "--fork",
            "--kill-child",
            "--",
            "/bin/sh",
            "-c",
            SCRIPT,
            "mutator-view",
        ])
        .arg(root)
        .arg(&self.directory)
        .arg(cwd)
        .arg(mutation_path)
        .arg(destination)
        .args(argv)
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(message)?)
        .stderr(output)
        .process_group(0);
        for name in crate::child_env::inherited_removals() {
            view.env_remove(name);
        }
        // A killed uml-viewer takes its unshare parent, the namespace's first
        // process and every remaining descendant with it.
        unsafe {
            view.pre_exec(|| {
                quality_core::libc::prctl(
                    quality_core::libc::PR_SET_PDEATHSIG,
                    quality_core::libc::SIGKILL,
                );
                Ok(())
            });
        }
        let mut child = view.spawn().map_err(message)?;
        let code = loop {
            if let Some(status) = child.try_wait().map_err(message)? {
                use std::os::unix::process::ExitStatusExt;
                break status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
            }
            if interrupts::pending().is_some() {
                // The command owns its process group; stop it now, so the
                // interrupt path can remove the scratch it can no longer use.
                kill_group(&child);
                let _ = child.wait();
                return Err(Failure::Interrupted);
            }
            if timeout.is_some_and(|limit| start.elapsed() >= limit) {
                // Each command owns a process group; reap descendants on timeout.
                kill_group(&child);
                child.wait().map_err(message)?;
                break 124;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // A successful shell may leave daemon descendants; the view owns them.
        kill_group(&child);
        Ok(Completed {
            code,
            elapsed: start.elapsed(),
            output: fs::read_to_string(output_path).unwrap_or_default(),
        })
    }
    #[cfg(windows)]
    pub fn run(
        &self,
        root: &Path,
        cwd: &Path,
        command: &str,
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        crate::copies::Slot::reused(&self.directory).run(root, cwd, command, mutation, timeout)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    pub fn run(
        &self,
        _root: &Path,
        _cwd: &Path,
        _command: &str,
        _mutation: Option<(&Path, &str)>,
        _timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        Err(Failure::Message(
            "mutation isolation is not supported on this platform".into(),
        ))
    }

    #[cfg(windows)]
    pub fn run_argv(
        &self,
        root: &Path,
        cwd: &Path,
        argv: &[String],
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        crate::copies::Slot::reused(&self.directory).run_argv(root, cwd, argv, mutation, timeout)
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    pub fn run_argv(
        &self,
        _root: &Path,
        _cwd: &Path,
        _argv: &[String],
        _mutation: Option<(&Path, &str)>,
        _timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        Err(Failure::Message(
            "mutation isolation is not supported on this platform".into(),
        ))
    }
}

/// The isolation level a snapshot records, as the original's `level`.
pub fn level() -> &'static str {
    if cfg!(windows) {
        "copy"
    } else {
        "namespace-view"
    }
}

/// The original's `Isolation: ...` line, naming the real mechanism.
#[cfg(windows)]
pub fn describe() -> &'static str {
    crate::copies::describe()
}

#[cfg(not(windows))]
pub fn describe() -> &'static str {
    "namespace views (a private overlayfs view per command)"
}

/// A report a command wrote inside its own view, never in the project.
pub fn report_path(view: &View, relative: &Path) -> PathBuf {
    #[cfg(windows)]
    let base = crate::copies::Slot::reused(&view.directory).project_root();
    #[cfg(not(windows))]
    let base = view.directory.join("upper");
    base.join(relative)
}

#[cfg(target_os = "linux")]
fn message(error: io::Error) -> Failure {
    Failure::Message(error.to_string())
}

#[cfg(target_os = "linux")]
fn kill_group(child: &std::process::Child) {
    unsafe {
        quality_core::libc::kill(-(child.id() as i32), quality_core::libc::SIGKILL);
    }
}
