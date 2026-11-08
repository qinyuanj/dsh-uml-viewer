//! SIGINT and SIGTERM stop a run instead of killing it mid-cleanup.
//!
//! The handler only records the signal number. The command poll loop and the
//! worker loop notice it, kill their children and unwind, so the caller removes
//! its scratch from ordinary code and exits `128 + signal`, as the original
//! does. Nothing but the store happens in the handler, so no allocation or
//! filesystem call runs in an async-signal context.

use std::sync::atomic::{AtomicI32, Ordering};

static STOP: AtomicI32 = AtomicI32::new(0);

/// The signal that stopped this run, once one arrived.
pub fn pending() -> Option<i32> {
    match STOP.load(Ordering::Relaxed) {
        0 => None,
        signum => Some(signum),
    }
}

/// The signal's name for the interrupt report.
pub fn name(signum: i32) -> &'static str {
    match signum {
        2 => "SIGINT",
        15 => "SIGTERM",
        _ => "a stop signal",
    }
}

#[cfg(target_os = "linux")]
mod armed {
    use super::STOP;
    use quality_core::libc;
    use std::sync::atomic::Ordering;

    extern "C" fn stop(signum: libc::c_int) {
        STOP.store(signum, Ordering::Relaxed);
    }

    fn install(signum: libc::c_int) -> Option<libc::sigaction> {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = stop as usize;
            // Restarted calls keep the poll loops simple; each loop checks
            // `pending` between its short waits anyway.
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            (libc::sigaction(signum, &action, &mut previous) == 0).then_some(previous)
        }
    }

    /// The handlers for one run; dropping this restores what was there before.
    pub struct Armed {
        previous: Vec<(libc::c_int, libc::sigaction)>,
    }

    impl Armed {
        pub fn new() -> Self {
            let previous = [libc::SIGINT, libc::SIGTERM]
                .into_iter()
                .filter_map(|signum| install(signum).map(|previous| (signum, previous)))
                .collect();
            Self { previous }
        }
    }

    impl Drop for Armed {
        fn drop(&mut self) {
            for (signum, previous) in &self.previous {
                unsafe {
                    quality_core::libc::sigaction(*signum, previous, std::ptr::null_mut());
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub use armed::Armed;

/// Other platforms refuse mutation before a command runs; no handler is needed.
#[cfg(not(target_os = "linux"))]
pub struct Armed;

#[cfg(not(target_os = "linux"))]
impl Armed {
    pub fn new() -> Self {
        Self
    }
}
