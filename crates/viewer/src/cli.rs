//! Viewer argument parsing (port of `parse_args` in
//! `love/uml/adapters/viewer.lua`): `[--backend herdr] [--restart]
//! [--coding-agent claude|codex] [diagram.edn]`, extra positionals ignored,
//! unknown options rejected.

use companion::CodingAgent;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub path: Option<PathBuf>,
    pub restart: bool,
    pub backend: bool,
    /// `--coding-agent`: the explicit provider, carried to the worker.
    pub agent: Option<CodingAgent>,
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
            let named = args.get(index + 1).map(|v| v.to_string_lossy());
            let agent = CodingAgent::from_option(named.as_deref()).map_err(|e| e.message)?;
            if options.agent.replace(agent).is_some() {
                return Err(CodingAgent::option_refused().message);
            }
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
