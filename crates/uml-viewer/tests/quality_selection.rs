//! The same scope is exercised at each tool's CLI and read from its public
//! report. Two duplicate functions per fixture file make every selected file
//! observable even when the selection has only one file. A future mutate
//! adapter can join TOOLS by reading its published forms' :file fields.
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use crate::crap::Project;

struct Tool {
    role: &'static str,
    flags: &'static [&'static str],
    snapshot: &'static str,
    files: fn(&str) -> BTreeSet<String>,
}

fn quoted_fields(text: &str, key: &str) -> BTreeSet<String> {
    text.split(&format!(":{key} \""))
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().to_string())
        .collect()
}

fn crap_files(snapshot: &str) -> BTreeSet<String> {
    quoted_fields(snapshot, "name")
        .into_iter()
        .map(|name| {
            let id = name
                .strip_suffix("_left")
                .or_else(|| name.strip_suffix("_right"))
                .unwrap();
            format!("{}.rs", id.replace("__", "/"))
        })
        .collect()
}

const TOOLS: &[Tool] = &[
    Tool {
        role: "crap",
        flags: &["--no-coverage"],
        snapshot: ".metrics/crap.edn",
        files: crap_files,
    },
    Tool {
        role: "dry",
        flags: &["--edn", "--min-lines", "1", "--min-nodes", "1"],
        snapshot: ".metrics/dry.edn",
        files: |text| quoted_fields(text, "file"),
    },
];

#[test]
fn quality_commands_publish_the_same_selected_source_files() {
    let files = [
        "src/a.rs",
        "src/nested/b.rs",
        "extra/c.rs",
        "src/drop/d.rs",
        "tests/e.rs",
        "target/f.rs",
        "build/g.rs",
        "testdata/h.rs",
        "vendor/i.rs",
    ];
    let scenarios: &[(&[&str], &[&str])] = &[
        (
            &[],
            &["src/a.rs", "src/nested/b.rs", "extra/c.rs", "src/drop/d.rs"],
        ),
        (
            &["-s", "src"],
            &["src/a.rs", "src/nested/b.rs", "src/drop/d.rs"],
        ),
        (
            &["--source-root", "src", "-s", "extra", "-s", "src/nested"],
            &["src/a.rs", "src/nested/b.rs", "extra/c.rs", "src/drop/d.rs"],
        ),
        (
            &["-s", "src", "--exclude", "src/drop", "--exclude", "**/b.?s"],
            &["src/a.rs"],
        ),
        (&["--exclude", "**"], &[]),
        (&["-s", "missing"], &[]),
        (&["-s", "src", "extra/c.rs", "src/a.rs"], &["src/a.rs"]),
        (
            &["-s", "src", ".", "--exclude", "src/drop"],
            &["src/a.rs", "src/nested/b.rs"],
        ),
        (
            &["src/a.rs", "src/a.rs", "target/f.rs"],
            &["src/a.rs", "target/f.rs"],
        ),
        (&["-s", "src/a.rs"], &["src/a.rs"]),
        (&["-s", "src", "nested/b"], &["src/nested/b.rs"]),
        (&["src/a.rs", "--exclude", "src/a.rs"], &[]),
    ];
    for (args, expected) in scenarios {
        for tool in TOOLS {
            let project = Project::new();
            project.write(
                "Cargo.toml",
                "[package]\nname = \"scope-fixture\"\nversion = \"0.1.0\"\n",
            );
            for file in files {
                let id = file.trim_end_matches(".rs").replace('/', "__");
                project.write(file, &format!("pub fn {id}_left(x: i32) -> i32 {{ x + 1 }}\npub fn {id}_right(y: i32) -> i32 {{ y + 2 }}\n"));
            }
            let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
                .arg(tool.role)
                .arg("--root")
                .arg(&project.root)
                .args(tool.flags)
                .args(*args)
                .current_dir(&project.root)
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY")
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(0),
                "{} {args:?}: {output:?}",
                tool.role
            );
            let snapshot = fs::read_to_string(project.root.join(tool.snapshot)).unwrap_or_default();
            let selected = (tool.files)(&snapshot);
            let expected: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
            assert_eq!(selected, expected, "{} {args:?}", tool.role);
            if expected.is_empty() {
                assert_eq!(output.stdout, b"No source files to analyze.\n");
                assert!(!project.root.join(tool.snapshot).exists());
            }
        }
    }
}
