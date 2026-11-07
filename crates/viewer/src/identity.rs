//! Rust viewer process identity (port of
//! `love/uml/adapters/process_identity.lua`): PID + starttime self-report in
//! the exact JSON shape the restart wrapper compares. The runtime field is
//! `rust` here; `uml_launch.verify_previous` accepts both.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "viewer-process-identity.json";

/// starttime of a live process: /proc/<pid>/stat field 22, which is index 19
/// of the fields after the closing parenthesis (comm in parens may contain
/// spaces).
#[cfg(target_os = "linux")]
pub fn starttime(pid: u32) -> Option<String> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<&str> = text.rsplit(')').next()?.split_whitespace().collect();
    fields.get(19).map(|s| s.to_string())
}

/// macOS: the kernel creation time in microseconds since the epoch, the same
/// text the companion and `uml_posix.proc_identity` record.
#[cfg(target_os = "macos")]
pub fn starttime(pid: u32) -> Option<String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: info is a writable buffer of exactly `size` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
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
    Some((info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec).to_string())
}

/// Windows: the `GetProcessTimes` creation FILETIME (100 ns ticks since
/// 1601) as decimal text, the same text the companion and
/// `uml_posix.proc_identity` record. An exited process has none.
#[cfg(windows)]
pub fn starttime(pid: u32) -> Option<String> {
    type Handle = *mut std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn GetProcessTimes(
            process: Handle,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    // SAFETY: OpenProcess has no memory preconditions; NULL is failure.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut times: [FileTime; 4] = Default::default();
    let [creation, exit, kernel, user] = &mut times;
    // SAFETY: process is a valid handle with query and synchronize access;
    // the FILETIME out pointers are valid. The handle is closed once.
    let started = unsafe {
        let running = WaitForSingleObject(process, 0) != 0;
        let read = running && GetProcessTimes(process, creation, exit, kernel, user) != 0;
        CloseHandle(process);
        read
    };
    started.then(|| ((u64::from(creation.high) << 32) | u64::from(creation.low)).to_string())
}

/// `{"pid":1234,"started":"5381724","runtime":"rust","cwd":"/abs/root"}` —
/// fixed key order, no spaces, no trailing newline. `cwd` is a JSON string,
/// so a Windows `C:\dir` arrives as `C:\\dir`.
pub fn encode(pid: u32, started: &str, cwd: &str) -> String {
    let cwd = json_text(cwd);
    format!("{{\"pid\":{pid},\"started\":\"{started}\",\"runtime\":\"rust\",\"cwd\":\"{cwd}\"}}")
}

/// The body of a JSON string literal holding `text`.
fn json_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out
}

fn write_atomic(target: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = target.with_extension("json.tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, target)
}

/// Write the identity document for the running process. Only called after a
/// successful startup; a refused startup never overwrites an existing file.
pub fn record(root: &Path) -> io::Result<PathBuf> {
    let pid = std::process::id();
    let started = starttime(pid)
        .ok_or_else(|| io::Error::other("Could not read the viewer creation identity"))?;
    let cwd = root.to_string_lossy();
    let target = root.join(".uml-viewer").join(FILE_NAME);
    write_atomic(&target, &encode(pid, &started, &cwd))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cwd_is_encoded_as_a_json_string() {
        assert_eq!(
            r#"{"pid":7,"started":"1","runtime":"rust","cwd":"C:\\Users\\a \"b\"\u0009c"}"#,
            encode(7, "1", "C:\\Users\\a \"b\"\tc")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_starttime_is_decimal_filetime_and_none_after_exit() {
        let own = starttime(std::process::id()).unwrap();
        let start: u64 = own.parse().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 100 ns ticks since 1601-01-01; this process started within the day.
        let filetime_now = (now + 1 + 11_644_473_600) * 10_000_000;
        assert!(start <= filetime_now && filetime_now - start < 864_000_000_000);
        assert_eq!(Some(own), starttime(std::process::id()));
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        // `child` still holds its handle, so the pid is not reused yet.
        assert_eq!(None, starttime(pid));
    }
}
