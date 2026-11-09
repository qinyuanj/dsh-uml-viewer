//! The window-free viewer model (#16): the scene frame and the pure modules
//! the egui window and the viewer panel share. No egui, eframe or winit.
//!
//! - `frame`: the drawable scene frame of one view, as typed draw lists
//!   (`Layers`, chrome) and as versioned JSON (`docs/scene-frame.md`);
//!   `cli::run_scene` is the `scene` role, `--frame` selecting it;
//! - `commands` (the scene → draw command model), `theme` (semantic color
//!   roles and the dark palette), `lod` (detail tiers, culling,
//!   decimation), `render_cache` (draw lists rebuilt only on change);
//! - `hover` (the optional bounded hover metadata of a resident frame, #26),
//! - `detail` (class card), `inspector`, `source` (source window),
//!   `search_box`, `strings` (UI text), `session` (window session file),
//!   `watch` and `project_files` (file stamps and the project boundary).

pub mod cli;
pub mod commands;
pub mod detail;
pub mod frame;
pub mod hover;
pub mod inspector;
pub mod lod;
pub mod project_files;
pub mod render_cache;
pub mod search_box;
pub mod session;
pub mod source;
pub mod strings;
pub mod theme;
pub mod watch;

pub use cli::run_scene;
pub use frame::{Frame, Layers};
