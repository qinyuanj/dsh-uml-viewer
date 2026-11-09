//! The viewer session (#17): the part of the viewer that does not draw. It
//! holds the view state and the discussion context, applies user actions,
//! hot-reloads the diagram and its quality snapshots, loads displayed
//! diagrams and restores a saved view. It knows no companion, mail, window
//! or plugin host: the egui window (`viewer`) and the resident mode
//! (`resident`, driven by the plugin host over stdin/stdout) are its two
//! shells. It reads the real project files, so it is not pure; the clock is
//! an input.
//!
//! - [`Session`]: the session itself;
//! - `display` (how a displayed diagram loads), `metrics` (quality snapshot
//!   loading and the reload stamp);
//! - `state`: the session state as JSON data the host persists;
//! - `resident`: the `resident` role (`docs/resident-protocol.md`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use edn::Value;
use scene::document::{self, ViewState};
use scene::events::{self, Dims};

use frame::{inspector, project_files, strings, watch};

pub mod display;
pub mod metrics;
pub mod resident;
pub mod state;

pub use resident::run as run_resident;

/// The hot-reload cadence.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How the session opens its first view.
#[derive(Debug, Clone, PartialEq)]
pub enum Start {
    /// Load the diagram and fit it to the canvas.
    Fresh,
    /// Show the waiting canvas until a diagram is displayed.
    Waiting,
    /// Load the diagram and reapply a saved view (the
    /// `frame::session::snapshot` map); without one keep the loaded view
    /// unfitted, as the window's restart always did.
    Restore(Option<Value>),
}

/// Canvas input in view units (points from the canvas origin), from real
/// input and driver replay alike.
#[derive(Debug, Clone, PartialEq)]
pub enum Canvas {
    Press(f64, f64),
    DoublePress(f64, f64),
    /// A right-button press: select what it hits for the element menu.
    SecondaryPress(f64, f64),
    Move(f64, f64),
    /// The pointer moved with the button down.
    Drag(f64, f64),
    Release(f64, f64),
    /// The pointer left the canvas (or went over a captured area).
    Leave,
    /// A press and release at one point (driver replay).
    Click(f64, f64),
    /// Press, drag and release in one step (driver replay).
    DragTo {
        from: [f64; 2],
        to: [f64; 2],
    },
    /// Wheel notches; the modifiers travel in the dims.
    Wheel {
        dx: f64,
        dy: f64,
    },
    /// A canvas key; `r` also forces a reload on the next tick.
    Key(String),
    /// The wheel over the class card scrolls its rows.
    ScrollCard {
        rows: Vec<frame::detail::Row>,
        delta: f64,
        view_h: f64,
    },
    /// The fit checkpoint: fit once when a fit is pending.
    Fit,
    /// Set the zoom (keeping the window centre) and/or centre a world point
    /// (the driver's `:view`).
    View {
        zoom: Option<f64>,
        center: Option<[f64; 2]>,
    },
    /// An unclamped camera: world centre and zoom; no fit.
    Camera {
        cx: f64,
        cy: f64,
        zoom: f64,
    },
    /// The shell owns the camera (the bench): drop a pending fit.
    HoldCamera,
}

/// What a shell asks the session to do beyond raw canvas input.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Choose the source diagram (`None`) or a proposal as the discussion.
    Choose(Option<Value>),
    /// Add a proposal with the shell's clock reading and default name.
    NewProposal {
        epoch: i64,
        name: String,
    },
    Declutter,
    Rename(Value, String),
    Delete(Value),
    CloseCard,
    /// Enter a search result: its namespace, selection, card and centre.
    Locate(Value),
    /// Enter a component (namespace, proposal package or nested group).
    Drill(Value),
    /// Leave one level.
    Back,
    /// **P**: return to the namespace tree of the source diagram.
    NamespaceTree,
    /// Select a hit (`None` clears): the dependency chain follows it.
    Select(Option<Value>),
}

/// What an action changed besides the view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// The document's proposals changed: GUI caches are stale.
    pub invalidate_gui: bool,
    /// The discussion context the user chose: the shell tells the companion.
    pub context: Option<Value>,
}

pub struct Session {
    state: ViewState,
    project_root: PathBuf,
    diagram: PathBuf,
    hint: Option<String>,
    status: Option<String>,
    /// When the IR and metrics stamps were last polled; `None` is due.
    last_poll: Option<Instant>,
    force_refresh: bool,
    /// SHA-256 of the exact UTF-8 bytes accepted by the document parser.
    loaded_digest: Option<String>,
}

impl Session {
    /// Open the first view of `diagram` in `project_root`. Only an
    /// unreadable IR is an error; a missing diagram opens with the hint.
    pub fn open(project_root: PathBuf, diagram: PathBuf, start: Start) -> Result<Session, String> {
        let mut session = Session {
            state: ViewState::default(),
            project_root,
            diagram,
            hint: None,
            status: None,
            last_poll: None,
            force_refresh: false,
            loaded_digest: None,
        };
        session.load_initial(start)?;
        Ok(session)
    }

    fn load_initial(&mut self, start: Start) -> Result<(), String> {
        let (snapshots, stamp) = metrics::snapshot(&self.project_root);
        let path = self.diagram.to_string_lossy().into_owned();
        let root = self.project_root.to_string_lossy().into_owned();
        if start == Start::Waiting {
            let mut state = document::waiting_state(Some(path), 0);
            state.project_root = Some(root);
            state.revision = 1;
            self.state = state;
            return Ok(());
        }
        let text = match std::fs::read_to_string(&self.diagram) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.state = ViewState {
                    path: Some(path.clone()),
                    project_root: Some(root),
                    revision: 1,
                    ..ViewState::default()
                };
                self.hint = Some(strings::missing_file(&path));
                return Ok(());
            }
            Err(error) => return Err(format!("cannot read {path}: {error}")),
        };
        let doc = scene::ir::read_document(&text).map_err(|e| format!("{path}: {e}"))?;
        self.loaded_digest = Some(display::digest(&text));
        let mtime = watch::mtime(&self.diagram).ok();
        let mut state = document::new_state(
            doc,
            mtime.map(Value::String),
            None,
            Some(&frame::session::view_options()),
            &snapshots,
            Some(stamp),
        );
        state.path = Some(path);
        state.project_root = Some(root);
        match start {
            Start::Restore(Some(snapshot)) => state = frame::session::restore(&state, &snapshot),
            Start::Restore(None) => {}
            _ => state.fit_pending = true,
        }
        self.state = state;
        Ok(())
    }

    /// A frame or poll at `at`: reload when the cadence elapsed or a
    /// refresh is due. `true` when the view changed.
    pub fn tick(&mut self, at: Instant) -> bool {
        let due = self
            .last_poll
            .is_none_or(|last| at.saturating_duration_since(last) >= POLL_INTERVAL);
        if !(self.force_refresh || due) {
            return false;
        }
        self.force_refresh = false;
        self.last_poll = Some(at);
        let revision = self.state.revision;
        self.reload();
        self.state.revision != revision
    }

    /// Reload on the next tick regardless of the cadence.
    pub fn refresh(&mut self) {
        self.force_refresh = true;
    }

    /// Poll the IR and metrics stamps; reload when either changed.
    fn reload(&mut self) {
        // A waiting canvas stays empty until a diagram is displayed.
        if self.state.waiting {
            return;
        }
        let path_text = self.diagram.to_string_lossy().into_owned();
        let Ok(mtime) = watch::mtime(&self.diagram) else {
            self.hint = Some(strings::missing_file(&path_text));
            return;
        };
        let (snapshots, stamp) = metrics::snapshot(&self.project_root);
        let current_mtime = Some(Value::String(mtime));
        if current_mtime == self.state.mtime && Some(stamp.clone()) == self.state.metrics_stamp {
            return;
        }
        let Ok(text) = std::fs::read_to_string(&self.diagram) else {
            self.hint = Some(strings::missing_file(&path_text));
            return;
        };
        let (next, reloaded, rejected) =
            document::reload(&self.state, &text, current_mtime, &snapshots, Some(stamp));
        if let Some(reason) = rejected {
            eprintln!("Document reload rejected: {reason}");
        } else if reloaded {
            self.loaded_digest = Some(display::digest(&text));
        }
        if next.document.is_some() {
            self.hint = None;
        }
        self.state = next;
    }

    /// Dispatch canvas input to the scene core.
    pub fn canvas(&mut self, input: Canvas, mut dims: Dims) {
        let s = &self.state;
        if input == Canvas::Fit && !s.fit_pending {
            return;
        }
        self.state = match input {
            Canvas::Press(x, y) => events::on_left_press(s, x, y),
            Canvas::DoublePress(x, y) => events::on_double_press(s, x, y),
            Canvas::SecondaryPress(x, y) => events::on_secondary_press(s, x, y),
            Canvas::Move(x, y) => events::on_move(s, x, y),
            Canvas::Drag(x, y) => events::on_drag(s, x, y),
            Canvas::Release(x, y) => events::on_release(s, x, y),
            Canvas::Leave => events::on_leave(s),
            Canvas::Click(x, y) => events::on_release(&events::on_left_press(s, x, y), x, y),
            Canvas::DragTo { from, to } => {
                let pressed = events::on_left_press(s, from[0], from[1]);
                let dragged = events::on_drag(&pressed, to[0], to[1]);
                events::on_release(&dragged, to[0], to[1])
            }
            Canvas::Wheel { dx, dy } => events::on_wheel(s, dx, dy, &dims),
            Canvas::Key(key) => {
                if key == "r" {
                    self.force_refresh = true;
                }
                dims.raw_key = Some(key.clone());
                events::on_key(s, Some(&Value::String(key)), &dims)
            }
            Canvas::ScrollCard {
                rows,
                delta,
                view_h,
            } => inspector::scroll_rows(s, &rows, delta, view_h),
            Canvas::Fit => events::settle(s, &dims),
            Canvas::View { zoom, center } => {
                let mut next = s.clone();
                if let Some(zoom) = zoom {
                    // Keep the view centre fixed, like ctrl+wheel at the middle.
                    let factor = zoom / next.zoom.max(1e-6);
                    let (cx, cy) = (dims.window_w / 2.0, dims.window_h / 2.0);
                    next = events::zoom_at(&next, factor, cx, cy);
                    next.fit_pending = false;
                }
                if let Some(point) = center {
                    centre_on(&mut next, point, &dims);
                }
                next
            }
            Canvas::Camera { cx, cy, zoom } => {
                let mut next = s.clone();
                next.zoom = zoom;
                centre_on(&mut next, [cx, cy], &dims);
                next.fit_pending = false;
                next
            }
            Canvas::HoldCamera => ViewState {
                fit_pending: false,
                ..s.clone()
            },
        };
    }

    /// Apply one action with the canvas geometry it was made in.
    pub fn act(&mut self, action: Action, dims: &Dims) -> Outcome {
        let mut outcome = Outcome::default();
        let state = &self.state;
        let files = Some(&project_files::Files as &dyn document::Boundary);
        self.state = match action {
            Action::Choose(id) => {
                let choice = inspector::choose(state, id.as_ref());
                let mut next = choice.state;
                next.discussion_context = Some(choice.context.clone());
                outcome.context = Some(choice.context);
                next
            }
            Action::NewProposal { epoch, name } => {
                outcome.invalidate_gui = true;
                events::add_proposal(state, epoch, &name, files)
            }
            Action::Declutter => events::cycle_declutter(state),
            Action::Rename(id, name) => {
                outcome.invalidate_gui = true;
                events::rename_proposal(state, Some(&id), &name, files)
            }
            Action::Delete(id) => {
                outcome.invalidate_gui = true;
                events::delete_proposal(state, Some(&id), files)
            }
            Action::CloseCard => events::close_detail(state),
            Action::Locate(id) => events::locate(state, &id, dims),
            Action::Drill(id) => events::drill(state, &id),
            Action::Back => events::back(state),
            Action::NamespaceTree => events::show_proposal(state, None),
            Action::Select(hit) => events::select(state, hit.as_ref()),
        };
        outcome
    }

    /// Apply one host command (`{:op :display :path …}` or
    /// `{:op :context …}`) through the shared scene command boundary.
    pub fn apply_command(&mut self, command: &Value) {
        let boundary = display::DisplayBoundary::new(&self.project_root);
        let next = document::apply_mail(&self.state, command, &boundary);
        if let Some(digest) = boundary.loaded_digest() {
            self.loaded_digest = Some(digest);
        }
        let path_changed = next.path != self.state.path;
        self.state = next;
        if let Some(path) = self.state.path.clone() {
            self.diagram = PathBuf::from(path);
        }
        if path_changed {
            self.hint = None;
        }
    }

    /// Reapply a saved view (a `frame::session::snapshot` map) to the
    /// loaded diagram, as a restart does; a waiting canvas has nothing to
    /// restore onto and stays as it is.
    pub fn restore(&mut self, snapshot: &Value) {
        if self.state.document.is_none() {
            return;
        }
        self.state = frame::session::restore(&self.state, snapshot);
    }

    /// Persist the fingerprint from this parser generation, never a later file read.
    pub fn saved_state(&self) -> serde_json::Value {
        let mut saved = state::to_json(&self.state);
        saved["loadedDiagram"] = if self.state.waiting || self.state.document.is_none() {
            serde_json::Value::Null
        } else {
            self.loaded_digest.as_ref().map_or(
                serde_json::Value::Null,
                |digest| serde_json::json!({"version": 1, "sha256": digest}),
            )
        };
        saved
    }

    pub fn state(&self) -> &ViewState {
        &self.state
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// The diagram the session shows (or waits to show).
    pub fn diagram(&self) -> &Path {
        &self.diagram
    }

    /// The canvas hint (a missing diagram), if any.
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// The status line, if any.
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn set_status(&mut self, line: Option<String>) {
        self.status = line;
    }
}

/// Centre the window on the world point at the current zoom: the camera is
/// the top-left world point.
fn centre_on(state: &mut ViewState, [x, y]: [f64; 2], dims: &Dims) {
    state.cam_x = x - dims.window_w / (2.0 * state.zoom);
    state.cam_y = y - dims.window_h / (2.0 * state.zoom);
}

/// The default diagram of a project: the generated `diagram.edn`, the
/// project-named example, then any example, else `diagram.edn`.
pub fn default_diagram(root: &Path) -> PathBuf {
    let candidates = [
        root.join("diagram.edn"),
        root.join("examples")
            .join(
                root.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
            .with_extension("edn"),
        root.join("examples/uml-viewer.edn"),
    ];
    for candidate in &candidates {
        if candidate.is_file() {
            return candidate.clone();
        }
    }
    if let Ok(entries) = std::fs::read_dir(root.join("examples"))
        && let Some(found) = entries
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "edn"))
    {
        return found;
    }
    candidates[0].clone()
}
