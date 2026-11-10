//! Seam ① through the real binary (#17): `uml-viewer resident` reads one
//! JSON action per stdin line and answers with JSON scene frames and events
//! on stdout (`docs/resident-protocol.md`). The plugin host drives exactly
//! this; nothing here reaches into the session.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

fn shop_ir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../frame/tests/fixtures/shop/shop.ir.edn")
}

/// A project directory holding `ir` as its `diagram.edn`.
fn project_from(label: &str, ir: &Path) -> PathBuf {
    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "uml-resident {label} {} {}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(ir, dir.join("diagram.edn")).unwrap();
    dir
}

fn project(label: &str) -> PathBuf {
    project_from(label, &shop_ir())
}

/// The state-event golden fixture (#45): two source namespace levels and a
/// proposal with a layer and a nested group.
fn levels_ir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/state/levels.ir.edn")
}

fn levels_project(label: &str) -> PathBuf {
    project_from(label, &levels_ir())
}

/// One running resident session.
struct Resident {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
}

impl Resident {
    fn start(project: &Path) -> Resident {
        let mut child = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
            .arg("resident")
            .current_dir(project)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                let value: Value = serde_json::from_str(&line)
                    .unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Resident {
            child,
            stdin,
            lines,
        }
    }

    fn send(&mut self, action: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{action}").unwrap();
        stdin.flush().unwrap();
    }

    fn next(&self) -> Value {
        self.lines
            .recv_timeout(Duration::from_secs(20))
            .expect("the resident session answered")
    }

    /// The next line with this `event`, skipping none: the protocol answers
    /// in order, so anything else first is a failure.
    fn expect(&self, event: &str) -> Value {
        let line = self.next();
        assert_eq!(Some(event), line["event"].as_str(), "{line}");
        assert_eq!(json!(1), line["version"], "{line}");
        line
    }

    /// Send an action with `seq` and return its answer of `event`.
    fn ask(&mut self, seq: i64, mut action: Value, event: &str) -> Value {
        action["version"] = json!(1);
        action["seq"] = json!(seq);
        self.send(action);
        let line = self.expect(event);
        assert_eq!(json!(seq), line["seq"], "{line}");
        line
    }

    /// Close stdin and wait for a clean exit.
    fn finish(mut self) {
        drop(self.stdin.take());
        let status = self.child.wait().unwrap();
        assert!(status.success(), "{status}");
    }
}

fn opened(project: &Path) -> Resident {
    let mut resident = Resident::start(project);
    let ready = resident.expect("ready");
    assert_eq!(json!("dsh-uml-viewer/resident"), ready["schema"]);
    let frame = resident.ask(
        1,
        json!({"action": "open", "viewport": {"width": 1220, "height": 884}}),
        "frame",
    );
    assert_eq!(
        json!("dsh-uml-viewer/scene-frame"),
        frame["frame"]["schema"]
    );
    resident
}

/// Ids of the class boxes a frame draws.
fn classes(frame: &Value) -> Vec<String> {
    items(frame, "class")
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .collect()
}

fn items<'a>(frame: &'a Value, kind: &str) -> Vec<&'a Value> {
    frame["frame"]["world"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == kind)
        .collect()
}

/// The frame's chrome kinds (#46: a view carries none; only the pointer's
/// hover popup is chrome, and these tests never point).
fn chrome_kinds(frame: &Value) -> Vec<String> {
    frame["frame"]["chrome"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["kind"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn open_answers_the_fitted_namespace_tree_of_the_project_diagram() {
    let dir = project("open");
    let mut resident = Resident::start(&dir);
    resident.expect("ready");
    let frame = resident.ask(
        1,
        json!({"action": "open", "viewport": {"width": 1220, "height": 884}}),
        "frame",
    );
    let mut ids = classes(&frame);
    ids.sort();
    assert_eq!(vec![":api", ":core", ":serde", ":store", ":util"], ids);
    assert_eq!(
        json!({"width": 1220.0, "height": 884.0}),
        frame["frame"]["viewport"]
    );
    // The diagram title is the top bar's breadcrumb now, not frame chrome (#46).
    assert_eq!(Vec::<String>::new(), chrome_kinds(&frame));
    let state = resident.ask(2, json!({"action": "save"}), "state");
    assert_eq!(json!("Shop"), state["breadcrumbs"][0]["name"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn drill_in_enters_a_component_and_back_returns_to_the_namespace_tree() {
    let dir = project("drill");
    let mut resident = opened(&dir);
    let frame = resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    let ids = classes(&frame);
    assert!(ids.contains(&":core.order".to_owned()), "{ids:?}");
    assert!(!ids.contains(&":api".to_owned()), "{ids:?}");
    // The return line left the frame (#46); the path it named is the state's.
    assert_eq!(Vec::<String>::new(), chrome_kinds(&frame));
    let state = resident.ask(9, json!({"action": "save"}), "state");
    assert_eq!(json!("core"), state["breadcrumbs"][1]["name"]);
    let frame = resident.ask(3, json!({"action": "back"}), "frame");
    assert!(classes(&frame).contains(&":api".to_owned()));
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// The world centre of a class box, read from the frame the client drew:
/// the client hit-tests and sends the point.
fn centre_of(frame: &Value, id: &str) -> [f64; 2] {
    let class = items(frame, "class")
        .into_iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no class {id}"));
    let r = &class["rect"];
    let n = |k: &str| r[k].as_f64().unwrap();
    [n("x") + n("w") / 2.0, n("y") + n("h") / 2.0]
}

#[test]
fn the_declutter_action_sets_the_mode_the_panel_names() {
    let dir = project("declutter mode");
    let mut resident = opened(&dir);
    // The More menu's Remove arrows toggle names the mode (#46): 移除箭头
    // draws compact indicators instead of arrows, and `save` reports the mode.
    let frame = resident.ask(
        2,
        json!({"action": "declutter", "mode": "triangles"}),
        "frame",
    );
    let edges = frame["frame"]["edges"].as_array().unwrap();
    assert!(!edges.is_empty(), "{frame}");
    assert!(
        edges.iter().all(|edge| edge["kind"] == "indicator"),
        "移除箭头 draws indicators, not strokes: {edges:?}"
    );
    let saved = resident.ask(3, json!({"action": "save"}), "state");
    assert_eq!(json!(":triangles"), saved["state"]["declutter"]);
    // Naming the mode again is not a cycle: it stays where it was put.
    let again = resident.ask(
        4,
        json!({"action": "declutter", "mode": "triangles"}),
        "frame",
    );
    assert!(
        again["frame"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|edge| edge["kind"] == "indicator")
    );
    // No mode still cycles, as the inspector's button did.
    resident.ask(5, json!({"action": "declutter"}), "frame");
    let cycled = resident.ask(6, json!({"action": "save"}), "state");
    assert_eq!(json!(":elements"), cycled["state"]["declutter"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn select_highlights_the_dependency_chain_and_dims_the_rest() {
    let dir = project("select");
    let mut resident = opened(&dir);
    let core = resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    let at = centre_of(&core, ":core.order");
    let frame = resident.ask(3, json!({"action": "select", "at": at}), "frame");
    let order = items(&frame, "class")
        .into_iter()
        .find(|c| c["id"] == ":core.order")
        .unwrap();
    assert_eq!(json!(true), order["selected"]);
    // core.order depends on core.price: that edge is on the chain.
    let chain: Vec<&Value> = frame["frame"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "stroke" && e["stroke"]["role"] == "dependency-chain")
        .collect();
    assert!(
        chain
            .iter()
            .any(|e| e["from"] == ":core.order" && e["to"] == ":core.price"),
        "{chain:?}"
    );
    assert!(
        frame["frame"]["world"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["dimmed"] == true),
        "something off the chain is dimmed"
    );
    let cleared = resident.ask(4, json!({"action": "select", "at": null}), "frame");
    assert!(
        items(&cleared, "class")
            .iter()
            .all(|c| c["selected"] == false)
    );
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn search_lists_matches_and_locate_enters_selects_and_opens_the_result() {
    let dir = project("search");
    let mut resident = opened(&dir);
    let found = resident.ask(2, json!({"action": "search", "query": "PRICE"}), "search");
    assert_eq!(json!("PRICE"), found["query"]);
    assert_eq!(
        json!([{"id": ":core.price", "name": "price", "ns": "shop.core.price",
                "label": "price  ·  shop.core.price"}]),
        found["results"]
    );
    let frame = resident.ask(3, json!({"action": "locate", "id": ":core.price"}), "frame");
    let price = items(&frame, "class")
        .into_iter()
        .find(|c| c["id"] == ":core.price")
        .expect("the result's namespace is open");
    assert_eq!(json!(true), price["selected"]);
    let empty = resident.ask(4, json!({"action": "search", "query": "  "}), "search");
    assert_eq!(json!([]), empty["results"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn switching_the_discussion_context_reports_it_and_shows_that_diagram() {
    let dir = project("context");
    let mut resident = opened(&dir);
    let context = resident.ask(
        2,
        json!({"action": "context", "proposalId": ":p-shop"}),
        "context",
    );
    assert_eq!(
        json!({"kind": "proposal", "proposalId": ":p-shop", "name": "2026-10-24 10:00:00",
               "edn": "{:context :proposal :name \"2026-10-24 10:00:00\" :proposal-id :p-shop}"}),
        context["context"]
    );
    let frame = resident.expect("frame");
    assert_eq!(json!(2), frame["seq"]);
    let groups: Vec<&Value> = items(&frame, "package").iter().map(|p| &p["id"]).collect();
    assert!(groups.contains(&&json!(":proposal.domain")), "{groups:?}");
    let context = resident.ask(
        3,
        json!({"action": "context", "proposalId": null}),
        "context",
    );
    assert_eq!(
        json!({"kind": "real", "edn": "{:context :real}"}),
        context["context"]
    );
    let frame = resident.expect("frame");
    assert!(classes(&frame).contains(&":api".to_owned()));
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// The state event's breadcrumbs: the path from the tree root to the current
/// component, each level with the id a `back` target names (#45).
#[test]
fn back_with_a_target_returns_to_a_named_breadcrumb_level() {
    let dir = project("back target");
    let mut resident = opened(&dir);
    let root = resident.ask(2, json!({"action": "save"}), "state");
    assert_eq!(json!([{"id": null, "name": "Shop"}]), root["breadcrumbs"]);
    assert_eq!(
        Value::Null,
        root["status"],
        "a live view has no status code"
    );
    resident.ask(3, json!({"action": "drill-in", "id": ":core"}), "frame");
    resident.ask(
        4,
        json!({"action": "drill-in", "id": ":core.order"}),
        "frame",
    );
    let deep = resident.ask(5, json!({"action": "save"}), "state");
    assert_eq!(
        json!([{"id": null, "name": "Shop"},
               {"id": ":core", "name": "core"},
               {"id": ":core.order", "name": "order"}]),
        deep["breadcrumbs"]
    );
    // Any level of the path is a target, not only the one above.
    resident.ask(6, json!({"action": "back", "target": ":core"}), "frame");
    let middle = resident.ask(7, json!({"action": "save"}), "state");
    assert_eq!(
        json!([{"id": null, "name": "Shop"}, {"id": ":core", "name": "core"}]),
        middle["breadcrumbs"]
    );
    resident.ask(8, json!({"action": "back", "target": null}), "frame");
    let again = resident.ask(9, json!({"action": "save"}), "state");
    assert_eq!(root["breadcrumbs"], again["breadcrumbs"]);
    // A target the path does not carry is an error, not one level back.
    for target in [json!(":api"), json!(":missing"), json!(7)] {
        let error = resident.ask(10, json!({"action": "back", "target": target}), "error");
        assert!(
            error["message"].as_str().unwrap().contains("breadcrumb"),
            "{error}"
        );
    }
    let unchanged = resident.ask(11, json!({"action": "save"}), "state");
    assert_eq!(again["breadcrumbs"], unchanged["breadcrumbs"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// A proposal's breadcrumb path starts at the proposal, not at the source
/// diagram: its root id is the proposal and its levels are the proposal's.
#[test]
fn a_proposal_breadcrumb_path_names_its_layers_and_returns_to_them() {
    let dir = project("proposal crumbs");
    let mut resident = opened(&dir);
    resident.ask(
        2,
        json!({"action": "context", "proposalId": ":p-shop"}),
        "context",
    );
    resident.expect("frame");
    let root = resident.ask(3, json!({"action": "save"}), "state");
    assert_eq!(
        json!([{"id": ":p-shop", "name": "2026-10-24 10:00:00"}]),
        root["breadcrumbs"]
    );
    resident.ask(
        4,
        json!({"action": "drill-in", "id": ":proposal.domain"}),
        "frame",
    );
    let layer = resident.ask(5, json!({"action": "save"}), "state");
    assert_eq!(
        json!([{"id": ":p-shop", "name": "2026-10-24 10:00:00"},
               {"id": ":proposal.domain", "name": "Domain"}]),
        layer["breadcrumbs"]
    );
    resident.ask(6, json!({"action": "back", "target": ":p-shop"}), "frame");
    let back = resident.ask(7, json!({"action": "save"}), "state");
    assert_eq!(root["breadcrumbs"], back["breadcrumbs"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// The state event carries the canvas status as a code the panel localizes,
/// with the parameters that code needs; frames keep their own text until #46.
#[test]
fn the_state_event_reports_the_canvas_status_code() {
    let dir = project("status codes");
    let mut resident = Resident::start(&dir);
    resident.expect("ready");
    resident.ask(
        1,
        json!({"action": "open", "waiting": true, "viewport": {"width": 1220, "height": 884}}),
        "frame",
    );
    let waiting = resident.ask(2, json!({"action": "save"}), "state");
    assert_eq!(json!([]), waiting["breadcrumbs"], "nothing is loaded yet");
    assert_eq!(json!({"code": "waiting-display"}), waiting["status"]);
    // A named diagram that is not there says which file is missing.
    resident.ask(
        3,
        json!({"action": "open", "diagram": "missing.edn"}),
        "frame",
    );
    let missing = resident.ask(4, json!({"action": "save"}), "state");
    let status = &missing["status"];
    assert_eq!(json!("missing-file"), status["code"]);
    assert!(
        status["path"].as_str().unwrap().ends_with("missing.edn"),
        "{status}"
    );
    assert_eq!(json!([]), missing["breadcrumbs"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_saved_session_state_restores_the_view_and_discussion_context() {
    let dir = project("restore");
    let mut resident = opened(&dir);
    resident.ask(
        2,
        json!({"action": "context", "proposalId": ":p-shop"}),
        "context",
    );
    resident.expect("frame");
    resident.ask(
        3,
        json!({"action": "context", "proposalId": null}),
        "context",
    );
    resident.expect("frame");
    let core = resident.ask(4, json!({"action": "drill-in", "id": ":core"}), "frame");
    let at = centre_of(&core, ":core.order");
    let shown = resident.ask(5, json!({"action": "select", "at": at}), "frame");
    let saved = resident.ask(6, json!({"action": "save"}), "state");
    resident.finish();
    let state = &saved["state"];
    assert_eq!(json!("dsh-uml-viewer/session-state"), state["schema"]);
    assert_eq!(json!(1), state["version"]);
    assert_eq!(json!([":core"]), state["focus"]);
    assert_eq!(
        json!({"kind": "real", "edn": "{:context :real}"}),
        state["context"]
    );

    // The host persisted the state as data; a new process restores it.
    let mut resident = Resident::start(&dir);
    resident.expect("ready");
    let restored = resident.ask(
        1,
        json!({"action": "open", "viewport": {"width": 1220, "height": 884},
               "state": state}),
        "frame",
    );
    assert_eq!(shown["frame"], restored["frame"]);
    let again = resident.ask(2, json!({"action": "save"}), "state");
    assert_eq!(*state, again["state"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// A view off the fitted default (drilled into `:core`, `:core.order`
/// selected) and the state saved for it: what the host persisted.
fn saved_view(dir: &Path) -> (Value, Value) {
    let mut resident = opened(dir);
    let core = resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    let at = centre_of(&core, ":core.order");
    let shown = resident.ask(3, json!({"action": "select", "at": at}), "frame");
    let saved = resident.ask(4, json!({"action": "save"}), "state");
    resident.finish();
    (shown, saved["state"].clone())
}

fn waiting_open(state: &Value) -> Value {
    json!({"action": "open", "waiting": true,
           "viewport": {"width": 1220, "height": 884}, "state": state})
}

#[test]
fn a_waiting_open_with_a_saved_view_restores_that_view_instead_of_the_empty_canvas() {
    // The host always opens waiting (a fresh panel shows nothing until
    // `uml_display`); a saved state is a view the user already had.
    let dir = project("waiting restore");
    let (shown, state) = saved_view(&dir);
    let mut resident = Resident::start(&dir);
    resident.expect("ready");
    let restored = resident.ask(1, waiting_open(&state), "frame");
    assert_eq!(json!(false), restored["frame"]["waiting"]);
    assert_eq!(shown["frame"], restored["frame"]);
    let again = resident.ask(2, json!({"action": "save"}), "state");
    assert_eq!(state, again["state"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_saved_view_whose_diagram_is_not_there_yet_is_restored_by_its_display() {
    let dir = project("waiting pending");
    let (shown, state) = saved_view(&dir);
    let ir = std::fs::read(dir.join("diagram.edn")).unwrap();
    std::fs::remove_file(dir.join("diagram.edn")).unwrap();
    let mut resident = Resident::start(&dir);
    resident.expect("ready");
    let opened = resident.ask(1, waiting_open(&state), "frame");
    assert_eq!(json!(true), opened["frame"]["waiting"]);
    // Closing the panel before the display must not lose the saved view.
    let pending = resident.ask(2, json!({"action": "save"}), "state");
    assert_eq!(state, pending["state"]);
    std::fs::write(dir.join("diagram.edn"), ir).unwrap();
    let displayed = resident.ask(
        3,
        json!({"action": "display", "path": "diagram.edn"}),
        "frame",
    );
    assert_eq!(shown["frame"], displayed["frame"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_view_saved_before_any_display_reopens_waiting() {
    let dir = project("waiting saved");
    let mut resident = Resident::start(&dir);
    resident.expect("ready");
    let first = resident.ask(
        1,
        json!({"action": "open", "waiting": true, "viewport": {"width": 1220, "height": 884}}),
        "frame",
    );
    assert_eq!(json!(true), first["frame"]["waiting"]);
    let saved = resident.ask(2, json!({"action": "save"}), "state");
    resident.finish();
    assert_eq!(
        Value::Null,
        saved["state"]["diagram"],
        "nothing was displayed"
    );
    let mut resident = Resident::start(&dir);
    resident.expect("ready");
    let reopened = resident.ask(1, waiting_open(&saved["state"]), "frame");
    assert_eq!(json!(true), reopened["frame"]["waiting"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_edited_diagram_hot_reloads_into_an_unprompted_frame() {
    let dir = project("reload");
    let mut resident = opened(&dir);
    let text = std::fs::read_to_string(dir.join("diagram.edn"))
        .unwrap()
        .replace(":title \"Shop\"", ":title \"Shop v2\"");
    std::fs::write(dir.join("diagram.edn"), text).unwrap();
    // A later mtime: the stamp is the file's modification time.
    std::fs::File::options()
        .write(true)
        .open(dir.join("diagram.edn"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(5))
        .unwrap();
    let frame = resident.expect("frame");
    assert_eq!(Value::Null, frame["seq"], "a reload answers no action");
    // The new title shows up in the top bar's path, not in frame chrome (#46).
    assert_eq!(Vec::<String>::new(), chrome_kinds(&frame));
    let state = resident.ask(9, json!({"action": "save"}), "state");
    assert_eq!(json!("Shop v2"), state["breadcrumbs"][0]["name"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_open_preserves_the_viewport_and_existing_session() {
    let dir = project("failed open");
    std::fs::write(dir.join("invalid.edn"), "{broken").unwrap();
    std::fs::create_dir(dir.join("directory")).unwrap();
    let mut resident = opened(&dir);
    resident.ask(
        100,
        json!({"action": "context", "proposalId": ":p-shop"}),
        "context",
    );
    resident.expect("frame");
    let before = resident.ask(2, json!({"action": "viewport"}), "frame");
    let saved = resident.ask(3, json!({"action": "save"}), "state");
    for (seq, fields) in [
        (4, json!({"state": {"schema": "bad", "version": 1}})),
        (5, json!({"diagram": "invalid.edn"})),
        (6, json!({"diagram": 42})),
        (30, json!({"diagram": "directory"})),
        (
            8,
            json!({"state": {"schema": "dsh-uml-viewer/session-state", "version": 1,
                            "context": {"edn": "{"}}}),
        ),
        (9, json!({"viewport": {"width": 0, "height": 222}})),
        (
            7,
            json!({"state": {"schema": "dsh-uml-viewer/session-state", "version": 2}}),
        ),
    ] {
        let mut action = fields;
        action["action"] = json!("open");
        if action.get("viewport").is_none() {
            action["viewport"] = json!({"width": 333, "height": 222});
        }
        resident.ask(seq, action, "error");
        let after = resident.ask(seq + 10, json!({"action": "viewport"}), "frame");
        assert_eq!(
            before["frame"], after["frame"],
            "failed open keeps the rendered view"
        );
        let state = resident.ask(seq + 20, json!({"action": "save"}), "state");
        assert_eq!(
            saved["state"], state["state"],
            "failed open keeps session state"
        );
    }
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_incomplete_context_action_keeps_the_proposal_discussion() {
    let dir = project("incomplete context");
    let mut resident = opened(&dir);
    resident.ask(
        2,
        json!({"action": "context", "proposalId": ":p-shop"}),
        "context",
    );
    let before = resident.expect("frame");
    let saved = resident.ask(3, json!({"action": "save"}), "state");
    resident.ask(4, json!({"action": "context"}), "error");
    let after = resident.ask(5, json!({"action": "viewport"}), "frame");
    assert_eq!(before["frame"], after["frame"]);
    let state = resident.ask(6, json!({"action": "save"}), "state");
    assert_eq!(saved["state"], state["state"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_malformed_line_or_unknown_action_is_an_error_event_and_the_session_continues() {
    let dir = project("errors");
    let mut resident = opened(&dir);
    resident.send(json!("not an object"));
    let error = resident.expect("error");
    assert_eq!(Value::Null, error["seq"]);
    let error = resident.ask(2, json!({"action": "fly"}), "error");
    assert!(
        error["message"].as_str().unwrap().contains("fly"),
        "{error}"
    );
    resident.send(json!({"version": 2, "seq": 3, "action": "save"}));
    let error = resident.expect("error");
    assert_eq!(json!(3), error["seq"]);
    let before = resident.ask(4, json!({"action": "save"}), "state");
    resident.ask(
        5,
        json!({"action": "context", "proposalId": ":missing"}),
        "error",
    );
    resident.ask(
        6,
        json!({"action": "display", "path": "missing.edn"}),
        "error",
    );
    let after = resident.ask(7, json!({"action": "save"}), "state");
    assert_eq!(
        before["state"], after["state"],
        "errors keep the usable view and context"
    );
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// One inbound action line is bounded exactly like an outbound event (#review):
/// a line that never ends stops the session with an `error` event and a failing
/// exit instead of buffering without limit. The host is trusted, but the bound
/// is documented in both directions.
#[test]
fn an_action_line_over_the_transport_cap_stops_the_session_with_a_reason() {
    let project = project("action-cap");
    let mut child = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("resident")
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let ready: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(ready["event"], "ready");
    // One unterminated line, one byte over the cap. The write itself may end in
    // a broken pipe: the session stops reading at the cap and exits.
    let over = vec![b'{'; viewer_session::resident::ACTION_LINE_MAX_BYTES + 1];
    let _ = stdin.write_all(&over);
    let _ = stdin.flush();
    let event: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(event["event"], "error", "{event}");
    assert_eq!(event["seq"], Value::Null, "{event}");
    assert!(
        event["message"]
            .as_str()
            .unwrap()
            .contains("action line over"),
        "{event}"
    );
    let status = child.wait().unwrap();
    assert_eq!(
        status.code(),
        Some(1),
        "a capped session fails, never exits 0"
    );
    let _ = std::fs::remove_dir_all(&project);
}

/// A `state` event golden (#45): the breadcrumb path and the status code the
/// panel's top bar and status line are built from. Frames have their own
/// goldens (`crates/frame/tests/golden_frame.rs`); this event has no frame, so
/// it keeps its own checked-in record. Regenerate with
/// `UPDATE_STATE_GOLDENS=1` and review the diff like the contract change it is.
fn state_golden(name: &str, event: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/state")
        .join(format!("{name}.state.json"));
    let fields = recorded(event);
    if std::env::var_os("UPDATE_STATE_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&fields).unwrap()),
        )
        .unwrap();
    }
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing golden {}: {e}", path.display())),
    )
    .unwrap();
    assert_eq!(
        expected,
        fields,
        "{name}: state event differs from {}; rerun with UPDATE_STATE_GOLDENS=1 and review the diff",
        path.display()
    );
}

/// The event's #45 fields and the switcher's proposal list (#47). A status's `path` is the machine's, so the golden
/// records the file it names rather than the checkout it was captured in.
fn recorded(event: &Value) -> Value {
    let mut status = event["status"].clone();
    if let Some(full) = status.get("path").and_then(Value::as_str) {
        let file = Path::new(full)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        status["path"] = json!(file);
    }
    json!({"breadcrumbs": event["breadcrumbs"], "status": status, "proposals": event["proposals"]})
}

fn saved(resident: &mut Resident, seq: i64) -> Value {
    resident.ask(seq, json!({"action": "save"}), "state")
}

#[test]
fn state_golden_root_names_the_diagram_alone() {
    let dir = levels_project("golden root");
    let mut resident = opened(&dir);
    state_golden("root", &saved(&mut resident, 2));
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn state_golden_source_names_both_namespace_levels() {
    let dir = levels_project("golden source");
    let mut resident = opened(&dir);
    resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    resident.ask(
        3,
        json!({"action": "drill-in", "id": ":core.inner"}),
        "frame",
    );
    state_golden("source-two-levels", &saved(&mut resident, 4));
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn state_golden_proposal_path_starts_at_the_proposal_and_descends() {
    let dir = levels_project("golden proposal");
    let mut resident = opened(&dir);
    resident.ask(
        2,
        json!({"action": "context", "proposalId": ":p-levels"}),
        "context",
    );
    resident.expect("frame");
    state_golden("proposal-root", &saved(&mut resident, 3));
    resident.ask(
        4,
        json!({"action": "drill-in", "id": ":proposal.domain"}),
        "frame",
    );
    state_golden("proposal-layer", &saved(&mut resident, 5));
    resident.ask(
        6,
        json!({"action": "drill-in", "id": ":core-group"}),
        "frame",
    );
    state_golden("proposal-nested-group", &saved(&mut resident, 7));
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn state_goldens_cover_the_waiting_and_missing_diagram_status_codes() {
    let dir = levels_project("golden status");
    let mut resident = Resident::start(&dir);
    resident.expect("ready");
    resident.ask(
        1,
        json!({"action": "open", "waiting": true, "viewport": {"width": 1220, "height": 884}}),
        "frame",
    );
    state_golden("waiting", &saved(&mut resident, 2));
    resident.ask(
        3,
        json!({"action": "open", "diagram": "missing.edn"}),
        "frame",
    );
    state_golden("missing-file", &saved(&mut resident, 4));
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}
