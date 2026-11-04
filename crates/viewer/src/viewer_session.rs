//! The viewer session (#161): the window-free part of the viewer. It holds
//! the view state and consumes `to-viewer.edn` mail, and answers each
//! event with the repaint and GUI-cache requests the eframe shell carries
//! out. It reads and writes the real project files, so it is not pure; its
//! only host seam is the [`CompanionLink`], and the clock is an input.
//!
//! Not the session snapshot (`session`, what a restart restores) and not
//! the Herdr session.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use companion::CodingAgent;
use companion::mailbox::Mailbox;
use edn::Value;
use edn::record::{get, local_name};
use scene::Rect;
use scene::document::{self, ViewState};
use scene::events::{self, Dims};
use scene::hit;

use crate::clock;
/// The clock the session takes as its input; the shell reads it.
pub(crate) use crate::clock::Now;
use crate::commands::{self, Command};
use crate::driver::ViewRecord;
use crate::mail::{self, DisplayBoundary};
use crate::session;
/// The window chrome a session snapshot saves beside the view.
pub(crate) use crate::session::{Chrome, WINDOW_MIN, window_size};
use crate::{
    detail, identity, inspector, mail_watch, mailbox, metrics, project_files, strings, watch,
};

/// What the session needs from the companion helper: poll its events, stop
/// it (keeping the companion or not), wake it for a delivery id, and tell
/// whether it settled. The production adapter wraps the real worker (a
/// mail-only launch is one with no helper); tests use a fake.
pub(crate) trait CompanionLink {
    /// The helper's diagnosis line at launch (`Companion: starting`, or why
    /// it did not start).
    fn launch_status(&self) -> Option<String>;
    /// Consume pending worker events.
    fn poll(&mut self) -> LinkPoll;
    /// Ask the helper to stop, keeping the companion or releasing it.
    fn stop(&mut self, stop: Stop) -> Result<(), String>;
    /// Wake the helper for the delivery `id` (the durable queue holds it).
    fn wake(&mut self, id: i64) -> Result<(), String>;
    /// `true` once the helper stopped or failed and exited (or never ran).
    fn settled(&self) -> bool;
}

/// How the helper stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The restart exit: the companion stays for the replacement viewer.
    KeepCompanion,
    /// A normal close: cancel recovery, remove the Herdr session and clear
    /// the ownership record.
    ReleaseCompanion,
}

/// One worker poll: did the helper reach ready on it, and its new status
/// line when its phase or message changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LinkPoll {
    pub ready: bool,
    pub status: Option<String>,
}

/// The launch settings the session opens with.
pub(crate) struct Launch {
    pub project_root: PathBuf,
    pub diagram: PathBuf,
    /// `--restart`: restore the saved session snapshot.
    pub restart: bool,
    /// `--backend herdr`: show the waiting canvas until `:display`.
    pub backend: bool,
    /// `--coding-agent`: the provider the launch named, for the worker.
    pub agent: Option<CodingAgent>,
    /// The saved session snapshot a restart restores (read once).
    pub snapshot: Option<Value>,
}

impl Launch {
    /// Resolve the diagram and read the session snapshot once: the saved
    /// path and window chrome are consulted only on `--restart`, and the
    /// same snapshot is what `open` restores. An explicit path wins over
    /// the saved one, which wins over the default diagram.
    pub(crate) fn resolve(
        project_root: PathBuf,
        explicit: Option<PathBuf>,
        restart: bool,
        backend: bool,
    ) -> (Launch, Chrome) {
        let snapshot = if restart {
            session::read(&project_root)
        } else {
            None
        };
        let default = default_diagram(&project_root);
        let diagram = canonical(&session::path(
            explicit.as_deref(),
            snapshot.as_ref(),
            &default,
        ));
        // A restart restores the saved window and inspector width; a fresh
        // launch uses the defaults.
        let chrome = session::chrome(snapshot.as_ref());
        let launch = Launch {
            project_root,
            diagram,
            restart,
            backend,
            agent: None,
            snapshot,
        };
        (launch, chrome)
    }
}

/// Default diagram (port of `session_file.default_diagram`): the generated
/// `diagram.edn`, the project-named example, then any example, else
/// `diagram.edn`.
fn default_diagram(root: &Path) -> PathBuf {
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

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Something the shell hands the session.
pub(crate) enum Event {
    /// The shell's own fallible startup (the bench) succeeded: a launch
    /// without a companion records its identity now, so a refused startup
    /// never overwrites an existing one.
    Started,
    /// A front-end action, with the canvas geometry at the time it was made.
    Action(Action, Dims),
    /// The mail watcher saw a change (or the startup frames).
    MailDue,
    /// A frame: hot reload when the poll cadence elapsed or a refresh is due.
    Tick,
    /// The frame's restart-exit checkpoint: after `:quit-for-restart`, stop
    /// the helper keeping the companion, poll it until it settled, then save
    /// the session snapshot (with this window chrome) and close with 0.
    RestartExit(Chrome),
    /// A normal close (the close control, a termination signal, the test
    /// duration): stop the helper without keeping the companion.
    NormalClose,
    /// The frame's normal-close checkpoint: close once the helper settled,
    /// or at the deadline regardless.
    NormalCloseCheckpoint,
    /// The window closed without the frame loop seeing it (the window manager
    /// closed it).
    WindowClosed,
    /// Canvas input the shell found uncaptured by the search box and the GUI,
    /// with the canvas geometry and modifiers at the time.
    Canvas(Canvas, Dims),
    /// A shell-side failure the status line reports (a source window that
    /// did not open).
    Notice(String),
}

/// Canvas input in view units (points from the canvas origin), from real
/// input and driver replay alike.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Canvas {
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
        rows: Vec<detail::Row>,
        delta: f64,
        view_h: f64,
    },
    /// The frame's fit checkpoint: fit once when a fit is pending.
    Fit,
    /// Set the zoom (keeping the window centre) and/or centre a world point
    /// (the driver's `:view`).
    View {
        zoom: Option<f64>,
        center: Option<[f64; 2]>,
    },
    /// The bench's unclamped camera: world centre and zoom; no fit.
    Camera {
        cx: f64,
        cy: f64,
        zoom: f64,
    },
    /// The shell owns the camera (the bench): drop a pending fit.
    HoldCamera,
}

/// How long a normal close waits for the helper to settle.
pub(crate) const NORMAL_CLOSE_DEADLINE: Duration = Duration::from_secs(5);

/// What the inspector, class card and menus ask the session to do. Opening
/// a source window is the shell's own (it is GUI state).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    /// Choose the real diagram (`None`) or a proposal as the discussion.
    Choose(Option<Value>),
    NewProposal,
    Declutter,
    Regen,
    Rename(Value, String),
    Delete(Value),
    CloseCard,
    Locate(Value),
    /// A right-click menu operation on the selection: mail it to the agent.
    Mail(String),
}

/// The hot-reload and mail cadence.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Whether the shell should draw another frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Repaint {
    #[default]
    None,
    Now,
    /// Draw again after this long (a stopping helper is polled).
    After(Duration),
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
    state: ViewState,
    project_root: PathBuf,
    diagram: PathBuf,
    hint: Option<String>,
    status: Option<String>,
    link: Box<dyn CompanionLink>,
    /// The latest helper or mailbox line; copied to `status` on a mail poll.
    link_status: Option<String>,
    inbox: Mailbox,
    mail_watch: mail_watch::Watch,
    /// A `--backend` launch: the identity waits for the helper's ready.
    backend: bool,
    /// The launch's explicit coding-agent provider, if any.
    agent: Option<CodingAgent>,
    /// The identity was recorded (at startup, or once the helper was ready).
    identity_recorded: bool,
    /// When the IR and metrics stamps were last polled; `None` is due.
    last_poll: Option<Instant>,
    force_refresh: bool,
    /// `:quit-for-restart` already asked the helper to stop with `keep`.
    restart_stop: bool,
    /// A normal close stops the helper; the viewer exits at this deadline.
    close_deadline: Option<Instant>,
}

impl ViewerSession {
    /// Load the first view, then start the companion link, then adopt the
    /// mail files as seen. The view (and its mail cursor) exists before the
    /// helper does, so a `:display` written during startup is consumed.
    pub(crate) fn open(
        launch: Launch,
        start_link: impl FnOnce() -> Box<dyn CompanionLink>,
    ) -> Result<ViewerSession, String> {
        let mut session = ViewerSession {
            state: ViewState::default(),
            inbox: Mailbox::inbox(&launch.project_root),
            project_root: launch.project_root,
            diagram: launch.diagram,
            hint: None,
            status: None,
            link: Box::new(NoLink),
            link_status: None,
            mail_watch: mail_watch::Watch::new(),
            backend: launch.backend,
            agent: launch.agent,
            identity_recorded: false,
            last_poll: None,
            force_refresh: false,
            restart_stop: false,
            close_deadline: None,
        };
        session.load_initial(launch.restart, launch.backend, launch.snapshot.as_ref())?;
        session.link = start_link();
        session.link_status = session.link.launch_status();
        session.mail_watch.baseline(&session.project_root);
        // The identity waits for `Event::Started` (or, with a companion, its
        // ready): a refused or invalid startup never overwrites an existing one.
        Ok(session)
    }

    /// Open with the production companion link: the real worker on a
    /// `--backend` launch, otherwise the mail queue without a helper.
    pub(crate) fn open_with_companion(launch: Launch) -> Result<ViewerSession, String> {
        let (root, restart, backend) =
            (launch.project_root.clone(), launch.restart, launch.backend);
        let agent = launch.agent;
        ViewerSession::open(launch, move || -> Box<dyn CompanionLink> {
            if backend {
                Box::new(mail::Companion::start(&root, restart, agent))
            } else {
                Box::new(mail::Companion::mail_only())
            }
        })
    }

    /// Apply one event at `now`.
    pub(crate) fn process(&mut self, event: Event, now: Now) -> Effects {
        match event {
            Event::Started => {
                if !self.backend {
                    self.record_identity();
                }
                Effects::default()
            }
            Event::Action(action, dims) => self.apply(action, &dims, now),
            Event::MailDue => self.poll_mail(),
            Event::Tick => {
                let due = self
                    .last_poll
                    .is_none_or(|at| now.at.saturating_duration_since(at) >= POLL_INTERVAL);
                if self.force_refresh || due {
                    self.force_refresh = false;
                    self.last_poll = Some(now.at);
                    self.reload();
                }
                // The frame that ran this tick draws the result; a reload
                // never asks for another one.
                Effects::default()
            }
            Event::RestartExit(chrome) => self.restart_exit(&chrome),
            Event::NormalClose => self.begin_normal_close(now),
            Event::NormalCloseCheckpoint => self.normal_close_checkpoint(now),
            Event::Canvas(input, dims) => {
                self.canvas(input, dims);
                Effects::default()
            }
            Event::Notice(line) => {
                self.status = Some(line);
                Effects {
                    repaint: Repaint::Now,
                    ..Effects::default()
                }
            }
            Event::WindowClosed => {
                // The helper then releases the session and ownership itself.
                if !self.link.settled() {
                    self.stop_link(Stop::ReleaseCompanion);
                }
                Effects::default()
            }
        }
    }

    /// Ask the helper to stop; a failure is reported, not fatal (the exit
    /// goes on).
    fn stop_link(&mut self, stop: Stop) {
        if let Err(error) = self.link.stop(stop) {
            eprintln!("uml viewer: could not stop the companion: {error}");
        }
    }

    /// `:quit-for-restart`: stop the helper keeping the companion (a failed helper needs
    /// no stop and must not strand the restart), poll until it settled so the
    /// replacement viewer binds the companion that is actually there, then
    /// save the snapshot and exit 0.
    fn restart_exit(&mut self, chrome: &Chrome) -> Effects {
        if !self.state.quit_for_restart && !self.restart_stop {
            return Effects::default();
        }
        if !self.restart_stop {
            self.stop_link(Stop::KeepCompanion);
            self.restart_stop = true;
        }
        self.poll_link();
        if !self.link.settled() {
            return Effects {
                repaint: Repaint::After(POLL_INTERVAL),
                ..Effects::default()
            };
        }
        let snapshot = session::snapshot(&self.state, chrome);
        if let Err(error) = session::save(&self.project_root, &snapshot) {
            eprintln!("uml viewer: could not save the session: {error}");
        }
        Effects {
            close: Some(0),
            ..Effects::default()
        }
    }

    /// Dispatch canvas input to the scene core. The frame that carried the
    /// input draws its result, so it asks for no repaint.
    fn canvas(&mut self, input: Canvas, mut dims: Dims) {
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

    /// A normal close asks the helper to stop without keeping the companion;
    /// the helper then cancels recovery, removes its Herdr session and clears
    /// the ownership record. A viewer without a helper has nothing to stop.
    fn begin_normal_close(&mut self, now: Now) -> Effects {
        if self.close_deadline.is_some() {
            return Effects::default();
        }
        self.close_deadline = Some(now.at + NORMAL_CLOSE_DEADLINE);
        self.stop_link(Stop::ReleaseCompanion);
        Effects {
            close: self.link.settled().then_some(0),
            ..Effects::default()
        }
    }

    /// Wait (bounded) for the helper to settle, then exit 0.
    fn normal_close_checkpoint(&mut self, now: Now) -> Effects {
        let Some(deadline) = self.close_deadline else {
            return Effects::default();
        };
        let settled = self.poll_link() || self.link.settled();
        if settled || now.at >= deadline {
            Effects {
                close: Some(0),
                ..Effects::default()
            }
        } else {
            // Poll the helper's events while it stops.
            Effects {
                repaint: Repaint::After(POLL_INTERVAL),
                ..Effects::default()
            }
        }
    }

    /// Apply one front-end action. Every action redraws; proposal writeback
    /// also invalidates the GUI caches.
    fn apply(&mut self, action: Action, dims: &Dims, now: Now) -> Effects {
        let outbox = Mailbox::outbox(&self.project_root);
        let mut effects = Effects {
            repaint: Repaint::Now,
            ..Effects::default()
        };
        let state = &self.state;
        match action {
            Action::Choose(id) => {
                let choice = inspector::choose(state, id.as_ref());
                self.state = choice.state;
                let extra: Vec<(Value, Value)> = match &choice.context {
                    Value::Map(entries) => entries.clone(),
                    _ => Vec::new(),
                };
                if let Err(error) = outbox.append("context", &extra) {
                    eprintln!("uml viewer: could not send the context: {error}");
                }
                self.state.discussion_context = Some(choice.context);
            }
            Action::NewProposal => {
                let name = clock::proposal_name(now.epoch);
                self.state =
                    events::add_proposal(state, now.epoch, &name, Some(&project_files::Files));
                effects.invalidate_gui = true;
            }
            Action::Declutter => self.state = events::cycle_declutter(state),
            Action::Regen => {
                // The durable queue is authoritative; the wake tells the
                // agent there is something to consume. Without a helper the
                // request still waits in the queue (#142).
                self.status = Some(match outbox.append("regen", &[]) {
                    Ok(cmd) => {
                        let id = companion::record::get_i64(&cmd, "id").unwrap_or(0);
                        if self.link.wake(id).is_ok() {
                            "Regen requested.".into()
                        } else {
                            format!(
                                "Regen queued; {} session not attached.",
                                companion::worker::agent_status_name(
                                    &self.project_root.to_string_lossy(),
                                    self.agent,
                                )
                            )
                        }
                    }
                    Err(_) => "Regen failed.".into(),
                });
                self.mail_watch.baseline(&self.project_root);
            }
            Action::Rename(id, name) => {
                self.state =
                    events::rename_proposal(state, Some(&id), &name, Some(&project_files::Files));
                effects.invalidate_gui = true;
            }
            Action::Delete(id) => {
                self.state = events::delete_proposal(state, Some(&id), Some(&project_files::Files));
                effects.invalidate_gui = true;
            }
            Action::CloseCard => self.state = events::close_detail(state),
            Action::Locate(id) => self.state = events::locate(state, &id, dims),
            Action::Mail(op) => {
                // The right-click menu writes the exact `to-agent.edn`
                // command and wakes the companion for that delivery.
                let (op, extra) = mailbox::menu_mail(state, state.selected.as_ref(), &op);
                match outbox.append(&op, &extra) {
                    Ok(cmd) => {
                        let id = companion::record::get_i64(&cmd, "id").unwrap_or(0);
                        if let Err(error) = self.link.wake(id) {
                            eprintln!("uml viewer: could not wake the companion: {error}");
                        }
                    }
                    Err(error) => self.status = Some(format!("Mail failed: {error}")),
                }
                self.mail_watch.baseline(&self.project_root);
            }
        }
        effects
    }

    /// Poll the IR and metrics stamps; reload when either changed.
    fn reload(&mut self) {
        // A companion launch shows the empty canvas until :display (#142).
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
        let (next, _, rejected) =
            document::reload(&self.state, &text, current_mtime, &snapshots, Some(stamp));
        if let Some(reason) = rejected {
            eprintln!("Document reload rejected: {reason}");
        }
        if next.document.is_some() {
            self.hint = None;
        }
        self.state = next;
    }

    pub(crate) fn state(&self) -> &ViewState {
        &self.state
    }

    /// What the mail driver observes: the cursor, whether the canvas still
    /// waits for `:display`, the loaded path and title and the discussion
    /// context.
    pub(crate) fn mail_record(&self) -> Value {
        let kw = Value::keyword;
        let s = &self.state;
        let title = s.document.as_ref().and_then(|d| get(d, "title"));
        let context = s
            .discussion_context
            .as_ref()
            .and_then(|c| get(c, "context"));
        Value::Map(vec![
            (kw("seen"), Value::Integer(s.mail_seen)),
            (kw("waiting"), Value::Bool(s.waiting)),
            (kw("path"), s.path.clone().map_or(Value::Nil, Value::String)),
            (kw("title"), title.cloned().unwrap_or(Value::Nil)),
            (kw("context"), context.cloned().unwrap_or(Value::Nil)),
        ])
    }

    /// The restart snapshot this session would save with `chrome`.
    pub(crate) fn session_record(&self, chrome: &Chrome) -> Value {
        session::snapshot(&self.state, chrome)
    }

    /// One diagnosis line for the class's quality rendering.
    pub(crate) fn quality_line(&self, id: &str) -> Option<String> {
        let target = Value::keyword(id);
        commands::commands(&self.state.scene, None, None)
            .into_iter()
            .find_map(|c| match c {
                Command::Class {
                    id: Some(cid),
                    missing,
                    band,
                    grade,
                    stroke,
                    ..
                } if cid == target => Some(format!(
                    "Detail missing={missing} band={band} continuousGrade={} stroke={},{},{}",
                    grade.map_or("nil".to_string(), |g| g.to_string()),
                    stroke.0,
                    stroke.1,
                    stroke.2
                )),
                _ => None,
            })
    }

    /// The navigation record the driver asserts (`:assert-view`); `anchor`
    /// is a screen point whose world position it carries. The search box is
    /// the shell's, so its part is left at the default for the shell to fill.
    pub(crate) fn view_record(&self, anchor: Option<[f64; 2]>) -> ViewRecord {
        let s = &self.state;
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
        let s = &self.state;
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
        let (wx, wy) = events::world_xy(&self.state, x, y);
        hit::at(&self.state.scene, wx, wy).is_none()
    }

    /// The screen point a few units inside a rendered class box (Lua
    /// `on_class`). The compiled scene prefixes ids per section (`d0-`);
    /// the authored IR id the driver names resolves through the suffix.
    pub(crate) fn class_point(&self, id: &str) -> Result<(f64, f64), String> {
        let s = &self.state;
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
        self.hint.as_deref()
    }

    /// The status line, if any.
    pub(crate) fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Poll the helper; `true` when it reached ready on this poll.
    fn poll_link(&mut self) -> bool {
        let polled = self.link.poll();
        if let Some(line) = polled.status {
            self.link_status = Some(line);
        }
        polled.ready
    }

    /// Write the process identity, once per session.
    fn record_identity(&mut self) {
        if self.identity_recorded {
            return;
        }
        if let Err(error) = identity::record(&self.project_root) {
            eprintln!("uml viewer: could not record the process identity: {error}");
        }
        self.identity_recorded = true;
    }

    fn load_initial(
        &mut self,
        restart: bool,
        backend: bool,
        snapshot: Option<&Value>,
    ) -> Result<(), String> {
        let (snapshots, stamp) = metrics::snapshot(&self.project_root);
        let path = self.diagram.to_string_lossy().into_owned();
        let root = self.project_root.to_string_lossy().into_owned();
        if backend && !restart {
            let mut state = document::waiting_state(Some(path), 0);
            state.project_root = Some(root);
            state.revision = 1;
            self.state = state;
            return Ok(());
        }
        let text = match std::fs::read_to_string(&self.diagram) {
            Ok(text) => text,
            Err(_) => {
                self.state = ViewState {
                    path: Some(path.clone()),
                    project_root: Some(root),
                    revision: 1,
                    ..ViewState::default()
                };
                self.hint = Some(strings::missing_file(&path));
                return Ok(());
            }
        };
        let doc = scene::ir::read_document(&text).map_err(|e| format!("{path}: {e}"))?;
        let mtime = watch::mtime(&self.diagram).ok();
        let mut state = document::new_state(
            doc,
            mtime.map(Value::String),
            None,
            Some(&session::view_options()),
            &snapshots,
            Some(stamp),
        );
        state.path = Some(path);
        state.project_root = Some(root);
        if restart {
            if let Some(snapshot) = snapshot {
                state = session::restore(&state, snapshot);
            }
        } else {
            state.fit_pending = true;
        }
        self.state = state;
        Ok(())
    }

    /// The mail loop: consume worker events, then the `to-viewer.edn`
    /// queue. A poll with nothing new does no work and asks for no repaint,
    /// so an idle viewer stays idle (#142).
    fn poll_mail(&mut self) -> Effects {
        let mut effects = Effects::default();
        let found = self.mail_watch.poll(&self.project_root);
        let mut changed = false;
        if found.worker {
            let ready = self.poll_link();
            changed = true;
            if ready {
                // Identity is written once, after the companion is ready.
                self.record_identity();
            }
        }
        if found.mail {
            if self.consume() {
                effects.invalidate_gui = true;
                changed = true;
            }
            // The pop rewrote the file this watcher reads.
            self.mail_watch.baseline(&self.project_root);
        }
        if let Some(status) = self.link_status.clone()
            && self.status.as_deref() != Some(status.as_str())
        {
            self.status = Some(status);
            changed = true;
        }
        if changed {
            effects.repaint = Repaint::Now;
        }
        effects
    }

    /// Take and apply every queued command, oldest first; each is popped
    /// only after it was applied, so the durable queue stays authoritative.
    fn consume(&mut self) -> bool {
        let boundary = DisplayBoundary::new(&self.project_root);
        let mut consumed = false;
        loop {
            let command = match self.inbox.take() {
                Ok(Some(command)) => command,
                Ok(None) => break,
                Err(error) => {
                    self.link_status = Some(format!("Mailbox: {error}"));
                    break;
                }
            };
            let next = document::apply_mail(&self.state, &command, &boundary);
            let path_changed = next.path != self.state.path;
            self.state = next;
            if let Some(path) = self.state.path.clone() {
                self.diagram = PathBuf::from(path);
            }
            if path_changed {
                self.hint = None;
            }
            consumed = true;
        }
        consumed
    }
}

/// Centre the window on the world point at the current zoom: the camera is
/// the top-left world point.
fn centre_on(state: &mut ViewState, [x, y]: [f64; 2], dims: &Dims) {
    state.cam_x = x - dims.window_w / (2.0 * state.zoom);
    state.cam_y = y - dims.window_h / (2.0 * state.zoom);
}

/// The link before `open` starts the real one.
struct NoLink;

impl CompanionLink for NoLink {
    fn launch_status(&self) -> Option<String> {
        None
    }
    fn poll(&mut self) -> LinkPoll {
        LinkPoll::default()
    }
    fn stop(&mut self, _stop: Stop) -> Result<(), String> {
        Ok(())
    }
    fn wake(&mut self, _id: i64) -> Result<(), String> {
        Ok(())
    }
    fn settled(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    /// What the fake link was asked to do.
    #[derive(Default)]
    struct Calls {
        woken: Vec<i64>,
        stops: Vec<Stop>,
        wake_fails: bool,
        /// The helper has not settled yet (it is still stopping).
        unsettled: bool,
        /// The next poll reports the helper reached ready.
        ready_next: bool,
        polls: usize,
    }

    /// The test adapter: no helper process; it records calls and fails a
    /// wake when told to.
    struct FakeLink(Rc<RefCell<Calls>>);

    impl CompanionLink for FakeLink {
        fn launch_status(&self) -> Option<String> {
            None
        }
        fn poll(&mut self) -> LinkPoll {
            let mut calls = self.0.borrow_mut();
            calls.polls += 1;
            LinkPoll {
                ready: std::mem::take(&mut calls.ready_next),
                status: None,
            }
        }
        fn stop(&mut self, stop: Stop) -> Result<(), String> {
            self.0.borrow_mut().stops.push(stop);
            Ok(())
        }
        fn wake(&mut self, id: i64) -> Result<(), String> {
            let mut calls = self.0.borrow_mut();
            calls.woken.push(id);
            if calls.wake_fails {
                Err("no helper".into())
            } else {
                Ok(())
            }
        }
        fn settled(&self) -> bool {
            !self.0.borrow().unsettled
        }
    }

    /// A fresh project directory per call.
    fn project(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "uml viewer session {label} {} {n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".uml-viewer")).unwrap();
        dir
    }

    const SECOND: &str = "{:title \"Second diagram\" :packages [{:label \"P\" :classes [{:id :target :ns \"app.target\"}]}]}";

    fn open(dir: &Path, backend: bool) -> (ViewerSession, Rc<RefCell<Calls>>) {
        let calls = Rc::new(RefCell::new(Calls::default()));
        let link = FakeLink(Rc::clone(&calls));
        let session = ViewerSession::open(
            Launch {
                project_root: dir.to_path_buf(),
                diagram: dir.join("diagram.edn"),
                restart: false,
                backend,
                agent: None,
                snapshot: None,
            },
            move || Box::new(link),
        )
        .unwrap();
        (session, calls)
    }

    fn at(start: Instant, ms: u64) -> Now {
        Now {
            at: start + std::time::Duration::from_millis(ms),
            epoch: 1_700_000_000,
        }
    }

    fn write_inbox(dir: &Path, text: &str) {
        std::fs::write(dir.join(".uml-viewer/to-viewer.edn"), text).unwrap();
    }

    fn context(state: &ViewState) -> Option<Value> {
        state
            .discussion_context
            .as_ref()
            .and_then(|c| get(c, "context"))
            .cloned()
    }

    const NESTED: &str = "{:title \"Nested\" :hierarchical true :prefix \"app\" :classes [{:id :a :name \"A\" :ns \"app.a\"} {:id :b :name \"B\" :ns \"app.b\"}] :edges [{:from :a :to :b :kind :dependency}]}";

    /// The compiled diagram of a state lays its source view out wide (#191).
    fn wide(state: &ViewState) -> bool {
        let diagram = get(&state.scene, "diagram");
        edn::record::flag(&state.extra, "wide")
            && diagram.and_then(|d| get(d, "direction")) == Some(&Value::keyword("lr"))
            && diagram.and_then(|d| get(d, "wide")).is_some()
    }

    #[test]
    fn launched_and_displayed_source_views_take_the_wide_layout() {
        let dir = project("wide");
        std::fs::write(dir.join("diagram.edn"), NESTED).unwrap();
        std::fs::write(dir.join("second.edn"), NESTED).unwrap();
        let (session, _) = open(&dir, false);
        assert!(wide(session.state()), "launch");
        let (mut session, _) = open(&dir, true);
        write_inbox(
            &dir,
            "{:next-id 2 :queue [{:id 1 :op :display :path \"second.edn\"}]}",
        );
        session.process(Event::MailDue, at(Instant::now(), 0));
        assert!(wide(session.state()), ":display");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn queued_mail_is_applied_oldest_first_and_popped() {
        let dir = project("mail");
        std::fs::write(dir.join("second.edn"), SECOND).unwrap();
        let (mut session, _) = open(&dir, true);
        assert!(
            session.state().waiting,
            "a backend launch waits for :display"
        );
        write_inbox(
            &dir,
            "{:next-id 3 :queue [{:id 1 :op :context :context :proposal :proposal-id :p :name \"中文\"} {:id 2 :op :display :path \"second.edn\"}]}",
        );
        let effects = session.process(Event::MailDue, at(Instant::now(), 0));
        let state = session.state();
        assert_eq!(2, state.mail_seen);
        assert!(!state.waiting, ":display ends the waiting canvas");
        assert_eq!(
            Some(&Value::String("Second diagram".into())),
            state.document.as_ref().and_then(|d| get(d, "title"))
        );
        assert_eq!(Some(Value::keyword("proposal")), context(state));
        assert_eq!(
            "{:next-id 3 :queue []}",
            std::fs::read_to_string(dir.join(".uml-viewer/to-viewer.edn")).unwrap()
        );
        assert_eq!(
            Effects {
                repaint: Repaint::Now,
                invalidate_gui: true,
                close: None,
            },
            effects
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn idle_ticks_ask_for_no_repaint_and_a_changed_diagram_reloads_on_the_poll_cadence() {
        let dir = project("reload");
        std::fs::write(dir.join("diagram.edn"), "{:title \"First\"}").unwrap();
        let (mut session, _) = open(&dir, false);
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
        assert_eq!(
            Effects::default(),
            session.process(Event::MailDue, at(start, 750)),
            "an idle mail poll must not repaint"
        );
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
    fn one_mail_poll_applies_the_whole_queue_before_the_shell_sees_quit_for_restart() {
        let dir = project("restart");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, calls) = open(&dir, false);
        write_inbox(
            &dir,
            "{:next-id 3 :queue [{:id 1 :op :context :context :real} {:id 2 :op :quit-for-restart}]}",
        );
        session.process(Event::MailDue, at(Instant::now(), 0));
        let state = session.state();
        assert!(state.quit_for_restart);
        assert_eq!(Some(Value::keyword("real")), context(state));
        assert_eq!(2, state.mail_seen);
        assert!(
            calls.borrow().stops.is_empty(),
            "the restart exit, not the mail, stops the helper"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn dims() -> Dims {
        Dims {
            window_w: 1500.0,
            window_h: 920.0,
            ..Dims::default()
        }
    }

    #[test]
    fn the_right_click_menu_writes_the_exact_to_agent_contract_and_wakes_each_delivery() {
        let dir = project("menu");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, calls) = open(&dir, false);
        let start = Instant::now();
        // Select the clicked class (compiled scene id `d0-target`).
        session.process(Event::Canvas(Canvas::Fit, dims()), at(start, 0));
        let (x, y) = inside(session.state(), "d0-target");
        session.process(Event::Canvas(Canvas::Click(x, y), dims()), at(start, 0));
        for op in [
            "refresh-crap",
            "refresh-mutate",
            "refresh-mutate-all",
            "omit",
        ] {
            let effects =
                session.process(Event::Action(Action::Mail(op.into()), dims()), at(start, 0));
            assert_eq!(Repaint::Now, effects.repaint);
        }
        assert_eq!(
            "{:next-id 5 :queue [{:id 1 :op :refresh-crap :target {:id :target :kind :class :ns \"app.target\"}} {:id 2 :op :refresh-mutate :target {:id :target :kind :class :ns \"app.target\"}} {:id 3 :op :refresh-mutate-all :target {:id :target :kind :class :ns \"app.target\"}} {:id 4 :op :omit :target {:id :target :kind :class :ns \"app.target\"}}]}",
            std::fs::read_to_string(dir.join(".uml-viewer/to-agent.edn")).unwrap()
        );
        assert_eq!(vec![1, 2, 3, 4], calls.borrow().woken);
        assert_eq!(
            Effects::default(),
            session.process(Event::MailDue, at(start, 0)),
            "the viewer's own writes are not news to its mail poll"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn regen_reports_whether_the_companion_was_woken_and_always_queues_the_request() {
        let dir = project("regen");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, calls) = open(&dir, false);
        let now = at(Instant::now(), 0);
        session.process(Event::Action(Action::Regen, dims()), now);
        assert_eq!(Some("Regen requested."), session.status());
        calls.borrow_mut().wake_fails = true;
        session.process(Event::Action(Action::Regen, dims()), now);
        assert_eq!(
            Some("Regen queued; Claude session not attached."),
            session.status()
        );
        assert_eq!(
            "{:next-id 3 :queue [{:id 1 :op :regen} {:id 2 :op :regen}]}",
            std::fs::read_to_string(dir.join(".uml-viewer/to-agent.edn")).unwrap()
        );
        assert_eq!(vec![1, 2], calls.borrow().woken);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The unattached status names the companion's provider: the recorded
    /// ownership first, else the launch's explicit provider.
    #[test]
    fn an_unattached_regen_names_the_codex_provider() {
        for recorded in [true, false] {
            let dir = project("regen codex");
            std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
            if recorded {
                std::fs::write(
                    dir.join(".uml-viewer/companion.edn"),
                    "{:coding-agent :codex :codex-session \"01a1a458-f60d-7561-aa06-d04df3bd14f7\"}",
                )
                .unwrap();
            }
            let calls = Rc::new(RefCell::new(Calls {
                wake_fails: true,
                ..Calls::default()
            }));
            let link = FakeLink(Rc::clone(&calls));
            let mut session = ViewerSession::open(
                Launch {
                    project_root: dir.to_path_buf(),
                    diagram: dir.join("diagram.edn"),
                    restart: false,
                    backend: false,
                    agent: (!recorded).then_some(CodingAgent::Codex),
                    snapshot: None,
                },
                move || Box::new(link),
            )
            .unwrap();
            session.process(Event::Action(Action::Regen, dims()), at(Instant::now(), 0));
            assert_eq!(
                Some("Regen queued; Codex session not attached."),
                session.status(),
                "recorded {recorded}"
            );
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn chrome() -> Chrome {
        Chrome {
            window_w: Some(1100.0),
            window_h: Some(700.0),
            inspector_w: Some(340.0),
        }
    }

    #[test]
    fn the_restart_exit_keeps_the_companion_and_saves_the_snapshot_only_once_it_settled() {
        let dir = project("restart exit");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, calls) = open(&dir, false);
        let start = Instant::now();
        assert_eq!(
            Effects::default(),
            session.process(Event::RestartExit(chrome()), at(start, 0)),
            "without :quit-for-restart the checkpoint does nothing"
        );
        assert!(calls.borrow().stops.is_empty());
        calls.borrow_mut().unsettled = true;
        write_inbox(&dir, "{:next-id 2 :queue [{:id 1 :op :quit-for-restart}]}");
        session.process(Event::MailDue, at(start, 0));
        let saved = dir.join(".uml-viewer/session.edn");
        for ms in [0, 250, 500] {
            let effects = session.process(Event::RestartExit(chrome()), at(start, ms));
            assert_eq!(
                Effects {
                    repaint: Repaint::After(POLL_INTERVAL),
                    ..Effects::default()
                },
                effects,
                "a stopping helper is polled on the cadence at {ms} ms"
            );
            assert!(!saved.exists(), "no snapshot before the helper settled");
        }
        assert_eq!(
            vec![Stop::KeepCompanion],
            calls.borrow().stops,
            "one stop, keeping the companion"
        );
        assert!(calls.borrow().polls >= 3, "the stopping helper is polled");
        calls.borrow_mut().unsettled = false;
        let effects = session.process(Event::RestartExit(chrome()), at(start, 750));
        assert_eq!(Some(0), effects.close);
        let snapshot = edn::parse(&std::fs::read_to_string(&saved).unwrap()).unwrap();
        assert_eq!(Some(&Value::Integer(1100)), get(&snapshot, "window-w"));
        assert_eq!(
            Some(&Value::Integer(340)),
            get(&snapshot, "inspector-width")
        );
        assert_eq!(Some(&Value::Integer(1)), get(&snapshot, "mail-seen"));
        assert_eq!(vec![Stop::KeepCompanion], calls.borrow().stops);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_normal_close_stops_without_keeping_and_exits_at_the_five_second_deadline() {
        let dir = project("normal close");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, calls) = open(&dir, false);
        let start = Instant::now();
        assert_eq!(
            Effects::default(),
            session.process(Event::NormalCloseCheckpoint, at(start, 0)),
            "before a close the checkpoint does nothing"
        );
        calls.borrow_mut().unsettled = true;
        assert_eq!(
            Effects::default(),
            session.process(Event::NormalClose, at(start, 0))
        );
        session.process(Event::NormalClose, at(start, 100));
        assert_eq!(
            vec![Stop::ReleaseCompanion],
            calls.borrow().stops,
            "one stop, not keeping"
        );
        for ms in [0, 1000, 4999] {
            assert_eq!(
                Effects {
                    repaint: Repaint::After(POLL_INTERVAL),
                    ..Effects::default()
                },
                session.process(Event::NormalCloseCheckpoint, at(start, ms)),
                "still waiting for the helper at {ms} ms"
            );
        }
        assert_eq!(
            Some(0),
            session
                .process(Event::NormalCloseCheckpoint, at(start, 5000))
                .close,
            "the deadline closes regardless"
        );
        assert!(!dir.join(".uml-viewer/session.edn").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_codex_normal_close_releases_the_companion_and_saves_no_snapshot() {
        let dir = project("codex close");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let calls = Rc::new(RefCell::new(Calls::default()));
        let link = FakeLink(Rc::clone(&calls));
        let mut session = ViewerSession::open(
            Launch {
                project_root: dir.to_path_buf(),
                diagram: dir.join("diagram.edn"),
                restart: false,
                backend: true,
                agent: Some(CodingAgent::Codex),
                snapshot: None,
            },
            move || Box::new(link),
        )
        .unwrap();
        let start = Instant::now();
        session.process(Event::NormalClose, at(start, 0));
        assert_eq!(vec![Stop::ReleaseCompanion], calls.borrow().stops);
        assert_eq!(
            Some(0),
            session
                .process(Event::NormalCloseCheckpoint, at(start, 10))
                .close
        );
        assert!(!dir.join(".uml-viewer/session.edn").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_normal_close_exits_as_soon_as_the_helper_settled() {
        let dir = project("settled close");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, calls) = open(&dir, false);
        let start = Instant::now();
        assert_eq!(
            Some(0),
            session.process(Event::NormalClose, at(start, 0)).close,
            "no helper to wait for"
        );
        let (mut session, calls2) = open(&dir, false);
        calls2.borrow_mut().unsettled = true;
        session.process(Event::NormalClose, at(start, 0));
        assert_eq!(
            None,
            session
                .process(Event::NormalCloseCheckpoint, at(start, 250))
                .close
        );
        calls2.borrow_mut().unsettled = false;
        assert_eq!(
            Some(0),
            session
                .process(Event::NormalCloseCheckpoint, at(start, 500))
                .close
        );
        assert_eq!(vec![Stop::ReleaseCompanion], calls.borrow().stops);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_closed_window_stops_an_unsettled_helper_without_keeping_it() {
        let dir = project("window closed");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, calls) = open(&dir, false);
        session.process(Event::WindowClosed, at(Instant::now(), 0));
        assert!(
            calls.borrow().stops.is_empty(),
            "a settled helper is left alone"
        );
        calls.borrow_mut().unsettled = true;
        session.process(Event::WindowClosed, at(Instant::now(), 0));
        assert_eq!(vec![Stop::ReleaseCompanion], calls.borrow().stops);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn identity_file(dir: &Path) -> PathBuf {
        dir.join(".uml-viewer").join(identity::FILE_NAME)
    }

    /// A new worker event file: what the mail watcher sees as worker news.
    fn worker_event(dir: &Path, n: u32) {
        std::fs::write(
            dir.join(format!(".uml-viewer/companion-worker-t.event-{n}.edn")),
            "{}",
        )
        .unwrap();
    }

    #[test]
    fn a_companion_launch_shows_the_waiting_canvas_and_records_its_identity_once_when_ready() {
        let dir = project("identity");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, calls) = open(&dir, true);
        let state = session.state();
        assert!(state.waiting, "a backend launch waits for :display");
        assert!(state.document.is_none(), "the canvas is empty");
        assert!(
            !identity_file(&dir).exists(),
            "no identity before the companion is ready"
        );
        let start = Instant::now();
        session.process(Event::Started, at(start, 0));
        assert!(
            !identity_file(&dir).exists(),
            "a companion launch waits for ready, not for the shell"
        );
        session.process(Event::Tick, at(start, 0));
        assert!(session.state().waiting, "hot reload does not end the wait");
        session.process(Event::MailDue, at(start, 0));
        assert!(!identity_file(&dir).exists(), "polled, not ready yet");
        calls.borrow_mut().ready_next = true;
        worker_event(&dir, 1);
        session.process(Event::MailDue, at(start, 250));
        assert!(identity_file(&dir).exists(), "recorded once ready");
        std::fs::remove_file(identity_file(&dir)).unwrap();
        calls.borrow_mut().ready_next = true;
        worker_event(&dir, 2);
        session.process(Event::MailDue, at(start, 500));
        assert!(!identity_file(&dir).exists(), "recorded only once");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_launch_without_a_companion_records_its_identity_only_once_the_shell_started() {
        let dir = project("identity open");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, _) = open(&dir, false);
        assert!(!session.state().waiting);
        assert!(
            !identity_file(&dir).exists(),
            "a shell startup that fails after open (the bench) must not overwrite the identity"
        );
        session.process(Event::Started, at(Instant::now(), 0));
        let text = std::fs::read_to_string(identity_file(&dir)).unwrap();
        assert!(
            text.starts_with(&format!("{{\"pid\":{},", std::process::id())),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_restart_restores_the_snapshot_it_resolved_once_without_refitting_and_keeps_consuming_mail()
    {
        let dir = project("restore");
        let diagram = dir.join("diagram.edn");
        std::fs::write(&diagram, SECOND).unwrap();
        std::fs::write(
            dir.join(".uml-viewer/session.edn"),
            format!(
                "{{:window-w 1100 :window-h 700 :inspector-width 340 :path \"{}\" :mail-seen 1 :focus [] :cam-x 33 :cam-y 21 :zoom 1.7 :selected {{:kind :class :id :d0-target}} :detail-id :d0-target :discussion-context {{:context :proposal :proposal-id :plan}}}}",
                diagram.display()
            ),
        )
        .unwrap();
        let (launch, chrome) = Launch::resolve(dir.clone(), None, true, false);
        assert_eq!(std::fs::canonicalize(&diagram).unwrap(), launch.diagram);
        assert_eq!(
            Chrome {
                window_w: Some(1100.0),
                window_h: Some(700.0),
                inspector_w: Some(340.0),
            },
            chrome
        );
        // The snapshot is read once, when the launch is resolved.
        std::fs::remove_file(dir.join(".uml-viewer/session.edn")).unwrap();
        let mut session = ViewerSession::open(launch, || Box::new(NoLink)).unwrap();
        let state = session.state();
        assert!(!state.fit_pending, "a restored view does not re-fit");
        assert_eq!((1.7, 33.0, 21.0), (state.zoom, state.cam_x, state.cam_y));
        assert_eq!(Some(Value::keyword("d0-target")), state.detail_id);
        assert_eq!(1, state.mail_seen);
        assert_eq!(Some(Value::keyword("proposal")), context(state));
        write_inbox(
            &dir,
            "{:next-id 3 :queue [{:id 2 :op :context :context :real}]}",
        );
        session.process(Event::MailDue, at(Instant::now(), 0));
        assert_eq!(2, session.state().mail_seen, "mail above the cursor");
        assert_eq!(Some(Value::keyword("real")), context(session.state()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_fresh_launch_ignores_the_saved_snapshot_and_fits() {
        let dir = project("fresh");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        std::fs::write(
            dir.join(".uml-viewer/session.edn"),
            "{:path \"/elsewhere.edn\" :window-w 1100 :zoom 1.7}",
        )
        .unwrap();
        let (launch, chrome) =
            Launch::resolve(dir.clone(), Some(dir.join("diagram.edn")), false, false);
        assert_eq!(Chrome::default(), chrome);
        let session = ViewerSession::open(launch, || Box::new(NoLink)).unwrap();
        assert!(session.state().fit_pending);
        let _ = std::fs::remove_dir_all(dir);
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
    fn canvas_input_reaches_the_scene_core_through_the_session() {
        let dir = project("canvas");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, _) = open(&dir, false);
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
        let (mut session, _) = open(&dir, false);
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
        let (mut session, _) = open(&dir, false);
        let effects = session.process(
            Event::Notice("Source not opened".into()),
            at(Instant::now(), 0),
        );
        assert_eq!(Some("Source not opened"), session.status());
        assert_eq!(Repaint::Now, effects.repaint);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_mail_record_reports_the_cursor_wait_title_and_context_after_display() {
        let dir = project("mail record");
        std::fs::write(dir.join("second.edn"), SECOND).unwrap();
        let (mut session, _) = open(&dir, true);
        let waiting = session.mail_record();
        assert_eq!(Some(&Value::Bool(true)), get(&waiting, "waiting"));
        assert!(
            get(&waiting, "title").is_none_or(|t| *t == Value::Nil),
            "no title before :display"
        );
        write_inbox(
            &dir,
            "{:next-id 3 :queue [{:id 1 :op :context :context :proposal :proposal-id :p :name \"中文\"} {:id 2 :op :display :path \"second.edn\"}]}",
        );
        session.process(Event::MailDue, at(Instant::now(), 0));
        let record = session.mail_record();
        assert_eq!(Some(&Value::Integer(2)), get(&record, "seen"));
        assert_eq!(Some(&Value::Bool(false)), get(&record, "waiting"));
        assert_eq!(
            Some(&Value::String("Second diagram".into())),
            get(&record, "title")
        );
        assert_eq!(Some(&Value::keyword("proposal")), get(&record, "context"));
        let path = get(&record, "path").cloned();
        assert!(
            matches!(&path, Some(Value::String(p)) if p.ends_with("second.edn")),
            "{path:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_class_without_quality_data_has_a_red_quality_line() {
        let dir = project("quality");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (session, _) = open(&dir, false);
        let line = session.quality_line("d0-target").expect("a class line");
        assert!(line.starts_with("Detail missing=true band=red"), "{line}");
        assert_eq!(None, session.quality_line("nowhere"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_session_record_is_the_snapshot_a_restart_would_save() {
        let dir = project("session record");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, _) = open(&dir, false);
        let record = session.session_record(&chrome());
        assert_eq!(Some(&Value::Integer(1100)), get(&record, "window-w"));
        write_inbox(&dir, "{:next-id 2 :queue [{:id 1 :op :quit-for-restart}]}");
        session.process(Event::MailDue, at(Instant::now(), 0));
        let expected = session.session_record(&chrome());
        session.process(Event::RestartExit(chrome()), at(Instant::now(), 0));
        let saved = std::fs::read_to_string(dir.join(".uml-viewer/session.edn")).unwrap();
        assert_eq!(expected, edn::parse(&saved).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_driver_geometry_queries_answer_from_the_view_state() {
        let dir = project("geometry");
        std::fs::write(dir.join("diagram.edn"), SECOND).unwrap();
        let (mut session, _) = open(&dir, false);
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
}
