//! The single delivered-platform decision (qinyuanj/home-uml-viewer ADR 0002,
//! ADR 0010, ADR 0018), asked by `ir`. [`decide`] is a pure function of a
//! declared OS/architecture pair, so the one host is testable anywhere;
//! [`support`] is the only place that reads the running machine.
//!
//! Delivered: native Windows x64 only. There is no Windows build-number floor:
//! ADR 0018 removed it, because every build it separated ran this code and the
//! number was only ever a date, not a capability.

/// The delivered host, as the refusal names it.
const DELIVERED: &str = "native Windows x64";

/// Whether `(os, arch)` (from `std::env::consts`) is the delivered host; the
/// refusal is one line that names what would be accepted.
pub fn decide(os: &str, arch: &str) -> Result<(), String> {
    match (os, arch) {
        ("windows", "x86_64") => Ok(()),
        _ => Err(format!(
            "Unsupported host: {os}/{arch}. uml-viewer is delivered on {DELIVERED}"
        )),
    }
}

/// [`decide`] for the running machine.
pub fn support() -> Result<(), String> {
    decide(std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_native_windows_x64_is_accepted() {
        assert_eq!(Ok(()), decide("windows", "x86_64"));
    }

    #[test]
    fn every_other_host_is_refused_naming_the_delivered_host() {
        for (os, arch) in [
            ("linux", "x86_64"),
            ("macos", "aarch64"),
            ("windows", "aarch64"),
            ("freebsd", "x86_64"),
        ] {
            assert_eq!(
                Err(format!(
                    "Unsupported host: {os}/{arch}. uml-viewer is delivered on native Windows x64"
                )),
                decide(os, arch)
            );
        }
    }

    #[test]
    fn support_matches_decide_for_the_running_host() {
        assert_eq!(
            decide(std::env::consts::OS, std::env::consts::ARCH),
            support()
        );
    }
}
