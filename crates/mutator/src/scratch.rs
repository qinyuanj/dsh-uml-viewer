//! Private scratch roots for overlay views.
//!
//! A root is a `0700` directory outside the project with an unpredictable
//! `mutator-<pid>-<counter>-<16 hex>` name, holding a `0600` owner file. The
//! owning process keeps that file's `flock` for its lifetime, so a later run
//! can take the lock only when the owner is gone. That is how a root abandoned
//! by a killed process is recognised without trusting a pid that the system may
//! have reused.
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
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
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!(
                            "cannot create the mutation scratch directory {}: {error}",
                            directory.display()
                        ),
                    ));
                }
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

/// Remove roots whose owner can no longer clean up: a killed run leaves its
/// scratch behind, and this call is what reclaims it.
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
        if !abandoned(&path) {
            continue;
        }
        if let Err(error) = force_remove(&path) {
            eprintln!(
                "Cannot remove abandoned mutation scratch {}: {error}",
                path.display()
            );
        }
    }
    Ok(())
}

/// Whether the owner of `directory` is gone: its lock can be taken.
#[cfg(target_os = "linux")]
fn abandoned(directory: &Path) -> bool {
    use std::os::unix::io::AsRawFd;
    let Ok(owner) = fs::File::open(directory.join("owner")) else {
        return false;
    };
    let locked = unsafe {
        quality_core::libc::flock(
            owner.as_raw_fd(),
            quality_core::libc::LOCK_EX | quality_core::libc::LOCK_NB,
        )
    };
    locked == 0
}

/// Windows: the owner file holds an exclusive `LockFileEx` byte-range lock
/// for the process's life, so a lock that can be taken means the owner died.
#[cfg(windows)]
fn abandoned(directory: &Path) -> bool {
    let Ok(owner) = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("owner"))
    else {
        return false;
    };
    // Can we take the owner's lock? Then nobody holds it any more.
    if windows_lock(&owner, true).is_err() {
        return false;
    }
    let _ = windows_lock(&owner, false);
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
    // The refusal below tells the user to "point TMPDIR outside it", so TMPDIR
    // is honoured first: on Windows nothing else reads it, and a confined
    // process often has a writable directory that is not the system temp.
    let given = std::env::var_os("TMPDIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let resolved = quality_core::paths::resolve(&given);
    if resolved.starts_with(quality_core::paths::resolve(project)) {
        return Err(io::Error::other(
            "the temporary directory is inside the project; point TMPDIR outside it",
        ));
    }
    // A relative TMPDIR would be joined against the project, so it is resolved.
    Ok(if given.is_absolute() { given } else { resolved })
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

#[cfg(unix)]
pub fn private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
pub fn private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
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
    //! check runs in a child test process, so `TMPDIR` belongs to it alone.

    use super::{Root, temporary};
    use std::{
        env,
        ffi::OsStr,
        fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
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
            .env("TMPDIR", tmpdir)
            .stdout(Stdio::from(log.try_clone().expect("clone the child log")))
            .stderr(Stdio::from(log))
            .status()
            .expect("start the isolated test");
        if !status.success() {
            let output = fs::read_to_string(&log_path).unwrap_or_default();
            panic!("isolated check `{test}` failed ({status}):\n{output}");
        }
    }

    fn child_base() -> PathBuf {
        PathBuf::from(env::var_os(BASE).expect("the isolated test base"))
    }

    #[test]
    fn temporary_returns_the_tmpdir_junction_it_was_given() {
        if env::var_os(CHILD).as_deref()
            == Some(OsStr::new(
                "temporary_returns_the_tmpdir_junction_it_was_given",
            ))
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

        run_isolated(
            "temporary_returns_the_tmpdir_junction_it_was_given",
            &link,
            &base,
        );

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
}
