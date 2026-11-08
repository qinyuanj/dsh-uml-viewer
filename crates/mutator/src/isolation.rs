//! Every command, including coverage and baseline, gets a disposable overlay.
//! Mounting over the canonical root in the child namespace preserves absolute
//! paths and sibling Cargo dependencies without exposing writes to the parent.
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
#[cfg(target_os = "linux")]
use std::{
    io::Write,
    process::{Command, Stdio},
    time::Instant,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
pub struct View {
    pub directory: PathBuf,
}
pub struct Result {
    pub code: i32,
    pub elapsed: Duration,
    pub output: String,
}

#[cfg(unix)]
fn private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path)
}
#[cfg(not(unix))]
fn private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}
/// A private 0600 file. A view is reused by a sharded collection, so a
/// previous command's output or mutation file is removed before recreating it.
#[cfg(target_os = "linux")]
fn private_file(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// An unpredictable suffix: a per-process random seed plus the clock.
fn nonce() -> u64 {
    use std::{
        collections::hash_map::RandomState,
        hash::{BuildHasher, Hasher},
    };
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default(),
    );
    hasher.finish()
}

/// Remove a tree without following links, restoring the permissions overlayfs
/// leaves at zero, and reporting the first failure instead of ignoring it.
fn force_remove(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        result => result?,
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        };
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Overlayfs leaves its work directory without any permissions.
        if metadata.permissions().mode() & 0o700 != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    let mut failure = None;
    for entry in fs::read_dir(path)? {
        match entry {
            Ok(entry) => {
                if let Err(error) = force_remove(&entry.path()) {
                    failure = failure.or(Some(error));
                }
            }
            Err(error) => failure = failure.or(Some(error)),
        }
    }
    if let Err(error) = fs::remove_dir(path) {
        failure = failure.or(Some(error));
    }
    failure.map_or(Ok(()), Err)
}

impl View {
    pub fn new(root: &Path) -> io::Result<Self> {
        let temporary = quality_core::paths::resolve(&std::env::temp_dir());
        if temporary.starts_with(quality_core::paths::resolve(root)) {
            return Err(io::Error::other(
                "the temporary directory is inside the project; point TMPDIR outside it",
            ));
        }
        for _ in 0..16 {
            let directory = temporary.join(format!(
                "mutator-{}-{}-{:016x}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                nonce()
            ));
            match private_directory(&directory) {
                Ok(()) => {
                    for name in ["upper", "work"] {
                        private_directory(&directory.join(name))?;
                    }
                    return Ok(Self { directory });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(
            "cannot create a private mutation scratch directory",
        ))
    }

    #[cfg(target_os = "linux")]
    pub fn run(
        &self,
        root: &Path,
        cwd: &Path,
        command: &str,
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> io::Result<Result> {
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
    pub fn run_argv(
        &self,
        root: &Path,
        cwd: &Path,
        argv: &[String],
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> io::Result<Result> {
        use std::os::unix::process::CommandExt;
        if root
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(|c| b",:\\".contains(c))
        {
            return Err(io::Error::other(
                "overlayfs cannot represent this project path (comma, colon or backslash)",
            ));
        }
        let mutation_path = self.directory.join("mutation");
        let destination = if let Some((path, source)) = mutation {
            private_file(&mutation_path)?.write_all(source.as_bytes())?;
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
unset CARGO_BUILD_TARGET_DIR CARGO_ENCODED_RUSTFLAGS LLVM_PROFILE_FILE
exec "$@"
"#;
        let output_path = self.directory.join("output");
        let output = private_file(&output_path)?;
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
        .stdout(output.try_clone()?)
        .stderr(output)
        .process_group(0);
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
        let mut child = view.spawn()?;
        let code = loop {
            if let Some(status) = child.try_wait()? {
                use std::os::unix::process::ExitStatusExt;
                break status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
            }
            if timeout.is_some_and(|limit| start.elapsed() >= limit) {
                // Each command owns a process group; reap descendants on timeout.
                unsafe {
                    quality_core::libc::kill(-(child.id() as i32), quality_core::libc::SIGKILL);
                }
                child.wait()?;
                break 124;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // A successful shell may leave daemon descendants; the view owns them.
        unsafe {
            quality_core::libc::kill(-(child.id() as i32), quality_core::libc::SIGKILL);
        }
        Ok(Result {
            code,
            elapsed: start.elapsed(),
            output: fs::read_to_string(output_path).unwrap_or_default(),
        })
    }
    #[cfg(not(target_os = "linux"))]
    pub fn run(
        &self,
        _root: &Path,
        _cwd: &Path,
        _command: &str,
        _mutation: Option<(&Path, &str)>,
        _timeout: Option<Duration>,
    ) -> io::Result<Result> {
        Err(io::Error::other(
            "mutation isolation is currently supported only on Linux",
        ))
    }

    #[cfg(not(target_os = "linux"))]
    pub fn run_argv(
        &self,
        _root: &Path,
        _cwd: &Path,
        _argv: &[String],
        _mutation: Option<(&Path, &str)>,
        _timeout: Option<Duration>,
    ) -> io::Result<Result> {
        Err(io::Error::other(
            "mutation isolation is currently supported only on Linux",
        ))
    }
}
impl Drop for View {
    fn drop(&mut self) {
        if let Err(error) = force_remove(&self.directory) {
            eprintln!(
                "Cannot remove mutation view {}: {error}",
                self.directory.display()
            );
        }
    }
}
