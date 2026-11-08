//! Public binary seam for the Rust dryer port.
use std::fs;
use std::process::{Command, Output};

use crate::crap::Project;

fn run(project: &Project, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("dry")
        .arg("--root")
        .arg(&project.root)
        .args(args)
        .current_dir(&project.root)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}

#[test]
fn help_is_the_original_and_non_rust_selection_is_explicitly_refused() {
    let project = Project::new();
    let output = run(&project, &["--help"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        include_bytes!("../../../fixtures/dry/help.txt")
    );
    assert!(output.stderr.is_empty());

    project.write("src/other.py", "def duplicate():\n    return 1\n");
    let output = run(&project, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("python is not ported yet")
    );
    assert!(!project.root.join(".metrics/dry.edn").exists());
}

#[test]
fn original_argument_and_exit_contract() {
    let project = Project::new();
    for (args, code, message) in [
        (&["--nope"][..], 1, "Unknown option: --nope\n\nUsage: dryer"),
        (&["--root"][..], 1, "--root requires a value"),
        (&["--min-lines", ""][..], 1, "--min-lines requires a value"),
        (
            &["--threshold", "nope"][..],
            1,
            "--threshold requires a number",
        ),
        (
            &["--min-nodes", "1.5"][..],
            1,
            "--min-nodes requires an integer",
        ),
        (&["--format", "csv"][..], 2, "Unknown format: csv\n"),
        (
            &["--exclude", "../outside"][..],
            1,
            "--exclude must be a path or glob inside the project root",
        ),
    ] {
        let output = run(&project, args);
        assert_eq!(output.status.code(), Some(code), "{args:?}");
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with(message),
            "{args:?}"
        );
        assert!(output.stdout.is_empty());
    }
    project.write("src/one.rs", "pub fn one() {}\n");
    let output = run(
        &project,
        &["--edn", "--text", "--min-lines", "1", "--min-nodes", "0"],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"No duplicate candidates found.\n");
}

#[test]
fn dry_publishes_empty_candidates_when_no_form_clears_size() {
    let project = Project::new();
    project.write("src/lib.rs", "pub fn small() {}\n");
    let output = run(&project, &["--edn", "--min-nodes", "999"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(output.stdout, b"{:candidates []}\n");
    assert_eq!(
        fs::read(project.root.join(".metrics/dry.edn")).unwrap(),
        output.stdout
    );
}
