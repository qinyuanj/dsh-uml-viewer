//! Argument parsing of the standalone window: one optional diagram path.

use std::ffi::OsString;
use std::path::PathBuf;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn a_bare_diagram_path_is_the_common_case() {
    let options = viewer::cli::parse(&args(&["diagram.edn"])).unwrap();
    assert_eq!(Some(PathBuf::from("diagram.edn")), options.path);
    assert_eq!(
        viewer::cli::Options::default(),
        viewer::cli::parse(&[]).unwrap()
    );
}

#[test]
fn extra_positionals_are_ignored_like_the_lua_parser() {
    let options = viewer::cli::parse(&args(&["a.edn", "b.edn"])).unwrap();
    assert_eq!(Some(PathBuf::from("a.edn")), options.path);
}

/// The companion lifecycle options left with the companion (#18): the window
/// runs standalone and names any of them as an unknown option.
#[test]
fn the_removed_companion_options_are_unknown_options() {
    for (values, named) in [
        (vec!["-x"], "-x"),
        (vec!["--restart", "d.edn"], "--restart"),
        (vec!["--backend", "herdr", "d.edn"], "--backend"),
        (vec!["--coding-agent", "codex"], "--coding-agent"),
    ] {
        assert_eq!(
            format!("unknown option: {named}"),
            viewer::cli::parse(&args(&values)).unwrap_err()
        );
    }
}

#[test]
fn watch_stamp_has_nanoseconds_and_catches_equal_length_edits() {
    let dir = std::env::temp_dir().join(format!("viewer-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("crap.edn");
    std::fs::write(&file, "{:crap 1}").unwrap();
    let before = viewer::watch::mtime(&file).unwrap();
    let parts: Vec<&str> = before.split(':').collect();
    assert_eq!(3, parts.len(), "sec:nsec:size");
    assert_eq!("9", parts[2]);
    // Equal-byte-length edit: only the nanosecond mtime may change.
    for _ in 0..100 {
        std::fs::write(&file, "{:crap 9}").unwrap();
        let after = viewer::watch::mtime(&file).unwrap();
        if after != before {
            assert_ne!(parts[1], after.split(':').nth(1).unwrap());
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(&file, "{:crap 1}").unwrap();
        let _ = viewer::watch::mtime(&file).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    panic!("nanosecond mtime never changed across equal-length edits");
}
