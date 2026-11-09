//! Scene core (#137): the pure-logic port of the LOVE scene pipeline
//! (ADR 0008). No egui, no windowing: IR EDN and quality snapshots in,
//! scene geometry EDN out, plus navigation state and proposal write-back.
//!
//! The interface (#163) is the `ViewState` state machine the viewer drives:
//!
//! - `document`: new and waiting states, hot reload, session restore and
//!   mail application, plus proposal write-back;
//! - `events`: pointer and keyboard events, zoom and fit, locate, proposal
//!   add/rename/delete/show, closing the class card, the declutter cycle and
//!   the element menu;
//! - `hit` (hit testing), `search`, `highlight` and `ir` (reading an IR
//!   document);
//! - `cli`: the headless scene export of the `scene` subcommand (`run`) and
//!   the goldens (`cli::project`, `cli::edge_curve`), with
//!   `document::compile_view` as the compile of one view.
//!
//! The root re-exports the value types (view state, write-back and mail
//! boundaries, dims, rect, highlight, quality snapshot, search match, zoom
//! step) and three queries: the class-card neighbourhood (`class_card`),
//! the quality rating of a box or card row (`rating`, `metric_rating`) and
//! the proposal list (`proposals`), with the text-width estimate and the
//! grade constants as values. [`targets`] answers what a panel's quality
//! refresh names. Layout, routing, composition, curves, hierarchy views,
//! policy, overlay and configuration are crate-internal.

use std::ffi::OsString;
use std::process::ExitCode;

/// Entry point of the `scene` role: export scene geometry, headless.
pub fn run(args: &[OsString]) -> ExitCode {
    cli::run(args)
}

pub use card::{Neighbourhood, Relation, class_card, class_card_in};
pub use config::{
    Band, GRADE_BEST, GRADE_MID, GRADE_WORST, MetricRating, Rating, metric_rating, rating,
};
pub use document::{Boundary, MailBoundary, ViewState, proposals};
pub use events::{Dims, ZOOM_STEP};
pub use geom::{Rect, vec2};
pub use highlight::Highlight;
pub use layout::{CHAR_W, text_w};
pub use overlay::Snapshot;
pub use search::Match;

pub mod cli;
pub mod document;
pub mod events;
pub mod highlight;
pub mod hit;
pub mod ir;
pub mod search;
pub mod targets;

mod card;
pub(crate) mod compose;
pub(crate) mod config;
pub(crate) mod curve;
pub(crate) mod geom;
pub(crate) mod hierarchy;
pub(crate) mod layout;
pub(crate) mod overlay;
pub(crate) mod policy;
pub(crate) mod route;
