//! Documented Win32 process boundary. Keep the primary thread handle from
//! CreateProcessW: assign the suspended process to a non-inheritable,
//! kill-on-close job before ResumeThread can let it create descendants.

use super::Program;
use std::ffi::{OsStr, OsString};
use std::fs::File;
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

fn prepare(program: &Program, cwd: &Path) -> io::Result<(OsString, Vec<u16>)> {
    match program {
        Program::Shell(text) => {
            let exe = comspec();
            let mut line = quote(&exe);
            line.extend(OsStr::new(&format!(" /d /v:off /s /c \"{text}\"")).encode_wide());
            Ok((exe, line))
        }
        Program::Argv(argv) => {
            let (first, rest) = argv
                .split_first()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
            let exe = resolve(first, cwd);
            let mut line = quote(exe.as_os_str());
            for arg in rest {
                line.push(b' ' as u16);
                line.extend(quote(OsStr::new(arg)));
            }
            Ok((exe.into_os_string(), line))
        }
    }
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
}
impl Attributes {
    fn new(handles: &[HANDLE]) -> io::Result<Self> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
        };
        unsafe {
            checked(InitializeProcThreadAttributeList(
                attributes.ptr(),
                1,
                0,
                &mut bytes,
            ))?;
            checked(UpdateProcThreadAttribute(
                attributes.ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
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
        unsafe {
            DeleteProcThreadAttributeList(self.ptr());
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
    let (exe, mut line) = prepare(program, cwd)?;
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
    let mut attributes = Attributes::new(&handles)?;
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
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
            null(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        ))?;
    }
    let process = Process(owned(info.hProcess)?);
    let thread = owned(info.hThread)?;
    unsafe {
        checked(AssignProcessToJobObject(
            job.as_raw_handle(),
            process.0.as_raw_handle(),
        ))?;
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
