//! Seam ① through the real binary (#27): the class card and the source window
//! travel over the resident protocol, and the `state` event lists the
//! proposals the discussion-context switcher offers (#47, which deleted the
//! inspector). Every card row, CRAP/mutation figure, no-data line and source
//! line is produced by `frame::detail` and `frame::source`; the assertions
//! below name the values
//! the fixtures and the shared model tests (`crates/frame/tests`) pin down.
//! The panel renders these events; it never recomputes them.

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

fn temp_dir(label: &str) -> PathBuf {
    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "uml-resident-overlays {label} {} {}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A project directory holding the shop diagram as `diagram.edn`.
fn shop_project(label: &str) -> PathBuf {
    let dir = temp_dir(label);
    std::fs::copy(shop_ir(), dir.join("diagram.edn")).unwrap();
    dir
}

/// A project whose IR names a real source file and carries literal quality
/// figures, so the expected summary text is the fixture's, not a recomputation.
fn source_project(label: &str) -> PathBuf {
    let dir = temp_dir(label);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("diagram.edn"),
        r#"{:hierarchical true
 :title "Shop"
 :prefix "shop"
 :levels [[core]]
 :order [core]
 :omit []
 :classes [{:id :core :name "core" :ns "shop.core" :lang :rust :level 0}
           {:id :core.order :name "order" :ns "shop.core.order" :lang :rust :level 0
            :file "src/order.rs"
            :crap {:mu 4 :max 8 :sigma 1.5} :killed 7 :survived 2 :uncovered 3
            :ops [{:name "place" :text "place()"} {:name "cancel" :text "cancel()"}
                  {:name "op1" :text "op1()"} {:name "op2" :text "op2()"}
                  {:name "op3" :text "op3()"} {:name "op4" :text "op4()"}
                  {:name "op5" :text "op5()"} {:name "op6" :text "op6()"}]}
           {:id :core.plain :name "plain" :ns "shop.core.plain" :lang :rust :level 0}]
 :edges [{:from :core.order :to :core :kind :dependency}]
 :proposals [{:id :p-shop :name "2026-10-24 10:00:00"
              :layers [{:id :domain :label "Domain" :nses [core]}] :omit []}]}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/order.rs"),
        "use std::fmt;\n\n/// An order.\npub struct Order {\n    pub id: u64,\n}\n\nimpl Order {\n\tpub fn place(&mut self) -> u64 {\n\t\tself.id\n\t}\n\n    pub fn cancel(&mut self) {\n        self.id = 0;\n    }\n}\n",
    )
    .unwrap();
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

    fn expect(&self, event: &str) -> Value {
        let line = self.next();
        assert_eq!(Some(event), line["event"].as_str(), "{line}");
        assert_eq!(json!(1), line["version"], "{line}");
        line
    }

    fn ask(&mut self, seq: i64, mut action: Value, event: &str) -> Value {
        action["version"] = json!(1);
        action["seq"] = json!(seq);
        self.send(action);
        let line = self.expect(event);
        assert_eq!(json!(seq), line["seq"], "{line}");
        line
    }

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
    let actions = ready["actions"].as_array().unwrap();
    assert!(
        !actions.contains(&json!("inspector")),
        "the inspector is gone (#47): {ready}"
    );
    for action in [
        "card",
        "open-card",
        "close-card",
        "scroll-card",
        "open-source",
    ] {
        assert!(
            ready["actions"]
                .as_array()
                .unwrap()
                .contains(&json!(action)),
            "ready must advertise {action}: {ready}"
        );
    }
    resident.ask(
        1,
        json!({"action": "open", "viewport": {"width": 1220, "height": 884}}),
        "frame",
    );
    resident
}

fn items<'a>(frame: &'a Value, kind: &str) -> Vec<&'a Value> {
    frame["frame"]["world"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == kind)
        .collect()
}

/// The world centre of a class box, read from the frame the client drew.
fn centre_of(frame: &Value, id: &str) -> [f64; 2] {
    let class = items(frame, "class")
        .into_iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no class {id}"));
    let r = &class["rect"];
    let n = |k: &str| r[k].as_f64().unwrap();
    [n("x") + n("w") / 2.0, n("y") + n("h") / 2.0]
}

/// The `state` reply to `save`, the event the panel reads after every frame.
fn saved(resident: &mut Resident, seq: i64) -> Value {
    resident.ask(seq, json!({"action": "save"}), "state")
}

#[test]
fn the_state_event_lists_the_proposals_the_switcher_offers() {
    let dir = shop_project("switcher");
    let mut resident = opened(&dir);
    // The shop fixture's one proposal, by EDN id and its display name; the
    // source diagram is the switcher's own first row, so it is not listed.
    let state = saved(&mut resident, 2);
    assert_eq!(
        json!([{"id": ":p-shop", "name": "2026-10-24 10:00:00"}]),
        state["proposals"]
    );
    assert_eq!(Value::Null, state["state"]["proposalId"]);

    // Choosing the proposal is the existing `context` action; the list does
    // not change, the view's proposal flag does.
    resident.ask(
        3,
        json!({"action": "context", "proposalId": ":p-shop"}),
        "context",
    );
    resident.expect("frame");
    let state = saved(&mut resident, 4);
    assert_eq!(json!(":p-shop"), state["state"]["proposalId"]);
    assert_eq!(
        json!([{"id": ":p-shop", "name": "2026-10-24 10:00:00"}]),
        state["proposals"]
    );

    // Choosing the source diagram returns to the namespace tree's root.
    let context = resident.ask(
        5,
        json!({"action": "context", "proposalId": null}),
        "context",
    );
    assert_eq!(json!("real"), context["context"]["kind"]);
    resident.expect("frame");
    let state = saved(&mut resident, 6);
    assert_eq!(Value::Null, state["state"]["proposalId"]);
    assert_eq!(json!([]), state["state"]["focus"]);

    // An id the diagram does not list is still refused.
    resident.ask(
        7,
        json!({"action": "context", "proposalId": ":p-missing"}),
        "error",
    );
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_inspector_action_is_unknown() {
    let dir = shop_project("no inspector");
    let mut resident = opened(&dir);
    let refused = resident.ask(2, json!({"action": "inspector"}), "error");
    assert!(
        refused["message"].as_str().unwrap().contains("inspector"),
        "{refused}"
    );
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn double_click_opens_the_class_card_and_close_card_empties_it() {
    let dir = source_project("card");
    let mut resident = opened(&dir);
    let core = resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    let at = centre_of(&core, ":core.order");
    let frame = resident.ask(3, json!({"action": "open-card", "at": at}), "frame");
    let order = items(&frame, "class")
        .into_iter()
        .find(|c| c["id"] == ":core.order")
        .expect("the class is still drawn");
    assert_eq!(
        json!(true),
        order["selected"],
        "the card's class is selected"
    );

    let card = resident.ask(5, json!({"action": "card", "viewHeight": 100.0}), "card");
    assert_eq!(json!(":core.order"), card["card"]["id"]);
    assert_eq!(json!("order"), card["card"]["title"]);
    let rows = card["card"]["items"].as_array().unwrap();
    assert_eq!(json!("name"), rows[0]["kind"]);
    assert_eq!(json!("order"), rows[0]["text"]);
    assert_eq!(json!(true), rows[0]["titleFont"]);
    assert_eq!(json!(0.0), card["card"]["scroll"]);
    let content = card["card"]["contentHeight"].as_f64().unwrap();
    let view_h = card["card"]["viewHeight"].as_f64().unwrap();
    assert!(
        content > view_h,
        "the card overflows a 100pt viewport: {card}"
    );
    assert_eq!(
        json!(content - view_h),
        card["card"]["maxScroll"],
        "maxScroll is frame::detail::max_scroll"
    );
    // The plan is the native one: metric columns, member rows, relationships.
    assert!(rows.iter().any(|r| r["kind"] == "col-header"));
    assert!(
        rows.iter()
            .any(|r| r["opName"] == "place" && r["clickable"] == true)
    );
    assert!(
        rows.iter()
            .any(|r| r["kind"] == "rel" && r["rel"] == ":core")
    );
    let header = rows.iter().find(|r| r["kind"] == "col-header").unwrap();
    let labels: Vec<&str> = header["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        vec!["Crap", "CC", "Cov", "killed", "survived", "uncovered"],
        labels
    );

    // Closing the card leaves none: `card` answers null instead of an error.
    resident.ask(8, json!({"action": "close-card"}), "frame");
    let card = resident.ask(9, json!({"action": "card"}), "card");
    assert_eq!(Value::Null, card["card"], "{card}");
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn double_click_on_a_namespace_drills_instead_of_opening_a_card() {
    let dir = shop_project("double click layer");
    let mut resident = opened(&dir);
    let root = resident.ask(2, json!({"action": "viewport"}), "frame");
    let at = centre_of(&root, ":core");
    let frame = resident.ask(3, json!({"action": "open-card", "at": at}), "frame");
    assert!(
        items(&frame, "class")
            .iter()
            .any(|c| c["id"] == ":core.order"),
        "double-clicking a namespace enters it"
    );
    let card = resident.ask(4, json!({"action": "card"}), "card");
    assert_eq!(Value::Null, card["card"], "no card opened: {card}");
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn card_scroll_steps_by_48_and_clamps_at_both_ends() {
    let dir = source_project("scroll");
    let mut resident = opened(&dir);
    resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    resident.ask(
        3,
        json!({"action": "open-card", "id": ":core.order"}),
        "frame",
    );
    let before = resident.ask(4, json!({"action": "card", "viewHeight": 120.0}), "card");
    let max = before["card"]["maxScroll"].as_f64().unwrap();
    assert!(max > 192.0, "the card must overflow: {before}");

    resident.ask(
        5,
        json!({"action": "scroll-card", "delta": 4.0, "viewHeight": 120.0}),
        "frame",
    );
    let card = resident.ask(6, json!({"action": "card", "viewHeight": 120.0}), "card");
    assert_eq!(
        json!(192.0),
        card["card"]["scroll"],
        "four notches of 48 points"
    );

    resident.ask(
        7,
        json!({"action": "scroll-card", "delta": 1000.0, "viewHeight": 120.0}),
        "frame",
    );
    let card = resident.ask(8, json!({"action": "card", "viewHeight": 120.0}), "card");
    assert_eq!(
        json!(max),
        card["card"]["scroll"],
        "overscroll stops at the content end"
    );

    resident.ask(
        9,
        json!({"action": "scroll-card", "delta": -1000.0, "viewHeight": 120.0}),
        "frame",
    );
    let card = resident.ask(10, json!({"action": "card", "viewHeight": 120.0}), "card");
    assert_eq!(
        json!(0.0),
        card["card"]["scroll"],
        "underscroll clamps to zero"
    );

    resident.ask(
        11,
        json!({"action": "scroll-card", "delta": 3.0, "viewHeight": 120.0}),
        "frame",
    );
    let card = resident.ask(12, json!({"action": "card", "viewHeight": 120.0}), "card");
    assert_eq!(json!(144.0), card["card"]["scroll"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_card_request_without_an_open_card_answers_null_and_changes_nothing() {
    let dir = shop_project("no card");
    let mut resident = opened(&dir);
    let card = resident.ask(2, json!({"action": "card"}), "card");
    assert_eq!(Value::Null, card["card"], "{card}");
    // A card on a class the diagram does not have is no open card either.
    resident.ask(
        3,
        json!({"action": "open-card", "id": ":missing.class"}),
        "frame",
    );
    let card = resident.ask(4, json!({"action": "card"}), "card");
    assert_eq!(Value::Null, card["card"], "{card}");
    resident.ask(5, json!({"action": "open-source", "id": ":api"}), "error");
    let state = saved(&mut resident, 6);
    assert_eq!(Value::Null, state["state"]["proposalId"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// Refusing a source path leaves all view/discussion/card state intact.
fn assert_source_escape_refused(dir: &Path) {
    let mut resident = opened(dir);
    resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    resident.ask(
        3,
        json!({"action": "open-card", "id": ":core.order"}),
        "frame",
    );
    let before = resident.ask(4, json!({"action": "save"}), "state");
    let error = resident.ask(
        5,
        json!({"action": "open-source", "id": ":core.order"}),
        "error",
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("inside the session project"),
        "{error}"
    );
    let after = resident.ask(6, json!({"action": "save"}), "state");
    assert_eq!(before["state"], after["state"]);
    resident.finish();
}

#[test]
fn source_parent_traversal_is_refused_without_changing_the_view() {
    let dir = source_project("source escape");
    let outside = temp_dir("outside");
    std::fs::write(outside.join("secret.rs"), "pub fn secret() {}\n").unwrap();
    let relative = format!(
        "../{}/secret.rs",
        outside.file_name().unwrap().to_string_lossy()
    );
    let ir = std::fs::read_to_string(dir.join("diagram.edn"))
        .unwrap()
        .replace("src/order.rs", &relative);
    std::fs::write(dir.join("diagram.edn"), ir).unwrap();
    assert_source_escape_refused(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&outside).unwrap();
}

#[test]
fn source_link_outside_the_project_is_refused_without_reading_it() {
    let dir = source_project("source link");
    let outside = temp_dir("outside linked");
    std::fs::write(outside.join("secret.rs"), "pub fn secret() {}\n").unwrap();
    let link = dir.join("linked");
    #[cfg(windows)]
    {
        let status = Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "creating the test junction failed");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let ir = std::fs::read_to_string(dir.join("diagram.edn"))
        .unwrap()
        .replace("src/order.rs", "linked/secret.rs");
    std::fs::write(dir.join("diagram.edn"), ir).unwrap();
    assert_source_escape_refused(&dir);
    // Remove the link itself first; never traverse it during fixture cleanup.
    #[cfg(windows)]
    std::fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&link).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&outside).unwrap();
}

#[test]
fn open_source_reads_the_session_project_and_lands_on_the_declaration_line() {
    let dir = source_project("source");
    let mut resident = opened(&dir);
    resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    resident.ask(
        3,
        json!({"action": "open-card", "id": ":core.order"}),
        "frame",
    );

    // The module row: the whole file, no target line.
    let module = resident.ask(
        4,
        json!({"action": "open-source", "id": ":core.order"}),
        "source",
    );
    let source = &module["source"];
    assert_eq!(json!("rust"), source["lang"]);
    assert_eq!(json!("src/order.rs"), source["title"]);
    assert_eq!(Value::Null, source["line"]);
    assert!(source["file"].as_str().unwrap().ends_with("order.rs"));
    let rows = source["rows"].as_array().unwrap();
    // 16 lines plus the empty row a trailing newline leaves (frame::source::rows).
    assert_eq!(17, rows.len(), "{rows:?}");
    assert_eq!(json!(1), rows[0]["number"]);
    assert_eq!(json!("use std::fmt;"), rows[0]["display"]);
    assert!(rows.iter().all(|r| r["marked"] == false));

    // A member: its declaration line, tab-expanded text and token runs.
    let member = resident.ask(
        5,
        json!({"action": "open-source", "id": ":core.order", "member": "place"}),
        "source",
    );
    let source = &member["source"];
    assert_eq!(
        json!(9),
        source["line"],
        "pub fn place is line 9 of the fixture"
    );
    assert_eq!(json!("src/order.rs:9"), source["title"]);
    let rows = source["rows"].as_array().unwrap();
    assert_eq!(17, rows.len());
    let target = &rows[8];
    assert_eq!(
        json!(true),
        target["marked"],
        "only the declaration row is marked"
    );
    assert_eq!(
        json!(1),
        rows.iter().filter(|r| r["marked"] == true).count()
    );
    assert_eq!(
        json!("  pub fn place(&mut self) -> u64 {"),
        target["display"]
    );
    let tokens = target["tokens"].as_array().unwrap();
    assert!(
        tokens
            .iter()
            .any(|t| t["text"] == "pub" && t["kind"] == "keyword"),
        "{tokens:?}"
    );
    assert!(tokens.iter().any(|t| t["text"] == "place"), "{tokens:?}");
    assert_eq!(
        json!("  pub fn place(&mut self) -> u64 {"),
        json!(
            tokens
                .iter()
                .map(|t| t["text"].as_str().unwrap())
                .collect::<String>()
        )
    );

    // A second window for another member; an unknown member is an error.
    let cancel = resident.ask(
        6,
        json!({"action": "open-source", "id": ":core.order", "member": "cancel"}),
        "source",
    );
    assert_eq!(json!(13), cancel["source"]["line"]);
    resident.ask(
        7,
        json!({"action": "open-source", "id": ":core.order", "member": "absent"}),
        "error",
    );
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

/// Every string inside a JSON value, depth first.
fn strings_in(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => vec![s.clone()],
        Value::Array(items) => items.iter().flat_map(strings_in).collect(),
        Value::Object(map) => map.values().flat_map(strings_in).collect(),
        _ => Vec::new(),
    }
}

fn has_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// The text every no-data marker of a frame shows; the frame has some.
fn no_data_text(frame: &Value) -> String {
    let texts: Vec<&str> = items(frame, "no-data")
        .iter()
        .map(|item| item["text"].as_str().unwrap())
        .collect();
    assert!(!texts.is_empty(), "no no-data marker: {frame}");
    assert!(texts.iter().all(|t| *t == texts[0]), "{texts:?}");
    texts[0].to_owned()
}

fn card_texts(card: &Value) -> Vec<String> {
    card["card"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["text"].as_str().map(str::to_owned))
        .collect()
}

/// #44: the text the resident lays out — the class card, the class box's
/// no-data marker and the source error — follows the language the panel
/// passes, English when none or an unsupported one was passed. The view the
/// session saves never records the language.
#[test]
fn the_card_no_data_marker_and_source_error_follow_the_passed_language() {
    let dir = source_project("language");
    let mut resident = opened(&dir);
    let frame = resident.ask(2, json!({"action": "drill-in", "id": ":core"}), "frame");
    assert_eq!("No data", no_data_text(&frame));
    resident.ask(
        3,
        json!({"action": "open-card", "id": ":core.order"}),
        "frame",
    );
    let card = resident.ask(4, json!({"action": "card"}), "card");
    let texts = card_texts(&card);
    for expected in ["Member", "Relationships", "depends on  core"] {
        assert!(
            texts.contains(&expected.to_owned()),
            "{expected}: {texts:?}"
        );
    }
    let everything = strings_in(&card);
    assert!(!everything.iter().any(|t| has_chinese(t)), "{everything:?}");
    let error = resident.ask(
        5,
        json!({"action": "open-source", "id": ":core.order", "member": "vanished"}),
        "error",
    );
    assert_eq!(
        json!("Could not open the source: src/order.rs/vanished"),
        error["message"]
    );

    // The panel passes Chinese: the answer is the frame in Chinese, and the
    // open card asked again is Chinese too.
    let frame = resident.ask(
        6,
        json!({"action": "language", "language": "zh-CN"}),
        "frame",
    );
    assert_eq!("无数据", no_data_text(&frame));
    let texts = card_texts(&resident.ask(7, json!({"action": "card"}), "card"));
    for expected in ["成员", "关系", "依赖  core"] {
        assert!(
            texts.contains(&expected.to_owned()),
            "{expected}: {texts:?}"
        );
    }
    let error = resident.ask(
        8,
        json!({"action": "open-source", "id": ":core.order", "member": "vanished"}),
        "error",
    );
    assert_eq!(
        json!("未能打开源码：src/order.rs/vanished"),
        error["message"]
    );
    let saved = resident.ask(9, json!({"action": "save"}), "state");
    let keys: Vec<&String> = saved["state"].as_object().unwrap().keys().collect();
    assert!(
        !keys.iter().any(|k| k.starts_with("lang")),
        "the saved view has no language: {keys:?}"
    );

    // Any other language reads English; a language must be a string.
    let frame = resident.ask(10, json!({"action": "language", "language": "fr"}), "frame");
    assert_eq!("No data", no_data_text(&frame));
    resident.ask(11, json!({"action": "language", "language": 7}), "error");
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}
