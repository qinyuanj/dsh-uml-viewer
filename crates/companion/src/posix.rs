//! POSIX boundary (port of `love/uml/adapters/posix_process.lua`,
//! `posix_lock.lua`, `posix_path.lua`, `atomic_file.lua`,
//! `clock.lua`, `platform.lua`). argv is passed literally to `posix_spawn`:
//! no shell is ever involved. Every spawn passes an explicit environment
//! built from `/proc/self/environ` — `envp = NULL` under glibc is not
//! "inherit", it is an empty environment (ADR 0007). Creation identity is the
//! kernel's starttime from `/proc/<pid>/stat`, compared as opaque text.
//!
//! libc is bound through direct `extern "C"` declarations so the workspace
//! stays dependency-free; each function is the same symbol the LuaJIT FFI
//! adapter called.

use std::ffi::{CString, OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::vocab::{Failure, Owner, Result, RunOut};

#[allow(non_camel_case_types)]
mod sys {
    pub type pid_t = i32;
    pub const WNOHANG: i32 = 1;
    pub const SIGTERM: i32 = 15;
    pub const O_WRONLY: i32 = 1;
    pub const LOCK_EX: i32 = 2;
    pub const LOCK_NB: i32 = 4;

    // The values that differ between the two hosts' headers.
    #[cfg(target_os = "linux")]
    mod host {
        pub const CLOCK_MONOTONIC: i32 = 1;
        pub const O_CREAT: i32 = 0x40;
        pub const O_TRUNC: i32 = 0x200;
        pub const POSIX_SPAWN_SETSID: i16 = 0x80;
    }
    #[cfg(target_os = "macos")]
    mod host {
        pub const CLOCK_MONOTONIC: i32 = 6;
        pub const O_CREAT: i32 = 0x200;
        pub const O_TRUNC: i32 = 0x400;
        pub const POSIX_SPAWN_SETSID: i16 = 0x400;
    }
    pub use host::*;

    /// `struct proc_bsdinfo` from `<sys/proc_info.h>` (macOS only).
    #[cfg(target_os = "macos")]
    #[repr(C)]
    pub struct ProcBsdInfo {
        pub pbi_flags: u32,
        pub pbi_status: u32,
        pub pbi_xstatus: u32,
        pub pbi_pid: u32,
        pub pbi_ppid: u32,
        pub pbi_uid: u32,
        pub pbi_gid: u32,
        pub pbi_ruid: u32,
        pub pbi_rgid: u32,
        pub pbi_svuid: u32,
        pub pbi_svgid: u32,
        pub rfu_1: u32,
        pub pbi_comm: [u8; 16],
        pub pbi_name: [u8; 32],
        pub pbi_nfiles: u32,
        pub pbi_pgid: u32,
        pub pbi_pjobc: u32,
        pub e_tdev: u32,
        pub e_tpgid: u32,
        pub pbi_nice: i32,
        pub pbi_start_tvsec: u64,
        pub pbi_start_tvusec: u64,
    }
    #[cfg(target_os = "macos")]
    pub const PROC_PIDTBSDINFO: i32 = 3;
    #[cfg(target_os = "macos")]
    pub const SZOMB: u32 = 5;
    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        pub fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut std::ffi::c_void,
            size: i32,
        ) -> i32;
    }

    #[repr(C)]
    pub struct Timespec {
        pub tv_sec: i64,
        pub tv_nsec: i64,
    }

    // Opaque libc structs: glibc owns the storage, we never inspect it.
    pub const FILE_ACTIONS_BYTES: usize = 1024;
    pub const ATTR_BYTES: usize = 1024;

    unsafe extern "C" {
        pub fn posix_spawn_file_actions_init(actions: *mut u8) -> i32;
        pub fn posix_spawn_file_actions_addopen(
            actions: *mut u8,
            fd: i32,
            path: *const std::ffi::c_char,
            oflag: i32,
            mode: u32,
        ) -> i32;
        pub fn posix_spawn_file_actions_addchdir_np(
            actions: *mut u8,
            path: *const std::ffi::c_char,
        ) -> i32;
        pub fn posix_spawn_file_actions_destroy(actions: *mut u8) -> i32;
        pub fn posix_spawnattr_init(attr: *mut u8) -> i32;
        pub fn posix_spawnattr_setflags(attr: *mut u8, flags: i16) -> i32;
        pub fn posix_spawn(
            pid: *mut pid_t,
            path: *const std::ffi::c_char,
            actions: *const u8,
            attr: *const u8,
            argv: *const *const std::ffi::c_char,
            envp: *const *const std::ffi::c_char,
        ) -> i32;
        pub fn waitpid(pid: pid_t, status: *mut i32, options: i32) -> pid_t;
        pub fn kill(pid: pid_t, sig: i32) -> i32;
        pub fn getpid() -> pid_t;
        pub fn nanosleep(req: *const Timespec, rem: *mut Timespec) -> i32;
        pub fn clock_gettime(clockid: i32, tp: *mut Timespec) -> i32;
        pub fn flock(fd: i32, operation: i32) -> i32;
    }
}

fn cstring(text: &OsStr) -> Result<CString> {
    CString::new(text.as_bytes()).map_err(|_| Failure::new("argv contains a NUL byte"))
}

/// System-wide monotonic clock in milliseconds (CLOCK_MONOTONIC). Shared
/// across the viewer, the detached controller and restarted processes; never
/// a per-process counter and never wall time.
pub fn monotonic_ms() -> i64 {
    let mut ts = sys::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts is a valid out buffer; clock_gettime(CLOCK_MONOTONIC) cannot
    // fail on Linux.
    unsafe { sys::clock_gettime(sys::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000
}

pub fn sleep(ms: i64) {
    if ms <= 0 {
        return;
    }
    let request = sys::Timespec {
        tv_sec: ms / 1000,
        tv_nsec: (ms % 1000) * 1_000_000,
    };
    // SAFETY: request is a valid in buffer; a NULL remainder is permitted.
    unsafe { sys::nanosleep(&request, std::ptr::null_mut()) };
}

pub fn current_pid() -> i32 {
    // SAFETY: getpid cannot fail.
    unsafe { sys::getpid() }
}

/// `NAME=value` entries of this process's environment, NUL-separated.
#[cfg(target_os = "linux")]
fn environ_bytes() -> Vec<u8> {
    std::fs::read("/proc/self/environ").unwrap_or_default()
}

/// macOS has no `/proc`; the std snapshot carries the same literal bytes.
#[cfg(target_os = "macos")]
fn environ_bytes() -> Vec<u8> {
    let mut raw = Vec::new();
    for (name, value) in std::env::vars_os() {
        raw.extend_from_slice(name.as_bytes());
        raw.push(b'=');
        raw.extend_from_slice(value.as_bytes());
        raw.push(0);
    }
    raw
}

/// Environment snapshot without the removed names, kept as literal
/// `NAME=value` bytes (`/proc/self/environ` on Linux).
fn environment(remove: &[&str]) -> Result<Vec<CString>> {
    let raw = environ_bytes();
    let mut out = Vec::new();
    for entry in raw.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let name_bytes = entry.split(|byte| *byte == b'=').next().unwrap_or(entry);
        let name = String::from_utf8_lossy(name_bytes);
        if remove
            .iter()
            .any(|removed| removed.as_bytes() == name_bytes || *removed == name)
        {
            continue;
        }
        out.push(CString::new(entry).map_err(|_| Failure::new("environment contains NUL"))?);
    }
    Ok(out)
}

/// `/proc/<pid>/stat`: field 2 (comm) may contain spaces and parens, so parse
/// from the last `)`. A zombie (state Z or X) is not a running process: an
/// exited child keeps its /proc entry until its parent reaps it.
#[cfg(target_os = "linux")]
fn stat_start(pid: i32) -> Option<String> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit(')').next()?.trim_start();
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // fields[0] is field 3 (state); starttime is field 22, so index 19 here.
    if fields
        .first()
        .is_some_and(|state| *state == "Z" || *state == "X")
    {
        return None;
    }
    fields.get(19).map(|start| (*start).to_owned())
}

#[cfg(target_os = "linux")]
fn start_and_name(pid: i32) -> Option<(String, String)> {
    let start = stat_start(pid)?;
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|text| text.trim_end().to_owned())
        .unwrap_or_default();
    Some((start, name))
}

/// macOS: `proc_pidinfo(PROC_PIDTBSDINFO)`. The start is the kernel's
/// creation time in microseconds since the epoch, as decimal text, so it
/// stays the same opaque digit string Linux records. A zombie is not running.
#[cfg(target_os = "macos")]
fn start_and_name(pid: i32) -> Option<(String, String)> {
    let mut info = std::mem::MaybeUninit::<sys::ProcBsdInfo>::zeroed();
    let size = std::mem::size_of::<sys::ProcBsdInfo>() as i32;
    // SAFETY: info is a writable buffer of exactly `size` bytes.
    let written = unsafe {
        sys::proc_pidinfo(
            pid,
            sys::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the kernel filled the whole struct.
    let info = unsafe { info.assume_init() };
    if info.pbi_status == sys::SZOMB {
        return None;
    }
    let start = info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec;
    let comm = info.pbi_comm.split(|byte| *byte == 0).next().unwrap_or(&[]);
    Some((
        start.to_string(),
        String::from_utf8_lossy(comm).into_owned(),
    ))
}

pub fn identity(pid: i32) -> Result<Option<Owner>> {
    if pid < 1 {
        return Err(Failure::new(format!("Invalid owned process PID: {pid}")));
    }
    let Some((start, name)) = start_and_name(pid) else {
        return Ok(None);
    };
    Ok(Some(Owner { pid, start, name }))
}

const TERM_GRACE_MS: i64 = 2000;
const RUN_TIMEOUT_MS: i64 = 10000;

fn temp_path(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let base = std::env::var_os("TMPDIR").unwrap_or_else(|| OsString::from("/tmp"));
    let ticks = monotonic_ms();
    PathBuf::from(base).join(format!(
        "uml-{tag}-{}-{}-{ticks}",
        current_pid(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

struct FileActions {
    storage: [u8; sys::FILE_ACTIONS_BYTES],
}

impl FileActions {
    fn new() -> Result<FileActions> {
        let mut actions = FileActions {
            storage: [0; sys::FILE_ACTIONS_BYTES],
        };
        // SAFETY: storage is a valid opaque buffer of the documented size.
        let rc = unsafe { sys::posix_spawn_file_actions_init(actions.storage.as_mut_ptr()) };
        if rc != 0 {
            return Err(Failure::new("posix_spawn_file_actions_init failed"));
        }
        Ok(actions)
    }

    fn add_open(&mut self, fd: i32, path: &Path, oflag: i32, mode: u32) -> Result<()> {
        let path = cstring(path.as_os_str())?;
        // SAFETY: actions was initialised; path outlives the call.
        let rc = unsafe {
            sys::posix_spawn_file_actions_addopen(
                self.storage.as_mut_ptr(),
                fd,
                path.as_ptr(),
                oflag,
                mode,
            )
        };
        if rc != 0 {
            return Err(Failure::new(format!(
                "posix_spawn_file_actions_addopen failed (error {rc})"
            )));
        }
        Ok(())
    }

    fn add_chdir(&mut self, cwd: &Path) -> Result<()> {
        let path = cstring(cwd.as_os_str())?;
        // SAFETY: actions was initialised; path outlives the call.
        let rc = unsafe {
            sys::posix_spawn_file_actions_addchdir_np(self.storage.as_mut_ptr(), path.as_ptr())
        };
        if rc != 0 {
            return Err(Failure::new(format!(
                "Detached process could not enter {} (error {rc})",
                cwd.display()
            )));
        }
        Ok(())
    }
}

impl Drop for FileActions {
    fn drop(&mut self) {
        // SAFETY: storage was initialised in new() before any use.
        unsafe { sys::posix_spawn_file_actions_destroy(self.storage.as_mut_ptr()) };
    }
}

struct SpawnArgs {
    _argv_text: Vec<CString>,
    _env_text: Vec<CString>,
    argv: Vec<*const std::ffi::c_char>,
    envp: Vec<*const std::ffi::c_char>,
}

impl SpawnArgs {
    fn new(argv: &[OsString], remove: &[&str]) -> Result<SpawnArgs> {
        SpawnArgs::with_env(argv, remove, &[])
    }

    /// Like [`SpawnArgs::new`], with `set` names replaced by their values.
    fn with_env(argv: &[OsString], remove: &[&str], set: &[(&str, &str)]) -> Result<SpawnArgs> {
        let argv_text: Vec<CString> = argv.iter().map(|arg| cstring(arg)).collect::<Result<_>>()?;
        let mut removed = remove.to_vec();
        removed.extend(set.iter().map(|(name, _)| *name));
        let mut env_text = environment(&removed)?;
        for (name, value) in set {
            env_text.push(
                CString::new(format!("{name}={value}"))
                    .map_err(|_| Failure::new("environment contains NUL"))?,
            );
        }
        let mut argv_ptrs: Vec<*const std::ffi::c_char> =
            argv_text.iter().map(|arg| arg.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());
        let mut envp: Vec<*const std::ffi::c_char> =
            env_text.iter().map(|entry| entry.as_ptr()).collect();
        envp.push(std::ptr::null());
        Ok(SpawnArgs {
            _argv_text: argv_text,
            _env_text: env_text,
            argv: argv_ptrs,
            envp,
        })
    }
}

fn wait_status(status: i32) -> i32 {
    let signal = status & 0x7f;
    if signal != 0 {
        return 128 + signal;
    }
    (status >> 8) & 0xff
}

fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn trim_end(text: &str) -> String {
    text.trim_end().to_owned()
}

/// Bounded synchronous command with captured stdio. A timeout asks only this
/// call's own child to stop (SIGTERM + grace, never SIGKILL).
pub fn run(argv: &[OsString], timeout_ms: Option<i64>, env_remove: &[&str]) -> RunOut {
    let timeout_ms = timeout_ms.unwrap_or(RUN_TIMEOUT_MS);
    if argv.is_empty() {
        return RunOut {
            exit: None,
            out: String::new(),
            err: "run requires a nonempty argv".to_owned(),
            timeout: false,
        };
    }
    let out_path = temp_path("out");
    let err_path = temp_path("err");
    let cleanup = |out_path: &Path, err_path: &Path| {
        let _ = std::fs::remove_file(out_path);
        let _ = std::fs::remove_file(err_path);
    };
    let mut actions = match FileActions::new() {
        Ok(actions) => actions,
        Err(error) => {
            return RunOut {
                exit: None,
                out: String::new(),
                err: error.message,
                timeout: false,
            };
        }
    };
    let flags = sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC;
    if actions.add_open(1, &out_path, flags, 0o644).is_err()
        || actions.add_open(2, &err_path, flags, 0o644).is_err()
    {
        cleanup(&out_path, &err_path);
        return RunOut {
            exit: None,
            out: String::new(),
            err: "process boundary failed (file actions)".to_owned(),
            timeout: false,
        };
    }
    let args = match SpawnArgs::new(argv, env_remove) {
        Ok(args) => args,
        Err(error) => {
            cleanup(&out_path, &err_path);
            return RunOut {
                exit: None,
                out: String::new(),
                err: error.message,
                timeout: false,
            };
        }
    };
    let mut pid: sys::pid_t = 0;
    // SAFETY: all pointers are valid and outlive the call; argv/envp are
    // NULL-terminated arrays built above.
    let spawned = unsafe {
        sys::posix_spawn(
            &mut pid,
            args._argv_text[0].as_ptr(),
            actions.storage.as_ptr(),
            std::ptr::null(),
            args.argv.as_ptr(),
            args.envp.as_ptr(),
        )
    };
    drop(actions);
    if spawned != 0 {
        cleanup(&out_path, &err_path);
        return RunOut {
            exit: None,
            out: String::new(),
            err: format!("process boundary failed (posix_spawn error {spawned})"),
            timeout: false,
        };
    }
    let mut status: i32 = 0;
    let deadline = monotonic_ms() + timeout_ms;
    let mut timed_out = false;
    loop {
        // SAFETY: status is a valid out buffer; pid is our own child.
        let reaped = unsafe { sys::waitpid(pid, &mut status, sys::WNOHANG) };
        if reaped == pid {
            break;
        }
        if monotonic_ms() >= deadline {
            // SAFETY: signalling our own child is safe.
            unsafe { sys::kill(pid, sys::SIGTERM) };
            let grace = monotonic_ms() + TERM_GRACE_MS;
            while unsafe { sys::waitpid(pid, &mut status, sys::WNOHANG) } != pid
                && monotonic_ms() < grace
            {
                sleep(5);
            }
            timed_out = true;
            break;
        }
        sleep(5);
    }
    let out = read_file(&out_path);
    let err = read_file(&err_path);
    cleanup(&out_path, &err_path);
    let exit_code = if timed_out {
        None
    } else {
        Some(wait_status(status))
    };
    RunOut {
        exit: exit_code,
        out: trim_end(&out),
        err: if timed_out {
            "process timed out".to_owned()
        } else {
            trim_end(&err)
        },
        timeout: timed_out,
    }
}

/// Detached project-owned process in its own session; stdio goes to the
/// given logs and the named variables are removed from its environment.
/// Returns its exact creation identity.
pub fn spawn_detached(
    executable: &OsStr,
    argv: &[OsString],
    cwd: &Path,
    log_path: Option<&Path>,
    error_log: Option<&Path>,
    env_remove: &[&str],
) -> Result<Owner> {
    let mut actions = FileActions::new()?;
    if !cwd.as_os_str().is_empty() {
        actions.add_chdir(cwd)?;
    }
    let flags = sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC;
    actions.add_open(0, Path::new("/dev/null"), 0, 0)?;
    let default_log = PathBuf::from("/dev/null");
    let log = log_path.unwrap_or(&default_log);
    actions.add_open(1, log, flags, 0o644)?;
    let err_log = error_log.unwrap_or(log);
    actions.add_open(2, err_log, flags, 0o644)?;
    let mut attr = [0u8; sys::ATTR_BYTES];
    // SAFETY: attr is a valid opaque buffer of the documented size.
    if unsafe { sys::posix_spawnattr_init(attr.as_mut_ptr()) } != 0 {
        return Err(Failure::new("posix_spawnattr_init failed"));
    }
    // SAFETY: attr was initialised.
    if unsafe { sys::posix_spawnattr_setflags(attr.as_mut_ptr(), sys::POSIX_SPAWN_SETSID) } != 0 {
        return Err(Failure::new("posix_spawnattr_setflags failed"));
    }
    let mut full: Vec<OsString> = vec![executable.to_os_string()];
    full.extend(argv.iter().cloned());
    let args = SpawnArgs::new(&full, env_remove)?;
    let mut pid: sys::pid_t = 0;
    // SAFETY: all pointers are valid and outlive the call.
    let spawned = unsafe {
        sys::posix_spawn(
            &mut pid,
            args._argv_text[0].as_ptr(),
            actions.storage.as_ptr(),
            attr.as_ptr(),
            args.argv.as_ptr(),
            args.envp.as_ptr(),
        )
    };
    drop(actions);
    if spawned != 0 {
        return Err(Failure::new(format!(
            "Detached process could not be launched (posix_spawn error {spawned})"
        )));
    }
    identity(pid)?
        .ok_or_else(|| Failure::new("Detached process exited before its identity was read"))
}

/// Child inheriting this process's stdio (the Herdr pane).
pub fn spawn_native(executable: &OsStr, argv: &[OsString], cwd: &Path) -> Result<Owner> {
    let mut actions = FileActions::new()?;
    if !cwd.as_os_str().is_empty() {
        actions.add_chdir(cwd)?;
    }
    let mut full: Vec<OsString> = vec![executable.to_os_string()];
    full.extend(argv.iter().cloned());
    let args = SpawnArgs::new(&full, &[])?;
    let mut pid: sys::pid_t = 0;
    // SAFETY: all pointers are valid and outlive the call.
    let spawned = unsafe {
        sys::posix_spawn(
            &mut pid,
            args._argv_text[0].as_ptr(),
            actions.storage.as_ptr(),
            std::ptr::null(),
            args.argv.as_ptr(),
            args.envp.as_ptr(),
        )
    };
    drop(actions);
    if spawned != 0 {
        return Err(Failure::new(format!(
            "Native launch failed (posix_spawn error {spawned})"
        )));
    }
    identity(pid)?.ok_or_else(|| Failure::new("Native child exited before its identity was read"))
}

/// Child in this process's own stdio (the Herdr pane), with `env_remove`
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
    let mut actions = FileActions::new()?;
    if !cwd.as_os_str().is_empty() {
        actions.add_chdir(cwd)?;
    }
    let args = SpawnArgs::with_env(argv, env_remove, env_set)?;
    let mut pid: sys::pid_t = 0;
    // SAFETY: all pointers are valid and outlive the call.
    let spawned = unsafe {
        sys::posix_spawn(
            &mut pid,
            args._argv_text[0].as_ptr(),
            actions.storage.as_ptr(),
            std::ptr::null(),
            args.argv.as_ptr(),
            args.envp.as_ptr(),
        )
    };
    drop(actions);
    if spawned != 0 {
        return Err(Failure::new(format!(
            "Runner child could not be launched (posix_spawn error {spawned})"
        )));
    }
    let observed = identity(pid)?
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
    })
}

pub struct AttachedChild {
    pub pid: i32,
    pub exit_path: PathBuf,
}

/// Block until the attached child this process spawned exits, then publish
/// the exit code. A child that was not reapable is reported as an unknown
/// exit (`{"exit":null}`), never as a verified abnormal one.
pub fn await_exit(child: &AttachedChild, exit_path: Option<&Path>) -> Option<i32> {
    let mut status: i32 = 0;
    // SAFETY: blocking wait on our own child.
    let reaped = unsafe { sys::waitpid(child.pid, &mut status, 0) };
    let exit_code = if reaped == child.pid {
        Some(wait_status(status))
    } else {
        None
    };
    let path = exit_path.unwrap_or(&child.exit_path);
    let record = match exit_code {
        Some(code) => format!("{{\"exit\":{code}}}"),
        None => "{\"exit\":null}".to_owned(),
    };
    let _ = std::fs::write(path, record);
    exit_code
}

// --- OS-owned file locks (port of posix_lock.lua) -------------------------

/// An exclusive flock held on an open file description. Released on close or
/// process exit, which is exactly the singleton and gate semantics the
/// lifecycle needs.
pub struct Flock {
    /// Held only for ownership: closing the descriptor releases the lock.
    _file: std::fs::File,
}

pub fn try_acquire(path: &Path) -> Result<Option<Flock>> {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| {
            Failure::new(format!("Cannot open lock file {}: {error}", path.display()))
        })?;
    // SAFETY: fd is a valid open descriptor; flock has no memory safety
    // preconditions. The descriptor is close-on-exec (Rust default), so a
    // spawned Herdr server or controller cannot inherit the lock.
    let rc = unsafe { sys::flock(file.as_raw_fd(), sys::LOCK_EX | sys::LOCK_NB) };
    if rc != 0 {
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

/// [`try_acquire`] retried for up to `timeout_ms`. A child forked by a
/// concurrent test holds a copy of a released lock's descriptor until it
/// execs, so a test that expects a free lock waits briefly; a lock that is
/// still held never frees.
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

/// `realpath(3)` identity (port of posix_path.canonical).
pub fn canonical(path: &str) -> Result<String> {
    if path.contains('\0') {
        return Err(Failure::new("invalid path"));
    }
    let resolved = std::fs::canonicalize(path)
        .map_err(|_| Failure::new(format!("cannot canonicalize path: {path}")))?;
    Ok(resolved.to_string_lossy().into_owned())
}

/// POSIX `rename` atomically replaces an existing file on the same volume.
/// Never delete the destination first: a failure must leave the old contents
/// intact (port of atomic_file.lua).
pub mod atomic {
    use super::{Failure, Result};
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub fn ensure_parent(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        ensure_parent(path)?;
        let temporary = path.with_file_name(format!(
            ".{}.{}.tmp",
            path.file_name().unwrap_or_default().to_string_lossy(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let outcome = (|| -> Result<()> {
            std::fs::write(&temporary, bytes)?;
            std::fs::rename(&temporary, path).map_err(|error| {
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

// --- Delivered platform decision (ADR 0007) --------------------------------

/// The single delivered-platform decision. The Lua adapter asked LOVE's
/// `getOS()`; the Rust equivalent is the compile-time target OS. Linux
/// (WSL2) and macOS are delivered.
pub fn platform_support() -> std::result::Result<(), String> {
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        Ok(())
    } else if cfg!(target_os = "windows") {
        Err("Windows native is not a supported platform (ADR 0007). Use WSL2 + WSLg".to_owned())
    } else {
        Err(format!(
            "Unsupported companion platform: {}. Run the viewer and Herdr natively",
            std::env::consts::OS
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// A forked child inherits open descriptors until it execs, so a lock that
    /// a parent already released can still look held for a few milliseconds.
    /// Fork-using tests here and the flock singleton test therefore never
    /// overlap; forks from other modules are covered by [`acquire_within`].
    static FORK_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn fork_guard() -> std::sync::MutexGuard<'static, ()> {
        FORK_GUARD.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn run_passes_argv_literally_including_spaces_unicode_and_metacharacters() {
        let _guard = fork_guard();
        let words = [
            "帶 空格",
            "a$(touch injected)b",
            "半角 ; & | > <",
            "quote\"inside",
        ];
        let mut argv = args(&[
            "/usr/bin/python3",
            "-c",
            "import json,sys;print(json.dumps(sys.argv[1:]))",
        ]);
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
        let _guard = fork_guard();
        let result = run(
            &args(&["/bin/sh", "-c", "printf out; printf err >&2; exit 23"]),
            None,
            &[],
        );
        assert_eq!(Some(23), result.exit);
        assert_eq!("out", result.out);
        assert_eq!("err", result.err);
    }

    #[test]
    fn run_times_out_a_bounded_command_and_reports_it() {
        let _guard = fork_guard();
        let result = run(&args(&["/bin/sleep", "10"]), Some(300), &[]);
        assert!(result.timeout);
        assert_eq!(None, result.exit);
    }

    #[test]
    fn a_timed_out_command_is_asked_to_stop_with_sigterm_so_it_can_release_resources() {
        let _guard = fork_guard();
        let marker = temp_path("marker");
        let script =
            "trap 'echo released > \"$0\"; exit 0' TERM; while :; do sleep 0.05; done".to_string();
        let argv = args(&["/bin/sh", "-c"]);
        let mut argv = argv;
        argv.push(OsString::from(script));
        argv.push(marker.as_os_str().to_os_string());
        let result = run(&argv, Some(300), &[]);
        assert!(result.timeout);
        let text = std::fs::read_to_string(&marker).unwrap_or_default();
        let _ = std::fs::remove_file(&marker);
        assert_eq!("released\n", text);
    }

    #[test]
    fn identity_is_shared_by_every_observer() {
        let own = identity(current_pid()).unwrap().unwrap();
        assert_eq!(current_pid(), own.pid);
        assert!(!own.start.is_empty());
        let again = identity(own.pid).unwrap().unwrap();
        assert_eq!(own.start, again.start);
    }

    #[test]
    fn identity_is_none_for_a_pid_that_is_not_running() {
        assert_eq!(None, identity(4194304).unwrap());
    }

    #[test]
    fn an_invalid_pid_is_rejected_instead_of_guessed() {
        assert!(identity(0).is_err());
        assert!(identity(-5).is_err());
    }

    #[test]
    fn a_detached_child_never_inherits_a_lock_its_parent_later_releases() {
        let _guard = fork_guard();
        let lock_path = temp_path("lock");
        let log = temp_path("log");
        let held = try_acquire(&lock_path).unwrap().expect("lock");
        let owner = spawn_detached(
            OsStr::new("/bin/sleep"),
            &args(&["2"]),
            Path::new("/tmp"),
            Some(&log),
            Some(&log.with_extension("err")),
            &[],
        )
        .unwrap();
        release(Some(held));
        let again = try_acquire(&lock_path).unwrap();
        release(again);
        // Clean up the detached child politely.
        unsafe { sys::kill(owner.pid, sys::SIGTERM) };
        let _ = std::fs::remove_file(&lock_path);
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(log.with_extension("err"));
    }

    #[test]
    fn spawn_detached_reports_the_exact_creation_identity() {
        let _guard = fork_guard();
        let log = temp_path("detached");
        let owner = spawn_detached(
            OsStr::new("/bin/sleep"),
            &args(&["2"]),
            Path::new("/tmp"),
            Some(&log),
            Some(&log.with_extension("err")),
            &[],
        )
        .unwrap();
        assert_eq!(owner.start, identity(owner.pid).unwrap().unwrap().start);
        unsafe { sys::kill(owner.pid, sys::SIGTERM) };
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(log.with_extension("err"));
    }

    #[test]
    fn run_removes_only_the_named_variables_from_the_child_environment() {
        let _guard = fork_guard();
        let home = std::env::var("HOME").unwrap_or_else(|_| "unset".to_owned());
        let present = run(
            &args(&["/bin/sh", "-c", "printf %s \"${HOME:-unset}\""]),
            Some(5000),
            &[],
        );
        assert_eq!(home, present.out);
        let removed = run(
            &args(&["/bin/sh", "-c", "printf %s \"${HOME:-unset}\""]),
            Some(5000),
            &["HOME"],
        );
        assert_eq!("unset", removed.out);
    }

    #[test]
    fn spawn_detached_removes_only_the_named_variables_from_the_child_environment() {
        let _guard = fork_guard();
        let log = temp_path("detached-env");
        spawn_detached(
            OsStr::new("/bin/sh"),
            // A brief lifetime, as for the attached child: an instant-exit
            // child can be a zombie before its identity is read.
            &args(&[
                "-c",
                "printf '%s %s' \"${HOME:-unset}\" \"${PATH:+path}\"; sleep 0.2",
            ]),
            Path::new("/tmp"),
            Some(&log),
            Some(&log.with_extension("err")),
            &["HOME"],
        )
        .unwrap();
        let deadline = monotonic_ms() + 5000;
        while read_file(&log).is_empty() && monotonic_ms() < deadline {
            sleep(20);
        }
        assert_eq!("unset path", read_file(&log));
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(log.with_extension("err"));
    }

    #[test]
    fn an_attached_child_records_its_identity_and_actual_exit() {
        let started = temp_path("started");
        let exit = temp_path("exit");
        let child = start_attached(
            // A brief lifetime: an instant-exit child can be a zombie before
            // its identity is read, and zombies count as gone by contract.
            &args(&["/bin/sh", "-c", "sleep 0.2; exit 37"]),
            Path::new("/tmp"),
            &[],
            &[],
            &started,
            &exit,
        )
        .unwrap();
        let record = crate::json::decode(&std::fs::read_to_string(&started).unwrap()).unwrap();
        assert_eq!(
            Some(child.pid as i64),
            record.get("pid").and_then(Json::as_i64)
        );
        assert!(record.get("start").and_then(Json::as_str).is_some());
        assert_eq!(Some(37), await_exit(&child, None));
        let exit_record = crate::json::decode(&std::fs::read_to_string(&exit).unwrap()).unwrap();
        assert_eq!(Some(37), exit_record.get("exit").and_then(Json::as_i64));
        let _ = std::fs::remove_file(&started);
        let _ = std::fs::remove_file(&exit);
    }

    #[test]
    fn flock_singleton_blocks_a_second_holder_until_released() {
        let _guard = fork_guard();
        let path = temp_path("flock");
        let held = try_acquire(&path).unwrap().expect("first lock");
        assert!(try_acquire(&path).unwrap().is_none());
        release(Some(held));
        assert!(acquire_within(&path, 5000).is_some());
        let _ = std::fs::remove_file(&path);
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
    fn canonical_resolves_the_real_path() {
        let directory = temp_path("canonical 项目");
        std::fs::create_dir_all(&directory).unwrap();
        let resolved = canonical(directory.to_str().unwrap()).unwrap();
        assert!(resolved.starts_with('/'));
        assert!(!resolved.contains("/./"));
        let _ = std::fs::remove_dir(&directory);
        assert!(canonical("/definitely/missing-uml-path").is_err());
    }

    #[test]
    fn the_delivered_platform_is_wsl_linux() {
        // The Rust port runs the delivered-platform decision as a compile-time
        // target check; on the development host it must report supported.
        assert!(platform_support().is_ok());
    }

    use crate::json::Json;
}
