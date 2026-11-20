//! Private scratch roots for overlay views.
//!
//! A root is a `0700` directory outside the project with an unpredictable
//! `mutator-<pid>-<counter>-<16 hex>` name, holding a `0600` owner file. The
//! owning process keeps that file's `flock` for its lifetime, so a later run
//! can take the lock only when the owner is gone. That is how a root abandoned
//! by a killed process is recognised without trusting a pid that the system may
//! have reused.
//!
//! Recognising it is not the whole job. The system releases a dead process's
//! handles asynchronously, and a killed run's Job Object terminates the
//! children whose copies sit inside the root after the owner itself is gone,
//! so the first look at an abandoned root can still find it pinned. A sweep
//! therefore re-probes one abandoned root for a bounded window instead of
//! skipping it on the first look, which would lose the guarantee exactly when
//! the host is busiest (#96).
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Roots this process owns, with the owner file whose lock proves it.
fn roots() -> MutexGuard<'static, BTreeMap<PathBuf, fs::File>> {
    static ROOTS: OnceLock<Mutex<BTreeMap<PathBuf, fs::File>>> = OnceLock::new();
    ROOTS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One scratch root; dropping it removes the tree.
pub struct Root {
    directory: PathBuf,
}

impl Root {
    pub fn new(project: &Path) -> io::Result<Self> {
        let temporary = temporary(project)?;
        for _ in 0..16 {
            let directory = temporary.join(format!(
                "mutator-{}-{}-{:016x}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                nonce()
            ));
            match private_directory(&directory) {
                Ok(()) => {
                    return match claim(&directory) {
                        Ok(owner) => {
                            roots().insert(directory.clone(), owner);
                            Ok(Self { directory })
                        }
                        Err(error) => {
                            let _ = force_remove(&directory);
                            Err(error)
                        }
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                // The failure already names the operation and the directory.
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(
            "cannot create a private mutation scratch directory",
        ))
    }

    pub fn path(&self) -> &Path {
        &self.directory
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        if let Err(error) = discard(&self.directory) {
            eprintln!(
                "Cannot remove mutation scratch {}: {error}",
                self.directory.display()
            );
        }
    }
}

/// A private owner file inside `directory`, locked for this process's life.
fn claim(directory: &Path) -> io::Result<fs::File> {
    let owner = private_file(&directory.join("owner"))?;
    #[cfg(windows)]
    {
        windows_lock(&owner, true)?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let locked = unsafe {
            quality_core::libc::flock(
                owner.as_raw_fd(),
                quality_core::libc::LOCK_EX | quality_core::libc::LOCK_NB,
            )
        };
        if locked != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(owner)
}

/// Remove one root this process owns and forget its owner file.
fn discard(directory: &Path) -> io::Result<()> {
    roots().remove(directory);
    force_remove(directory)
}

/// Remove every root this process still owns: the interrupt path, which cannot
/// wait for the guards to unwind.
pub fn discard_all() {
    let directories: Vec<PathBuf> = {
        let mut roots = roots();
        let directories: Vec<PathBuf> = roots.keys().cloned().collect();
        roots.clear();
        directories
    };
    for directory in directories {
        if let Err(error) = force_remove(&directory) {
            eprintln!(
                "Cannot remove mutation scratch {}: {error}",
                directory.display()
            );
        }
    }
}

/// How long one abandoned root keeps being re-probed while the system releases
/// what its dead owner left behind.
///
/// That release is asynchronous, so the first look after a kill can find the
/// root still pinned: `LockFileEx` documents that "the time it takes for the
/// operating system to unlock these locks depends upon available system
/// resources", and a killed run's Job Object terminates the children whose
/// copies sit inside the root after the owner itself is gone. The window has to
/// outlast that under load, and it has to end: a root something else keeps open
/// is reported, not waited on forever.
const RECLAIM_WINDOW: Duration = Duration::from_secs(5);
/// The pause after a failed probe; it doubles up to `RECLAIM_PAUSE_MAX`, so a
/// root that frees up in a millisecond costs about that.
const RECLAIM_PAUSE_MIN: Duration = Duration::from_millis(2);
const RECLAIM_PAUSE_MAX: Duration = Duration::from_millis(50);

/// Remove roots whose owner can no longer clean up: a killed run leaves its
/// scratch behind, and this call is what reclaims it.
///
/// One look at a root is not a verdict; see `RECLAIM_WINDOW` and `reclaim`.
pub fn sweep(project: &Path) -> io::Result<()> {
    let temporary = temporary(project)?;
    let Ok(entries) = fs::read_dir(&temporary) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with("mutator-") {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        // Never a link, only a directory of ours, and only this user's.
        if metadata.file_type().is_symlink() || !metadata.is_dir() || !owned_by_me(&metadata) {
            continue;
        }
        reclaim(&path, &name);
    }
    Ok(())
}

/// Reclaim one root, re-probing it while its dead owner's handles are released.
///
/// The lock decides abandonment. The pid only decides how long to wait: it is
/// read from the name (`mutator-<pid>-...`), and it is never the verdict
/// because the system may have reused it — a root whose lock can be taken is
/// abandoned whatever its name says.
fn reclaim(directory: &Path, name: &str) {
    let owner = name_pid(name);
    let deadline = Instant::now() + RECLAIM_WINDOW;
    let mut pause = RECLAIM_PAUSE_MIN;
    let mut failure: Option<io::Error> = None;
    loop {
        match owner_state(directory) {
            // No owner file (a run killed between creating the directory and
            // claiming it, or a tree another sweep already cleared), or a
            // platform with no owner lock: the same answer every time, and
            // never this process's to remove.
            Owner::Foreign => return,
            // The lock is still held by a run that is still working: that root
            // is not abandoned, and it is not worth one pause of the window. A
            // lock still held by a dead owner is what the window is for.
            Owner::Live if owner.map_or(true, running) => return,
            Owner::Live => {}
            Owner::Gone(file) => {
                // The owner file goes last: its handle holds the lock other
                // sweeps probe for, and on Windows a delete-pending entry keeps
                // its directory non-empty.
                let cleared = clear_children(directory);
                drop(file);
                match cleared.and_then(|()| force_remove(directory)) {
                    Ok(()) => return,
                    Err(error) => failure = Some(error),
                }
            }
        }
        if Instant::now() >= deadline {
            match failure {
                Some(error) => eprintln!(
                    "Cannot remove abandoned mutation scratch {}: {error}",
                    directory.display()
                ),
                None => eprintln!(
                    "Cannot remove abandoned mutation scratch {}: its owner lock was still held after {RECLAIM_WINDOW:?}",
                    directory.display()
                ),
            }
            return;
        }
        std::thread::sleep(pause);
        pause = (pause + pause).min(RECLAIM_PAUSE_MAX);
    }
}

/// What one probe of a root's owner lock found.
// A platform without an owner lock only ever answers `Foreign`.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
enum Owner {
    /// The lock is ours: the owner is gone, and no other sweep touches the root
    /// while this handle lives.
    Gone(fs::File),
    /// Somebody holds the lock: a live owner, or a dead one the system has not
    /// finished releasing.
    Live,
    /// Not a root this process may reclaim.
    Foreign,
}

/// Whether the owner of `directory` is gone: its lock can be taken.
#[cfg(any(target_os = "linux", windows))]
fn owner_state(directory: &Path) -> Owner {
    let Ok(owner) = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("owner"))
    else {
        return Owner::Foreign;
    };
    // A lock that can be taken means the owner died; the handle keeps it until
    // the tree is gone.
    if take_lock(&owner) {
        Owner::Gone(owner)
    } else {
        Owner::Live
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn owner_state(_directory: &Path) -> Owner {
    Owner::Foreign
}

/// Take the owner's lock without waiting; `false` while the owner holds it.
#[cfg(target_os = "linux")]
fn take_lock(owner: &fs::File) -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe {
        quality_core::libc::flock(
            owner.as_raw_fd(),
            quality_core::libc::LOCK_EX | quality_core::libc::LOCK_NB,
        ) == 0
    }
}

#[cfg(windows)]
fn take_lock(owner: &fs::File) -> bool {
    windows_lock(owner, true).is_ok()
}

/// Remove everything inside a root except its owner file, whose open handle
/// holds the lock that keeps other sweeps off the tree.
fn clear_children(directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name() == OsStr::new("owner") {
            continue;
        }
        force_remove(&entry.path())?;
    }
    Ok(())
}

/// The pid a root's name records: `mutator-<pid>-<counter>-<16 hex>`.
fn name_pid(name: &str) -> Option<u32> {
    name.strip_prefix("mutator-")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

/// Whether the process a root's name records is still running.
///
/// Only the waiting decision above uses it, so a false "running" costs a
/// pause at most. Windows: an openable handle that has not been signalled.
#[cfg(windows)]
fn running(pid: u32) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    // SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION
    let handle = unsafe { OpenProcess(0x0010_0000 | 0x0000_1000, 0, pid) };
    if handle.is_null() {
        // ERROR_INVALID_PARAMETER is "no such process". Access denied, and any
        // other failure, are read as running: waiting costs time, guessing
        // wrong removes a live run's root.
        return io::Error::last_os_error().raw_os_error() != Some(87);
    }
    let running = unsafe { WaitForSingleObject(handle, 0) } != 0;
    unsafe { CloseHandle(handle) };
    running
}

/// Unix: signal 0 reaches the process, or ESRCH says it is gone; permission
/// denied means somebody else's process, which is still a process.
#[cfg(target_os = "linux")]
fn running(pid: u32) -> bool {
    let sent = unsafe { quality_core::libc::kill(pid as i32, 0) };
    sent == 0 || io::Error::last_os_error().raw_os_error() != Some(quality_core::libc::ESRCH)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn running(_pid: u32) -> bool {
    true
}

#[cfg(windows)]
fn windows_lock(owner: &fs::File, unlock: bool) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut std::ffi::c_void,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LockFileEx(
            file: *mut std::ffi::c_void,
            flags: u32,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn UnlockFileEx(
            file: *mut std::ffi::c_void,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }
    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: std::ptr::null_mut(),
    };
    let handle = owner.as_raw_handle();
    // LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY
    let result = unsafe {
        if unlock {
            LockFileEx(handle, 0x0000_0002 | 0x0000_0001, 0, 1, 0, &mut overlapped)
        } else {
            UnlockFileEx(handle, 0, 1, 0, &mut overlapped)
        }
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn abandoned(_directory: &Path) -> bool {
    false
}

#[cfg(unix)]
fn owned_by_me(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == unsafe { quality_core::libc::geteuid() }
}

#[cfg(not(unix))]
fn owned_by_me(_metadata: &fs::Metadata) -> bool {
    true
}

/// The temporary directory, refused while it is inside the project.
///
/// The path comes back as the system gave it, not resolved. A temporary
/// directory that is a junction — a sandbox redirect is one — is writable under
/// the name it was granted by, while its resolved target can be a directory this
/// process may not create in; resolving it here would move the scratch tree to
/// that target and fail. The refusal below still compares resolved forms, so a
/// junction that hides the temporary directory inside the project is still
/// caught.
pub fn temporary(project: &Path) -> io::Result<PathBuf> {
    let given = environment_temporary();
    let resolved = quality_core::paths::resolve(&given);
    let project = quality_core::paths::resolve(project);
    if resolved.starts_with(&project) {
        return Err(io::Error::other(format!(
            "the temporary directory {} is inside the project {}; point {} at a writable directory outside the project",
            given.display(),
            project.display(),
            TEMPORARY_VARIABLE
        )));
    }
    // A relative name would be joined against the project, so it is resolved.
    Ok(if given.is_absolute() { given } else { resolved })
}

/// The variable a caller sets to choose the temporary directory.
#[cfg(windows)]
const TEMPORARY_VARIABLE: &str = "TMP";

#[cfg(not(windows))]
const TEMPORARY_VARIABLE: &str = "TMPDIR";

/// The directory the platform's own tools read, so one setting covers the
/// scratch root and the linker a run's build starts.
///
/// Windows: `TMP`, then `TEMP` — the order `GetTempPath`, and with it the MSVC
/// linker, reads them. The linker never looks at `TMPDIR`, so a scratch root
/// taken from `TMPDIR` while `TMP` stayed at an unwritable directory let the
/// run's own build fail with `LNK1104: cannot open file ...\lnk{...}.tmp`
/// (#88). `std::env::temp_dir` is exactly that chain, so naming the scratch
/// through it makes `TMP` place both the root and the link's temporary files.
///
/// Unix: `TMPDIR`, then the system temporary directory. The C toolchain reads
/// `TMPDIR` too, so there one name again covers both.
fn environment_temporary() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::temp_dir()
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("TMPDIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    }
}

/// A temporary directory a test fixture may build its tree in (#88).
///
/// A fixture is not a run: it keeps its own name and its own cleanup, but it
/// must start where `temporary` accepts, or a host whose system temporary
/// directory is unwritable fails the fixture with a bare access error.
#[cfg(test)]
pub fn fixture() -> PathBuf {
    temporary(Path::new(env!("CARGO_MANIFEST_DIR")))
        .expect("a writable temporary directory for a fixture")
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

/// Name the operation and the directory when a private directory cannot be
/// created (#82): a sandbox reports a bare `os error 5` (or 32), which does not
/// say which directory the run needed. Every creation site gets this message,
/// so no caller can emit the bare error.
fn report_creation(path: &Path, result: io::Result<()>) -> io::Result<()> {
    result.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot create the private directory {}: {error}",
                path.display()
            ),
        )
    })
}

#[cfg(unix)]
pub fn private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    report_creation(path, fs::DirBuilder::new().mode(0o700).create(path))
}

#[cfg(not(unix))]
pub fn private_directory(path: &Path) -> io::Result<()> {
    report_creation(path, fs::create_dir(path))
}

#[cfg(unix)]
pub fn private_file(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
pub fn private_file(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Remove a tree without following links, restoring the permissions overlayfs
/// leaves at zero, and reporting the first failure instead of ignoring it.
pub fn force_remove(path: &Path) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    //! The two failures this module must not hide: a temporary directory that
    //! is a junction, and a scratch directory that cannot be created. Each
    //! check runs in a child test process, so the temporary directory belongs to
    //! it alone.

    // Test code is the outside harness: these checks launch `cmd /C mklink` and
    // the test executable itself. The module lives under `#[cfg(test)]`, so the
    // lift is test-only by construction and takes the documented
    // `cfg_attr(test, allow(...))` form (clippy.toml, scripts/test_process_boundary.py).
    #![cfg_attr(test, allow(clippy::disallowed_methods))]

    use super::{Root, claim, fixture, nonce, private_directory, sweep, temporary};
    use std::{
        env,
        ffi::OsStr,
        fs,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    const CHILD: &str = "MUTATOR_SCRATCH_TEST";
    const BASE: &str = "MUTATOR_SCRATCH_TEST_BASE";

    fn test_target() -> PathBuf {
        let executable = env::current_exe().expect("the test executable");
        executable
            .parent()
            .and_then(Path::parent)
            .expect("target/<profile>/deps/<test>")
            .join("tmp")
    }

    fn base(name: &str) -> PathBuf {
        let base = test_target().join(format!("scratch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("create the test base");
        base
    }

    #[cfg(windows)]
    fn directory_link(link: &Path, target: &Path) {
        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("start mklink");
        assert!(
            status.success(),
            "mklink /J {} {}",
            link.display(),
            target.display()
        );
    }

    #[cfg(unix)]
    fn directory_link(link: &Path, target: &Path) {
        std::os::unix::fs::symlink(target, link).expect("create a directory symlink");
    }

    fn run_isolated(test: &str, tmpdir: &Path, base: &Path) {
        let log_path = base.join("child.log");
        let log = fs::File::create(&log_path).expect("create the child log");
        let status = Command::new(env::current_exe().expect("the test executable"))
            .args(["--exact", &format!("scratch::tests::{test}"), "--nocapture"])
            .env(CHILD, test)
            .env(BASE, base)
            // Every name the platform's tools read, so the check is about the
            // junction, not about which spelling this host honoured.
            .env("TMPDIR", tmpdir)
            .env("TMP", tmpdir)
            .env("TEMP", tmpdir)
            .stdout(Stdio::from(log.try_clone().expect("clone the child log")))
            .stderr(Stdio::from(log))
            .status()
            .expect("start the isolated test");
        let output = fs::read_to_string(&log_path).unwrap_or_default();
        if !status.success() {
            panic!("isolated check `{test}` failed ({status}):\n{output}");
        }
        // `--exact` selects nothing and still exits 0 when this literal and the
        // test's own name have drifted apart, so the exit status alone would let
        // the check pass without running. Trust the child's result line instead.
        let ran = format!("test scratch::tests::{test} ... ok");
        if !output.lines().any(|line| line.trim() == ran) {
            panic!(
                "the isolated child did not run `{test}` (renamed, or the filter does not match); its log:\n{output}"
            );
        }
    }

    fn child_base() -> PathBuf {
        PathBuf::from(env::var_os(BASE).expect("the isolated test base"))
    }

    #[test]
    fn temporary_returns_the_junction_it_was_given() {
        if env::var_os(CHILD).as_deref()
            == Some(OsStr::new("temporary_returns_the_junction_it_was_given"))
        {
            let base = child_base();
            let link = base.join("link");
            let project = base.join("project");
            assert_eq!(
                temporary(&project).expect("temporary directory"),
                link,
                "the granted junction was resolved to its target"
            );
            return;
        }

        let base = base("junction");
        let target = base.join("target");
        let link = base.join("link");
        let project = base.join("project");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&project).unwrap();
        directory_link(&link, &target);

        run_isolated("temporary_returns_the_junction_it_was_given", &link, &base);

        let _ = fs::remove_dir(&link);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn root_new_names_the_directory_it_could_not_create() {
        if env::var_os(CHILD).as_deref()
            == Some(OsStr::new(
                "root_new_names_the_directory_it_could_not_create",
            ))
        {
            let base = child_base();
            let project = base.join("project");
            let message = Root::new(&project)
                .err()
                .expect("a scratch directory under a file cannot be created")
                .to_string();
            let blocked = base.join("blocked");
            assert!(
                message.contains(&blocked.display().to_string()),
                "the failure names no path: {message}"
            );
            assert!(
                message.contains("cannot create"),
                "the failure names no operation: {message}"
            );
            return;
        }

        let base = base("uncreatable");
        let project = base.join("project");
        let blocked = base.join("blocked");
        fs::create_dir_all(&project).unwrap();
        fs::write(&blocked, "not a directory").unwrap();

        run_isolated(
            "root_new_names_the_directory_it_could_not_create",
            &blocked.join("child"),
            &base,
        );

        let _ = fs::remove_dir_all(&base);
    }

    const HOLD: &str = "MUTATOR_SCRATCH_HOLD";
    const ROOT_PID: &str = "MUTATOR_SCRATCH_ROOT_PID";

    /// The child side of both window cases: create one root, claim it, publish
    /// its path, then hold the owner lock for `HOLD` milliseconds.
    ///
    /// `ROOT_PID` names the root after a process that is already gone; without
    /// it the root carries this child's pid, which is running.
    fn hold_the_root() {
        let base = child_base();
        let pid = env::var(ROOT_PID)
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or_else(std::process::id);
        let root = fixture().join(format!("mutator-{pid}-0-{:016x}", nonce()));
        private_directory(&root).expect("create the held root");
        let owner = claim(&root).expect("claim the held root");
        // Published after the claim, so a parent that sees it knows the lock is
        // held from here on.
        fs::write(base.join("root"), root.to_str().expect("a utf-8 root"))
            .expect("publish the held root");
        let millis = env::var(HOLD)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1_000);
        std::thread::sleep(Duration::from_millis(millis));
        drop(owner);
    }

    /// Start `test` as a child that holds one root's owner lock, and return it
    /// with the root it published.
    fn spawn_holder(
        test: &str,
        base: &Path,
        root_pid: Option<u32>,
        millis: u64,
    ) -> (Child, PathBuf) {
        let log_path = base.join("holder.log");
        let log = fs::File::create(&log_path).expect("create the holder log");
        let mut command = Command::new(env::current_exe().expect("the test executable"));
        command
            .args(["--exact", &format!("scratch::tests::{test}"), "--nocapture"])
            .env(CHILD, test)
            .env(BASE, base)
            .env(HOLD, millis.to_string())
            .stdout(Stdio::from(log.try_clone().expect("clone the holder log")))
            .stderr(Stdio::from(log));
        if let Some(pid) = root_pid {
            command.env(ROOT_PID, pid.to_string());
        }
        let child = command.spawn().expect("start the lock holder");
        let published = base.join("root");
        let start = Instant::now();
        while !published.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "the lock holder published no root: {}",
                fs::read_to_string(&log_path).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let root = PathBuf::from(fs::read_to_string(&published).expect("the published root"));
        (child, root)
    }

    /// A pid that is certainly gone: a child of this test executable that has
    /// already exited and been reaped.
    fn dead_pid() -> u32 {
        let mut child = Command::new(env::current_exe().expect("the test executable"))
            .args(["--exact", "scratch::tests::no_test_has_this_name"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start a child that exits");
        let pid = child.id();
        child.wait().expect("reap the child that exits");
        pid
    }

    /// #96: a root whose owner is gone has to be reclaimed even while the
    /// system is still releasing it. This is the window the CI flake fell into:
    /// the sweep looked once, found the lock still held, and skipped a root it
    /// will never look at again. The child here establishes that window
    /// deliberately — a dead pid in the name, the owner lock still held.
    #[test]
    fn a_root_whose_owner_is_gone_is_reclaimed_while_its_lock_is_released() {
        const CASE: &str = "a_root_whose_owner_is_gone_is_reclaimed_while_its_lock_is_released";
        if env::var_os(CHILD).as_deref() == Some(OsStr::new(CASE)) {
            hold_the_root();
            return;
        }
        let base = base("dying-owner");
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        let (mut holder, root) = spawn_holder(CASE, &base, Some(dead_pid()), 500);
        let start = Instant::now();
        sweep(&project).expect("sweep");
        let waited = start.elapsed();
        let status = holder.wait().expect("the lock holder ended");
        assert!(status.success(), "the lock holder {status}");
        assert!(
            !root.exists(),
            "the sweep left a root whose owner is gone: {}",
            root.display()
        );
        assert!(
            waited >= Duration::from_millis(300),
            "the sweep removed a root whose owner lock was still held: {waited:?}"
        );
        let _ = fs::remove_dir_all(&base);
    }

    /// The window belongs to dead owners only: a live run's root is left alone
    /// at once, so a sweep never spends it on the runs it is running beside.
    #[test]
    fn a_root_whose_owner_is_running_is_left_alone_at_once() {
        const CASE: &str = "a_root_whose_owner_is_running_is_left_alone_at_once";
        if env::var_os(CHILD).as_deref() == Some(OsStr::new(CASE)) {
            hold_the_root();
            return;
        }
        let base = base("live-owner");
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        let (mut holder, root) = spawn_holder(CASE, &base, None, 30_000);
        let start = Instant::now();
        sweep(&project).expect("sweep");
        let waited = start.elapsed();
        let kept = root.exists();
        let _ = holder.kill();
        let _ = holder.wait();
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&base);
        assert!(
            kept,
            "the sweep removed the root of a run that is still working: {}",
            root.display()
        );
        assert!(
            waited < Duration::from_millis(2_500),
            "the sweep waited {waited:?} on a live owner instead of moving on"
        );
    }
}
