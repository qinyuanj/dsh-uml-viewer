//! Keep stdout for the one record line: while a tool runs, the process's
//! standard output (and so every child launched with inherited streams)
//! writes to standard error instead.

use std::io::Write;

/// Run `work` with stdout pointing at stderr, then restore it.
pub fn stdout_to_stderr<T>(work: impl FnOnce() -> T) -> T {
    let _ = std::io::stdout().flush();
    let saved = imp::swap();
    let result = work();
    let _ = std::io::stdout().flush();
    imp::restore(saved);
    result
}

#[cfg(unix)]
mod imp {
    use quality_core::libc;

    pub fn swap() -> Option<i32> {
        // SAFETY: plain descriptor calls on the process's own stdio.
        unsafe {
            let saved = libc::dup(1);
            if saved < 0 {
                return None;
            }
            if libc::dup2(2, 1) < 0 {
                libc::close(saved);
                return None;
            }
            Some(saved)
        }
    }

    pub fn restore(saved: Option<i32>) {
        if let Some(saved) = saved {
            // SAFETY: `saved` is the duplicate taken by `swap`.
            unsafe {
                libc::dup2(saved, 1);
                libc::close(saved);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };

    // Rust's stdout and the quality process boundary both read the standard
    // handle at each use, so swapping it covers the tools and their children.
    pub fn swap() -> Option<HANDLE> {
        // SAFETY: reading and setting this process's standard handles.
        unsafe {
            let saved = GetStdHandle(STD_OUTPUT_HANDLE);
            let error = GetStdHandle(STD_ERROR_HANDLE);
            (SetStdHandle(STD_OUTPUT_HANDLE, error) != 0).then_some(saved)
        }
    }

    pub fn restore(saved: Option<HANDLE>) {
        if let Some(saved) = saved {
            // SAFETY: restoring the handle `swap` read.
            unsafe {
                SetStdHandle(STD_OUTPUT_HANDLE, saved);
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn swap() -> Option<()> {
        None
    }
    pub fn restore(_: Option<()>) {}
}
