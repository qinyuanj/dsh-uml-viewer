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

fn roots() -> MutexGuard<'static, BTreeMap<PathBuf, fs::File>> {
    static ROOTS: OnceLock<Mutex<BTreeMap<PathBuf, fs::File>>> = OnceLock::new();
    ROOTS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

fn claim(directory: &Path) -> io::Result<fs::File> {
    let owner = private_file(&directory.join("owner"))?;
    #[cfg(windows)]
    {
        lock_file(&owner)?;
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

fn discard(directory: &Path) -> io::Result<()> {
    roots().remove(directory);
    // The directory leaves the registry before the removal, so a transient
    // refusal has nowhere left to wait: wait here, where the caller can
    // still see the failure, instead of leaking an untracked copy.
    remove_patiently(directory)
}

pub fn discard_all() {
    let directories: Vec<PathBuf> = {
        let mut roots = roots();
        let directories: Vec<PathBuf> = roots.keys().cloned().collect();
        roots.clear();
        directories
    };
    for directory in directories {
        if let Err(error) = remove_patiently(&directory) {
            eprintln!(
                "Cannot remove mutation scratch {}: {error}",
                directory.display()
            );
        }
    }
}

const RECLAIM_WINDOW: Duration = Duration::from_secs(5);
const RECLAIM_PAUSE_MIN: Duration = Duration::from_millis(2);
const RECLAIM_PAUSE_MAX: Duration = Duration::from_millis(50);

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
        if metadata.file_type().is_symlink() || !metadata.is_dir() || !owned_by_me(&metadata) {
            continue;
        }
        reclaim(&path, &name);
    }
    Ok(())
}

fn reclaim(directory: &Path, name: &str) {
    let owner = name_pid(name);
    let deadline = Instant::now() + RECLAIM_WINDOW;
    let mut pause = RECLAIM_PAUSE_MIN;
    let mut failure: Option<io::Error> = None;
    loop {
        match owner_state(directory) {
            Owner::Foreign => return,
            Owner::Live if owner.is_none_or(running) => return,
            Owner::Live => {}
            Owner::Gone(file) => {
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

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
enum Owner {
    Gone(fs::File),
    Live,
    Foreign,
}

#[cfg(any(target_os = "linux", windows))]
fn owner_state(directory: &Path) -> Owner {
    let Ok(owner) = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("owner"))
    else {
        return Owner::Foreign;
    };
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
    lock_file(owner).is_ok()
}

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

fn name_pid(name: &str) -> Option<u32> {
    name.strip_prefix("mutator-")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

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
fn lock_file(owner: &fs::File) -> io::Result<()> {
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
    }
    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: std::ptr::null_mut(),
    };
    let handle = owner.as_raw_handle();
    // LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY. The lock lives as
    // long as the handle: closing the file releases it, which is what dropping
    // the guard or the probed owner does.
    let result = unsafe { LockFileEx(handle, 0x0000_0002 | 0x0000_0001, 0, 1, 0, &mut overlapped) };
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
        // A Windows directory junction, and a directory symlink with it, is a
        // reparse point: `symlink_metadata` reports it as a link, so
        // `remove_file` refuses it with ERROR_ACCESS_DENIED (os error 5, #105).
        // `remove_dir` takes the link itself and never follows it into the
        // target, which is what removing a tree means here.
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            if metadata.file_type().is_symlink_dir() {
                return match fs::remove_dir(path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    result => result,
                };
            }
        }
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

pub fn remove_patiently(path: &Path) -> io::Result<()> {
    retry_while_held(RECLAIM_WINDOW, || force_remove(path))
}

fn retry_while_held<F>(window: Duration, mut remove: F) -> io::Result<()>
where
    F: FnMut() -> io::Result<()>,
{
    let deadline = Instant::now() + window;
    let mut pause = RECLAIM_PAUSE_MIN;
    loop {
        match remove() {
            Ok(()) => return Ok(()),
            Err(error) if !a_handle_held_it(&error) => return Err(error),
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
                std::thread::sleep(pause);
                pause = (pause + pause).min(RECLAIM_PAUSE_MAX);
            }
        }
    }
}

/// Whether the error is another process's open handle, not a verdict.
///
/// ERROR_SHARING_VIOLATION (32) and ERROR_LOCK_VIOLATION (33) both mean the
/// holder has to close first. ERROR_ACCESS_DENIED (5) is the same wait from
/// the other side: Windows refuses the removal of a directory that is another
/// process's current directory. The numbers are Windows' own.
fn a_handle_held_it(error: &io::Error) -> bool {
    #[cfg(windows)]
    let held = matches!(error.raw_os_error(), Some(5) | Some(32) | Some(33));
    #[cfg(not(windows))]
    let held = {
        let _ = error;
        false
    };
    held
}

#[cfg(test)]
mod tests {
    #![cfg_attr(test, allow(clippy::disallowed_methods))]

    use super::{Root, claim, fixture, nonce, private_directory, sweep, temporary};
    use std::{
        env,
        ffi::OsStr,
        fs, io,
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

    #[test]
    fn force_remove_takes_a_directory_link_and_leaves_its_target() {
        let root = base("link");
        let target = test_target().join(format!("scratch-link-target-{}", std::process::id()));
        let _ = fs::remove_dir_all(&target);
        fs::create_dir_all(&target).expect("create the link target");
        fs::write(target.join("kept.txt"), "kept").expect("seed the link target");
        fs::create_dir_all(root.join("inner")).expect("create the directory holding the link");
        directory_link(&root.join("inner").join("link"), &target);

        super::force_remove(&root).expect("remove a tree that holds a directory link");

        assert!(!root.exists(), "the tree that held the link is gone");
        assert!(
            target.join("kept.txt").is_file(),
            "the link's target is untouched"
        );
        let _ = fs::remove_dir_all(&target);
    }

    #[cfg(windows)]
    #[test]
    fn a_held_handle_is_waited_for_until_the_removal_succeeds() {
        let attempts = std::cell::Cell::new(0);
        let result = super::retry_while_held(Duration::from_secs(1), || {
            attempts.set(attempts.get() + 1);
            match attempts.get() {
                1 | 2 => Err(io::Error::from_raw_os_error(32)),
                _ => Ok(()),
            }
        });

        result.expect("the third attempt removes it");
        assert_eq!(attempts.get(), 3, "it waited out the two refused attempts");
    }

    #[cfg(windows)]
    #[test]
    fn a_handle_that_never_clears_is_reported_at_the_end_of_the_window() {
        let attempts = std::cell::Cell::new(0);
        let started = Instant::now();
        let result = super::retry_while_held(Duration::from_millis(60), || {
            attempts.set(attempts.get() + 1);
            Err(io::Error::from_raw_os_error(32))
        });

        let error = result.expect_err("a handle that never clears is an error");
        assert_eq!(
            error.raw_os_error(),
            Some(32),
            "the original error survives"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(60),
            "it spends the whole window before giving up"
        );
        assert!(attempts.get() > 1, "it tries more than once");
    }

    #[test]
    fn an_error_that_is_not_a_held_handle_comes_back_at_once() {
        let attempts = std::cell::Cell::new(0);
        let result = super::retry_while_held(Duration::from_secs(5), || {
            attempts.set(attempts.get() + 1);
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "refused"))
        });

        assert!(result.is_err(), "a refusal is still a refusal");
        assert_eq!(
            attempts.get(),
            1,
            "nothing is waited on that will not clear"
        );
    }

    fn run_isolated(test: &str, tmpdir: &Path, base: &Path) {
        let log_path = base.join("child.log");
        let log = fs::File::create(&log_path).expect("create the child log");
        let status = Command::new(env::current_exe().expect("the test executable"))
            .args(["--exact", &format!("scratch::tests::{test}"), "--nocapture"])
            .env(CHILD, test)
            .env(BASE, base)
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

    #[cfg(windows)]
    #[test]
    fn root_drop_waits_for_a_transient_file_handle() {
        let base = base("discard-all");
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        let root = Root::new(&project).expect("create a registered root");
        let path = root.path().to_path_buf();
        let held = path.join("held.txt");
        fs::write(&held, "held").unwrap();
        #[cfg(windows)]
        let file = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&held)
                .expect("hold a file in the root")
        };
        let start = Instant::now();
        let dropper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            drop(file);
        });
        // Dropping this root must wait for its own held file. Calling the
        // process-global `discard_all` here deleted other tests' live roots
        // (#144), including an in-progress Lua collection scratch.
        drop(root);
        let waited = start.elapsed();
        dropper.join().expect("the file holder ended");
        assert!(
            !path.exists(),
            "the dropped root survived a released file handle"
        );
        assert!(
            waited >= Duration::from_millis(300),
            "the root was removed while a file was held: {waited:?}"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(windows)]
    #[test]
    fn the_scratch_fixture_does_not_delete_another_live_root() {
        let base = base("scratch-fixture-isolation");
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        let root = Root::new(&project).expect("create a registered root");
        let path = root.path().to_path_buf();
        let _ = fs::remove_dir_all(&base);
        assert!(
            path.exists(),
            "a scratch fixture removed another live root: {}",
            path.display()
        );
        drop(root);
    }

    #[cfg(windows)]
    #[test]
    fn a_dropped_root_waits_for_a_transient_file_handle() {
        let base = base("drop-root");
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        let root = Root::new(&project).expect("create a registered root");
        let held = root.path().join("held.txt");
        fs::write(&held, "held").unwrap();
        let path = root.path().to_path_buf();
        #[cfg(windows)]
        let file = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&held)
                .expect("hold a file in the root")
        };
        let dropper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            drop(file);
        });
        let start = Instant::now();
        drop(root);
        let waited = start.elapsed();
        dropper.join().expect("the file holder ended");
        assert!(
            !path.exists(),
            "the dropped root survived a held file handle"
        );
        assert!(
            waited >= Duration::from_millis(300),
            "the dropped root was removed while a file was held: {waited:?}"
        );
        let _ = fs::remove_dir_all(&base);
    }

    const HOLD: &str = "MUTATOR_SCRATCH_HOLD";
    const ROOT_PID: &str = "MUTATOR_SCRATCH_ROOT_PID";

    fn hold_the_root() {
        let base = child_base();
        let pid = env::var(ROOT_PID)
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or_else(std::process::id);
        let root = fixture().join(format!("mutator-{pid}-0-{:016x}", nonce()));
        private_directory(&root).expect("create the held root");
        let owner = claim(&root).expect("claim the held root");
        fs::write(base.join("root"), root.to_str().expect("a utf-8 root"))
            .expect("publish the held root");
        let millis = env::var(HOLD)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1_000);
        std::thread::sleep(Duration::from_millis(millis));
        drop(owner);
    }

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
