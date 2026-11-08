//! Rust viewer crate (#139): egui/eframe canvas for the UML viewer.
//!
//! The window-free viewer model (the scene frame, `commands`, `theme`,
//! `lod`, the class card, inspector, source window, search box, strings,
//! session file, watch, project files and render cache) lives in the egui-free
//! `frame` crate (#16) and is re-exported here; `color` resolves its
//! semantic roles to egui colors. The viewer session itself (view, actions,
//! hot reload, display) is the companion-free `viewer-session` crate (#17);
//! `shell_session` (crate-private) wraps it with what only the window still
//! does until #18: the companion link, mail, the process identity, normal
//! close and the restart exit. It is tested without a window behind a fake
//! companion link. `app` is the eframe adapter around it; `paint`, `gui` and
//! the driver's pixel, font and window steps are exercised by the declarative
//! EDN driver scripts under `scripts/drivers/`.
// Tests are the outside harness and launch their own children; production
// code launches through `companion::command_stdout` (clippy.toml).
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod app;
pub mod bench;
pub mod cli;
pub mod clock;
pub mod color;
pub mod driver;
pub mod evidence;
pub mod fonts;
pub mod gui;
pub mod identity;
mod mail;
pub mod mail_watch;
pub mod mailbox;
pub mod paint;
pub mod real_input;
mod shell_session;
pub mod signal;

/// Quality snapshot loading moved to the viewer session crate (#17).
pub use viewer_session::metrics;

pub use frame::{
    commands, detail, inspector, lod, project_files, render_cache, search_box, session, source,
    strings, theme, watch,
};

use std::ffi::OsString;
use std::process::ExitCode;

/// Entry point of the `viewer` role: open the egui window.
pub fn run(args: &[OsString]) -> ExitCode {
    match cli::parse(args) {
        Ok(options) => app::run(options),
        Err(message) => {
            eprintln!("{message}");
            eprintln!(
                "usage: uml-viewer viewer [--backend herdr] [--restart] [--coding-agent claude|codex] [diagram.edn]"
            );
            ExitCode::from(2)
        }
    }
}
