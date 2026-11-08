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

/// A project directory holding the shop diagram as `diagram.edn`.
fn project(label: &str) -> PathBuf {
    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "uml-resident {label} {} {}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(shop_ir(), dir.join("diagram.edn")).unwrap();
    dir
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

fn chrome_texts(frame: &Value) -> Vec<String> {
    frame["frame"]["chrome"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["text"].as_str().map(str::to_owned))
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
    assert!(chrome_texts(&frame).contains(&"Shop".to_owned()));
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
    assert!(chrome_texts(&frame).contains(&"< 返回 · core".to_owned()));
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

#[test]
fn an_edited_diagram_hot_reloads_into_an_unprompted_frame() {
    let dir = project("reload");
    let resident = opened(&dir);
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
    assert!(chrome_texts(&frame).contains(&"Shop v2".to_owned()));
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
