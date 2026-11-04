//! Rust viewer crate (#139): egui/eframe canvas for the UML viewer.
//!
//! The pure drawing model (`commands`, `theme`, `lod`) is window-free and
//! unit-tested. The viewer session (`viewer_session`, crate-private) holds
//! the view and owns actions, reload, mail, canvas input, normal close and
//! the restart exit; it is tested without a window behind a fake companion
//! link. `app` is the eframe adapter around it; `paint`, `gui` and the
//! driver's pixel, font and window steps are exercised by the declarative
//! EDN driver scripts under `scripts/drivers/`.

pub mod app;
pub mod bench;
pub mod cli;
pub mod clock;
pub mod commands;
pub mod detail;
pub mod driver;
pub mod evidence;
pub mod fonts;
pub mod gui;
pub mod identity;
pub mod inspector;
pub mod lod;
mod mail;
pub mod mail_watch;
pub mod mailbox;
pub mod metrics;
pub mod paint;
pub mod project_files;
pub mod real_input;
pub mod render_cache;
pub mod search_box;
pub mod session;
pub mod signal;
pub mod source;
pub mod strings;
pub mod theme;
mod viewer_session;
pub mod watch;

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
