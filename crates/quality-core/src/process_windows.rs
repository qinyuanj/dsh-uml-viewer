//! Documented Win32 process boundary. Keep the primary thread handle from
//! CreateProcessW: assign the suspended process to a non-inheritable,
//! kill-on-close job before ResumeThread can let it create descendants.

use super::Program;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use std::ptr::{null, null_mut};
use std::time::Duration;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Console::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::*;

fn checked(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        // All callers transfer a newly created handle exactly once.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut text: Vec<u16> = value.encode_wide().collect();
    if text.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in process argument",
        ));
    }
    text.push(0);
    Ok(text)
}

/// MSVCRT argv quoting, including empty strings and backslashes before quotes.
fn quote(value: &OsStr) -> Vec<u16> {
    let mut out = vec![b'"' as u16];
    let mut slashes = 0;
    for ch in value.encode_wide() {
        if ch == b'\\' as u16 {
            slashes += 1;
        } else {
            out.extend(std::iter::repeat_n(
                b'\\' as u16,
                if ch == b'"' as u16 {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            out.push(ch);
            slashes = 0;
        }
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    out
}

/// Only quote an argument when the command line would otherwise rewrite it.
/// Quoting every argument is not the same line to a program that parses its own
/// switches: `cmd.exe "/d" "/c" "set"` leaves a stray quote after `/c`, so cmd
/// runs `"set` and exits 1 instead of printing the environment. MSVCRT reads a
/// bare argument as itself, so this quotes the same set as `list2cmdline`.
fn append_argument(line: &mut Vec<u16>, arg: &OsStr) {
    line.push(b' ' as u16);
    if needs_quotes(arg) {
        line.extend(quote(arg));
    } else {
        line.extend(arg.encode_wide());
    }
}

/// A space, tab, newline or vertical tab would split the argument, a quote
/// would rewrite it, and an empty one would vanish: only those are quoted.
fn needs_quotes(value: &OsStr) -> bool {
    value.is_empty()
        || value
            .encode_wide()
            .any(|ch| matches!(ch, 0x20 | 0x09 | 0x0a | 0x0b | 0x22))
}

fn comspec() -> OsString {
    std::env::var_os("COMSPEC").unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32/cmd.exe")
            .into_os_string()
    })
}

struct Prepared {
    exe: OsString,
    line: Vec<u16>,
    environment: Option<Vec<(OsString, OsString)>>,
}

fn prepare(program: &Program, cwd: &Path) -> io::Result<Prepared> {
    match program {
        Program::Shell(text) => {
            let exe = comspec();
            let mut line = quote(&exe);
            line.extend(OsStr::new(&format!(" /d /v:off /s /c \"{text}\"")).encode_wide());
            Ok(Prepared {
                exe,
                line,
                environment: None,
            })
        }
        Program::Argv(argv) => {
            let (first, rest) = argv
                .split_first()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
            let launcher = resolve(first, cwd);
            if let Some((exe, args)) = node_entry(&launcher, rest) {
                let mut line = quote(&exe);
                for arg in &args {
                    append_argument(&mut line, OsStr::new(arg));
                }
                return Ok(Prepared {
                    exe,
                    line,
                    environment: None,
                });
            }
            if launcher.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat")
            }) {
                return batch(&launcher, rest);
            }
            let exe = launcher.into_os_string();
            let mut line = quote(&exe);
            for arg in rest {
                append_argument(&mut line, OsStr::new(arg));
            }
            Ok(Prepared {
                exe,
                line,
                environment: None,
            })
        }
    }
}

fn node_entry(launcher: &Path, args: &[String]) -> Option<(OsString, Vec<String>)> {
    let stem = launcher.file_stem()?.to_str()?.to_ascii_lowercase();
    let parent = launcher.parent()?;
    let entry = if stem == "npm" || stem == "npx" {
        parent
            .join("node_modules/npm/bin")
            .join(format!("{stem}-cli.js"))
    } else if stem == "vitest" && parent.file_name().is_some_and(|name| name == ".bin") {
        let package = parent.parent()?.join("vitest");
        let text = fs::read_to_string(package.join("package.json")).ok()?;
        let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
        let bin = manifest.get("bin")?;
        let entry = bin.as_str().or_else(|| bin.get("vitest")?.as_str())?;
        package.join(entry)
    } else {
        return None;
    };
    entry.is_file().then(|| {
        let node = parent.join("node.exe");
        let node = if node.is_file() {
            node
        } else {
            super::which("node").unwrap_or_else(|| PathBuf::from("node"))
        };
        (
            node.into_os_string(),
            std::iter::once(entry.to_string_lossy().into_owned())
                .chain(args.iter().cloned())
                .collect(),
        )
    })
}

fn batch(launcher: &Path, args: &[String]) -> io::Result<Prepared> {
    let exe = comspec();
    let prefix = format!("CRAPPER_ARG_{}_", std::process::id());
    let mut variables: Vec<(OsString, OsString)> = Vec::new();
    let mut line = quote(&exe);
    line.extend(OsStr::new(" /d /v:off /s /c \"").encode_wide());
    for (index, arg) in std::iter::once(&launcher.to_string_lossy().into_owned())
        .chain(args.iter())
        .enumerate()
    {
        if arg.contains(['\0', '\r', '\n']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows batch arguments cannot contain NUL or newlines",
            ));
        }
        // cmd keeps doubled quotes quoted; MSVCRT sees them as literal quotes.
        let mut quoted = String::new();
        let mut slashes = 0;
        for ch in arg.chars() {
            if ch == '\\' {
                slashes += 1;
                continue;
            }
            if ch == '\"' {
                quoted.push_str(&"\\".repeat(slashes * 2));
                quoted.push_str("\"\"");
            } else {
                quoted.push_str(&"\\".repeat(slashes));
                quoted.push(ch);
            }
            slashes = 0;
        }
        quoted.push_str(&"\\".repeat(slashes * 2));
        let name = format!("{prefix}{index}");
        variables.push((name.clone().into(), format!("\"{quoted}\"").into()));
        line.extend(format!("%{name}% ").encode_utf16());
    }
    line.extend(OsStr::new("\"").encode_wide());
    Ok(Prepared {
        exe,
        line,
        environment: Some(variables),
    })
}

fn environment(added: Vec<(OsString, OsString)>, removed: &[OsString]) -> Vec<u16> {
    // Overrides replace, never duplicate: with two HOME entries the child
    // resolves the first one and the redirection would silently not happen.
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(key, _)| {
            !removed.iter().any(|other| {
                key.to_string_lossy().to_lowercase() == other.to_string_lossy().to_lowercase()
            }) && !added.iter().any(|(other, _)| {
                key.to_string_lossy().to_lowercase() == other.to_string_lossy().to_lowercase()
            })
        })
        .collect();
    vars.extend(added);
    vars.sort_by(|a, b| {
        a.0.to_string_lossy()
            .to_lowercase()
            .cmp(&b.0.to_string_lossy().to_lowercase())
    });
    let mut result = Vec::new();
    for (key, value) in vars {
        result.extend(key.encode_wide());
        result.push(b'=' as u16);
        result.extend(value.encode_wide());
        result.push(0);
    }
    result.push(0);
    result
}

fn resolve(name: &str, cwd: &Path) -> PathBuf {
    let path = Path::new(name);
    if path
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
    {
        let explicit = cwd.join(path);
        if path.extension().is_none() {
            let extensions =
                std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
            for ext in extensions.split(';').filter(|ext| !ext.is_empty()) {
                let candidate = PathBuf::from(format!("{}{ext}", explicit.display()));
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
        explicit
    } else {
        super::which(name).unwrap_or_else(|| path.to_path_buf())
    }
}

fn duplicate(handle: HANDLE) -> io::Result<OwnedHandle> {
    let mut copy = null_mut();
    unsafe {
        checked(DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut copy,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        ))?;
    }
    owned(copy)
}

fn inherited(which: u32) -> io::Result<OwnedHandle> {
    let handle = unsafe { GetStdHandle(which) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        // A GUI/no-console caller may have no standard handles at all.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("NUL")?;
        duplicate(file.as_raw_handle())
    } else {
        duplicate(handle)
    }
}

fn pipe() -> io::Result<(File, OwnedHandle)> {
    let mut read = null_mut();
    let mut write = null_mut();
    unsafe {
        checked(CreatePipe(&mut read, &mut write, null(), 0))?;
    }
    let read = owned(read)?;
    let write = owned(write)?;
    let inherited_write = duplicate(write.as_raw_handle())?;
    Ok((File::from(read), inherited_write))
}

struct Attributes {
    // Word allocation gives the opaque list native pointer alignment.
    storage: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn new(handles: &[HANDLE], jobs: &[HANDLE]) -> io::Result<Self> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        unsafe {
            checked(InitializeProcThreadAttributeList(
                attributes.ptr(),
                2,
                0,
                &mut bytes,
            ))?;
            attributes.initialized = true;
            checked(UpdateProcThreadAttribute(
                attributes.ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
                null_mut(),
                null(),
            ))?;
            checked(UpdateProcThreadAttribute(
                attributes.ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                jobs.as_ptr().cast(),
                std::mem::size_of_val(jobs),
                null_mut(),
                null(),
            ))?;
        }
        Ok(attributes)
    }
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.ptr());
            }
        }
    }
}

/// The job this command's tree belongs to. Killing a job (also via
/// kill-on-close) only starts termination; it does not wait for descendants to
/// release their files and working directories.
struct Job {
    handle: OwnedHandle,
    drained: bool,
}
impl Job {
    fn as_raw_handle(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }

    /// Terminate every member and wait, bounded, until each process object has
    /// signalled. The call is the same whether the leader was killed or exited
    /// by itself: its descendants are never allowed to outlive the command.
    fn finish(&mut self) -> io::Result<()> {
        if self.drained {
            return Ok(());
        }
        unsafe {
            checked(TerminateJobObject(self.as_raw_handle(), 124))?;
        }
        let started = std::time::Instant::now();
        let limit = Duration::from_secs(10);
        // Termination stops member threads before returning, but their I/O
        // teardown can continue. Pin and wait on each remaining member. The pid
        // list can include exited processes whose handles observers still own.
        for pid in self.process_ids()? {
            // PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION
            let process = match owned(unsafe { OpenProcess(0x0010_0000 | 0x1000, 0, pid) }) {
                Ok(process) => process,
                Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => {
                    continue; // Exited and no longer openable.
                }
                Err(error) => return Err(error),
            };
            let mut member = 0;
            unsafe {
                checked(IsProcessInJob(
                    process.as_raw_handle(),
                    self.as_raw_handle(),
                    &mut member,
                ))?;
            }
            if member == 0 {
                continue; // PID reuse: never wait on another job's process.
            }
            let remaining = limit.saturating_sub(started.elapsed());
            match unsafe {
                WaitForSingleObject(process.as_raw_handle(), remaining.as_millis() as u32)
            } {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => return Err(drain_timeout()),
                _ => return Err(io::Error::last_os_error()),
            }
        }
        self.drained = true;
        Ok(())
    }

    /// Query after termination, growing the aligned variable-length buffer if
    /// this tree has more members than the initial allocation.
    fn process_ids(&self) -> io::Result<Vec<u32>> {
        let offset = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
            / size_of::<usize>();
        let mut storage = vec![0usize; offset + 64];
        loop {
            let ok = unsafe {
                QueryInformationJobObject(
                    self.as_raw_handle(),
                    JobObjectBasicProcessIdList,
                    storage.as_mut_ptr().cast(),
                    std::mem::size_of_val(storage.as_slice()) as u32,
                    null_mut(),
                )
            };
            if ok != 0 {
                let info = unsafe { &*storage.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
                let count = info.NumberOfProcessIdsInList as usize;
                return Ok(storage[offset..offset + count]
                    .iter()
                    .map(|pid| *pid as u32)
                    .collect());
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
                return Err(error);
            }
            storage.resize(storage.len() * 2, 0);
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        // Error/drop paths keep kill-on-close as a fallback, without retrying a
        // failed drain or masking the original error with another long wait.
        if !self.drained {
            unsafe {
                TerminateJobObject(self.as_raw_handle(), 124);
            }
        }
    }
}

/// A bounded drain that did not finish is a failed cleanup, reported to the
/// caller rather than covered by a longer wait.
fn drain_timeout() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "the process job did not terminate within 10 seconds",
    )
}

fn job() -> io::Result<Job> {
    let job = owned(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        checked(SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ))?;
    }
    Ok(Job {
        handle: job,
        drained: false,
    })
}

// Also covers failure before assignment/resume: no suspended child is stranded.
struct Process(OwnedHandle);
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            if WaitForSingleObject(self.0.as_raw_handle(), 0) != WAIT_OBJECT_0 {
                TerminateProcess(self.0.as_raw_handle(), 1);
                WaitForSingleObject(self.0.as_raw_handle(), INFINITE);
            }
        }
    }
}

fn reader(file: Option<File>) -> Option<std::thread::JoinHandle<io::Result<Vec<u8>>>> {
    file.map(|mut file| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
        })
    })
}
fn finish(reader: Option<std::thread::JoinHandle<io::Result<Vec<u8>>>>) -> io::Result<Vec<u8>> {
    match reader {
        None => Ok(Vec::new()),
        Some(reader) => reader
            .join()
            .map_err(|_| io::Error::other("process output reader panicked"))?,
    }
}

/// One command supervised until it ends, a timeout expires or a stop arrives.
///
/// The same creation path as [`execute`]: suspended `CreateProcessW` with the
/// kill-on-close job in `PROC_THREAD_ATTRIBUTE_JOB_LIST`. The job is the only
/// thing that can reach descendants, so a timeout, a stop, a returned error or
/// an abruptly killed caller all end the whole tree.
pub(super) struct Running {
    // Struct fields drop in declaration order: terminate the tree before the
    // leader's guard waits when a caller drops an unconsumed supervisor.
    job: Job,
    process: Process,
    // The inherited ends stay alive until the command has been reaped.
    handles: [OwnedHandle; 3],
    attributes: Attributes,
    // Captured pipes are read only after the job has ended every writer.
    out: Option<File>,
    err: Option<File>,
}
pub(super) enum Ended {
    /// The command's own exit code. A Windows exit code is unsigned; a
    /// negative value here is the shell's own spelling, never a signal.
    Exited(i32),
    /// The supervisor asked the job to stop: `reason` is `timeout` or `stop`.
    Killed(&'static str),
}
pub(super) fn start(
    program: &Program,
    cwd: &Path,
    environment: &[(OsString, OsString)],
    removed: &[OsString],
    output: &File,
) -> io::Result<Running> {
    let prepared = prepare(program, cwd)?;
    spawn_prepared(
        prepared,
        cwd,
        environment,
        removed,
        Streams::File(output.try_clone()?),
    )
}
pub(super) fn supervise(
    running: Running,
    elapsed: impl Fn() -> Duration,
    timeout: Option<Duration>,
    stop: impl Fn() -> bool,
) -> io::Result<Ended> {
    let Running {
        process,
        mut job,
        handles,
        attributes,
        out: _,
        err: _,
    } = running;
    let ended = loop {
        match unsafe { WaitForSingleObject(process.0.as_raw_handle(), 25) } {
            WAIT_OBJECT_0 => {
                let mut code = 0;
                unsafe {
                    checked(GetExitCodeProcess(process.0.as_raw_handle(), &mut code))?;
                }
                break Ended::Exited(code as i32);
            }
            WAIT_TIMEOUT => {}
            _ => return Err(io::Error::last_os_error()),
        }
        let reason = if stop() {
            Some("stop")
        } else if timeout.is_some_and(|limit| elapsed() >= limit) {
            Some("timeout")
        } else {
            None
        };
        if let Some(reason) = reason {
            job.finish()?;
            break Ended::Killed(reason);
        }
    };
    // The leader's exit does not prove its descendants ended: the job still
    // owns them. Terminate and drain before the caller removes its copy.
    job.finish()?;
    drop(job);
    drop(handles);
    drop(attributes);
    drop(process);
    Ok(ended)
}

/// The suspended creation both entries share. `streams` decides between
/// captured pipes, inherited terminal handles and one redirected output file.
fn spawn(
    exe: OsString,
    mut line: Vec<u16>,
    cwd: &Path,
    environment: Option<Vec<u16>>,
    streams: Streams,
) -> io::Result<Running> {
    if line.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in process argument",
        ));
    }
    line.push(0);
    let exe = wide(&exe)?;
    let cwd = wide(cwd.as_os_str())?;
    let job = job()?;
    let stdin = match streams {
        Streams::Capture | Streams::File(_) => {
            let file = File::open("NUL")?;
            duplicate(file.as_raw_handle())?
        }
        Streams::Inherit => inherited(STD_INPUT_HANDLE)?,
    };
    let (out_reader, stdout) = match streams {
        Streams::Capture => {
            let (read, write) = pipe()?;
            (Some(read), write)
        }
        Streams::File(ref file) => (None, duplicate(file.as_raw_handle())?),
        Streams::Inherit => (None, inherited(STD_OUTPUT_HANDLE)?),
    };
    let (err_reader, stderr) = match streams {
        Streams::Capture => {
            let (read, write) = pipe()?;
            (Some(read), write)
        }
        Streams::File(ref file) => (None, duplicate(file.as_raw_handle())?),
        Streams::Inherit => (None, inherited(STD_ERROR_HANDLE)?),
    };
    let handles = [
        stdin.as_raw_handle(),
        stdout.as_raw_handle(),
        stderr.as_raw_handle(),
    ];
    // Assign the job atomically during creation (Windows 10 documented
    // PROC_THREAD_ATTRIBUTE_JOB_LIST), before the primary thread is resumed.
    let jobs = [job.as_raw_handle()];
    let mut attributes = Attributes::new(&handles, &jobs)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handles[0];
    startup.StartupInfo.hStdOutput = handles[1];
    startup.StartupInfo.hStdError = handles[2];
    startup.lpAttributeList = attributes.ptr();
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        checked(CreateProcessW(
            exe.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED
                | EXTENDED_STARTUPINFO_PRESENT
                | if environment.is_some() {
                    CREATE_UNICODE_ENVIRONMENT
                } else {
                    0
                },
            environment
                .as_ref()
                .map_or(null(), |env| env.as_ptr())
                .cast_mut()
                .cast(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        ))?;
    }
    let process = Process(owned(info.hProcess)?);
    let thread = owned(info.hThread)?;
    unsafe {
        if ResumeThread(thread.as_raw_handle()) == u32::MAX {
            return Err(io::Error::last_os_error());
        }
    }
    drop(thread);
    Ok(Running {
        process,
        job,
        handles: [stdin, stdout, stderr],
        attributes,
        out: out_reader,
        err: err_reader,
    })
}

fn spawn_prepared(
    prepared: Prepared,
    cwd: &Path,
    added: &[(OsString, OsString)],
    removed: &[OsString],
    streams: Streams,
) -> io::Result<Running> {
    let Prepared {
        exe,
        line,
        environment,
    } = prepared;
    // Batch's literal-argument variables and the caller's own overrides.
    let mut overrides = environment.unwrap_or_default();
    overrides.extend_from_slice(added);
    let environment = if overrides.is_empty() && removed.is_empty() {
        None
    } else {
        Some(self::environment(overrides, removed))
    };
    spawn(exe, line, cwd, environment, streams)
}
enum Streams {
    Capture,
    Inherit,
    File(File),
}

pub(super) fn execute(program: &Program, cwd: &Path, capture: bool) -> io::Result<Output> {
    let prepared = prepare(program, cwd)?;
    let running = spawn_prepared(
        prepared,
        cwd,
        &[],
        &[],
        if capture {
            Streams::Capture
        } else {
            Streams::Inherit
        },
    )?;
    let Running {
        process,
        mut job,
        handles,
        attributes,
        out,
        err,
    } = running;
    let waited = unsafe { WaitForSingleObject(process.0.as_raw_handle(), INFINITE) };
    if waited != WAIT_OBJECT_0 {
        return Err(io::Error::last_os_error());
    }
    let mut code = 0;
    unsafe {
        checked(GetExitCodeProcess(process.0.as_raw_handle(), &mut code))?;
    }
    // Descendants may still own files and captured-pipe writers after the
    // leader exits. Wait for their termination before returning or reading EOF.
    job.finish()?;
    drop(job);
    drop(handles);
    drop(attributes);
    let out = finish(reader(out));
    let err = finish(reader(err));
    Ok(Output {
        status: ExitStatus::from_raw(code),
        stdout: out?,
        stderr: err?,
    })
}
