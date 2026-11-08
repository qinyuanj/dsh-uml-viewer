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
    environment: Option<Vec<u16>>,
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
                let line = args.iter().fold(quote(&exe), |mut line, arg| {
                    line.push(b' ' as u16);
                    line.extend(quote(OsStr::new(arg)));
                    line
                });
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
                line.push(b' ' as u16);
                line.extend(quote(OsStr::new(arg)));
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
        environment: Some(environment(variables)),
    })
}

fn environment(added: Vec<(OsString, OsString)>) -> Vec<u16> {
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
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

fn job() -> io::Result<OwnedHandle> {
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
    Ok(job)
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

pub(super) fn execute(program: &Program, cwd: &Path, capture: bool) -> io::Result<Output> {
    let Prepared {
        exe,
        mut line,
        environment,
    } = prepare(program, cwd)?;
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
    let stdin = if capture {
        let file = File::open("NUL")?;
        duplicate(file.as_raw_handle())?
    } else {
        inherited(STD_INPUT_HANDLE)?
    };
    let (out_reader, stdout) = if capture {
        let (read, write) = pipe()?;
        (Some(read), write)
    } else {
        (None, inherited(STD_OUTPUT_HANDLE)?)
    };
    let (err_reader, stderr) = if capture {
        let (read, write) = pipe()?;
        (Some(read), write)
    } else {
        (None, inherited(STD_ERROR_HANDLE)?)
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
    drop(attributes);
    drop((stdin, stdout, stderr));
    let stdout = reader(out_reader);
    let stderr = reader(err_reader);
    let waited = unsafe { WaitForSingleObject(process.0.as_raw_handle(), INFINITE) };
    // Closing the only job handle ends descendants even if the leader exited
    // normally. Do this before reading EOF: descendants can inherit the pipes.
    drop(job);
    if waited != WAIT_OBJECT_0 {
        return Err(io::Error::last_os_error());
    }
    let mut code = 0;
    unsafe {
        checked(GetExitCodeProcess(process.0.as_raw_handle(), &mut code))?;
    }
    let out = finish(stdout);
    let err = finish(stderr);
    Ok(Output {
        status: ExitStatus::from_raw(code),
        stdout: out?,
        stderr: err?,
    })
}
