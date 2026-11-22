use std::sync::atomic::{AtomicI32, Ordering};

static STOP: AtomicI32 = AtomicI32::new(0);

pub fn pending() -> Option<i32> {
    match STOP.load(Ordering::Relaxed) {
        0 => None,
        signum => Some(signum),
    }
}

pub fn name(signum: i32) -> &'static str {
    match signum {
        2 => "SIGINT",
        15 => "SIGTERM",
        21 => "SIGBREAK",
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
            action.sa_sigaction = stop as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            (libc::sigaction(signum, &action, &mut previous) == 0).then_some(previous)
        }
    }

    pub struct Armed {
        previous: Vec<(libc::c_int, libc::sigaction)>,
    }

    impl Armed {
        pub fn new() -> Self {
            STOP.store(0, Ordering::Relaxed);
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

#[cfg(windows)]
mod armed {
    use super::STOP;
    use std::sync::atomic::Ordering;

    const CTRL_C_EVENT: u32 = 0;
    const CTRL_BREAK_EVENT: u32 = 1;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }
    unsafe extern "system" fn stop(event: u32) -> i32 {
        let signum = match event {
            CTRL_C_EVENT => 2,
            CTRL_BREAK_EVENT => 21,
            _ => return 0, // close/logoff/shutdown keep their default
        };
        STOP.store(signum, Ordering::Relaxed);
        1
    }

    pub struct Armed;
    impl Armed {
        pub fn new() -> Self {
            STOP.store(0, Ordering::Relaxed);
            unsafe {
                SetConsoleCtrlHandler(Some(stop), 1);
            }
            Self
        }
    }
    impl Drop for Armed {
        fn drop(&mut self) {
            unsafe {
                SetConsoleCtrlHandler(Some(stop), 0);
            }
        }
    }
}

#[cfg(windows)]
pub use armed::Armed;

#[cfg(not(any(target_os = "linux", windows)))]
pub struct Armed;

#[cfg(not(any(target_os = "linux", windows)))]
impl Armed {
    pub fn new() -> Self {
        Self
    }
}
