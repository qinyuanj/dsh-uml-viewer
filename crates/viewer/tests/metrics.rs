//! Metrics snapshot loading and the evidence file contract.

use edn::Value;

fn temp_project(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("viewer-metrics-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn root_walks_up_to_the_metrics_directory() {
    let dir = temp_project("root");
    std::fs::create_dir_all(dir.join(".metrics")).unwrap();
    std::fs::create_dir_all(dir.join("nested/deep")).unwrap();
    let diagram = dir.join("nested/deep/diagram.edn");
    std::fs::write(&diagram, "{}").unwrap();
    assert_eq!(
        viewer::project_files::canonical(&dir),
        viewer::metrics::root(&diagram)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_collects_crap_and_mutate_files_sorted_by_path() {
    let dir = temp_project("snap");
    std::fs::create_dir_all(dir.join(".metrics/mutate")).unwrap();
    std::fs::write(dir.join(".metrics/crap.edn"), "{:entries []}").unwrap();
    std::fs::write(dir.join(".metrics/mutate/b.edn"), "{:namespace :b}").unwrap();
    std::fs::write(dir.join(".metrics/mutate/a.edn"), "{:namespace :a}").unwrap();
    let (files, stamp) = viewer::metrics::snapshot(&dir);
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        vec![
            ".metrics/crap.edn",
            ".metrics/mutate/a.edn",
            ".metrics/mutate/b.edn"
        ],
        paths
    );
    assert_eq!("{:entries []}", files[0].text);
    // The stamp is the encoded [path mtime size] vector.
    let Value::String(text) = &stamp else {
        panic!("stamp must be a string")
    };
    let decoded = edn::parse(text).unwrap();
    let Value::Vector(rows) = decoded else {
        panic!("stamp decodes to a vector")
    };
    assert_eq!(3, rows.len());
    let Value::Vector(first) = &rows[0] else {
        panic!("row is a vector")
    };
    assert_eq!(Value::String(".metrics/crap.edn".into()), first[0]);
    assert_eq!(Value::Integer(13), first[2]);
    assert!(matches!(&first[1], Value::String(m) if m.split(':').count() == 3));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_equal_byte_length_edit_changes_the_stamp() {
    let dir = temp_project("stamp");
    std::fs::create_dir_all(dir.join(".metrics")).unwrap();
    let crap = dir.join(".metrics/crap.edn");
    std::fs::write(&crap, "{:crap 1}").unwrap();
    let (_, first) = viewer::metrics::snapshot(&dir);
    for _ in 0..100 {
        std::fs::write(&crap, "{:crap 9}").unwrap();
        let (_, second) = viewer::metrics::snapshot(&dir);
        if second != first {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(&crap, "{:crap 1}").unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    panic!("equal-length edit never changed the metrics stamp");
}

#[test]
fn publish_writes_the_encoded_scene_without_a_trailing_newline() {
    let dir = temp_project("evidence");
    let evidence = viewer::evidence::Evidence::new(dir.clone());
    let scene = edn::parse("{:classes [] :mu 1}").unwrap();
    evidence.publish(&scene, 1).unwrap();
    evidence.publish(&scene, 2).unwrap();
    let text = std::fs::read_to_string(dir.join("scene-1.edn")).unwrap();
    assert!(!text.ends_with('\n'));
    assert!(text.contains(":mu 1"));
    assert!(dir.join("scene-2.edn").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
