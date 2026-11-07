//! The companion host: every process, clock, file and environment capability
//! the companion roles use, wrapped exactly once. Two constructors only:
//! [`Host::native`], the production adapter over this target's OS boundary
//! (POSIX on Linux and macOS, Windows on Windows; ADR 0007, ADR 0010), and (in
//! tests) `Host::fake`, whose every capability fails loudly with its name
//! until a test declares it. A fake never falls back to the real OS.
//!
//! Each role entry assembles one host and passes it down; the Herdr backend
//! hands its own host to every recovery call.

use crate::posix;
use crate::vocab::{Failure, Owner, Result, RunOut};
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

type ClockFn = Rc<dyn Fn() -> i64>;
type SleepFn = Rc<dyn Fn(i64)>;
/// Process identity (`/proc` starttime and name); `None` for an absent,
/// zombie or out-of-range pid.
type OwnerFn = Rc<dyn Fn(i64) -> Option<Owner>>;
/// A bounded synchronous command with an explicit argv (no shell).
type RunFn = Rc<dyn Fn(&[String], Option<i64>) -> RunOut>;
/// A detached child; `Ok(None)` is a child that exited at once.
type SpawnFn = Rc<dyn Fn(&Detached) -> Result<Option<Owner>>>;
/// `realpath(3)`, or the path itself when it cannot be resolved.
type CanonicalFn = Rc<dyn Fn(&str) -> String>;
/// Names of the variables this process would pass to a child.
type VariablesFn = Rc<dyn Fn() -> Vec<String>>;
/// One environment variable; `None` when unset or not Unicode.
type VariableFn = Rc<dyn Fn(&str) -> Option<String>>;
/// Raw file text. `Ok(None)` is an absent file; `Err` is a read failure.
type ReadFn = Rc<dyn Fn(&Path) -> Result<Option<String>>>;
/// Atomic replacement (write a sibling, then `rename`), creating the parent.
type WriteFn = Rc<dyn Fn(&Path, &[u8]) -> Result<()>>;
/// Best-effort removal; an absent file is not an error.
type RemoveFn = Rc<dyn Fn(&Path)>;
/// The file names in a directory; empty when it cannot be read.
type ListFn = Rc<dyn Fn(&Path) -> Vec<String>>;
/// Whether a file can be opened for reading (an executable probe).
type OpenableFn = Rc<dyn Fn(&Path) -> bool>;
/// A fresh generation token (UUIDv4 shape).
type TokenFn = Rc<dyn Fn() -> String>;

/// A detached spawn request: the executable, its argv after the executable,
/// the working directory, the stdout/stderr logs and the inherited variables
/// to remove.
pub struct Detached<'a> {
    pub executable: &'a str,
    pub argv: &'a [String],
    pub cwd: &'a str,
    pub stdout: Option<&'a str>,
    pub stderr: Option<&'a str>,
    pub remove: &'a [String],
}

#[derive(Clone)]
pub struct Host {
    pub clock: ClockFn,
    pub sleep: SleepFn,
    pub owner: OwnerFn,
    pub run: RunFn,
    pub spawn_detached: SpawnFn,
    pub canonical: CanonicalFn,
    pub variables: VariablesFn,
    pub variable: VariableFn,
    pub read: ReadFn,
    pub write: WriteFn,
    pub remove: RemoveFn,
    pub list: ListFn,
    pub openable: OpenableFn,
    pub token: TokenFn,
}

fn read(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|error| Failure::new(format!("{error}")))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    posix::atomic::ensure_parent(path)?;
    posix::atomic::write(path, bytes)
}

fn spawn_detached(request: &Detached) -> Result<Option<Owner>> {
    let argv: Vec<std::ffi::OsString> = request.argv.iter().map(std::ffi::OsString::from).collect();
    let remove: Vec<&str> = request.remove.iter().map(String::as_str).collect();
    posix::spawn_detached(
        std::ffi::OsStr::new(request.executable),
        &argv,
        Path::new(request.cwd),
        request.stdout.map(Path::new),
        request.stderr.map(Path::new),
        &remove,
    )
    .map(Some)
}

impl Host {
    /// The production adapter: the real OS boundary of this target.
    pub fn native() -> Host {
        Host {
            clock: Rc::new(posix::monotonic_ms),
            sleep: Rc::new(posix::sleep),
            owner: Rc::new(|pid| {
                if pid < 1 || pid > i32::MAX as i64 {
                    return None;
                }
                posix::identity(pid as i32).ok().flatten()
            }),
            run: Rc::new(|argv, timeout| {
                let argv: Vec<std::ffi::OsString> =
                    argv.iter().map(std::ffi::OsString::from).collect();
                posix::run(&argv, timeout, &[])
            }),
            spawn_detached: Rc::new(spawn_detached),
            canonical: Rc::new(|path| posix::canonical(path).unwrap_or_else(|_| path.to_owned())),
            variables: Rc::new(|| {
                std::env::vars_os()
                    .map(|(name, _)| name.to_string_lossy().into_owned())
                    .collect()
            }),
            variable: Rc::new(|name| std::env::var(name).ok()),
            read: Rc::new(read),
            write: Rc::new(write),
            remove: Rc::new(|path| {
                let _ = std::fs::remove_file(path);
            }),
            list: Rc::new(|directory| {
                std::fs::read_dir(directory)
                    .map(|entries| {
                        entries
                            .filter_map(|entry| entry.ok())
                            .map(|entry| entry.file_name().to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default()
            }),
            openable: Rc::new(|path| std::fs::File::open(path).is_ok()),
            token: Rc::new(new_token),
        }
    }
}

/// Unique generation tokens (UUIDv4 shape). Uniqueness comes from the
/// monotonic clock, the pid and a process-local counter, not from
/// cryptography — tokens identify one generation, nothing more.
pub fn new_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seed = posix::monotonic_ms() as u64
        ^ (posix::current_pid() as u64) << 32
        ^ COUNTER.fetch_add(0x9e3779b9, Ordering::Relaxed);
    let mut state = seed ^ 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32
    };
    let mut text = String::with_capacity(36);
    for (index, c) in "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".chars().enumerate() {
        let _ = index;
        match c {
            'x' => text.push_str(&format!("{:x}", next() % 16)),
            'y' => text.push_str(&format!("{:x}", next() % 4 + 8)),
            other => text.push(other),
        }
    }
    text
}

#[cfg(test)]
impl Host {
    /// A host with no declared capability: each one panics with its name.
    /// Tests declare exactly what the code under test may touch.
    pub fn fake() -> Host {
        fn undeclared(capability: &str) -> ! {
            panic!("fake companion host: undeclared capability `{capability}`")
        }
        Host {
            clock: Rc::new(|| undeclared("clock")),
            sleep: Rc::new(|_| undeclared("sleep")),
            owner: Rc::new(|_| undeclared("owner")),
            run: Rc::new(|_, _| undeclared("run")),
            spawn_detached: Rc::new(|_| undeclared("spawn_detached")),
            canonical: Rc::new(|_| undeclared("canonical")),
            variables: Rc::new(|| undeclared("variables")),
            variable: Rc::new(|_| undeclared("variable")),
            read: Rc::new(|_| undeclared("read")),
            write: Rc::new(|_, _| undeclared("write")),
            remove: Rc::new(|_| undeclared("remove")),
            list: Rc::new(|_| undeclared("list")),
            openable: Rc::new(|_| undeclared("openable")),
            token: Rc::new(|| undeclared("token")),
        }
    }
}

#[cfg(test)]
pub(crate) mod memory {
    //! An in-memory file store for the fake host: path → text, so tests
    //! write and read project files without touching the disk.

    use super::Host;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    #[derive(Clone, Default)]
    pub struct Files {
        pub files: Rc<RefCell<HashMap<PathBuf, String>>>,
    }

    impl Files {
        pub fn new() -> Files {
            Files::default()
        }

        pub fn put(&self, path: impl AsRef<Path>, text: &str) {
            self.files
                .borrow_mut()
                .insert(path.as_ref().to_path_buf(), text.to_owned());
        }

        pub fn get(&self, path: impl AsRef<Path>) -> Option<String> {
            self.files.borrow().get(path.as_ref()).cloned()
        }

        /// `host` with read, write and remove declared over this store.
        pub fn serve(&self, host: Host) -> Host {
            let read = self.files.clone();
            let write = self.files.clone();
            let remove = self.files.clone();
            Host {
                read: Rc::new(move |path| Ok(read.borrow().get(path).cloned())),
                write: Rc::new(move |path, bytes| {
                    write.borrow_mut().insert(
                        path.to_path_buf(),
                        String::from_utf8_lossy(bytes).into_owned(),
                    );
                    Ok(())
                }),
                remove: Rc::new(move |path| {
                    remove.borrow_mut().remove(path);
                }),
                ..host
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "undeclared capability `variable`")]
    fn a_fake_host_names_the_capability_a_test_forgot_to_declare() {
        let host = Host {
            clock: Rc::new(|| 0),
            ..Host::fake()
        };
        assert_eq!(0, (host.clock)());
        (host.variable)("HOME");
    }

    #[test]
    fn tokens_are_unique_and_uuid_shaped() {
        let a = new_token();
        let b = new_token();
        assert_ne!(a, b);
        assert_eq!(36, a.len());
        assert_eq!(Some('4'), a.chars().nth(14));
    }
}
