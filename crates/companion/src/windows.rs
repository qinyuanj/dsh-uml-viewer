//! Native Windows boundary (ADR 0010), compiled in place of `posix.rs` on
//! Windows (`lib.rs`), so it exposes exactly the same free functions. Every
//! child is created by `std::process::Command`, which calls `CreateProcessW`
//! with the literal MSVCRT quoting of each argv item and an explicit
//! Unicode environment block. No shell is ever involved.
//!
//! - Creation identity is the `GetProcessTimes` creation FILETIME as decimal
//!   text (100 ns ticks since 1601), so it stays the all-digit opaque string
//!   Linux and macOS record. A signalled (exited) process has no identity,
//!   just as a zombie has none on Linux.
//! - A bounded [`run`] puts its child in a fresh Job Object. On timeout it
//!   sends CTRL_BREAK to the child's process group, waits a grace period,
//!   then terminates that job only.
//! - Detached children break away from any job and inherit no handle except
//!   their own stdio, so a lock held by the parent never leaks into them.
//! - Locks are `LockFileEx` byte-range locks, released when the handle
//!   closes or the process exits.
//!
//! Kernel32/ntdll are bound through direct `extern "system"` declarations so
//! the crate stays dependency-free, as the POSIX boundary does with libc.

use std::ffi::{OsStr, OsString};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::vocab::{Failure, Owner, Result, RunOut};

#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod sys {
    use std::ffi::c_void;

    pub type HANDLE = *mut c_void;
    pub type BOOL = i32;

    pub const SYNCHRONIZE: u32 = 0x0010_0000;
    pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    pub const WAIT_OBJECT_0: u32 = 0;
    pub const INFINITE: u32 = 0xFFFF_FFFF;
    pub const CTRL_BREAK_EVENT: u32 = 1;

    pub const CREATE_SUSPENDED: u32 = 0x0000_0004;
    pub const DETACHED_PROCESS: u32 = 0x0000_0008;
    pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x1;
    pub const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x2;

    pub const ERROR_ACCESS_DENIED: i32 = 5;
    pub const ERROR_SHARING_VIOLATION: i32 = 32;
    pub const ERROR_LOCK_VIOLATION: i32 = 33;

    #[repr(C)]
    #[derive(Default)]
    pub struct FILETIME {
        pub low: u32,
        pub high: u32,
    }

    #[repr(C)]
    pub struct OVERLAPPED {
        pub internal: usize,
        pub internal_high: usize,
        pub offset: u32,
        pub offset_high: u32,
        pub event: HANDLE,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn OpenProcess(access: u32, inherit: BOOL, pid: u32) -> HANDLE;
        pub fn CloseHandle(handle: HANDLE) -> BOOL;
        pub fn WaitForSingleObject(handle: HANDLE, milliseconds: u32) -> u32;
        pub fn GetProcessTimes(
            process: HANDLE,
            creation: *mut FILETIME,
            exit: *mut FILETIME,
            kernel: *mut FILETIME,
            user: *mut FILETIME,
        ) -> BOOL;
        pub fn QueryFullProcessImageNameW(
            process: HANDLE,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> BOOL;
        pub fn GetExitCodeProcess(process: HANDLE, code: *mut u32) -> BOOL;
        pub fn QueryPerformanceCounter(count: *mut i64) -> BOOL;
        pub fn QueryPerformanceFrequency(frequency: *mut i64) -> BOOL;
        pub fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> HANDLE;
        pub fn AssignProcessToJobObject(job: HANDLE, process: HANDLE) -> BOOL;
        pub fn TerminateJobObject(job: HANDLE, code: u32) -> BOOL;
        pub fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> BOOL;
        pub fn GetConsoleCP() -> u32;
        pub fn AttachConsole(pid: u32) -> BOOL;
        pub fn FreeConsole() -> BOOL;
        pub fn LockFileEx(
            file: HANDLE,
            flags: u32,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut OVERLAPPED,
        ) -> BOOL;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        pub fn NtResumeProcess(process: HANDLE) -> i32;
    }
}

/// An owned kernel handle, closed on drop.
struct Handle(sys::HANDLE);

impl Handle {
    fn open_process(pid: u32, access: u32) -> Option<Handle> {
        // SAFETY: OpenProcess has no memory preconditions; NULL is failure.
        let handle = unsafe { sys::OpenProcess(access, 0, pid) };
        (!handle.is_null()).then_some(Handle(handle))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: self.0 is a handle this value owns exclusively.
        unsafe { sys::CloseHandle(self.0) };
    }
}

/// System-wide monotonic clock in milliseconds. `QueryPerformanceCounter`
/// is one counter for every process on the machine, so the viewer, the
/// detached controller and a restarted process compare the same values;
/// `std::time::Instant` is not comparable across processes.
pub fn monotonic_ms() -> i64 {
    let (mut count, mut frequency) = (0i64, 0i64);
    // SAFETY: both are valid out pointers; QPC cannot fail on XP or later.
    unsafe {
        sys::QueryPerformanceCounter(&mut count);
        sys::QueryPerformanceFrequency(&mut frequency);
    }
    (i128::from(count) * 1000 / i128::from(frequency.max(1))) as i64
}

pub fn sleep(ms: i64) {
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

pub fn current_pid() -> i32 {
    std::process::id() as i32
}

/// The environment a child receives: this process's variables without the
/// removed names. Windows variable names are case-insensitive, so removing
/// `PATH` also removes `Path`.
fn environment(remove: &[&str]) -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            !remove
                .iter()
                .any(|removed| removed.eq_ignore_ascii_case(&name))
        })
        .collect()
}

/// `command` with exactly `environment(remove)` plus `set`, as an explicit
/// environment block.
fn with_environment(command: &mut Command, remove: &[&str], set: &[(&str, &str)]) {
    let mut removed = remove.to_vec();
    removed.extend(set.iter().map(|(name, _)| *name));
    command.env_clear().envs(environment(&removed));
    for (name, value) in set {
        command.env(name, value);
    }
}

fn in_directory(command: &mut Command, cwd: &Path) {
    if !cwd.as_os_str().is_empty() {
        command.current_dir(cwd);
    }
}

/// The creation FILETIME (decimal) and image base name of a live process.
fn start_and_name(pid: u32) -> Option<(String, String)> {
    let process = Handle::open_process(
        pid,
        sys::PROCESS_QUERY_LIMITED_INFORMATION | sys::SYNCHRONIZE,
    )?;
    // SAFETY: process is a valid handle with SYNCHRONIZE access.
    if unsafe { sys::WaitForSingleObject(process.0, 0) } == sys::WAIT_OBJECT_0 {
        return None;
    }
    let mut times: [sys::FILETIME; 4] = Default::default();
    let [creation, exit, kernel, user] = &mut times;
    // SAFETY: four valid FILETIME out pointers; the handle has query access.
    if unsafe { sys::GetProcessTimes(process.0, creation, exit, kernel, user) } == 0 {
        return None;
    }
    let start = (u64::from(creation.high) << 32) | u64::from(creation.low);
    let mut buffer = [0u16; 32768];
    let mut size = buffer.len() as u32;
    // SAFETY: buffer holds `size` UTF-16 units; size is updated in place.
    let named =
        unsafe { sys::QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &mut size) };
    let name = if named == 0 {
        String::new()
    } else {
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit('\\').next().unwrap_or_default().to_owned()
    };
    Some((start.to_string(), name))
}

pub fn identity(pid: i32) -> Result<Option<Owner>> {
    if pid < 1 {
        return Err(Failure::new(format!("Invalid owned process PID: {pid}")));
    }
    let Some((start, name)) = start_and_name(pid as u32) else {
        return Ok(None);
    };
    Ok(Some(Owner { pid, start, name }))
}

const TERM_GRACE_MS: i64 = 2000;
const RUN_TIMEOUT_MS: i64 = 10000;

fn temp_path(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "uml-{tag}-{}-{}-{}",
        current_pid(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
        monotonic_ms()
    ))
}

fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Whether this process has a console its children can share. A detached
/// process has none (`GetConsoleCP` fails).
fn has_console() -> bool {
    // SAFETY: no preconditions.
    unsafe { sys::GetConsoleCP() != 0 }
}

/// Serializes borrowing a child's console: `AttachConsole` is process-wide.
static CONSOLE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// CTRL_BREAK to the process group led by `pid` (the child was created with
/// `CREATE_NEW_PROCESS_GROUP`). A process without a console borrows the
/// child's console for the call. `false` when the event could not be sent.
fn ctrl_break(pid: u32, shared_console: bool) -> bool {
    // SAFETY: the console calls have no memory preconditions.
    unsafe {
        if shared_console {
            return sys::GenerateConsoleCtrlEvent(sys::CTRL_BREAK_EVENT, pid) != 0;
        }
        let _guard = CONSOLE.lock().unwrap_or_else(|error| error.into_inner());
        if sys::AttachConsole(pid) == 0 {
            return false;
        }
        let sent = sys::GenerateConsoleCtrlEvent(sys::CTRL_BREAK_EVENT, pid) != 0;
        sys::FreeConsole();
        sent
    }
}

fn process_exited(process: sys::HANDLE, wait_ms: u32) -> bool {
    // SAFETY: process is a live process handle owned by the caller.
    unsafe { sys::WaitForSingleObject(process, wait_ms) == sys::WAIT_OBJECT_0 }
}

fn exit_code(process: sys::HANDLE) -> Option<i32> {
    let mut code = 0u32;
    // SAFETY: valid handle and out pointer.
    (unsafe { sys::GetExitCodeProcess(process, &mut code) } != 0).then_some(code as i32)
}

fn failed_run(err: String) -> RunOut {
    RunOut {
        exit: None,
        out: String::new(),
        err,
        timeout: false,
    }
}

/// Bounded synchronous command with captured stdio, in its own Job Object.
/// A timeout asks the child's process group to stop (CTRL_BREAK), waits the
/// grace period, then terminates this call's job and nothing else.
pub fn run(argv: &[OsString], timeout_ms: Option<i64>, env_remove: &[&str]) -> RunOut {
    let timeout_ms = timeout_ms.unwrap_or(RUN_TIMEOUT_MS);
    let Some((program, args)) = argv.split_first() else {
        return failed_run("run requires a nonempty argv".to_owned());
    };
    let out_path = temp_path("out");
    let err_path = temp_path("err");
    let cleanup = || {
        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_file(&err_path);
    };
    let (Ok(out_file), Ok(err_file)) = (
        std::fs::File::create(&out_path),
        std::fs::File::create(&err_path),
    ) else {
        cleanup();
        return failed_run("process boundary failed (output files)".to_owned());
    };
    let shared_console = has_console();
    let mut flags = sys::CREATE_SUSPENDED | sys::CREATE_NEW_PROCESS_GROUP;
    if !shared_console {
        flags |= sys::CREATE_NO_WINDOW;
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(out_file)
        .stderr(err_file)
        .creation_flags(flags);
    with_environment(&mut command, env_remove, &[]);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            cleanup();
            return failed_run(format!("process boundary failed ({error})"));
        }
    };
    let process = child.as_raw_handle() as sys::HANDLE;
    // SAFETY: CreateJobObjectW with NULL attributes and name is always valid.
    let job = Handle(unsafe { sys::CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
    // SAFETY: job and process are valid handles; the child is still
    // suspended, so nothing it starts can escape the job.
    let contained =
        !job.0.is_null() && unsafe { sys::AssignProcessToJobObject(job.0, process) } != 0;
    // SAFETY: process is our suspended child.
    let resumed = unsafe { sys::NtResumeProcess(process) } == 0;
    if !contained || !resumed {
        let _ = child.kill();
        let _ = child.wait();
        cleanup();
        return failed_run("process boundary failed (job object)".to_owned());
    }
    let timed_out = !process_exited(process, timeout_ms.clamp(0, i64::from(u32::MAX - 1)) as u32);
    if timed_out {
        if ctrl_break(child.id(), shared_console) {
            process_exited(process, TERM_GRACE_MS as u32);
        }
        // SAFETY: job is this call's own job object.
        unsafe { sys::TerminateJobObject(job.0, 1) };
        let _ = child.wait();
    }
    let code = if timed_out { None } else { exit_code(process) };
    drop(child);
    let out = read_file(&out_path);
    let err = read_file(&err_path);
    cleanup();
    RunOut {
        exit: code,
        out: out.trim_end().to_owned(),
        err: if timed_out {
            "process timed out".to_owned()
        } else {
            err.trim_end().to_owned()
        },
        timeout: timed_out,
    }
}

/// Detached project-owned process: no console, its own process group,
/// outside any job of ours, stdio to the given logs and the named variables
/// removed. Returns its exact creation identity.
pub fn spawn_detached(
    executable: &OsStr,
    argv: &[OsString],
    cwd: &Path,
    log_path: Option<&Path>,
    error_log: Option<&Path>,
    env_remove: &[&str],
) -> Result<Owner> {
    let log = |path: Option<&Path>| -> Result<Stdio> {
        Ok(match path {
            Some(path) => Stdio::from(std::fs::File::create(path)?),
            None => Stdio::null(),
        })
    };
    let stdout = log(log_path)?;
    let stderr = match (error_log, log_path) {
        (Some(path), _) => log(Some(path))?,
        (None, Some(path)) => Stdio::from(
            std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .map_err(Failure::from)?,
        ),
        (None, None) => Stdio::null(),
    };
    let flags = sys::DETACHED_PROCESS | sys::CREATE_NEW_PROCESS_GROUP | sys::CREATE_NO_WINDOW;
    let mut command = Command::new(executable);
    command
        .args(argv)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    in_directory(&mut command, cwd);
    with_environment(&mut command, env_remove, &[]);
    // Break away from a job we may run in (a launcher or terminal job), so
    // closing it does not end the detached process. A job that forbids
    // breakaway refuses the flag; then the child stays in it.
    let child = command
        .creation_flags(flags | sys::CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
        .or_else(|_| command.creation_flags(flags).spawn())
        .map_err(|error| {
            Failure::new(format!("Detached process could not be launched ({error})"))
        })?;
    identity(child.id() as i32)?
        .ok_or_else(|| Failure::new("Detached process exited before its identity was read"))
}

/// Child sharing this process's console and stdio (the Herdr pane).
pub fn spawn_native(executable: &OsStr, argv: &[OsString], cwd: &Path) -> Result<Owner> {
    let mut command = Command::new(executable);
    command.args(argv);
    in_directory(&mut command, cwd);
    with_environment(&mut command, &[], &[]);
    let child = command
        .spawn()
        .map_err(|error| Failure::new(format!("Native launch failed ({error})")))?;
    identity(child.id() as i32)?
        .ok_or_else(|| Failure::new("Native child exited before its identity was read"))
}

/// Child in this process's own console (the Herdr pane), with `env_remove`
/// unset and `env_set` set. Records its exact identity in `started_path`
/// before waiting, so the supervising controller can verify it.
pub fn start_attached(
    argv: &[OsString],
    cwd: &Path,
    env_remove: &[&str],
    env_set: &[(&str, &str)],
    started_path: &Path,
    exit_path: &Path,
) -> Result<AttachedChild> {
    let Some((program, args)) = argv.split_first() else {
        return Err(Failure::new("Runner child needs a nonempty argv"));
    };
    let mut command = Command::new(program);
    command.args(args);
    in_directory(&mut command, cwd);
    with_environment(&mut command, env_remove, env_set);
    let process = command
        .spawn()
        .map_err(|error| Failure::new(format!("Runner child could not be launched ({error})")))?;
    let observed = identity(process.id() as i32)?
        .ok_or_else(|| Failure::new("Runner child exited before its identity was read"))?;
    std::fs::write(
        started_path,
        format!(
            "{{\"pid\":{},\"start\":\"{}\",\"name\":\"{}\"}}",
            observed.pid, observed.start, observed.name
        ),
    )?;
    Ok(AttachedChild {
        pid: observed.pid,
        exit_path: exit_path.to_path_buf(),
        process,
    })
}

pub struct AttachedChild {
    pub pid: i32,
    pub exit_path: PathBuf,
    /// Held so the exit code stays readable after the child exits.
    process: std::process::Child,
}

/// Block until the attached child this process spawned exits, then publish
/// the exit code. An unreadable exit is reported as unknown
/// (`{"exit":null}`), never as a verified abnormal one.
pub fn await_exit(child: &AttachedChild, exit_path: Option<&Path>) -> Option<i32> {
    let process = child.process.as_raw_handle() as sys::HANDLE;
    let code = if process_exited(process, sys::INFINITE) {
        exit_code(process)
    } else {
        None
    };
    let path = exit_path.unwrap_or(&child.exit_path);
    let record = match code {
        Some(code) => format!("{{\"exit\":{code}}}"),
        None => "{\"exit\":null}".to_owned(),
    };
    let _ = std::fs::write(path, record);
    code
}

// --- OS-owned file locks ---------------------------------------------------

/// An exclusive `LockFileEx` lock on an open handle. Released on close or
/// process exit: the singleton and gate semantics the lifecycle needs.
pub struct Flock {
    /// Held only for ownership: closing the handle releases the lock.
    _file: std::fs::File,
}

/// The locked byte lies far past any content, so the lock file itself stays
/// readable by anyone who wants to inspect it.
const LOCK_OFFSET: u64 = 1 << 62;

pub fn try_acquire(path: &Path) -> Result<Option<Flock>> {
    // Rust opens files non-inheritable, so no child ever holds this handle.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| {
            Failure::new(format!("Cannot open lock file {}: {error}", path.display()))
        })?;
    let mut overlapped = sys::OVERLAPPED {
        internal: 0,
        internal_high: 0,
        offset: LOCK_OFFSET as u32,
        offset_high: (LOCK_OFFSET >> 32) as u32,
        event: std::ptr::null_mut(),
    };
    // SAFETY: the handle is open and synchronous; overlapped lives across
    // the call, which returns at once with FAIL_IMMEDIATELY.
    let locked = unsafe {
        sys::LockFileEx(
            file.as_raw_handle() as sys::HANDLE,
            sys::LOCKFILE_EXCLUSIVE_LOCK | sys::LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    };
    if locked == 0 {
        return Ok(None);
    }
    Ok(Some(Flock { _file: file }))
}

pub fn acquire(path: &Path) -> Result<Flock> {
    loop {
        if let Some(lock) = try_acquire(path)? {
            return Ok(lock);
        }
        sleep(25);
    }
}

pub fn release(lock: Option<Flock>) {
    drop(lock);
}

/// [`try_acquire`] retried for up to `timeout_ms`, as on POSIX.
#[cfg(test)]
pub(crate) fn acquire_within(path: &Path, timeout_ms: i64) -> Option<Flock> {
    let deadline = monotonic_ms() + timeout_ms;
    loop {
        let lock = try_acquire(path).unwrap();
        if lock.is_some() || monotonic_ms() > deadline {
            return lock;
        }
        sleep(25);
    }
}

// --- Paths, stamps and atomic replacement ---------------------------------

/// The final path of an existing file or directory, without the `\\?\`
/// verbatim prefix `std::fs::canonicalize` adds: `C:\dir\file`, and
/// `\\server\share\…` for UNC paths. Separators stay `\`, the form Herdr
/// reports pane directories in.
pub fn canonical(path: &str) -> Result<String> {
    if path.contains('\0') {
        return Err(Failure::new("invalid path"));
    }
    let resolved = std::fs::canonicalize(path)
        .map_err(|_| Failure::new(format!("cannot canonicalize path: {path}")))?;
    let text = resolved.to_string_lossy();
    Ok(if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    })
}

/// Atomic replacement on the same volume. `std::fs::rename` replaces an
/// existing target (`FileRenameInfoEx` with POSIX semantics, falling back to
/// `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`). A reader that opened the
/// target without `FILE_SHARE_DELETE` makes the replace fail with a sharing
/// or access violation for as long as it holds the file, so the replace is
/// retried for a bounded time. The destination is never deleted first.
pub mod atomic {
    use super::{Failure, Result, sys};
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// How long a replace waits out readers holding the target.
    const SHARING_RETRY_MS: i64 = 2000;

    pub fn ensure_parent(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn transient(error: &std::io::Error) -> bool {
        matches!(
            error.raw_os_error(),
            Some(
                sys::ERROR_ACCESS_DENIED | sys::ERROR_SHARING_VIOLATION | sys::ERROR_LOCK_VIOLATION
            )
        )
    }

    fn replace(temporary: &Path, path: &Path) -> std::io::Result<()> {
        let deadline = super::monotonic_ms() + SHARING_RETRY_MS;
        loop {
            match std::fs::rename(temporary, path) {
                Err(error) if transient(&error) && super::monotonic_ms() < deadline => {
                    super::sleep(10);
                }
                outcome => return outcome,
            }
        }
    }

    pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        ensure_parent(path)?;
        let temporary = path.with_file_name(format!(
            ".{}.{}.{}.tmp",
            path.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let outcome = (|| -> Result<()> {
            std::fs::write(&temporary, bytes)?;
            replace(&temporary, path).map_err(|error| {
                Failure::new(format!(
                    "atomic replace of {} failed: {error}",
                    path.display()
                ))
            })
        })();
        if outcome.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Json;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// The Python on PATH (a declared prerequisite), never the Store alias.
    fn python() -> OsString {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|directory| directory.join("python.exe"))
            .find(|candidate| {
                candidate.is_file() && !candidate.to_string_lossy().contains("WindowsApps")
            })
            .expect("python.exe on PATH")
            .into_os_string()
    }

    fn python_argv(code: &str) -> Vec<OsString> {
        vec![python(), OsString::from("-c"), OsString::from(code)]
    }

    fn terminate(pid: i32) {
        const PROCESS_TERMINATE: u32 = 0x0001;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn TerminateProcess(process: sys::HANDLE, code: u32) -> sys::BOOL;
        }
        if let Some(process) = Handle::open_process(pid as u32, PROCESS_TERMINATE) {
            // SAFETY: a valid handle with PROCESS_TERMINATE access.
            unsafe { TerminateProcess(process.0, 1) };
        }
    }

    fn wait_for_text(path: &Path) -> String {
        let deadline = monotonic_ms() + 10000;
        while read_file(path).is_empty() && monotonic_ms() < deadline {
            sleep(20);
        }
        read_file(path)
    }

    #[test]
    fn run_passes_argv_literally_including_spaces_unicode_and_quotes() {
        let words = [
            "帶 空格",
            "quote\"inside",
            "C:\\dir with space\\",
            "back\\\\\"slash",
            "",
            "半角 ; & | > < ^ %PATH%",
            "$(touch injected)",
        ];
        let mut argv = python_argv("import json,sys;print(json.dumps(sys.argv[1:]))");
        argv.extend(words.iter().map(OsString::from));
        let result = run(&argv, None, &[]);
        assert_eq!(Some(0), result.exit, "{}", result.err);
        let decoded = crate::json::decode(&result.out).unwrap();
        let items = decoded.array_items().unwrap();
        let received: Vec<&str> = items.iter().filter_map(Json::as_str).collect();
        assert_eq!(words.to_vec(), received);
    }

    #[test]
    fn run_reports_the_real_exit_code_and_both_streams() {
        let result = run(
            &python_argv("import sys;sys.stdout.write('out');sys.stderr.write('err');sys.exit(23)"),
            None,
            &[],
        );
        assert_eq!(Some(23), result.exit);
        assert_eq!("out", result.out);
        assert_eq!("err", result.err);
    }

    #[test]
    fn run_reports_a_missing_program_without_an_exit_code() {
        let result = run(&args(&["C:\\definitely\\missing-uml.exe"]), None, &[]);
        assert_eq!(None, result.exit);
        assert!(!result.timeout);
        assert!(
            result.err.starts_with("process boundary failed"),
            "{}",
            result.err
        );
    }

    #[test]
    fn run_times_out_a_bounded_command_and_reports_it() {
        let started = monotonic_ms();
        let result = run(&python_argv("import time;time.sleep(30)"), Some(300), &[]);
        assert!(result.timeout);
        assert_eq!(None, result.exit);
        assert!(monotonic_ms() - started < 10000);
    }

    #[test]
    fn a_timed_out_command_is_asked_to_stop_with_ctrl_break_so_it_can_release_resources() {
        let marker = temp_path("marker");
        let script = "import signal,sys,time\n\
             def stop(*_):\n    open(sys.argv[1],'w').write('released')\n    sys.exit(0)\n\
             signal.signal(signal.SIGBREAK, stop)\n\
             print('ready', flush=True)\n\
             while True: time.sleep(0.05)";
        let mut argv = python_argv(script);
        argv.push(marker.clone().into_os_string());
        let result = run(&argv, Some(1500), &[]);
        assert!(result.timeout);
        let text = read_file(&marker);
        let _ = std::fs::remove_file(&marker);
        assert_eq!("released", text);
    }

    #[test]
    fn a_timeout_ends_the_whole_job_of_that_call_and_nothing_else() {
        let grandchild_pid = temp_path("grandchild");
        let bystander = std::process::Command::new(python())
            .args(["-c", "import time;time.sleep(30)"])
            .spawn()
            .unwrap();
        // The child starts a grandchild that ignores CTRL_BREAK, records its
        // pid, then hangs.
        let script = "import subprocess,sys,time\n\
             g = subprocess.Popen([sys.executable,'-c','import signal,time;signal.signal(signal.SIGBREAK, signal.SIG_IGN);time.sleep(30)'])\n\
             open(sys.argv[1],'w').write(str(g.pid))\n\
             time.sleep(30)";
        let mut argv = python_argv(script);
        argv.push(grandchild_pid.clone().into_os_string());
        let result = run(&argv, Some(2000), &[]);
        assert!(result.timeout);
        let pid: i32 = read_file(&grandchild_pid).parse().expect("grandchild pid");
        let _ = std::fs::remove_file(&grandchild_pid);
        let deadline = monotonic_ms() + 5000;
        while identity(pid).unwrap().is_some() && monotonic_ms() < deadline {
            sleep(20);
        }
        assert_eq!(None, identity(pid).unwrap(), "grandchild survived");
        let alive = identity(bystander.id() as i32).unwrap();
        terminate(bystander.id() as i32);
        assert!(alive.is_some(), "a process outside the job was ended");
    }

    #[test]
    fn identity_is_decimal_filetime_shared_by_every_observer() {
        let own = identity(current_pid()).unwrap().unwrap();
        assert_eq!(current_pid(), own.pid);
        assert!(
            own.start.bytes().all(|byte| byte.is_ascii_digit()),
            "{}",
            own.start
        );
        // 100 ns ticks since 1601-01-01: this process started within the day.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let filetime_now = (now.as_secs() + 1 + 11_644_473_600) * 10_000_000;
        let start: u64 = own.start.parse().unwrap();
        assert!(start <= filetime_now && filetime_now - start < 864_000_000_000);
        assert!(own.name.ends_with(".exe"), "{}", own.name);
        assert_eq!(own, identity(own.pid).unwrap().unwrap());
    }

    #[test]
    fn identity_is_none_once_the_process_has_exited_even_while_its_handle_is_open() {
        let mut child = std::process::Command::new(python())
            .args(["-c", "import time;time.sleep(0.5)"])
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let alive = identity(pid).unwrap().expect("live child");
        assert_eq!("python.exe", alive.name);
        child.wait().unwrap();
        // `child` still holds the process handle, so the pid is not reused.
        assert_eq!(None, identity(pid).unwrap());
    }

    #[test]
    fn identity_is_none_for_a_pid_that_is_not_running() {
        assert_eq!(None, identity(i32::MAX - 3).unwrap());
    }

    #[test]
    fn an_invalid_pid_is_rejected_instead_of_guessed() {
        assert!(identity(0).is_err());
        assert!(identity(-5).is_err());
    }

    #[test]
    fn a_detached_child_never_inherits_a_lock_its_parent_later_releases() {
        let lock_path = temp_path("lock");
        let log = temp_path("log");
        let held = try_acquire(&lock_path).unwrap().expect("lock");
        let owner = spawn_detached(
            &python(),
            &args(&["-c", "import time;time.sleep(5)"]),
            &std::env::temp_dir(),
            Some(&log),
            Some(&log.with_extension("err")),
            &[],
        )
        .unwrap();
        release(Some(held));
        let again = try_acquire(&lock_path).unwrap();
        let free = again.is_some();
        release(again);
        terminate(owner.pid);
        let _ = std::fs::remove_file(&lock_path);
        assert!(free, "the detached child kept the released lock");
    }

    #[test]
    fn spawn_detached_reports_the_exact_creation_identity() {
        let log = temp_path("detached");
        let owner = spawn_detached(
            &python(),
            &args(&["-c", "import time;time.sleep(5)"]),
            &std::env::temp_dir(),
            Some(&log),
            Some(&log.with_extension("err")),
            &[],
        )
        .unwrap();
        let observed = identity(owner.pid).unwrap();
        terminate(owner.pid);
        assert_eq!(Some(owner), observed);
    }

    #[test]
    fn run_removes_only_the_named_variables_case_insensitively() {
        let code = "import os;print(os.environ.get('USERPROFILE','unset'), 'PATH' in os.environ)";
        let present = run(&python_argv(code), Some(5000), &[]);
        let profile = std::env::var("USERPROFILE").unwrap();
        assert_eq!(format!("{profile} True"), present.out);
        let removed = run(&python_argv(code), Some(5000), &["userprofile"]);
        assert_eq!("unset True", removed.out);
    }

    #[test]
    fn spawn_detached_removes_only_the_named_variables_and_runs_in_cwd() {
        let log = temp_path("detached-env");
        let directory = temp_path("detached cwd 项目");
        std::fs::create_dir_all(&directory).unwrap();
        spawn_detached(
            &python(),
            &args(&[
                "-c",
                "import os,time;print(os.environ.get('USERPROFILE','unset'), 'PATH' in os.environ, os.path.basename(os.getcwd()), flush=True);time.sleep(0.3)",
            ]),
            &directory,
            Some(&log),
            Some(&log.with_extension("err")),
            &["UserProfile"],
        )
        .unwrap();
        let text = wait_for_text(&log);
        let _ = std::fs::remove_dir(&directory);
        assert_eq!(
            format!(
                "unset True {}",
                directory.file_name().unwrap().to_string_lossy()
            ),
            text.trim_end()
        );
    }

    #[test]
    fn an_attached_child_records_its_identity_and_actual_exit() {
        let started = temp_path("started");
        let exit = temp_path("exit");
        let child = start_attached(
            &python_argv("import os,sys,time;time.sleep(0.2);sys.exit(int(os.environ['UML_X']))"),
            &std::env::temp_dir(),
            &[],
            &[("UML_X", "37")],
            &started,
            &exit,
        )
        .unwrap();
        let record = crate::json::decode(&std::fs::read_to_string(&started).unwrap()).unwrap();
        assert_eq!(
            Some(child.pid as i64),
            record.get("pid").and_then(Json::as_i64)
        );
        assert_eq!(
            Some("python.exe"),
            record.get("name").and_then(Json::as_str)
        );
        let start = record.get("start").and_then(Json::as_str).unwrap();
        assert!(start.bytes().all(|byte| byte.is_ascii_digit()), "{start}");
        assert_eq!(Some(37), await_exit(&child, None));
        let exit_record = crate::json::decode(&std::fs::read_to_string(&exit).unwrap()).unwrap();
        assert_eq!(Some(37), exit_record.get("exit").and_then(Json::as_i64));
        let _ = std::fs::remove_file(&started);
        let _ = std::fs::remove_file(&exit);
    }

    #[test]
    fn lock_singleton_blocks_a_second_holder_until_released() {
        let path = temp_path("flock");
        let held = try_acquire(&path).unwrap().expect("first lock");
        assert!(try_acquire(&path).unwrap().is_none());
        // The lock does not stop anyone reading the file.
        assert_eq!("", std::fs::read_to_string(&path).unwrap());
        release(Some(held));
        assert!(try_acquire(&path).unwrap().is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_lock_is_released_when_its_holder_process_exits() {
        let path = temp_path("flock-process");
        let ready = temp_path("flock-ready");
        // The child holds an exclusive lock on the same byte through
        // msvcrt.locking at our offset, then exits without unlocking.
        let script = "import sys,time,ctypes,ctypes.wintypes as w\n\
             k=ctypes.WinDLL('kernel32',use_last_error=True)\n\
             import msvcrt\n\
             f=open(sys.argv[1],'a+b')\n\
             class O(ctypes.Structure):\n    _fields_=[('a',ctypes.c_size_t),('b',ctypes.c_size_t),('lo',w.DWORD),('hi',w.DWORD),('e',w.HANDLE)]\n\
             o=O(0,0,0,1<<30,None)\n\
             assert k.LockFileEx(w.HANDLE(msvcrt.get_osfhandle(f.fileno())),3,0,1,0,ctypes.byref(o))\n\
             open(sys.argv[2],'w').write('held')\n\
             time.sleep(0.5)";
        let mut command = std::process::Command::new(python());
        command.args(["-c", script]).arg(&path).arg(&ready);
        let mut child = command.spawn().unwrap();
        assert_eq!("held", wait_for_text(&ready));
        assert!(try_acquire(&path).unwrap().is_none(), "the child's lock");
        child.wait().unwrap();
        assert!(try_acquire(&path).unwrap().is_some());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&ready);
    }

    #[test]
    fn atomic_replacement_keeps_an_unrelated_adjacent_backup() {
        let path = temp_path("atomic");
        let backup = path.with_extension("fixture-backup");
        std::fs::write(&path, "original").unwrap();
        std::fs::write(&backup, "unrelated").unwrap();
        atomic::write(&path, b"replacement").unwrap();
        assert_eq!("replacement", std::fs::read_to_string(&path).unwrap());
        assert_eq!("unrelated", std::fs::read_to_string(&backup).unwrap());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup);
    }

    #[test]
    fn atomic_replacement_waits_out_a_reader_that_does_not_share_delete() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        let path = temp_path("atomic-shared");
        std::fs::write(&path, "original").unwrap();
        let reader = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        let holder = std::thread::spawn(move || {
            sleep(300);
            drop(reader);
        });
        atomic::write(&path, b"replacement").unwrap();
        holder.join().unwrap();
        assert_eq!("replacement", std::fs::read_to_string(&path).unwrap());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn canonical_resolves_the_real_path_without_the_verbatim_prefix() {
        let directory = temp_path("canonical 项目");
        std::fs::create_dir_all(&directory).unwrap();
        let resolved = canonical(directory.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_dir(&directory);
        assert!(!resolved.starts_with(r"\\?\"), "{resolved}");
        let bytes = resolved.as_bytes();
        assert!(
            bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":\\",
            "{resolved}"
        );
        assert!(resolved.ends_with("canonical 项目") || resolved.contains("canonical 项目"));
        assert!(canonical(r"C:\definitely\missing-uml-path").is_err());
    }

    #[test]
    fn the_monotonic_clock_advances_with_sleep() {
        let before = monotonic_ms();
        sleep(50);
        let elapsed = monotonic_ms() - before;
        assert!((50..5000).contains(&elapsed), "{elapsed}");
    }
}
