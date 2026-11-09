//! Seam ① through the real binary (#27): the class card, the inspector
//! (selected element + proposal list) and the source window travel over the
//! resident protocol. Every summary, count, CRAP/mutation figure, no-data
//! line, card row and source line is produced by `frame::inspector`,
//! `frame::detail` and `frame::source`; the assertions below name the values
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
    for action in [
        "inspector",
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

fn summary_lines(inspector: &Value) -> &Vec<Value> {
    inspector["inspector"]["summary"]["lines"]
        .as_array()
        .unwrap()
}

fn line_text(line: &Value) -> String {
    match line["type"].as_str().unwrap() {
        "label" => line["text"].as_str().unwrap().to_owned(),
        "segments" => line["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(" "),
        other => panic!("unknown summary line {other}"),
    }
}

#[test]
fn inspector_lists_the_source_diagram_and_proposals_with_the_empty_help() {
    let dir = shop_project("inspector");
    let mut resident = opened(&dir);
    let shown = resident.ask(2, json!({"action": "inspector"}), "inspector");
    assert_eq!(
        json!([
            {"id": null, "label": "Shop", "active": true},
            {"id": ":p-shop", "label": "2026-10-24 10:00:00", "active": false}
        ]),
        shown["inspector"]["entries"]
    );
    assert_eq!(json!("empty"), shown["inspector"]["summary"]["kind"]);
    assert_eq!(Value::Null, shown["inspector"]["detail"]);
    assert_eq!(Value::Null, shown["inspector"]["selected"]);
    // The no-selection explanation is frame::strings::HELP, verbatim.
    assert_eq!(
        json!(
            "单击类或命名空间查看摘要。\n双击类打开详情卡片。\n双击命名空间进入下一层。\nEsc（或 ←）返回上一层。\n滚轮平移；Shift+滚轮横向平移。\nCtrl+滚轮或 Ctrl+/− 以光标为中心缩放。\n空白处左键拖拽平移；Ctrl+0 适配全图。\nR 重新加载；F3 切换性能显示。\n单击上方列表中的图或提案即可切换。"
        ),
        summary_lines(&shown)[0]["text"]
    );
    assert_eq!(json!("muted"), summary_lines(&shown)[0]["color"]["role"]);
    assert_eq!(json!("精简：无"), shown["inspector"]["declutterLabel"]);

    // Choosing a proposal makes its entry active; P returns to the source tree.
    resident.ask(
        3,
        json!({"action": "context", "proposalId": ":p-shop"}),
        "context",
    );
    resident.expect("frame");
    let proposal = resident.ask(4, json!({"action": "inspector"}), "inspector");
    assert_eq!(
        json!([
            {"id": null, "label": "Shop", "active": false},
            {"id": ":p-shop", "label": "2026-10-24 10:00:00", "active": true}
        ]),
        proposal["inspector"]["entries"]
    );
    resident.ask(5, json!({"action": "namespace-tree"}), "frame");
    let tree = resident.ask(6, json!({"action": "inspector"}), "inspector");
    assert_eq!(json!(true), tree["inspector"]["entries"][0]["active"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn double_click_opens_the_class_card_and_the_inspector_summarizes_the_class() {
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

    let inspector = resident.ask(4, json!({"action": "inspector"}), "inspector");
    assert_eq!(json!("class"), inspector["inspector"]["summary"]["kind"]);
    assert_eq!(json!(":core.order"), inspector["inspector"]["detail"]);
    let texts: Vec<String> = summary_lines(&inspector).iter().map(line_text).collect();
    // Identity, package, level and the fixture's literal quality figures,
    // exactly as frame::inspector::class_summary orders them.
    assert_eq!(
        vec![
            "order".to_owned(),
            "包 core".to_owned(),
            "层级 0".to_owned(),
            "Crap μ 4.0 max 8.0 σ 1.5".to_owned(),
            "7 killed / 2 survived 3 uncovered".to_owned(),
        ],
        texts
    );
    let crap = summary_lines(&inspector)
        .iter()
        .find(|l| line_text(l).starts_with("Crap"))
        .unwrap();
    assert_eq!(json!("accent"), crap["color"]["role"]);

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

    // An unmeasured class keeps its 无数据 line, in the no-data role.
    let plain = centre_of(&frame, ":core.plain");
    resident.ask(6, json!({"action": "open-card", "at": plain}), "frame");
    let inspector = resident.ask(7, json!({"action": "inspector"}), "inspector");
    let last = summary_lines(&inspector).last().unwrap();
    assert_eq!(json!("无数据"), last["text"]);
    assert_eq!(json!("no-data"), last["color"]["role"]);

    resident.ask(8, json!({"action": "close-card"}), "frame");
    let inspector = resident.ask(9, json!({"action": "inspector"}), "inspector");
    assert_eq!(Value::Null, inspector["inspector"]["detail"]);
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
    let inspector = resident.ask(4, json!({"action": "inspector"}), "inspector");
    assert_eq!(Value::Null, inspector["inspector"]["detail"]);
    resident.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn selecting_a_package_summarizes_its_class_count_and_missing_quality() {
    let dir = shop_project("package summary");
    let mut resident = opened(&dir);
    let root = resident.ask(2, json!({"action": "viewport"}), "frame");
    // The root view's own package band, above every class it contains.
    let package = items(&root, "package")
        .into_iter()
        .find(|p| p["id"] == ":view")
        .expect("the root draws its view package");
    let r = &package["rect"];
    let at = [
        r["x"].as_f64().unwrap() + 12.0,
        r["y"].as_f64().unwrap() + 16.0,
    ];
    resident.ask(3, json!({"action": "select", "at": at}), "frame");
    let inspector = resident.ask(4, json!({"action": "inspector"}), "inspector");
    assert_eq!(json!("package"), inspector["inspector"]["summary"]["kind"]);
    let texts: Vec<String> = summary_lines(&inspector).iter().map(line_text).collect();
    assert!(texts.contains(&"类数：4".to_owned()), "{texts:?}");
    assert!(texts.contains(&"无数据".to_owned()), "{texts:?}");
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
fn a_card_request_without_an_open_card_is_an_error_that_changes_nothing() {
    let dir = shop_project("no card");
    let mut resident = opened(&dir);
    resident.ask(2, json!({"action": "card"}), "error");
    resident.ask(
        3,
        json!({"action": "open-card", "id": ":missing.class"}),
        "frame",
    );
    resident.ask(4, json!({"action": "card"}), "error");
    resident.ask(5, json!({"action": "open-source", "id": ":api"}), "error");
    let inspector = resident.ask(6, json!({"action": "inspector"}), "inspector");
    assert_eq!(json!("empty"), inspector["inspector"]["summary"]["kind"]);
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
