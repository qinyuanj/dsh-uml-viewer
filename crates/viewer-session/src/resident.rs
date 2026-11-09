//! `uml-viewer resident` (#17): the viewer session as a long-running child
//! of the plugin host. One JSON action per stdin line; one JSON event per
//! stdout line: scene frames (`docs/scene-frame.md`) and the events the
//! host acts on. The protocol is `docs/resident-protocol.md`.
//!
//! ```text
//! uml-viewer resident [--project <dir>]
//! ```
//!
//! The project is the working directory unless `--project` names one.
//! End of input ends the session with exit 0.

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Instant;

use edn::Value;
use scene::events::Dims;
use serde_json::{Value as Json, json};

use crate::{Action, Canvas, POLL_INTERVAL, Session, Start, state};

pub const SCHEMA: &str = "dsh-uml-viewer/resident";
pub const VERSION: i64 = 1;

/// The actions this version understands, in the order the doc lists them.
pub const ACTIONS: &[&str] = &[
    "open",
    "viewport",
    "camera",
    "display",
    "reload",
    "drill-in",
    "back",
    "namespace-tree",
    "select",
    "search",
    "locate",
    "context",
    "declutter",
    "save",
    "quit",
];

const USAGE: &str = "usage: uml-viewer resident [--project <dir>]";

/// The `resident` role.
pub fn run(args: &[OsString]) -> ExitCode {
    let project = match parse(args) {
        Ok(project) => project,
        Err(message) => {
            eprintln!("uml-viewer resident: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let project = frame::project_files::canonical(&project);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut resident = Resident::new(project);
    let mut emit = |lines: Vec<Json>| -> bool {
        lines
            .iter()
            .all(|line| writeln!(out, "{line}").and_then(|_| out.flush()).is_ok())
    };
    if !emit(vec![ready()]) {
        return ExitCode::FAILURE;
    }
    loop {
        let lines = match rx.recv_timeout(POLL_INTERVAL) {
            Ok(line) => resident.line(&line),
            Err(RecvTimeoutError::Timeout) => resident.tick(Instant::now()),
            Err(RecvTimeoutError::Disconnected) => return ExitCode::SUCCESS,
        };
        if !emit(lines) {
            // The host closed our stdout: nobody is listening any more.
            return ExitCode::SUCCESS;
        }
        if resident.quit {
            return ExitCode::SUCCESS;
        }
    }
}

fn parse(args: &[OsString]) -> Result<PathBuf, String> {
    let mut project = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].to_str() {
            Some("--project") => {
                i += 1;
                project = Some(PathBuf::from(
                    args.get(i).ok_or("--project requires a directory")?,
                ));
            }
            _ => return Err(format!("unknown argument {:?}", args[i])),
        }
        i += 1;
    }
    match project {
        Some(project) => Ok(project),
        None => std::env::current_dir().map_err(|e| format!("no working directory: {e}")),
    }
}

/// The first line: who is speaking, and what it understands.
pub fn ready() -> Json {
    json!({
        "event": "ready",
        "schema": SCHEMA,
        "version": VERSION,
        "frameSchema": frame::frame::SCHEMA,
        "frameVersion": frame::frame::VERSION,
        "stateSchema": state::SCHEMA,
        "stateVersion": state::VERSION,
        "actions": ACTIONS,
    })
}

fn event(name: &str, seq: &Json, mut fields: Json) -> Json {
    fields["event"] = json!(name);
    fields["version"] = json!(VERSION);
    fields["seq"] = seq.clone();
    fields
}

fn error(seq: &Json, message: impl Into<String>) -> Json {
    event("error", seq, json!({"message": message.into()}))
}

/// A saved view whose diagram was not there when the session opened
/// waiting: the first display of that diagram restores it, and until then
/// `save` answers it unchanged, so closing the panel early loses nothing.
struct Pending {
    diagram: PathBuf,
    snapshot: Value,
    state: Json,
}

/// The protocol state: the session once opened, and the client viewport.
pub struct Resident {
    project: PathBuf,
    session: Option<Session>,
    pending: Option<Pending>,
    width: f64,
    height: f64,
    quit: bool,
}

impl Resident {
    pub fn new(project: PathBuf) -> Resident {
        Resident {
            project,
            session: None,
            pending: None,
            width: frame::cli::DEFAULT_WIDTH,
            height: frame::cli::DEFAULT_HEIGHT,
            quit: false,
        }
    }

    fn dims(&self) -> Dims {
        Dims {
            window_w: self.width,
            window_h: self.height,
            ..Dims::default()
        }
    }

    /// The frame of the current view, after its pending fit.
    fn frame(&mut self, seq: &Json) -> Json {
        let dims = self.dims();
        let Some(session) = self.session.as_mut() else {
            return error(seq, "no session: send open first");
        };
        session.canvas(Canvas::Fit, dims);
        let built = frame::frame::build(
            session.state(),
            self.width,
            self.height,
            session.hint(),
            session.status(),
        );
        event("frame", seq, json!({"frame": built.to_json()}))
    }

    /// A poll without input: a hot reload answers with an unprompted frame.
    pub fn tick(&mut self, at: Instant) -> Vec<Json> {
        let reloaded = self.session.as_mut().is_some_and(|s| s.tick(at));
        if reloaded {
            vec![self.frame(&Json::Null)]
        } else {
            Vec::new()
        }
    }

    /// Handle one input line.
    pub fn line(&mut self, line: &str) -> Vec<Json> {
        if line.trim().is_empty() {
            return Vec::new();
        }
        let action: Json = match serde_json::from_str(line) {
            Ok(action @ Json::Object(_)) => action,
            Ok(_) => return vec![error(&Json::Null, "an action is a JSON object")],
            Err(e) => return vec![error(&Json::Null, format!("not JSON: {e}"))],
        };
        let seq = action.get("seq").cloned().unwrap_or(Json::Null);
        if action.get("version").and_then(Json::as_i64) != Some(VERSION) {
            return vec![error(&seq, format!("version must be {VERSION}"))];
        }
        let Some(name) = action.get("action").and_then(Json::as_str) else {
            return vec![error(&seq, "missing action")];
        };
        match self.handle(name, &action, &seq) {
            Ok(lines) => lines,
            Err(message) => vec![error(&seq, message)],
        }
    }

    fn session(&mut self) -> Result<&mut Session, String> {
        self.session
            .as_mut()
            .ok_or_else(|| "no session: send open first".to_owned())
    }

    fn handle(&mut self, name: &str, action: &Json, seq: &Json) -> Result<Vec<Json>, String> {
        let dims = self.dims();
        match name {
            "open" => {
                // Validate both candidates before committing either: an
                // invalid state/diagram must retain the current viewport,
                // and an invalid viewport must retain the current session.
                let (width, height) = self.viewport(action)?;
                self.open(action)?;
                self.width = width;
                self.height = height;
            }
            "viewport" => {
                let (width, height) = self.viewport(action)?;
                self.session()?;
                self.width = width;
                self.height = height;
            }
            "camera" => {
                let camera = |key: &str| {
                    action
                        .get(key)
                        .and_then(Json::as_f64)
                        .filter(|n| n.is_finite())
                        .ok_or_else(|| format!("camera needs a number {key}"))
                };
                let (x, y, zoom) = (camera("x")?, camera("y")?, camera("zoom")?);
                if zoom <= 0.0 {
                    return Err("camera zoom must be positive".into());
                }
                // The frame's camera is the top-left world point.
                let (cx, cy) = (
                    x + dims.window_w / (2.0 * zoom),
                    y + dims.window_h / (2.0 * zoom),
                );
                self.session()?
                    .canvas(Canvas::Camera { cx, cy, zoom }, dims);
                return Ok(vec![event("ack", seq, json!({}))]);
            }
            "display" => {
                let path = text(action, "path")?;
                self.session()?;
                let resolved = resolve(&self.project, path);
                let text = std::fs::read_to_string(&resolved)
                    .map_err(|e| format!("cannot display {}: {e}", resolved.display()))?;
                scene::ir::read_document(&text)
                    .map_err(|e| format!("cannot display {}: {e}", resolved.display()))?;
                let mut command = Value::Map(vec![]);
                command.assoc(Value::keyword("op"), Value::keyword("display"));
                command.assoc(Value::keyword("path"), Value::String(path.to_owned()));
                let pending = self.pending.take();
                let session = self.session()?;
                session.apply_command(&command);
                if let Some(pending) = pending
                    && frame::project_files::canonical(session.diagram()) == pending.diagram
                    && same_loaded_diagram(&pending.state, session)
                {
                    session.restore(&pending.snapshot);
                }
            }
            "reload" => {
                let session = self.session()?;
                session.refresh();
                session.tick(Instant::now());
            }
            "drill-in" => {
                let session = self.session()?;
                let target = match action.get("at") {
                    Some(at) if !at.is_null() => {
                        let hit = hit_at(session, at)?;
                        scene::events::layer_id(hit.as_ref())
                    }
                    _ => Some(edn_field(action, "id")?),
                };
                if let Some(id) = target {
                    session.act(Action::Drill(id), &dims);
                }
            }
            "back" => {
                self.session()?.act(Action::Back, &dims);
            }
            "namespace-tree" => {
                self.session()?.act(Action::NamespaceTree, &dims);
            }
            "select" => {
                let session = self.session()?;
                let hit = match action.get("at") {
                    None => return Err("select needs at (a world point, or null)".into()),
                    Some(Json::Null) => None,
                    Some(at) => hit_at(session, at)?,
                };
                session.act(Action::Select(hit), &dims);
            }
            "search" => {
                let query = text(action, "query")?;
                let results = self
                    .session()?
                    .state()
                    .document
                    .as_ref()
                    .map(|doc| scene::search::search(doc, query))
                    .unwrap_or_default();
                let results: Vec<Json> = results
                    .iter()
                    .map(|m| {
                        json!({"id": edn_text(&m.id), "name": m.name, "ns": m.ns,
                               "label": m.label})
                    })
                    .collect();
                return Ok(vec![event(
                    "search",
                    seq,
                    json!({"query": query, "results": results}),
                )]);
            }
            "locate" => {
                let id = edn_field(action, "id")?;
                self.session()?.act(Action::Locate(id), &dims);
            }
            "context" => {
                let id = match action.get("proposalId") {
                    None => return Err("context needs proposalId (an EDN id, or null)".into()),
                    Some(Json::Null) => None,
                    Some(_) => Some(edn_field(action, "proposalId")?),
                };
                if let Some(id) = &id {
                    let session = self.session()?;
                    if !frame::inspector::entries(session.state())
                        .iter()
                        .any(|entry| entry.id.as_ref() == Some(id))
                    {
                        return Err(format!("unknown proposal {}", edn_text(id)));
                    }
                }
                let outcome = self.session()?.act(Action::Choose(id), &dims);
                let context = outcome
                    .context
                    .as_ref()
                    .map_or(Json::Null, state::context_json);
                return Ok(vec![
                    event("context", seq, json!({"context": context})),
                    self.frame(seq),
                ]);
            }
            "declutter" => {
                self.session()?.act(Action::Declutter, &dims);
            }
            "save" => {
                let saved = match &self.pending {
                    Some(pending) => pending.state.clone(),
                    None => self.session()?.saved_state(),
                };
                return Ok(vec![event("state", seq, json!({"state": saved}))]);
            }
            "quit" => {
                self.quit = true;
                return Ok(vec![event("ack", seq, json!({}))]);
            }
            other => return Err(format!("unknown action {other}")),
        }
        Ok(vec![self.frame(seq)])
    }

    fn viewport(&self, action: &Json) -> Result<(f64, f64), String> {
        let Some(viewport) = action.get("viewport").filter(|v| !v.is_null()) else {
            return Ok((self.width, self.height));
        };
        let side = |key: &str| {
            viewport
                .get(key)
                .and_then(Json::as_f64)
                .filter(|n| n.is_finite() && *n > 0.0)
                .ok_or_else(|| format!("viewport {key} must be a positive number"))
        };
        Ok((side("width")?, side("height")?))
    }

    /// Open the session: the named diagram, else the saved state's, else
    /// the project default; a saved state restores the view, `waiting`
    /// shows the empty canvas until `display`. A saved state that names a
    /// displayed diagram wins over `waiting`: it is the view the user had
    /// (the window's restart), not a fresh launch. When that diagram is not
    /// there yet the canvas waits and its first display restores the view.
    fn open(&mut self, action: &Json) -> Result<(), String> {
        let saved = action.get("state").filter(|s| !s.is_null());
        let snapshot = saved.map(state::snapshot).transpose()?;
        let named = match action.get("diagram") {
            None | Some(Json::Null) => None,
            Some(Json::String(path)) => Some(path.clone()),
            Some(other) => return Err(format!("diagram must be a path, not {other}")),
        };
        let diagram = named
            .or_else(|| saved.and_then(state::diagram))
            .map(|p| resolve(&self.project, &p))
            .unwrap_or_else(|| crate::default_diagram(&self.project));
        let diagram = frame::project_files::canonical(&diagram);
        let waiting = action.get("waiting").and_then(Json::as_bool) == Some(true);
        let displayed = saved.and_then(state::diagram).is_some();
        let mut pending = None;
        let start = match snapshot {
            Some(snapshot) if waiting && displayed && !diagram.is_file() => {
                pending = Some(Pending {
                    diagram: diagram.clone(),
                    snapshot,
                    state: saved.cloned().unwrap_or(Json::Null),
                });
                Start::Waiting
            }
            Some(snapshot) if !waiting || displayed => Start::Restore(Some(snapshot)),
            _ if waiting => Start::Waiting,
            _ => Start::Fresh,
        };
        let mut session = Session::open(self.project.clone(), diagram.clone(), start)?;
        // Verify against the bytes this Session actually parsed, closing the
        // host-read → resident-open race as well as refusing legacy metadata.
        if displayed
            && pending.is_none()
            && !saved.is_some_and(|s| same_loaded_diagram(s, &session))
        {
            session = Session::open(self.project.clone(), diagram, Start::Waiting)?;
        }
        self.session = Some(session);
        self.pending = pending;
        Ok(())
    }
}

fn same_loaded_diagram(saved: &Json, session: &Session) -> bool {
    let loaded = session.saved_state();
    saved.get("loadedDiagram").is_some_and(|fingerprint| {
        fingerprint.get("version").and_then(Json::as_i64) == Some(1)
            && fingerprint.get("sha256").and_then(Json::as_str).is_some()
            && Some(fingerprint) == loaded.get("loadedDiagram")
    })
}

fn resolve(project: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project.join(path)
    }
}

fn text<'a>(action: &'a Json, key: &str) -> Result<&'a str, String> {
    action
        .get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("{key} must be a string"))
}

fn edn_text(value: &Value) -> String {
    edn::encode(value).unwrap_or_default()
}

/// An id or hit field: EDN text, as in the scene frame.
fn edn_field(action: &Json, key: &str) -> Result<Value, String> {
    edn::parse(text(action, key)?).map_err(|e| format!("{key}: {e}"))
}

/// What the scene has at a world point `[x, y]`.
fn hit_at(session: &Session, at: &Json) -> Result<Option<Value>, String> {
    let point = at
        .as_array()
        .filter(|p| p.len() == 2)
        .and_then(|p| Some((p[0].as_f64()?, p[1].as_f64()?)))
        .ok_or("at must be a world point [x, y]")?;
    Ok(scene::hit::at(&session.state().scene, point.0, point.1))
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ask(resident: &mut Resident, mut action: Json) -> Json {
        action["version"] = json!(1);
        let lines = resident.line(&action.to_string());
        assert_eq!(1, lines.len(), "{lines:?}");
        assert_ne!(json!("error"), lines[0]["event"], "{lines:?}");
        lines[0].clone()
    }

    #[test]
    fn replacing_ir_between_load_and_save_never_labels_the_old_view_with_new_bytes() {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "uml-loaded-fingerprint-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let diagram = directory.join("diagram.edn");
        let a = "{:title \"A\"}";
        let b = "{:title \"B\"}";
        std::fs::write(&diagram, a).unwrap();
        let mut resident = Resident::new(directory.clone());
        ask(&mut resident, json!({"action": "open", "waiting": true}));
        ask(
            &mut resident,
            json!({"action": "display", "path": "diagram.edn"}),
        );
        std::fs::write(&diagram, b).unwrap();
        // Direct protocol calls deliberately do not tick: no timing assumption
        // about the 250 ms idle poll can conceal this load/save race.
        ask(
            &mut resident,
            json!({"action": "camera", "x": 12, "y": 34, "zoom": 0.6}),
        );
        let saved = ask(&mut resident, json!({"action": "save"}))["state"].clone();
        assert_eq!(
            json!({"version": 1, "sha256": crate::display::digest(a)}),
            saved["loadedDiagram"]
        );
        assert_eq!(json!({"x": 12.0, "y": 34.0, "zoom": 0.6}), saved["camera"]);
        ask(&mut resident, json!({"action": "quit"}));
        let mut reopened = Resident::new(directory.clone());
        let changed = ask(
            &mut reopened,
            json!({"action": "open", "waiting": true, "state": saved}),
        );
        assert_eq!(json!(true), changed["frame"]["waiting"]);
        std::fs::write(&diagram, a).unwrap();
        let same = ask(
            &mut reopened,
            json!({"action": "open", "waiting": true, "state": saved}),
        );
        assert_eq!(json!(false), same["frame"]["waiting"]);
        let again = ask(&mut reopened, json!({"action": "save"}));
        assert_eq!(saved, again["state"]);
        let mut legacy = saved;
        legacy.as_object_mut().unwrap().remove("loadedDiagram");
        let old = ask(
            &mut reopened,
            json!({"action": "open", "waiting": true, "state": legacy}),
        );
        assert_eq!(json!(true), old["frame"]["waiting"]);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
