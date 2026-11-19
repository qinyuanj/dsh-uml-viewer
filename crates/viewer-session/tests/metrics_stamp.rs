//! The reload stamp covers every quality snapshot the overlay reads (#92):
//! refreshing `.metrics/dry.edn` alone must reload the panel, exactly like a
//! CRAP or mutation refresh does.

use edn::Value;

fn project(prefix: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("dsh-{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".metrics/mutate")).unwrap();
    root
}

fn stamp_text(stamp: &Value) -> String {
    match stamp {
        Value::String(text) => text.clone(),
        other => panic!("the stamp is an encoded string, got {other:?}"),
    }
}

#[test]
fn the_stamp_covers_the_duplicate_structure_snapshot() {
    let root = project("dry-stamp");
    std::fs::write(root.join(".metrics/crap.edn"), "{:entries []}\n").unwrap();
    std::fs::write(
        root.join(".metrics/mutate/a.edn"),
        "{:namespace \"a\" :forms []}\n",
    )
    .unwrap();
    let (files, before) = viewer_session::metrics::snapshot(&root);
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(vec![".metrics/crap.edn", ".metrics/mutate/a.edn"], paths);
    assert!(!stamp_text(&before).contains("dry.edn"));

    // The duplicate-structure snapshot joins the same set ...
    std::fs::write(root.join(".metrics/dry.edn"), "{:candidates []}\n").unwrap();
    let (files, after) = viewer_session::metrics::snapshot(&root);
    assert!(files.iter().any(|f| f.path == ".metrics/dry.edn"));
    assert!(stamp_text(&after).contains(".metrics/dry.edn"));
    assert_ne!(stamp_text(&before), stamp_text(&after));

    // ... and it is stamped by path, mtime and size, so rewriting it alone
    // changes the stamp and the session reloads.
    std::fs::write(root.join(".metrics/dry.edn"), "{:candidates [] }\n").unwrap();
    let (_, rewritten) = viewer_session::metrics::snapshot(&root);
    assert_ne!(stamp_text(&after), stamp_text(&rewritten));
}
