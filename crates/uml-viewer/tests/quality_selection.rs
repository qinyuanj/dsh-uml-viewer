//! The same scope is exercised at each tool's CLI and read from its public
//! report. Two duplicate functions per fixture file make every selected file
//! observable even when the selection has only one file. Mutate joins through
//! its published forms' :file fields, one snapshot per selected file; it is
//! Linux-only, so the other platforms keep the selection contract for the tools
//! that run there.
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use crate::crap::Project;

struct Tool {
    role: &'static str,
    flags: &'static [&'static str],
    /// Mutate's namespace isolation is Linux-only today.
    linux_only: bool,
    /// The selected files as the tool's own published report names them.
    report: fn(&Path) -> BTreeSet<String>,
    /// An empty selection: the tool's own message on its own stream.
    empty: fn(&Output),
    /// Whether any report exists at all.
    published: fn(&Path) -> bool,
    /// Extra per-tool proof that the run did real work.
    verify: fn(&Path),
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

fn read_named(
    root: &Path,
    relative: &str,
    files: impl Fn(&str) -> BTreeSet<String>,
) -> BTreeSet<String> {
    files(&fs::read_to_string(root.join(relative)).unwrap_or_default())
}

fn crap_report(root: &Path) -> BTreeSet<String> {
    read_named(root, ".metrics/crap.edn", crap_files)
}

fn dry_report(root: &Path) -> BTreeSet<String> {
    read_named(root, ".metrics/dry.edn", |text| quoted_fields(text, "file"))
}

/// Every `:file` a mutation snapshot published, across all its snapshots.
fn mutate_report(root: &Path) -> BTreeSet<String> {
    fn walk(directory: &Path, files: &mut BTreeSet<String>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "edn") {
                let text = fs::read_to_string(&path).unwrap_or_default();
                files.extend(quoted_fields(&text, "file"));
            }
        }
    }
    let mut files = BTreeSet::new();
    walk(&root.join(".metrics").join("mutate"), &mut files);
    files
}

fn empty_quality(output: &Output) {
    assert_eq!(output.stdout, b"No source files to analyze.\n");
}

fn empty_mutate(output: &Output) {
    // The documented deviation: mutate's empty selection goes to stderr.
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("No source files to mutate."),
        "{output:?}"
    );
}

fn mutate_snapshots(root: &Path) -> bool {
    root.join(".metrics").join("mutate").exists()
}

fn mutate_verify(root: &Path) {
    // Not a hollow selection: each snapshot's sites were executed and killed.
    fn check(directory: &Path) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                check(&path);
            } else if path.extension().is_some_and(|ext| ext == "edn") {
                let text = fs::read_to_string(&path).unwrap_or_default();
                assert!(text.contains(":killed 1"), "{}: {text}", path.display());
                assert!(text.contains(":survived 0"), "{}: {text}", path.display());
            }
        }
    }
    check(&root.join(".metrics").join("mutate"));
}

fn noop_verify(_root: &Path) {}

const TOOLS: &[Tool] = &[
    Tool {
        role: "crap",
        flags: &["--no-coverage"],
        linux_only: false,
        report: crap_report,
        empty: empty_quality,
        published: |root| root.join(".metrics/crap.edn").exists(),
        verify: noop_verify,
    },
    Tool {
        role: "dry",
        flags: &["--edn", "--min-lines", "1", "--min-nodes", "1"],
        linux_only: false,
        report: dry_report,
        empty: empty_quality,
        published: |root| root.join(".metrics/dry.edn").exists(),
        verify: noop_verify,
    },
    Tool {
        role: "mutate",
        // The test command passes pristine code and fails every mutant, so a
        // complete run of the selected files exits 0 with killed sites.
        flags: &[
            "--no-coverage",
            "--mutate-all",
            "--max-workers",
            "2",
            "--test-command",
            "if [ -f \"$MUTATOR_VIEW/mutation\" ]; then exit 1; fi; true",
        ],
        linux_only: true,
        report: mutate_report,
        empty: empty_mutate,
        published: mutate_snapshots,
        verify: mutate_verify,
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
            if tool.linux_only && !cfg!(target_os = "linux") {
                continue;
            }
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
            let selected = (tool.report)(&project.root);
            let expected: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
            assert_eq!(selected, expected, "{} {args:?}", tool.role);
            (tool.verify)(&project.root);
            if expected.is_empty() {
                (tool.empty)(&output);
                assert!(
                    !(tool.published)(&project.root),
                    "{} {args:?}: an empty selection publishes nothing",
                    tool.role
                );
            }
        }
    }
}
