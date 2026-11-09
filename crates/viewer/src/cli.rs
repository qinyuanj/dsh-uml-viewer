//! Viewer argument parsing (port of `parse_args` in
//! `love/uml/adapters/viewer.lua`): `[diagram.edn]`, extra positionals
//! ignored, every option rejected. The window runs standalone (#18); the
//! companion's `--backend`, `--restart` and `--coding-agent` left with it.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub path: Option<PathBuf>,
}

pub fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut options = Options::default();
    for arg in args {
        let value = arg.to_string_lossy().into_owned();
        if value.starts_with('-') {
            return Err(format!("unknown option: {value}"));
        }
        if options.path.is_none() {
            options.path = Some(PathBuf::from(value));
        }
    }
    Ok(options)
}
