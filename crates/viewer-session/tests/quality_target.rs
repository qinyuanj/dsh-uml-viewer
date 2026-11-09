//! Quality refresh targets (#28) as the resident answers them: canonical
//! project-relative files, deduplicated, bounded, or an explicit refusal.
//! Authored under the RESOURCE-POLICY of spec #203 and **NOT RUN**: no Rust
//! test binary may be built or executed in this session. Run
//! `cargo test -p viewer-session --locked` when the policy allows it.

use std::fs;
use std::path::{Path, PathBuf};

use edn::Value;
use serde_json::{Value as Json, json};
use viewer_session::{Action, Session, Start, targets};

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "uml-viewer-quality-target-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

fn keyword(text: &str) -> Value {
    edn::parse(text).expect("a keyword")
}

fn doc(text: &str) -> Value {
    scene::ir::read_document(text).expect("the fixture is a hierarchical IR document")
}

fn no_context() -> Value {
    Value::Map(vec![])
}

fn proposal_context(id: &str) -> Value {
    Value::Map(vec![(Value::keyword("proposal-id"), keyword(id))])
}

fn target_in(root: &Path, ir: &str, id: &str) -> Result<Json, String> {
    targets::quality_target_for(&doc(ir), &[], &no_context(), root, &keyword(id))
}

fn files(answer: &Json) -> Vec<String> {
    answer["files"]
        .as_array()
        .expect("files is an array")
        .iter()
        .map(|file| file.as_str().expect("a string").to_owned())
        .collect()
}

const SOURCE: &str = r#"{:hierarchical true :prefix "demo" :omit []
  :classes [{:id :api :file "src/api.rs"}
            {:id :core :file "src/core/lib.rs"}
            {:id :core.order :file "src/core/order.rs"}
            {:id :core.order.line :file "src/core/order.rs"}
            {:id :core.cart :file "src/core/cart.rs"}
            {:id :quil :name "quil" :foreign true}
            {:id :core.sourceless :name "Sourceless"}]
  :proposals []}"#;

fn source_project(label: &str) -> PathBuf {
    let root = scratch(label);
    for path in [
        "src/api.rs",
        "src/core/lib.rs",
        "src/core/order.rs",
        "src/core/cart.rs",
    ] {
        write(&root, path, "// fixture\n");
    }
    root
}

#[test]
fn a_class_answer_names_its_own_file() {
    let root = source_project("class");
    let answer = target_in(&root, SOURCE, ":api").unwrap();
    assert_eq!(
        answer,
        json!({"id": ":api", "kind": "class", "files": ["src/api.rs"]})
    );
}

#[test]
fn a_component_answer_is_sorted_and_each_file_once() {
    let root = source_project("component");
    let source = SOURCE.replace(r#"{:id :core.sourceless :name "Sourceless"}"#, "");
    let answer = target_in(&root, &source, ":core").unwrap();
    assert_eq!(answer["kind"], json!("component"));
    // `:core.order` and `:core.order.line` collapse to one file, and the
    // order is the producer's, so two requests always agree.
    assert_eq!(
        files(&answer),
        ["src/core/cart.rs", "src/core/lib.rs", "src/core/order.rs"]
    );
    assert_eq!(target_in(&root, &source, ":core").unwrap(), answer);
}

#[test]
fn a_class_without_a_source_file_refuses_itself_and_its_component() {
    let root = source_project("sourceless");
    let error = target_in(&root, SOURCE, ":core.sourceless").unwrap_err();
    assert!(error.contains("has no source file"), "{error}");
    // A component never silently drops the class it cannot refresh.
    let error = target_in(&root, SOURCE, ":core").unwrap_err();
    assert!(error.contains(":core.sourceless"), "{error}");
}

#[test]
fn a_foreign_or_unknown_id_is_refused() {
    let root = source_project("foreign");
    let error = target_in(&root, SOURCE, ":quil").unwrap_err();
    assert!(
        error.contains("no class or namespace component :quil"),
        "{error}"
    );
    let error = target_in(&root, SOURCE, ":nowhere").unwrap_err();
    assert!(error.contains(":nowhere"), "{error}");
}

#[test]
fn an_escaped_or_absolute_source_path_is_refused() {
    let root = scratch("escape");
    let outside = std::env::temp_dir().join(format!(
        "uml-viewer-quality-target-outside-{}.rs",
        std::process::id()
    ));
    fs::write(&outside, "// outside\n").unwrap();
    write(&root, "src/api.rs", "// fixture\n");
    let ir = format!(
        r#"{{:hierarchical true :omit []
  :classes [{{:id :escaped :file "../outside.rs"}}
            {{:id :absolute :file "{}"}}]
  :proposals []}}"#,
        outside.to_string_lossy().replace('\\', "/")
    );
    for id in [":escaped", ":absolute"] {
        let error = target_in(&root, &ir, id).unwrap_err();
        assert!(
            error.contains("normalized project-relative path"),
            "{id}: {error}"
        );
    }
    let _ = fs::remove_file(&outside);
}

#[test]
fn a_source_file_that_is_gone_is_refused_rather_than_delivered() {
    let root = scratch("missing");
    write(&root, "src/api.rs", "// fixture\n");
    let error = target_in(
        &root,
        r#"{:hierarchical true :omit []
  :classes [{:id :api :file "src/api.rs"} {:id :api.old :file "src/api/old.rs"}]
  :proposals []}"#,
        ":api",
    )
    .unwrap_err();
    assert!(error.contains("source file \"src/api/old.rs\""), "{error}");
}

#[test]
fn a_proposal_component_is_resolved_from_the_shown_proposal() {
    let root = scratch("proposal");
    for path in [
        "src/core/order.rs",
        "src/core/cart.rs",
        "src/api.rs",
        "src/util.rs",
    ] {
        write(&root, path, "// fixture\n");
    }
    let ir = r#"{:hierarchical true :omit []
  :classes [{:id :core.order :file "src/core/order.rs"}
            {:id :core.order.total :file "src/core/order.rs"}
            {:id :core.cart :file "src/core/cart.rs"}
            {:id :api :file "src/api.rs"}
            {:id :util :file "src/util.rs"}]
  :proposals [{:id :p-1 :name "2026-11-09 10:00" :omit []
               :layers [{:id :domain :label "Domain" :nses [:core]}
                        {:id :io :label "IO" :nses [:core.cart]}]}]}"#;
    let document = doc(ir);
    // Without a proposal the layer id is not in the shown tree.
    let error = targets::quality_target_for(
        &document,
        &[],
        &no_context(),
        &root,
        &keyword(":proposal.domain"),
    )
    .unwrap_err();
    assert!(error.contains("no class or namespace component"), "{error}");
    let answer = targets::quality_target_for(
        &document,
        &[],
        &proposal_context(":p-1"),
        &root,
        &keyword(":proposal.domain"),
    )
    .unwrap();
    assert_eq!(answer["kind"], json!("component"));
    assert_eq!(files(&answer), ["src/core/cart.rs", "src/core/order.rs"]);
    // Leftovers are the classes no layer claims.
    let unassigned = targets::quality_target_for(
        &document,
        &[],
        &proposal_context(":p-1"),
        &root,
        &keyword(":proposal.unassigned"),
    )
    .unwrap();
    assert_eq!(files(&unassigned), ["src/api.rs", "src/util.rs"]);
}

#[test]
fn the_resident_reads_the_context_its_loaded_session_shows() {
    let root = scratch("loaded");
    for path in ["src/api.rs", "src/core/order.rs", "src/util.rs"] {
        write(&root, path, "// fixture\n");
    }
    let ir = r#"{:hierarchical true :prefix "demo" :omit [] :levels [] :order []
  :edge-kinds {} :omit-edges [] :edges []
  :classes [{:id :core.order :file "src/core/order.rs"}
            {:id :api :file "src/api.rs"}
            {:id :util :file "src/util.rs"}]
  :proposals [{:id :p-1 :name "2026-11-09 10:00" :omit []
               :layers [{:id :domain :label "Domain" :nses [:core]}]}]}"#;
    write(&root, "diagram.edn", ir);
    let mut session = Session::open(root.clone(), root.join("diagram.edn"), Start::Fresh)
        .expect("the fixture diagram loads");
    // Before the context is chosen the source tree is shown.
    assert!(targets::quality_target_json(&session, &keyword(":proposal.domain")).is_err());
    session.act(
        Action::Choose(Some(keyword(":p-1"))),
        &scene::events::Dims::default(),
    );
    let answer = targets::quality_target_json(&session, &keyword(":proposal.domain")).unwrap();
    assert_eq!(files(&answer), ["src/core/order.rs"]);
    let unassigned =
        targets::quality_target_json(&session, &keyword(":proposal.unassigned")).unwrap();
    assert_eq!(files(&unassigned), ["src/api.rs", "src/util.rs"]);
}

#[test]
fn a_waiting_canvas_is_refused_with_the_no_diagram_reason() {
    let root = scratch("waiting");
    let session = Session::open(root.clone(), root.join("diagram.edn"), Start::Waiting).unwrap();
    let error = targets::quality_target_json(&session, &keyword(":api")).unwrap_err();
    assert!(error.contains("no diagram"), "{error}");
}

#[test]
fn more_files_than_the_bound_is_refused() {
    let root = scratch("bound");
    let over = targets::QUALITY_TARGET_MAX_FILES + 1;
    fs::create_dir_all(root.join("src/bulk")).unwrap();
    let mut classes = String::new();
    for index in 0..over {
        let path = format!("src/bulk/f{index}.rs");
        fs::write(root.join(&path), "// fixture\n").unwrap();
        classes.push_str(&format!("{{:id :bulk.c{index} :file \"{path}\"}} "));
    }
    let ir = format!("{{:hierarchical true :omit [] :classes [{classes}] :proposals []}}");
    let error = target_in(&root, &ir, ":bulk").unwrap_err();
    assert!(
        error.contains(&format!(
            "over {} source files",
            targets::QUALITY_TARGET_MAX_FILES
        )),
        "{error}"
    );
}
