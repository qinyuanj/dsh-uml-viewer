//! Every command, including coverage and baseline, gets a disposable overlay.
//! Mounting over the canonical root in the child namespace preserves absolute
//! paths and sibling Cargo dependencies without exposing writes to the parent.
//!
//! Copy isolation is the Windows mechanism (`crate::copies`); the paths a call
//! named with `--copy` are validated here for every platform, because a Linux
//! view already reaches them and must still refuse the same bad arguments.
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
    path::{Component, Path, PathBuf},
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

/// A path outside the project root that a call declared with `--copy`.
///
/// On Windows the copy sits at `<slot>/project`, so `..` from it is the slot and
/// `relative` is where the path must appear: the same position it has beside the
/// real project root. Linux records the same validated paths and copies nothing,
/// because its view is mounted over the real project root and `..` already
/// reaches the real parent directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outside {
    /// The real path on this host, symlinks resolved.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub source: PathBuf,
    /// Where a copy shows it, relative to the copy's parent: no `..`, no root.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub relative: PathBuf,
}

/// `--copy`: the outside paths a call named, validated before any command runs.
///
/// Only a path **outside** `root` is accepted: one inside it is already in the
/// copy, so accepting it would promise something the copy does not do. `..` is
/// allowed only as a single leading component that leaves `root`, so the target
/// `<slot>/<relative>` can never leave this call's scratch slot. The argument
/// must exist. Every failure is a usage error naming the value, as the other
/// option errors do.
pub fn resolve_outside(root: &Path, requested: &[String]) -> Result<Vec<Outside>, String> {
    let root = quality_core::paths::resolve(root);
    let mut resolved: Vec<Outside> = Vec::new();
    for value in requested {
        let mut relative = PathBuf::new();
        let mut leaves = false;
        for component in Path::new(value).components() {
            match component {
                Component::Normal(name) => relative.push(name),
                Component::CurDir => {}
                Component::ParentDir if relative.pop() => {}
                Component::ParentDir => {
                    if leaves {
                        return Err(format!("--copy accepts at most one leading '..': {value}"));
                    }
                    leaves = true;
                }
                Component::Prefix(_) | Component::RootDir => {
                    return Err(format!(
                        "--copy takes a path relative to the project root, not an absolute path: \
                         {value}"
                    ));
                }
            }
        }
        if !leaves {
            return Err(format!(
                "--copy path is inside the project root, where the copy already has it: {value}"
            ));
        }
        if relative.as_os_str().is_empty() {
            return Err(format!(
                "--copy path names the project's parent directory itself, not a path in it: {value}"
            ));
        }
        // Every remaining component is normal, so the slot target is inside the
        // scratch root by construction; the tests pin the normalisation.
        debug_assert!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        );
        let source = quality_core::paths::resolve(&root.join(value));
        if source.starts_with(&root) {
            return Err(format!(
                "--copy path resolves back inside the project root: {value}"
            ));
        }
        match std::fs::metadata(&source) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(format!("--copy path does not exist: {value}"));
            }
            Err(error) => {
                return Err(format!("--copy path cannot be read ({error}): {value}"));
            }
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => {}
            Ok(_) => {
                return Err(format!("--copy path is not a file or directory: {value}"));
            }
        }
        // Two spellings of one position (`../contracts`, `.././contracts`) are
        // one outside copy, not two copies onto the same path.
        if resolved
            .iter()
            .any(|existing| existing.relative == relative)
        {
            continue;
        }
        resolved.push(Outside { source, relative });
    }
    Ok(resolved)
}

/// Commands that need the JVM's private home and a seeded dependency cache.
#[cfg(target_os = "linux")]
const JVM_COMMANDS: [&str; 3] = ["mvn", "clj", "clojure"];

/// A command's own overlay layers: a fresh view of the project every time.
#[cfg(target_os = "linux")]
fn reset_layers(directory: &Path) -> io::Result<()> {
    for name in ["upper", "work", "mutation", "output", "m2"] {
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
        // `--copy` needs no work here: the view is mounted over the real project
        // root, so `..` resolves to the real parent directory and an outside
        // path beside the project is already readable. It is still validated
        // (`resolve_outside`) so both platforms refuse the same arguments.
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
        let jvm = argv
            .iter()
            .any(|arg| JVM_COMMANDS.iter().any(|name| arg.contains(name)));
        let java_home_option = if jvm {
            let mut option = std::ffi::OsString::from("-Duser.home=");
            option.push(root.join("target/mutator-home"));
            java_tool_option(&option)
        } else {
            std::ffi::OsString::new()
        };
        let cache_seeded = if jvm {
            let home = std::env::var_os("HOME")
                .ok_or_else(|| Failure::Message("HOME is required to seed the JVM cache".into()))?;
            seed_cache(&PathBuf::from(home).join(".m2"), &self.directory.join("m2"))
                .map_err(message)?
        } else {
            false
        };
        // All substitutions are positional arguments, never shell source.
        const SCRIPT: &str = r#"set -eu
root=$1
layers=$2
cwd=$3
mutation=$4
destination=$5
java_home_option=$6
cache_seeded=$7
shift 7
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
# JVM tools locate their repositories through java.user.home, not just HOME.
# Rust has already rejected links and copied regular cache files into private
# scratch storage. Mount that independent seed; never preserve cache symlinks
# with cp, or traverse the caller's cache again after the preflight.
if [ -n "$java_home_option" ]; then
    if [ "$cache_seeded" = 1 ]; then
        mkdir -p "$home/.m2"
        mount --bind "$layers/m2" "$home/.m2"
    fi
    export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:-} $java_home_option"
fi
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
        .arg(java_home_option)
        .arg(if cache_seeded { "1" } else { "0" })
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

/// Every report a finished command's view showed, not only those it wrote:
/// the original reads them all from the project after an in-place command.
/// A Windows copy (`base`, from `report_path(view, "")`) holds the whole
/// project. A Linux view's upper layer (`base`) is read through to the project
/// it was mounted over (`lower`, the canonical root); a whiteout or an opaque
/// directory there stays deleted.
pub fn report_tree<'a>(base: &'a Path, lower: &'a Path) -> quality_core::ReportTree<'a> {
    #[cfg(windows)]
    {
        let _ = lower;
        quality_core::ReportTree::plain(base)
    }
    #[cfg(not(windows))]
    {
        quality_core::ReportTree::overlay(base, lower)
    }
}

/// JAVA_TOOL_OPTIONS uses the JVM's quote parser, not shell escaping. Adjacent
/// quoted fragments form one option; a literal double quote is put in a
/// single-quoted fragment, while every other character (including ') is inside
/// double quotes. Backslashes are literal here (and excluded by the root policy).
#[cfg(target_os = "linux")]
fn java_tool_option(value: &std::ffi::OsStr) -> std::ffi::OsString {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let mut encoded = vec![b'"'];
    for byte in value.as_bytes() {
        if *byte == b'"' {
            encoded.extend_from_slice(b"\"'\"'\"");
        } else {
            encoded.push(*byte);
        }
    }
    encoded.push(b'"');
    std::ffi::OsString::from_vec(encoded)
}

/// Preflight the entire cache before copying any bytes. Missing caches are fine;
/// links (including a .m2 root link), special files and unreadable entries fail
/// closed before the command is launched.
#[cfg(target_os = "linux")]
fn seed_cache(source: &Path, destination: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(source) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let directory = open_cache_entry(source)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::other("JVM cache root must be a directory"));
    }
    walk_cache(&directory, None)?;
    scratch::private_directory(destination)?;
    if let Err(error) = walk_cache(&directory, Some(destination)) {
        // A partially seeded cache must never be mistaken for the caller's.
        let _ = scratch::force_remove(destination);
        return Err(error);
    }
    Ok(true)
}

/// O_NOFOLLOW rejects a link swapped in after symlink_metadata. O_NONBLOCK
/// prevents a raced-in FIFO from blocking open; metadata below rejects it.
#[cfg(target_os = "linux")]
fn open_cache_entry(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "refusing symlink in JVM cache: {}",
            path.display()
        )));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(quality_core::libc::O_NOFOLLOW | quality_core::libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(io::Error::other("JVM cache contains a non-regular entry"));
    }
    Ok(file)
}

/// Read each directory through its held descriptor, so replacing an ancestor
/// with a symlink cannot redirect traversal. Every child is opened no-follow,
/// in both passes. Only bytes are copied into newly created private files: no
/// symlinks, hard links or caller-owned inode permissions reach the writable
/// seed. Concurrent ordinary-file writes can still yield an inconsistent cache.
#[cfg(target_os = "linux")]
fn walk_cache(directory: &fs::File, destination: Option<&Path>) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let anchored = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    for entry in fs::read_dir(anchored)? {
        let entry = entry?;
        let mut source = open_cache_entry(&entry.path())?;
        let target = destination.map(|path| path.join(entry.file_name()));
        if source.metadata()?.is_dir() {
            if let Some(target) = &target {
                scratch::private_directory(target)?;
            }
            walk_cache(&source, target.as_deref())?;
        } else if let Some(target) = &target {
            io::copy(&mut source, &mut scratch::private_file(target)?)?;
        }
    }
    Ok(())
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, symlink},
    };

    fn temporary() -> Root {
        Root::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn jvm_home_option_quotes_spaces_and_both_quote_characters() {
        for (input, expected) in [
            (
                "-Duser.home=/project/target/mutator-home",
                "\"-Duser.home=/project/target/mutator-home\"",
            ),
            (
                "-Duser.home=/space dir/target/mutator-home",
                "\"-Duser.home=/space dir/target/mutator-home\"",
            ),
            (
                "-Duser.home=/single'quote/target/mutator-home",
                "\"-Duser.home=/single'quote/target/mutator-home\"",
            ),
            (
                "-Duser.home=/double\"quote/target/mutator-home",
                "\"-Duser.home=/double\"'\"'\"quote/target/mutator-home\"",
            ),
            (
                "-Duser.home=/both'\" quotes/target/mutator-home",
                "\"-Duser.home=/both'\"'\"'\" quotes/target/mutator-home\"",
            ),
            (
                "-Duser.home=/\" -Dother=value '/target/mutator-home",
                "\"-Duser.home=/\"'\"'\" -Dother=value '/target/mutator-home\"",
            ),
            (
                "-Duser.home=/$(touch marker)`id`/target/mutator-home",
                "\"-Duser.home=/$(touch marker)`id`/target/mutator-home\"",
            ),
        ] {
            assert_eq!(java_tool_option(input.as_ref()), expected, "{input}");
        }
    }

    #[test]
    fn jvm_home_option_preserves_path_bytes_and_quoted_whitespace() {
        let input = std::ffi::OsStr::from_bytes(b"-Duser.home=/nonutf8-\xff/space\tline\n");
        assert_eq!(
            java_tool_option(input).as_bytes(),
            b"\"-Duser.home=/nonutf8-\xff/space\tline\n\""
        );
    }

    #[test]
    fn missing_cache_needs_no_seed() {
        let temporary = temporary();
        let destination = temporary.path().join("seed");
        assert!(!seed_cache(&temporary.path().join("missing"), &destination).unwrap());
        assert!(!destination.exists());
    }

    #[test]
    fn regular_cache_is_an_independent_copy_even_for_hardlinks() {
        let temporary = temporary();
        let source = temporary.path().join("cache");
        let destination = temporary.path().join("seed");
        fs::create_dir_all(source.join("repository/group")).unwrap();
        let original = source.join("repository/group/artifact.jar");
        fs::write(&original, b"cached bytes").unwrap();
        fs::hard_link(&original, source.join("alias.jar")).unwrap();
        assert!(seed_cache(&source, &destination).unwrap());
        let copied = destination.join("repository/group/artifact.jar");
        assert_eq!(fs::read(&copied).unwrap(), b"cached bytes");
        assert_ne!(
            fs::metadata(&original).unwrap().ino(),
            fs::metadata(&copied).unwrap().ino()
        );
        fs::write(&copied, b"view writes").unwrap();
        fs::write(destination.join("alias.jar"), b"alias writes").unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"cached bytes");
    }

    #[test]
    fn root_cache_symlinks_are_refused_including_dangling_links() {
        let temporary = temporary();
        let outside = temporary.path().join("outside");
        fs::create_dir(&outside).unwrap();
        for (name, target) in [
            ("linked", outside),
            ("dangling", temporary.path().join("missing")),
        ] {
            let source = temporary.path().join(name);
            let destination = temporary.path().join(format!("seed-{name}"));
            symlink(target, &source).unwrap();
            assert!(seed_cache(&source, &destination).is_err());
            assert!(!destination.exists());
        }
    }

    #[test]
    fn nested_cache_symlinks_fail_preflight_before_any_copy() {
        let temporary = temporary();
        let outside = temporary.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("file"), b"caller bytes").unwrap();
        for (name, target) in [
            ("file", outside.join("file")),
            ("directory", outside.clone()),
            ("dangling", outside.join("missing")),
        ] {
            let source = temporary.path().join(name);
            let destination = temporary.path().join(format!("seed-{name}"));
            fs::create_dir_all(source.join("repository/group")).unwrap();
            fs::write(source.join("regular"), b"normal entry").unwrap();
            symlink(target, source.join("repository/group/link")).unwrap();
            assert!(seed_cache(&source, &destination).is_err());
            assert!(!destination.exists());
        }
        assert_eq!(fs::read(outside.join("file")).unwrap(), b"caller bytes");
    }

    #[test]
    fn internal_cache_symlinks_are_also_refused() {
        let temporary = temporary();
        let source = temporary.path().join("cache");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file"), b"cached bytes").unwrap();
        symlink("file", source.join("link")).unwrap();
        assert!(seed_cache(&source, &temporary.path().join("seed")).is_err());
    }

    #[test]
    fn link_introduced_after_preflight_is_refused_by_copy() {
        let temporary = temporary();
        let source = temporary.path().join("cache");
        let destination = temporary.path().join("seed");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file"), b"cached bytes").unwrap();
        let directory = open_cache_entry(&source).unwrap();
        walk_cache(&directory, None).unwrap();
        fs::remove_file(source.join("file")).unwrap();
        symlink(temporary.path().join("outside"), source.join("file")).unwrap();
        fs::create_dir(&destination).unwrap();
        assert!(walk_cache(&directory, Some(destination.as_path())).is_err());
        assert!(!destination.join("file").exists());
    }

    #[test]
    fn opened_cache_directory_cannot_be_redirected_by_ancestor_replacement() {
        let temporary = temporary();
        let source = temporary.path().join("cache");
        let destination = temporary.path().join("seed");
        let outside = temporary.path().join("outside");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(source.join("file"), b"cached bytes").unwrap();
        fs::write(outside.join("file"), b"outside bytes").unwrap();
        let directory = open_cache_entry(&source).unwrap();
        walk_cache(&directory, None).unwrap();
        fs::rename(&source, temporary.path().join("moved")).unwrap();
        symlink(&outside, &source).unwrap();
        fs::create_dir(&destination).unwrap();
        walk_cache(&directory, Some(destination.as_path())).unwrap();
        assert_eq!(fs::read(destination.join("file")).unwrap(), b"cached bytes");
        assert_eq!(fs::read(outside.join("file")).unwrap(), b"outside bytes");
    }

    #[test]
    fn resetting_a_worker_discards_its_writable_cache_seed() {
        let temporary = temporary();
        let seed = temporary.path().join("m2");
        fs::create_dir(&seed).unwrap();
        fs::write(seed.join("downloaded.jar"), b"previous command").unwrap();
        reset_layers(temporary.path()).unwrap();
        assert!(!seed.exists());
        assert!(temporary.path().join("upper").is_dir());
        assert!(temporary.path().join("work").is_dir());
    }

    #[test]
    fn regular_file_is_not_a_cache_root() {
        let temporary = temporary();
        let source = temporary.path().join("cache");
        fs::write(&source, b"file").unwrap();
        assert!(seed_cache(&source, &temporary.path().join("seed")).is_err());
    }
}

#[cfg(test)]
#[path = "isolation_outside_tests.rs"]
mod outside_tests;
