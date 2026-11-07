//! Element menu contract (#142): the menu writes the exact `to-agent.edn`
//! commands and target shape the LOVE viewer wrote. The queue protocol is
//! tested with `companion::mailbox`.

use companion::mailbox::Mailbox;
use edn::Value;
use edn::record::get;
use scene::document;
use viewer::mailbox::{self, MENU_OPS};

const DIAGRAM: &str = "{:title \"Second diagram\" :hierarchical true :prefix \"app\" :classes [{:id :app.target :name \"target\" :ns \"app.target\" :contents [{:id :inner}]}] :proposals [{:id :p :name \"P\" :layers []}]}";

fn project() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "uml mailbox {} {}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".uml-viewer")).unwrap();
    dir
}

#[test]
fn the_menu_operations_append_in_menu_order_with_their_target() {
    let dir = project();
    let outbox = Mailbox::outbox(&dir);
    let target = Value::Map(vec![
        (Value::keyword("kind"), Value::keyword("class")),
        (Value::keyword("id"), Value::keyword("app.target")),
    ]);
    let extra = vec![(Value::keyword("target"), target)];
    for op in MENU_OPS {
        outbox.append(op, &extra).unwrap();
    }
    let text = std::fs::read_to_string(dir.join(".uml-viewer/to-agent.edn")).unwrap();
    let expected = "{:next-id 5 :queue [\
{:id 1 :op :refresh-crap :target {:id :app.target :kind :class}} \
{:id 2 :op :refresh-mutate :target {:id :app.target :kind :class}} \
{:id 3 :op :refresh-mutate-all :target {:id :app.target :kind :class}} \
{:id 4 :op :omit :target {:id :app.target :kind :class}}]}";
    assert_eq!(expected, text);
    let _ = std::fs::remove_dir_all(dir);
}

fn state() -> scene::document::ViewState {
    let doc = scene::ir::read_document(DIAGRAM).unwrap();
    let mut state = document::new_state(doc, None, None, None, &[], None);
    state
        .extra
        .assoc(Value::keyword("proposal-id"), Value::keyword("p"));
    state
}

#[test]
fn a_menu_target_carries_the_namespace_and_the_proposal_under_discussion() {
    let state = state();
    let selected = edn::parse("{:kind :class :id :app.target}").unwrap();
    let target = mailbox::target(&state, Some(&selected));
    assert_eq!(Some(&Value::keyword("component")), get(&target, "kind"));
    assert_eq!(Some(&Value::keyword("app.target")), get(&target, "id"));
    assert_eq!(
        Some(&Value::String("app.target".into())),
        get(&target, "ns")
    );
    assert_eq!(Some(&Value::keyword("p")), get(&target, "proposal-id"));
}

#[test]
fn a_leaf_class_without_contents_is_a_class_target() {
    let state = state();
    let selected = edn::parse("{:kind :class :id :app}").unwrap();
    let target = mailbox::target(&state, Some(&selected));
    assert_eq!(Some(&Value::keyword("class")), get(&target, "kind"));
    assert_eq!(
        None,
        get(&target, "ns"),
        "a class without a namespace carries none"
    );
    // A proposal row and a package are components.
    let package = edn::parse("{:kind :package :id :app.target}").unwrap();
    assert_eq!(
        Some(&Value::keyword("component")),
        get(&mailbox::target(&state, Some(&package)), "kind")
    );
    let proposal = edn::parse("{:kind :class :id :p :drill? true}").unwrap();
    assert_eq!(
        Some(&Value::keyword("component")),
        get(&mailbox::target(&state, Some(&proposal)), "kind")
    );
}

#[test]
fn a_scene_id_resolves_to_the_authored_document_class_and_its_namespace() {
    // The compiled scene prefixes section ids (`d0-target`); the mail
    // contract names the authored IR class, as the LOVE viewer did.
    let doc = scene::ir::read_document(
        "{:title \"Second diagram\" :packages [{:label \"P\" :classes [{:id :target :ns \"app.target\"}]}]}",
    )
    .unwrap();
    let state = document::new_state(doc, None, None, None, &[], None);
    let selected = edn::parse("{:kind :class :id :d0-target}").unwrap();
    let target = mailbox::target(&state, Some(&selected));
    assert_eq!(Some(&Value::keyword("target")), get(&target, "id"));
    assert_eq!(
        Some(&Value::String("app.target".into())),
        get(&target, "ns")
    );
    assert_eq!(Some(&Value::keyword("class")), get(&target, "kind"));
}

#[test]
fn a_dummy_class_is_a_component_target_for_any_lua_truthy_flag() {
    // `mail_menu.lua` `target`: `... or G.get(class,'dummy?') or ...`, a Lua
    // truthiness test on the document class. A hierarchical document is
    // read raw, so an authored `:dummy?` reaches the target.
    let kind = |fragment: &str| {
        let doc = scene::ir::read_document(&format!(
            "{{:title \"Dummy\" :hierarchical true :prefix \"app\" :classes [{{:id :app.stub :ns \"app.stub\" {fragment}}}]}}"
        ))
        .unwrap();
        let state = document::new_state(doc, None, None, None, &[], None);
        let selected = edn::parse("{:kind :class :id :app.stub}").unwrap();
        get(&mailbox::target(&state, Some(&selected)), "kind").cloned()
    };
    for falsy in ["", ":dummy? false", ":dummy? nil"] {
        assert_eq!(Some(Value::keyword("class")), kind(falsy), "{falsy}");
    }
    for truthy in [":dummy? true", ":dummy? :yes", ":dummy? 1"] {
        assert_eq!(Some(Value::keyword("component")), kind(truthy), "{truthy}");
    }
}

const WRITER: &str = "UML_VIEWER_MAILBOX_WRITER_ROOT";
const REPLACEMENTS: i64 = 400;

/// `{:next-id n :queue [{:id 1 …} … {:id n-1 …}]}`.
fn queue_of(next: i64) -> Value {
    let items = (1..next)
        .map(|id| {
            edn::record::record(vec![
                ("id", Value::Integer(id)),
                ("op", Value::keyword("context")),
                ("context", Value::keyword("real")),
            ])
        })
        .collect();
    edn::record::record(vec![
        ("next-id", Value::Integer(next)),
        ("queue", Value::Vector(items)),
    ])
}

/// The writer half: another process (the companion's role) atomically
/// replaces the viewer's inbox with ever longer queues.
#[test]
fn concurrent_inbox_writer_child() {
    let Some(root) = std::env::var_os(WRITER) else {
        return;
    };
    let inbox = Mailbox::inbox(std::path::Path::new(&root));
    for next in 2..=REPLACEMENTS {
        inbox
            .write(&queue_of(next))
            .expect("an atomic replace failed");
    }
}

#[test]
fn the_viewer_reads_whole_queues_while_another_process_replaces_its_inbox() {
    let dir = project();
    let inbox = Mailbox::inbox(&dir);
    inbox.write(&queue_of(1)).unwrap();
    let mut writer = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "mailbox::concurrent_inbox_writer_child",
            "--test-threads",
            "1",
        ])
        .env(WRITER, &dir)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut reads = 0;
    let mut last = 1;
    let status = loop {
        let done = writer.try_wait().unwrap();
        // The viewer's read: the whole file, then the EDN queue.
        let text = std::fs::read_to_string(inbox.path())
            .unwrap_or_else(|error| panic!("read {reads} failed during a replace: {error}"));
        let boxed =
            edn::parse(&text).unwrap_or_else(|error| panic!("torn queue {text:?}: {error:?}"));
        let next = edn::record::get(&boxed, "next-id")
            .and_then(edn::record::num)
            .unwrap() as i64;
        assert_eq!(queue_of(next), boxed, "a whole queue");
        assert!(next >= last, "the inbox went back from {last} to {next}");
        last = next;
        reads += 1;
        if let Some(status) = done {
            break status;
        }
    };
    assert!(status.success(), "the writer's atomic replace failed");
    assert_eq!(
        REPLACEMENTS, last,
        "the last replace is what the viewer reads"
    );
    assert!(reads > 10, "the reads overlapped the writes ({reads})");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn menu_mail_names_the_operation_and_the_target() {
    let state = state();
    let selected = edn::parse("{:kind :class :id :app.target}").unwrap();
    let (op, extra) = mailbox::menu_mail(&state, Some(&selected), "omit");
    assert_eq!("omit", op);
    assert_eq!(1, extra.len());
    assert_eq!(Value::keyword("target"), extra[0].0);
}
