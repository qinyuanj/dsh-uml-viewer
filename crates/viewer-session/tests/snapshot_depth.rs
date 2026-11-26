//! #163: the CLI `scene` export and the resident must discover the same
//! mutation snapshots, at every depth. They share
//! `scene::snapshots::load_snapshots`; these tests pin both the record set and
//! the geometry that set produces.

use edn::Value;
use std::path::PathBuf;

const LOOSE: &str = "{:namespace \"demo.loose\" :forms [{:id \"defn/a\" :killed 2 :survived 0}]}\n";
const NESTED: &str =
    "{:namespace \"demo.nested\" :forms [{:id \"defn/b\" :killed 1 :survived 1}]}\n";

const IR: &str = r#"{:hierarchical true :prefix "demo"
  :classes [{:id :demo :ns "demo"}
            {:id :demo.loose :ns "demo.loose"}
            {:id :demo.nested :ns "demo.nested"}]}"#;

fn project(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("dsh-{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".metrics/mutate")).unwrap();
    std::fs::write(root.join(".metrics/crap.edn"), "{:entries []}\n").unwrap();
    std::fs::write(root.join(".metrics/dry.edn"), "{:candidates []}\n").unwrap();
    root
}

fn paths(files: &[scene::Snapshot]) -> Vec<String> {
    files.iter().map(|s| s.path.clone()).collect()
}

fn compiled(files: &[scene::Snapshot]) -> String {
    let doc = scene::ir::read_document(IR).unwrap();
    let scene = scene::document::compile_view(&doc, files, &[], &Value::Map(vec![]));
    edn::encode(&scene).unwrap()
}

#[test]
fn the_cli_export_and_the_resident_discover_the_same_snapshots() {
    let root = project("snapshot-depth");
    std::fs::write(root.join(".metrics/mutate/loose.edn"), LOOSE).unwrap();
    std::fs::create_dir_all(root.join(".metrics/mutate/crate")).unwrap();
    std::fs::write(root.join(".metrics/mutate/crate/nested.edn"), NESTED).unwrap();
    let cli = scene::cli::load_snapshots(&root);
    let (resident, _) = viewer_session::metrics::snapshot(&root);
    assert_eq!(
        vec![
            ".metrics/crap.edn",
            ".metrics/dry.edn",
            ".metrics/mutate/crate/nested.edn",
            ".metrics/mutate/loose.edn",
        ],
        paths(&cli)
    );
    assert_eq!(paths(&cli), paths(&resident));
}

#[test]
fn nesting_a_snapshot_never_changes_the_geometry() {
    // The panel reads the nested tree; the oracle must lay it out identically.
    let nested = project("snapshot-nested");
    std::fs::write(nested.join(".metrics/mutate/loose.edn"), LOOSE).unwrap();
    std::fs::create_dir_all(nested.join(".metrics/mutate/crate")).unwrap();
    std::fs::write(nested.join(".metrics/mutate/crate/nested.edn"), NESTED).unwrap();
    let flat = project("snapshot-flat");
    std::fs::write(flat.join(".metrics/mutate/loose.edn"), LOOSE).unwrap();
    std::fs::write(flat.join(".metrics/mutate/nested.edn"), NESTED).unwrap();
    let nested_files = scene::snapshots::load_snapshots(&nested);
    let flat_files = scene::snapshots::load_snapshots(&flat);
    assert_eq!(4, nested_files.len(), "both mutation snapshots are found");
    assert_eq!(4, flat_files.len());
    assert_eq!(
        compiled(&flat_files),
        compiled(&nested_files),
        "the same snapshot at another depth draws the same diagram"
    );
}
