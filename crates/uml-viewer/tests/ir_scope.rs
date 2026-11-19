//! #83: IR names a policy source root that `uml-quality.json` gives no
//! quality provider for. A class with no data stays red (AGENTS.md); this
//! warning only explains *why* it can never leave red.

use std::path::PathBuf;
use std::process::Output;

use edn::Value;
use edn::record::{get, seq};

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A private project root removed when dropped.
struct Project(PathBuf);

impl Project {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("uml-ir-scope-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Project(dir)
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn run(&self) -> Output {
        super::binary([
            "ir".as_ref(),
            "--root".as_ref(),
            self.0.as_os_str(),
            "policy.edn".as_ref(),
            "out.edn".as_ref(),
        ])
    }

    fn document(&self) -> Value {
        edn::parse(&std::fs::read_to_string(self.0.join("out.edn")).unwrap()).unwrap()
    }
}

/// Classes the IR holds whose `:file` is under `root`.
fn classes_under(doc: &Value, root: &str) -> usize {
    let prefix = format!("{root}/");
    seq(get(doc, "classes"))
        .iter()
        .filter(|class| match get(class, "file") {
            Some(Value::String(file)) => file == root || file.starts_with(&prefix),
            _ => false,
        })
        .count()
}

/// A policy scanning one covered Rust root and one uncovered TypeScript root.
fn mixed_project(tag: &str) -> Project {
    let project = Project::new(tag);
    project.write(
        "policy.edn",
        "{:title \"Scope\" :hierarchical true :prefix \"app\" \
         :sources [{:lang :rust :root \"crates/lib/src\" :prefix \"app.lib\"} \
                   {:lang :typescript :root \"packages/web/src\" :prefix \"app.web\"}] \
         :levels [] :order [] :out \"out.edn\"}",
    );
    project.write(
        "uml-quality.json",
        r#"{"version":1,"standardTools":{"sourceRoots":["crates"]}}"#,
    );
    project.write(
        "crates/lib/src/lib.rs",
        "pub struct Widget {\n    pub size: u32,\n}\n",
    );
    project.write(
        "packages/web/src/widget.ts",
        "export class Alpha {\n  size = 1;\n}\nexport class Beta {}\n",
    );
    // The TypeScript scanner makes one class per source file.
    project.write("packages/web/src/model.ts", "export class Gamma {}\n");
    project
}

fn count_in(classes: usize) -> String {
    format!(
        "{classes} {} get no CRAP or mutation data",
        if classes == 1 { "class" } else { "classes" }
    )
}

#[test]
fn an_uncovered_policy_source_root_is_named_with_its_class_count_and_the_fix() {
    let project = mixed_project("uncovered");
    let output = project.run();
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));

    let covered = classes_under(&project.document(), "crates/lib/src");
    let uncovered = classes_under(&project.document(), "packages/web/src");
    assert!(
        covered > 0 && uncovered > 1,
        "covered {covered}, uncovered {uncovered}"
    );

    let err = stderr(&output);
    assert!(err.contains("packages/web/src"), "{err}");
    assert!(err.contains(&count_in(uncovered)), "{err}");
    assert!(err.contains("standardTools.sourceRoots"), "{err}");
    assert!(err.contains("uml-quality.json"), "{err}");
    // The covered root is not reported.
    assert!(!err.contains("\"crates/lib/src\""), "{err}");
}

#[test]
fn a_quality_scope_that_covers_every_policy_root_is_silent() {
    let project = mixed_project("covered");
    project.write(
        "uml-quality.json",
        r#"{"version":1,"standardTools":{"sourceRoots":["crates","packages"]}}"#,
    );
    let output = project.run();
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
    let err = stderr(&output);
    assert!(!err.contains("has no quality provider"), "{err}");
}

#[test]
fn a_whole_tree_scope_declares_nothing_uncovered() {
    // No manifest, and an empty root list, both mean the whole tree.
    for (tag, config) in [
        ("absent", None),
        ("empty", Some(r#"{"version":1,"standardTools":{}}"#)),
        (
            "empty-roots",
            Some(r#"{"version":1,"standardTools":{"sourceRoots":[]}}"#),
        ),
    ] {
        let project = mixed_project(tag);
        match config {
            Some(text) => project.write("uml-quality.json", text),
            None => std::fs::remove_file(project.0.join("uml-quality.json")).unwrap(),
        }
        let output = project.run();
        assert_eq!(Some(0), output.status.code(), "{tag}: {}", stderr(&output));
        let err = stderr(&output);
        assert!(!err.contains("has no quality provider"), "{tag}: {err}");
    }
}

#[test]
fn an_operations_override_is_not_described_by_standard_source_roots() {
    // An `operations` command owns its own scope; sourceRoots no longer
    // describe what the quality run scores, so this check stays silent.
    let project = mixed_project("operations");
    project.write(
        "uml-quality.json",
        r#"{"version":1,"standardTools":{"sourceRoots":["crates"]},
            "operations":{
              "crap":{"argv":["node","crap.mjs"],"forwardArgs":"none","validResultExitCodes":[0],"scope":"whole-tree"},
              "mutate":{"argv":["node","mutate.mjs"],"forwardArgs":"none","validResultExitCodes":[0]}}}"#,
    );
    let output = project.run();
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
    let err = stderr(&output);
    assert!(!err.contains("has no quality provider"), "{err}");
}

#[test]
fn a_sibling_name_sharing_a_prefix_with_a_provider_is_still_uncovered() {
    // "crates-extra" is not under "crates"; only a path boundary counts.
    let project = Project::new("prefix");
    project.write(
        "policy.edn",
        "{:title \"Scope\" :hierarchical true :prefix \"app\" \
         :sources [{:lang :rust :root \"crates/lib/src\" :prefix \"app.lib\"} \
                   {:lang :rust :root \"crates-extra/lib/src\" :prefix \"app.extra\"}] \
         :levels [] :order [] :out \"out.edn\"}",
    );
    project.write(
        "uml-quality.json",
        r#"{"version":1,"standardTools":{"sourceRoots":["crates"]}}"#,
    );
    project.write(
        "crates/lib/src/lib.rs",
        "pub struct Widget {\n    pub size: u32,\n}\n",
    );
    project.write(
        "crates-extra/lib/src/lib.rs",
        "pub struct Gadget {\n    pub size: u32,\n}\n",
    );
    let output = project.run();
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("\"crates-extra/lib/src\""), "{err}");
    assert!(!err.contains("\"crates/lib/src\""), "{err}");
}
