//! Do not let an outer coverage run configure isolated Cargo commands.
//! Apply removals before the adapter's own target directory and before an
//! inner coverage command installs its own instrumentation settings.
use std::ffi::{OsStr, OsString};

const OUTER_SETTINGS: &[&str] = &[
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "LLVM_PROFILE_FILE",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_TARGET_DIR",
];

/// Every inherited name an isolated command must not see. Both adapters set
/// their own `CARGO_TARGET_DIR`; the coverage prefix is matched by name because
/// its settings grow between coverage releases.
pub(crate) fn inherited_removals() -> Vec<OsString> {
    std::env::vars_os()
        .map(|(name, _)| name)
        .filter(|name| is_outer_setting(name))
        .collect()
}

fn is_outer_setting(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    // Windows environment names are case-insensitive; Unix names are not.
    #[cfg(windows)]
    let name = name.to_ascii_uppercase();
    let name: &str = &name;
    OUTER_SETTINGS.contains(&name) || name.starts_with("CARGO_LLVM_COV")
}
