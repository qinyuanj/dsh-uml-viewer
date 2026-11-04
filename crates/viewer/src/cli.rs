//! Viewer argument parsing (port of `parse_args` in
//! `love/uml/adapters/viewer.lua`): `[--backend herdr] [--restart]
//! [--coding-agent claude|codex] [diagram.edn]`, extra positionals ignored,
//! unknown options rejected.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub path: Option<PathBuf>,
    pub restart: bool,
    pub backend: bool,
    /// `--coding-agent`: the explicit provider name, carried to the worker.
    pub coding_agent: Option<String>,
}

pub fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let value = args[index].to_string_lossy().into_owned();
        if value == "--restart" {
            options.restart = true;
        } else if value == "--backend" {
            let selected = args
                .get(index + 1)
                .map(|v| v.to_string_lossy().into_owned())
                .unwrap_or_default();
            if selected != "herdr" || options.backend {
                return Err("Unknown or duplicate companion backend; use --backend herdr".into());
            }
            options.backend = true;
            index += 1;
        } else if value == "--coding-agent" {
            let selected = args
                .get(index + 1)
                .map(|v| v.to_string_lossy().into_owned())
                .filter(|name| name == "claude" || name == "codex");
            if selected.is_none() || options.coding_agent.is_some() {
                return Err(
                    "Unknown or duplicate coding-agent provider; use --coding-agent claude|codex"
                        .into(),
                );
            }
            options.coding_agent = selected;
            index += 1;
        } else if value.starts_with('-') {
            return Err(format!("unknown option: {value}"));
        } else if options.path.is_none() {
            options.path = Some(PathBuf::from(value));
        }
        index += 1;
    }
    Ok(options)
}
