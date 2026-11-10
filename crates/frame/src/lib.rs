//! The window-free viewer model (#16): the scene frame and the pure modules
//! the resident session and the viewer panel share. It links no window or
//! GUI stack.
//!
//! - `frame`: the drawable scene frame of one view, as typed draw lists
//!   (`Layers`, chrome) and as versioned JSON (`docs/scene-frame.md`);
//!   `cli::run_scene` is the `scene` role, `--frame` selecting it;
//! - `commands` (the scene → draw command model), `theme` (semantic color
//!   roles and the dark palette), `lod` (detail tiers, culling,
//!   decimation);
//! - `hover` (the optional bounded hover metadata of a resident frame, #26),
//! - `legend` (the diagram's legend, worded in the resident's language, #50),
//! - `detail` (class card), `discussion` (the discussion-context switcher's
//!   proposals and choice), `source` (source window),
//!   `strings` (UI text), `session` (view snapshots the resident restores),
//!   `watch` and `project_files` (file stamps and the project boundary).

pub mod cli;
pub mod commands;
pub mod detail;
pub mod discussion;
pub mod frame;
pub mod hover;
pub mod legend;
pub mod lod;
pub mod project_files;
pub mod session;
pub mod source;
pub mod strings;
pub mod theme;
pub mod watch;

pub use cli::run_scene;
pub use frame::{Frame, Layers};
