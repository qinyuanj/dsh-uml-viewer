//! Idle wake-ups (#143): an unchanged poll requests no repaint; a changed
//! diagram, metrics snapshot or mailbox does.

use std::path::PathBuf;
use viewer::watch::Wake;

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("viewer-wake-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".metrics/mutate")).unwrap();
    std::fs::write(dir.join("diagram.edn"), "{:packages []}").unwrap();
    dir
}

#[test]
fn an_unchanged_poll_requests_no_repaint() {
    let dir = project("same");
    let mut wake = Wake::new(&dir.join("diagram.edn"), &dir);
    assert!(!wake.poll());
    assert!(!wake.poll());
}

#[test]
fn a_changed_diagram_requests_one_repaint() {
    let dir = project("ir");
    let mut wake = Wake::new(&dir.join("diagram.edn"), &dir);
    std::fs::write(dir.join("diagram.edn"), "{:packages [{:label \"P\"}]}").unwrap();
    assert!(wake.poll());
    assert!(!wake.poll());
}

#[test]
fn an_equal_length_edit_of_the_diagram_requests_a_repaint() {
    // The stamp is the mtime (100 ns FILETIME ticks on Windows, ns on Linux)
    // plus the size, so a same-size edit is seen by its mtime alone.
    let dir = project("same-size");
    let diagram = dir.join("diagram.edn");
    std::fs::write(&diagram, "{:title \"A\"}").unwrap();
    let mut wake = Wake::new(&diagram, &dir);
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(&diagram, "{:title \"B\"}").unwrap();
        if wake.poll() {
            assert!(!wake.poll(), "one repaint per change");
            return;
        }
    }
    panic!("an equal-length edit never changed the diagram stamp");
}

#[test]
fn new_metrics_and_mail_request_a_repaint() {
    let dir = project("metrics");
    let mut wake = Wake::new(&dir.join("diagram.edn"), &dir);
    std::fs::write(dir.join(".metrics/mutate/a.edn"), "{}").unwrap();
    assert!(wake.poll());
    std::fs::create_dir_all(dir.join(".uml-viewer")).unwrap();
    std::fs::write(
        dir.join(".uml-viewer/to-viewer.edn"),
        "{:next-id 1 :queue []}",
    )
    .unwrap();
    assert!(wake.poll());
    assert!(!wake.poll());
}
