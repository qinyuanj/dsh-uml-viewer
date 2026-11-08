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

impl View {
    pub fn new() -> io::Result<Self> {
        let directory = std::env::temp_dir().join(format!(
            "mutator-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory)?;
        for name in ["upper", "work"] {
            fs::create_dir(directory.join(name))?;
        }
        Ok(Self { directory })
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
            fs::write(&mutation_path, source)?;
            path.to_path_buf()
        } else {
            PathBuf::new()
        };
        // All substitutions are positional arguments, never shell source.
        const SCRIPT: &str = r#"set -eu
root=$1
layers=$2
cwd=$3
command=$4
mutation=$5
destination=$6
mount --make-rprivate /
mount -t overlay overlay -o "userxattr,lowerdir=$root,upperdir=$layers/upper,workdir=$layers/work" "$root"
if [ -n "$destination" ]; then cp -- "$mutation" "$destination"; fi
cd -- "$cwd"
# Prevent a harness's external artifact path from escaping the view.
export CARGO_TARGET_DIR="$root/target/mutator-build"
unset CARGO_ENCODED_RUSTFLAGS LLVM_PROFILE_FILE
exec /bin/sh -c "$command"
"#;
        let output_path = self.directory.join("output");
        let output = fs::File::create(&output_path)?;
        let start = Instant::now();
        let mut child = Command::new("unshare")
            .args([
                "--user",
                "--map-root-user",
                "--mount",
                "--",
                "/bin/sh",
                "-c",
                SCRIPT,
                "mutator-view",
            ])
            .arg(root)
            .arg(&self.directory)
            .arg(cwd)
            .arg(command)
            .arg(mutation_path)
            .arg(destination)
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output)
            .process_group(0)
            .spawn()?;
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
}
impl Drop for View {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
