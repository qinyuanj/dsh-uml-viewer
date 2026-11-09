//! A termination request as a quit request (#142): the process exits 0
//! through the normal shutdown path instead of dying in place (the
//! `terminate` driver). The handler only sets a flag; the frame loop acts on
//! it.
//!
//! POSIX: SIGTERM and SIGINT. Windows has no SIGTERM: the console control
//! events CTRL_C, CTRL_BREAK (what a bounded `run` sends its job's group
//! first) and CTRL_CLOSE are the requests. A viewer without a console is
//! closed with `WM_CLOSE` instead, which eframe turns into the same close.

use std::sync::atomic::{AtomicBool, Ordering};

static TERMINATED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
mod imp {
    use std::os::raw::c_int;
    use std::sync::atomic::Ordering;

    const SIGINT: c_int = 2;
    const SIGTERM: c_int = 15;

    extern "C" fn handler(_signal: c_int) {
        super::TERMINATED.store(true, Ordering::SeqCst);
    }

    unsafe extern "C" {
        /// BSD semantics on glibc: interrupted syscalls restart, so a flag is
        /// all the handler needs to be.
        fn signal(sig: c_int, handler: usize) -> usize;
    }

    pub fn install() {
        for sig in [SIGTERM, SIGINT] {
            // SAFETY: the handler is a process-wide flag write; installing it
            // is safe before any thread is spawned.
            // A function item to its machine address: the C signature the
            // libc `signal` expects.
            unsafe { signal(sig, handler as extern "C" fn(c_int) as usize) };
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    const CTRL_C_EVENT: u32 = 0;
    const CTRL_BREAK_EVENT: u32 = 1;
    const CTRL_CLOSE_EVENT: u32 = 2;

    type HandlerRoutine = unsafe extern "system" fn(u32) -> i32;

    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: Option<HandlerRoutine>, add: i32) -> i32;
    }

    /// Runs on a thread the system creates. CTRL_C/CTRL_BREAK are handled
    /// (the process lives on to close normally). After CTRL_CLOSE returns
    /// the system ends the process, so that handler holds the process open
    /// for the normal close, within the system's own 5 s limit: the viewer's
    /// exit ends this thread with the process.
    unsafe extern "system" fn handler(event: u32) -> i32 {
        match event {
            CTRL_C_EVENT | CTRL_BREAK_EVENT => {
                super::TERMINATED.store(true, Ordering::SeqCst);
                1
            }
            CTRL_CLOSE_EVENT => {
                super::TERMINATED.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(4500));
                1
            }
            _ => 0,
        }
    }

    pub fn install() {
        // SAFETY: registers a handler that only writes a process-wide flag
        // (and sleeps); no preconditions.
        unsafe { SetConsoleCtrlHandler(Some(handler), 1) };
    }
}

/// Install the handlers once, before the window exists.
pub fn install() {
    imp::install();
}

/// A termination request arrived.
pub fn terminated() -> bool {
    TERMINATED.load(Ordering::SeqCst)
}

#[cfg(all(test, windows))]
mod tests {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CTRL_BREAK_EVENT: u32 = 1;
    const CHILD: &str = "UML_VIEWER_SIGNAL_TEST_CHILD";

    unsafe extern "system" {
        fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
        fn AttachConsole(pid: u32) -> i32;
        fn FreeConsole() -> i32;
        fn GetConsoleCP() -> u32;
    }

    /// The child half: install, report readiness, wait for the request.
    #[test]
    fn signal_child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        super::install();
        println!("installed");
        let deadline = Instant::now() + Duration::from_secs(20);
        while !super::terminated() {
            assert!(Instant::now() < deadline, "no termination request arrived");
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("terminated");
    }

    #[test]
    fn ctrl_break_is_a_quit_request_not_a_kill() {
        // SAFETY: console queries and calls without memory preconditions.
        let shared = unsafe { GetConsoleCP() } != 0;
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "signal::tests::signal_child", "--nocapture"])
            .env(CHILD, "1")
            .creation_flags(CREATE_NEW_PROCESS_GROUP | if shared { 0 } else { CREATE_NO_WINDOW })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !String::from_utf8_lossy(&seen).contains("installed") {
            assert!(Instant::now() < deadline, "the child never installed");
            let mut chunk = [0u8; 256];
            let n = std::io::Read::read(&mut stdout, &mut chunk).unwrap();
            assert!(n > 0, "the child ended early");
            seen.extend_from_slice(&chunk[..n]);
        }
        // SAFETY: as above; a console borrowed for the call is freed again.
        let sent = unsafe {
            if shared {
                GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) != 0
            } else {
                let attached = AttachConsole(child.id()) != 0;
                let sent = attached && GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) != 0;
                if attached {
                    FreeConsole();
                }
                sent
            }
        };
        assert!(sent, "CTRL_BREAK could not be sent");
        std::io::Read::read_to_end(&mut stdout, &mut seen).unwrap();
        let status = child.wait().unwrap();
        let text = String::from_utf8_lossy(&seen);
        assert!(text.contains("terminated"), "{text}");
        assert_eq!(Some(0), status.code(), "the child exits on its own: {text}");
    }
}
