//! The single delivered-platform decision (ADR 0007, ADR 0010, ADR 0018),
//! asked by `ir`. [`decide`] is a pure function of a declared OS name, so
//! every platform's answer is testable on any host; [`support`] is the only
//! place that reads the running machine.
//!
//! Delivered: Linux (WSL2), macOS, and native Windows. There is no Windows
//! build-number floor: ADR 0018 removed it, because every build it separated
//! ran this code and the number was only ever a date, not a capability.

/// What a platform is delivered on, as the refusal names it.
const DELIVERED: &str = "WSL2, macOS and native Windows";

/// Whether `os` (a `std::env::consts::OS` name) is a delivered platform; the
/// refusal is one line that names what would be accepted.
pub fn decide(os: &str) -> Result<(), String> {
    match os {
        "linux" | "macos" | "windows" => Ok(()),
        other => Err(format!(
            "Unsupported platform: {other}. uml-viewer is delivered on {DELIVERED}"
        )),
    }
}

/// [`decide`] for the running machine.
pub fn support() -> Result<(), String> {
    decide(std::env::consts::OS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delivered_operating_systems_are_accepted() {
        assert_eq!(Ok(()), decide("linux"));
        assert_eq!(Ok(()), decide("macos"));
        assert_eq!(Ok(()), decide("windows"));
    }

    #[test]
    fn any_other_os_is_refused_naming_the_delivered_platforms() {
        assert_eq!(
            Err("Unsupported platform: freebsd. uml-viewer is delivered on WSL2, macOS and native Windows".to_owned()),
            decide("freebsd")
        );
    }

    #[test]
    fn the_development_host_is_a_delivered_platform() {
        // Linux (WSL2), macOS and native Windows all run the suite.
        assert_eq!(Ok(()), support(), "{}", std::env::consts::OS);
    }
}
