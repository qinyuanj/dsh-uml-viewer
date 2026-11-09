//! The egui window's session (#161, #17, #18): the viewer session
//! (`viewer_session::Session`) plus the window's own events. It answers each
//! event with the repaint, GUI-cache and close requests the eframe shell
//! carries out; the clock is an input. The window runs standalone: no
//! companion, mail, process identity or restart (#18), so a session writes
//! nothing into the project but the proposal write-back it is asked for.
//!
//! Not the session snapshot (`session`, the panel's saved view) and not a
//! DSH session.

use std::path::PathBuf;

use edn::Value;
use edn::record::{get, local_name};
use scene::Rect;
use scene::document::ViewState;
use scene::events::{self, Dims};
use scene::hit;
use viewer_session::{Session, Start};

use crate::clock;
/// The clock the session takes as its input; the shell reads it.
pub(crate) use crate::clock::Now;
use crate::commands::{self, Command};
use crate::driver::ViewRecord;
use crate::project_files;
/// The window chrome: the default window and inspector size.
pub(crate) use crate::session::{Chrome, WINDOW_MIN, window_size};
/// Canvas input and the hot-reload cadence are the viewer session's.
pub(crate) use viewer_session::{Canvas, POLL_INTERVAL};

/// The launch settings the session opens with.
pub(crate) struct Launch {
    pub project_root: PathBuf,
    pub diagram: PathBuf,
}

impl Launch {
    /// Resolve the diagram: an explicit path wins over the default diagram.
    pub(crate) fn resolve(project_root: PathBuf, explicit: Option<PathBuf>) -> Launch {
        let default = viewer_session::default_diagram(&project_root);
        let diagram = project_files::canonical(explicit.as_deref().unwrap_or(&default));
        Launch {
            project_root,
            diagram,
        }
    }
}

/// Something the shell hands the session.
pub(crate) enum Event {
    /// A front-end action, with the canvas geometry at the time it was made.
    Action(Action, Dims),
    /// A frame: hot reload when the poll cadence elapsed or a refresh is due.
    Tick,
    /// A close (the close control, a termination signal, the test duration):
    /// there is nothing to release, so the window closes with 0 at once.
    Close,
    /// Canvas input the shell found uncaptured by the search box and the GUI,
    /// with the canvas geometry and modifiers at the time.
    Canvas(Canvas, Dims),
    /// A shell-side failure the status line reports (a source window that
    /// did not open).
    Notice(String),
}

/// What the inspector and class card ask the session to do. Opening a
/// source window is the shell's own (it is GUI state).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    /// Choose the real diagram (`None`) or a proposal as the discussion.
    Choose(Option<Value>),
    NewProposal,
    Declutter,
    Rename(Value, String),
    Delete(Value),
    CloseCard,
    Locate(Value),
}

/// Whether the shell should draw another frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Repaint {
    #[default]
    None,
    Now,
}

/// The side effects an event asks the shell for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Effects {
    pub repaint: Repaint,
    /// The inspector and card caches are stale (new document or proposals).
    pub invalidate_gui: bool,
    /// Close the window with this exit code.
    pub close: Option<u8>,
}

pub(crate) struct ViewerSession {
    /// The view, actions and hot reload.
    core: Session,
}

impl ViewerSession {
    /// Load the first view, fitted to the canvas.
    pub(crate) fn open(launch: Launch) -> Result<ViewerSession, String> {
        let core = Session::open(launch.project_root, launch.diagram, Start::Fresh)?;
        Ok(ViewerSession { core })
    }

    /// Apply one event at `now`.
    pub(crate) fn process(&mut self, event: Event, now: Now) -> Effects {
        match event {
            Event::Action(action, dims) => self.apply(action, &dims, now),
            Event::Tick => {
                // The frame that ran this tick draws the result; a reload
                // never asks for another one.
                self.core.tick(now.at);
                Effects::default()
            }
            Event::Close => Effects {
                close: Some(0),
                ..Effects::default()
            },
            Event::Canvas(input, dims) => {
                // The frame that carried the input draws its result.
                self.core.canvas(input, dims);
                Effects::default()
            }
            Event::Notice(line) => {
                self.core.set_status(Some(line));
                redraw()
            }
        }
    }

    /// Apply one front-end action. Every action redraws; proposal writeback
    /// also invalidates the GUI caches. A chosen discussion context stays in
    /// the view: no agent is listening to this window.
    fn apply(&mut self, action: Action, dims: &Dims, now: Now) -> Effects {
        let core_action = match action {
            Action::Choose(id) => viewer_session::Action::Choose(id),
            Action::NewProposal => viewer_session::Action::NewProposal {
                epoch: now.epoch,
                name: clock::proposal_name(now.epoch),
            },
            Action::Declutter => viewer_session::Action::Declutter,
            Action::Rename(id, name) => viewer_session::Action::Rename(id, name),
            Action::Delete(id) => viewer_session::Action::Delete(id),
            Action::CloseCard => viewer_session::Action::CloseCard,
            Action::Locate(id) => viewer_session::Action::Locate(id),
        };
        let outcome = self.core.act(core_action, dims);
        Effects {
            invalidate_gui: outcome.invalidate_gui,
            ..redraw()
        }
    }

    pub(crate) fn state(&self) -> &ViewState {
        self.core.state()
    }

    /// One diagnosis line for the class's quality rendering.
    pub(crate) fn quality_line(&self, id: &str) -> Option<String> {
        let target = Value::keyword(id);
        commands::commands(&self.state().scene, None, None)
            .into_iter()
            .find_map(|c| match c {
                Command::Class {
                    id: Some(cid),
                    missing,
                    band,
                    grade,
                    stroke,
                    ..
                } if cid == target => {
                    let stroke = frame::theme::rgb(stroke);
                    Some(format!(
                        "Detail missing={missing} band={band} continuousGrade={} stroke={},{},{}",
                        grade.map_or("nil".to_string(), |g| g.to_string()),
                        stroke.0,
                        stroke.1,
                        stroke.2
                    ))
                }
                _ => None,
            })
    }

    /// The navigation record the driver asserts (`:assert-view`); `anchor`
    /// is a screen point whose world position it carries. The search box is
    /// the shell's, so its part is left at the default for the shell to fill.
    pub(crate) fn view_record(&self, anchor: Option<[f64; 2]>) -> ViewRecord {
        let s = self.state();
        let name = |v: &Value| local_name(v).unwrap_or_default().to_owned();
        ViewRecord {
            focus: s.focus.iter().map(name).collect(),
            zoom: events::zoom_of(s),
            cam_x: s.cam_x,
            cam_y: s.cam_y,
            selected: s
                .selected
                .as_ref()
                .map(|v| get(v, "id").map(name).unwrap_or_else(|| name(v))),
            detail: s.detail_id.as_ref().map(name),
            declutter: get(&s.extra, "declutter")
                .map(name)
                .unwrap_or_else(|| "full".into()),
            dragging: s.drag.is_some(),
            anchor: anchor.map(|[x, y]| events::world_xy(s, x, y)),
            ..ViewRecord::default()
        }
    }

    /// Does the view equal a fresh fit of the canvas `dims` describe?
    pub(crate) fn is_fitted(&self, dims: &Dims) -> bool {
        let s = self.state();
        let fitted = events::fit(s, dims);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        !s.fit_pending
            && near(fitted.zoom, s.zoom)
            && near(fitted.cam_x, s.cam_x)
            && near(fitted.cam_y, s.cam_y)
    }

    /// No scene element under this screen point (the GUI's areas are the
    /// shell's to add).
    pub(crate) fn is_blank(&self, x: f64, y: f64) -> bool {
        let (wx, wy) = events::world_xy(self.state(), x, y);
        hit::at(&self.state().scene, wx, wy).is_none()
    }

    /// The screen point a few units inside a rendered class box (Lua
    /// `on_class`). The compiled scene prefixes ids per section (`d0-`);
    /// the authored IR id the driver names resolves through the suffix.
    pub(crate) fn class_point(&self, id: &str) -> Result<(f64, f64), String> {
        let s = self.state();
        let wanted = Value::keyword(id);
        let suffix = format!("-{id}");
        let class = hit::class_by_id(&s.scene, &wanted)
            .or_else(|| {
                edn::record::seq(get(&s.scene, "classes")).iter().find(|c| {
                    get(c, "id")
                        .and_then(local_name)
                        .is_some_and(|n| n.ends_with(&suffix))
                })
            })
            .ok_or_else(|| format!("no class {id} on the scene"))?;
        let r = get(class, "rect")
            .and_then(Rect::from_value)
            .ok_or_else(|| format!("class {id} has no rect"))?;
        let z = events::zoom_of(s);
        Ok(((r.x + 6.0 - s.cam_x) * z, (r.y + 6.0 - s.cam_y) * z))
    }

    /// The canvas hint (a missing diagram), if any.
    pub(crate) fn hint(&self) -> Option<&str> {
        self.core.hint()
    }

    /// The status line, if any.
    pub(crate) fn status(&self) -> Option<&str> {
        self.core.status()
    }
}

/// Every action redraws.
fn redraw() -> Effects {
    Effects {
        repaint: Repaint::Now,
        ..Effects::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    /// A fresh project directory per call, with no `.uml-viewer/` state.
    fn project(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "uml viewer session {label} {} {n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open(dir: &Path) -> ViewerSession {
        ViewerSession::open(Launch {
            project_root: dir.to_path_buf(),
            diagram: dir.join("diagram.edn"),
        })
        .unwrap()
    }

    /// Every file under `dir`, relative, sorted.
    fn files(dir: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for entry in std::fs::read_dir(&next).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let relative = path.strip_prefix(dir).unwrap();
                    found.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        found.sort();
        found
    }

    const SECOND: &str = "{:title \"Second diagram\" :packages [{:label \"P\" :classes [{:id :target :ns \"app.target\"}]}]}";

    fn at(start: Instant, ms: u64) -> Now {
        Now {
            at: start + std::time::Duration::from_millis(ms),
            epoch: 1_700_000_000,
        }
    }

    fn dims() -> Dims {
        Dims {
            window_w: 1500.0,
            window_h: 920.0,
            ..Dims::default()
        }
    }

    /// A screen point a few units inside the class box (the driver's
    /// `click-class`).
    fn inside(state: &ViewState, id: &str) -> (f64, f64) {
        let class = edn::record::seq(get(&state.scene, "classes"))
            .iter()
            .find(|c| get(c, "id") == Some(&Value::keyword(id)))
            .cloned()
            .expect("class on the scene");
        let r = get(&class, "rect")
            .and_then(scene::Rect::from_value)
            .unwrap();
        let z = events::zoom_of(state);
        ((r.x + 6.0 - state.cam_x) * z, (r.y + 6.0 - state.cam_y) * z)
    }

    #[test]
    fn idle_ticks_ask_for_no_repaint_and_a_changed_diagram_reloads_on_the_poll_cadence() {
        let dir = project("reload");
        std::fs::write(dir.join("diagram.edn"), "{:title \"First\"}").unwrap();
        let mut session = open(&dir);
        let title = |s: &ViewerSession| {
            s.state()
                .document
                .as_ref()
                .and_then(|d| get(d, "title"))
                .cloned()
        };
        let start = Instant::now();
        for ms in [0, 250, 500, 750] {
            assert_eq!(
                Effects::default(),
                session.process(Event::Tick, at(start, ms)),
                "an idle tick at {ms} ms must not repaint"
            );
        }
        let revision = session.state().revision;
        // A later mtime: the stamp is the file's modification time.
        std::fs::write(dir.join("diagram.edn"), "{:title \"Second\"}").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(dir.join("diagram.edn"))
            .unwrap()
            .set_modified(later)
            .unwrap();
        session.process(Event::Tick, at(start, 900));
        assert_eq!(
            Some(Value::String("First".into())),
            title(&session),
            "no reload before the poll interval elapsed"
        );
        session.process(Event::Tick, at(start, 1000));
        assert_eq!(Some(Value::String("Second".into())), title(&session));
        assert_eq!(revision + 1, session.state().revision);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn canvas_input_reaches_the_scene_core_through_the_session() {
        let dir = project("canvas");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let mut session = open(&dir);
        let now = at(Instant::now(), 0);
        assert!(session.state().fit_pending);
        session.process(Event::Canvas(Canvas::Fit, dims()), now);
        assert!(!session.state().fit_pending, "the fit checkpoint fits once");
        let (x, y) = inside(session.state(), "d0-target");
        session.process(Event::Canvas(Canvas::Click(x, y), dims()), now);
        assert_eq!(
            Some(Value::keyword("d0-target")),
            session
                .state()
                .selected
                .as_ref()
                .and_then(|s| get(s, "id"))
                .cloned()
        );
        session.process(Event::Canvas(Canvas::DoublePress(x, y), dims()), now);
        assert_eq!(Some(Value::keyword("d0-target")), session.state().detail_id);
        let zoom = session.state().zoom;
        let ctrl = Dims {
            control: true,
            ..dims()
        };
        session.process(Event::Canvas(Canvas::Wheel { dx: 0.0, dy: 1.0 }, ctrl), now);
        assert!(session.state().zoom > zoom, "ctrl+wheel zooms in");
        session.process(
            Event::Canvas(
                Canvas::View {
                    zoom: Some(2.0),
                    center: Some([100.0, 50.0]),
                },
                dims(),
            ),
            now,
        );
        let state = session.state();
        assert_eq!(2.0, state.zoom);
        assert_eq!(
            (100.0 - 1500.0 / 4.0, 50.0 - 920.0 / 4.0),
            (state.cam_x, state.cam_y)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_r_key_reloads_on_the_next_tick_without_waiting_for_the_cadence() {
        let dir = project("r key");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let mut session = open(&dir);
        let start = Instant::now();
        session.process(Event::Tick, at(start, 0));
        std::fs::remove_file(dir.join("diagram.edn")).unwrap();
        session.process(
            Event::Canvas(Canvas::Key("r".into()), dims()),
            at(start, 10),
        );
        session.process(Event::Tick, at(start, 10));
        assert!(session.hint().is_some(), "R forced the reload");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_notice_becomes_the_status_line() {
        let dir = project("notice");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let mut session = open(&dir);
        let effects = session.process(
            Event::Notice("Source not opened".into()),
            at(Instant::now(), 0),
        );
        assert_eq!(Some("Source not opened"), session.status());
        assert_eq!(Repaint::Now, effects.repaint);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_class_without_quality_data_has_a_red_quality_line() {
        let dir = project("quality");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let session = open(&dir);
        let line = session.quality_line("d0-target").expect("a class line");
        assert!(line.starts_with("Detail missing=true band=red"), "{line}");
        assert_eq!(None, session.quality_line("nowhere"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_driver_geometry_queries_answer_from_the_view_state() {
        let dir = project("geometry");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let mut session = open(&dir);
        let now = at(Instant::now(), 0);
        assert!(!session.is_fitted(&dims()), "a pending fit is not fitted");
        session.process(Event::Canvas(Canvas::Fit, dims()), now);
        assert!(session.is_fitted(&dims()));
        // The driver names the authored IR id; the scene prefixes it.
        let (x, y) = session.class_point("target").unwrap();
        assert_eq!(Ok((x, y)), session.class_point("d0-target"));
        assert!(!session.is_blank(x, y), "the class is under its own point");
        assert!(session.is_blank(-500.0, -500.0));
        assert!(session.class_point("nowhere").is_err());
        let record = session.view_record(Some([x, y]));
        assert_eq!(None, record.selected);
        assert_eq!("full", record.declutter);
        session.process(Event::Canvas(Canvas::Click(x, y), dims()), now);
        session.process(
            Event::Canvas(Canvas::Wheel { dx: 40.0, dy: 0.0 }, dims()),
            now,
        );
        assert!(!session.is_fitted(&dims()), "a pan leaves the fit");
        let record = session.view_record(None);
        assert_eq!(Some("d0-target".to_string()), record.selected);
        assert!(!record.dragging);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_standalone_session_writes_nothing_into_the_project() {
        let dir = project("standalone");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let mut session = open(&dir);
        let start = Instant::now();
        session.process(Event::Canvas(Canvas::Fit, dims()), at(start, 0));
        let (x, y) = inside(session.state(), "d0-target");
        session.process(Event::Canvas(Canvas::Click(x, y), dims()), at(start, 0));
        let effects = session.process(Event::Action(Action::Choose(None), dims()), at(start, 0));
        assert_eq!(Repaint::Now, effects.repaint);
        session.process(Event::Action(Action::Declutter, dims()), at(start, 10));
        session.process(Event::Tick, at(start, 300));
        assert_eq!(vec!["diagram.edn".to_string()], files(&dir));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_close_closes_with_zero_at_once() {
        let dir = project("close");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let mut session = open(&dir);
        assert_eq!(
            Effects {
                close: Some(0),
                ..Effects::default()
            },
            session.process(Event::Close, at(Instant::now(), 0))
        );
        assert_eq!(vec!["diagram.edn".to_string()], files(&dir));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_explicit_diagram_wins_over_the_default_and_a_launch_fits() {
        let dir = project("fresh");
        std::fs::write(dir.join("other.edn"), SECOND).unwrap();
        let launch = Launch::resolve(dir.clone(), Some(dir.join("other.edn")));
        assert_eq!(
            project_files::canonical(&dir.join("other.edn")),
            launch.diagram
        );
        let session = ViewerSession::open(launch).unwrap();
        assert!(session.state().fit_pending);
        assert!(session.state().document.is_some());
        let _ = std::fs::remove_dir_all(dir);
    }
}
