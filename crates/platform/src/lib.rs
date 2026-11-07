//! The single delivered-platform decision (ADR 0007, ADR 0010), shared by
//! `ir` and `companion`. [`decide`] is a pure function of a declared
//! [`Platform`], so every platform's answer is testable on any host;
//! [`host`] is the only place that reads the running machine.
//!
//! Delivered: Linux (WSL2), macOS, and native Windows from Windows 10 22H2
//! (build 19045). An older Windows build is refused naming that minimum.

/// The oldest delivered Windows build: Windows 10 22H2.
pub const MINIMUM_WINDOWS_BUILD: u32 = 19045;

/// A host as the decision sees it: the `std::env::consts::OS` name and,
/// on Windows, the OS build number (`0` elsewhere or when unreadable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform<'a> {
    pub os: &'a str,
    pub build: u32,
}

/// Whether `platform` is a delivered platform; the refusal is one line that
/// names what would be accepted.
pub fn decide(platform: &Platform) -> Result<(), String> {
    match platform.os {
        "linux" | "macos" => Ok(()),
        "windows" if platform.build >= MINIMUM_WINDOWS_BUILD => Ok(()),
        "windows" => Err(format!(
            "Windows build {} is not supported: the minimum is Windows 10 22H2 (build {MINIMUM_WINDOWS_BUILD})",
            platform.build
        )),
        other => Err(format!(
            "Unsupported platform: {other}. uml-viewer is delivered on WSL2, macOS and Windows 10 22H2 (build {MINIMUM_WINDOWS_BUILD}) or later"
        )),
    }
}

/// The running machine. The Windows build comes from `RtlGetVersion`:
/// `GetVersionExW` reports 9200 to a process without a compatibility
/// manifest.
pub fn host() -> Platform<'static> {
    Platform {
        os: std::env::consts::OS,
        build: host_build(),
    }
}

/// [`decide`] for the running machine.
pub fn support() -> Result<(), String> {
    decide(&host())
}

#[cfg(windows)]
fn host_build() -> u32 {
    #[repr(C)]
    struct OsVersionInfoW {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
        csd_version: [u16; 128],
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
    }
    let mut info = OsVersionInfoW {
        size: std::mem::size_of::<OsVersionInfoW>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform_id: 0,
        csd_version: [0; 128],
    };
    // SAFETY: info is a writable OSVERSIONINFOW whose size field is set, as
    // RtlGetVersion requires; it returns STATUS_SUCCESS (0) or leaves it.
    let status = unsafe { RtlGetVersion(&mut info) };
    if status == 0 { info.build } else { 0 }
}

#[cfg(not(windows))]
fn host_build() -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(os: &str, build: u32) -> Result<(), String> {
        decide(&Platform { os, build })
    }

    #[test]
    fn windows_10_22h2_build_19045_is_delivered() {
        assert_eq!(Ok(()), declared("windows", 19045));
        assert_eq!(Ok(()), declared("windows", 26220));
    }

    #[test]
    fn an_older_windows_build_is_refused_naming_the_minimum() {
        assert_eq!(
            Err(
                "Windows build 19044 is not supported: the minimum is Windows 10 22H2 (build 19045)"
                    .to_owned()
            ),
            declared("windows", 19044)
        );
    }

    #[test]
    fn linux_and_macos_are_delivered_whatever_the_build() {
        assert_eq!(Ok(()), declared("linux", 0));
        assert_eq!(Ok(()), declared("macos", 0));
    }

    #[test]
    fn any_other_os_is_refused_naming_the_delivered_platforms() {
        assert_eq!(
            Err("Unsupported platform: freebsd. uml-viewer is delivered on WSL2, macOS and Windows 10 22H2 (build 19045) or later".to_owned()),
            declared("freebsd", 0)
        );
    }

    #[test]
    fn the_development_hosts_are_delivered_platforms() {
        // Linux (WSL2), macOS and this Windows 11 host all run the suite.
        let host = host();
        assert_eq!(std::env::consts::OS, host.os);
        assert_eq!(Ok(()), support(), "{host:?}");
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_build_is_read_from_the_kernel_not_the_manifest_shim() {
        // GetVersionExW would report 9200 to this unmanifested test binary.
        assert!(host().build >= MINIMUM_WINDOWS_BUILD, "{:?}", host());
    }
}
