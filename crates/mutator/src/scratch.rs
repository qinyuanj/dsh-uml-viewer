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

#[cfg(not(target_os = "linux"))]
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
pub fn temporary(project: &Path) -> io::Result<PathBuf> {
    let temporary = quality_core::paths::resolve(&std::env::temp_dir());
    if temporary.starts_with(quality_core::paths::resolve(project)) {
        return Err(io::Error::other(
            "the temporary directory is inside the project; point TMPDIR outside it",
        ));
    }
    Ok(temporary)
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
