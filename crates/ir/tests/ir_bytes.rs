//! Host-independent IR bytes (#201 seam 1): each fixture project's `ir`
//! output must equal, byte for byte, the document committed in
//! `fixtures/ir-bytes/`, which was generated on Linux. On Windows this holds
//! for a CRLF working tree too, so it proves the path model and the four
//! scanners are host-independent, not just structurally equal.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assert_bytes_match(fixture: &str) {
    let repo = repo();
    let output =
        std::env::temp_dir().join(format!("uml-ir-bytes-{fixture}-{}.edn", std::process::id()));
    let args: Vec<OsString> = [
        "--root",
        repo.join(fixture).to_str().unwrap(),
        "--policy",
        "policy.edn",
        "--output",
        output.to_str().unwrap(),
    ]
    .iter()
    .map(OsString::from)
    .collect();
    assert_eq!(std::process::ExitCode::SUCCESS, ir::run(&args), "{fixture}");
    let expected_path = repo.join(format!("fixtures/ir-bytes/{fixture}.edn"));
    let expected = std::fs::read(&expected_path).unwrap();
    let actual = std::fs::read(&output).unwrap();
    assert!(
        actual == expected,
        "{fixture}: IR bytes differ from the Linux reference; output kept at {} \
         (diagnose with `python scripts/compare_ir.py {} {}`)",
        output.display(),
        output.display(),
        expected_path.display()
    );
    let _ = std::fs::remove_file(&output);
}

#[test]
fn rust_fixture_bytes_match_the_linux_reference() {
    assert_bytes_match("rust-fixture");
}

#[test]
fn python_fixture_bytes_match_the_linux_reference() {
    assert_bytes_match("python-fixture");
}

#[test]
fn typescript_fixture_bytes_match_the_linux_reference() {
    assert_bytes_match("typescript-fixture");
}

#[test]
fn lua_fixture_bytes_match_the_linux_reference() {
    assert_bytes_match("lua-fixture");
}

#[test]
fn the_linux_reference_is_portable_text() {
    // Guards the reference itself: a CRLF checkout (`core.autocrlf`) or a
    // host path leaking into IR would make the byte check meaningless.
    for fixture in [
        "rust-fixture",
        "python-fixture",
        "typescript-fixture",
        "lua-fixture",
    ] {
        let text = std::fs::read_to_string(repo().join(format!("fixtures/ir-bytes/{fixture}.edn")))
            .unwrap();
        assert!(!text.contains('\r'), "{fixture}: CR in the reference");
        assert!(
            !text.contains('\\'),
            "{fixture}: backslash in the reference"
        );
        assert!(!text.contains("/home/"), "{fixture}: host path");
        assert!(!text.contains("/mnt/"), "{fixture}: host path");
    }
}
