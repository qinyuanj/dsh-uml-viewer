//! The same scope is exercised at each tool's CLI and read from its public
//! report, for every language. Two duplicate functions per fixture file make
//! every selected file observable even when the selection has only one file.
//! Mutate joins through its published forms' :file fields, one snapshot per
//! selected file; it is Linux-only, so the other platforms keep the selection
//! contract for the tools that run there.
//!
//! dryer's one intentional discovery difference is `.cljd` (its
//! `discover.py`); the fixtures use `.clj`, so both tools must agree exactly.
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
    /// The languages each tool handles when not all of them: an empty list means "all".
    languages: &'static [&'static str],
    /// The selected files as the tool's own published report names them.
    report: fn(&Path, &str) -> BTreeSet<String>,
    /// An empty selection: the tool's own message on its own stream.
    empty: fn(&Output),
    /// Whether any report exists at all.
    published: fn(&Path) -> bool,
    /// Extra per-tool proof that the run did real work.
    verify: fn(&Path),
}

struct Fixture {
    language: &'static str,
    extension: &'static str,
    /// Two functions named `<id>_left` and `<id>_right` with the same body,
    /// so crap lists both and dry reports the file's pair.
    source: fn(&str) -> String,
}

fn quoted_fields(text: &str, key: &str) -> BTreeSet<String> {
    text.split(&format!(":{key} \""))
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().to_string())
        .collect()
}

fn crap_files(snapshot: &str, extension: &str) -> BTreeSet<String> {
    quoted_fields(snapshot, "name")
        .into_iter()
        .map(|name| {
            let id = name
                .strip_suffix("_left")
                .or_else(|| name.strip_suffix("_right"))
                .unwrap();
            format!("{}.{}", id.replace("__", "/"), extension)
        })
        .collect()
}

fn read_named(
    root: &Path,
    relative: &str,
    extension: &str,
    files: impl Fn(&str, &str) -> BTreeSet<String>,
) -> BTreeSet<String> {
    files(
        &fs::read_to_string(root.join(relative)).unwrap_or_default(),
        extension,
    )
}

fn crap_report(root: &Path, extension: &str) -> BTreeSet<String> {
    read_named(root, ".metrics/crap.edn", extension, crap_files)
}

fn dry_report(root: &Path, _extension: &str) -> BTreeSet<String> {
    quoted_fields(
        &fs::read_to_string(root.join(".metrics/dry.edn")).unwrap_or_default(),
        "file",
    )
}

/// Every `:file` a mutation snapshot published, across all its snapshots.
fn mutate_report(root: &Path, _extension: &str) -> BTreeSet<String> {
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
    // Not a hollow selection: every form with sites ran them all, all were
    // killed by the fail-every-mutant command, and none survived.
    fn forms(text: &str) -> Vec<(usize, usize, usize, usize)> {
        let mut found = Vec::new();
        for entry in text.split(":id \"").skip(1) {
            let count = |key: &str| -> usize {
                entry
                    .split(&format!(":{key} "))
                    .nth(1)
                    .and_then(|rest| rest.split(|ch: char| !ch.is_ascii_digit()).next())
                    .and_then(|digits| digits.parse().ok())
                    .unwrap_or(0)
            };
            found.push((
                count("killed"),
                count("survived"),
                count("uncovered"),
                count("sites"),
            ));
        }
        found
    }
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
                let found = forms(&text);
                assert!(!found.is_empty(), "{}: {text}", path.display());
                let mut ran = 0;
                for (killed, survived, uncovered, sites) in found {
                    assert_eq!(survived, 0, "{}: {text}", path.display());
                    assert_eq!(killed + uncovered, sites, "{}: {text}", path.display());
                    ran += killed;
                }
                assert!(ran > 0, "{}: no site was killed: {text}", path.display());
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
        languages: &[],
        report: crap_report,
        empty: empty_quality,
        published: |root| root.join(".metrics/crap.edn").exists(),
        verify: noop_verify,
    },
    Tool {
        role: "dry",
        flags: &["--edn", "--min-lines", "1", "--min-nodes", "1"],
        linux_only: false,
        languages: &[],
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
        // Every language this build mutates: Rust (native), Lua (#11), Python,
        // TypeScript, Go, Java and Clojure (#14).
        languages: &[
            "rust",
            "python",
            "typescript",
            "lua",
            "go",
            "java",
            "clojure",
        ],
        report: mutate_report,
        empty: empty_mutate,
        published: mutate_snapshots,
        verify: mutate_verify,
    },
];

const FIXTURES: &[Fixture] = &[
    Fixture {
        language: "rust",
        extension: "rs",
        source: |id| {
            format!(
                "pub fn {id}_left(x: i32) -> i32 {{ x + 1 }}\npub fn {id}_right(y: i32) -> i32 {{ y + 2 }}\n"
            )
        },
    },
    Fixture {
        language: "python",
        extension: "py",
        source: |id| {
            format!("def {id}_left(x):\n    return x + 1\n\ndef {id}_right(x):\n    return x + 1\n")
        },
    },
    Fixture {
        language: "typescript",
        extension: "ts",
        source: |id| {
            format!(
                "export function {id}_left(x: number) {{\n  return x + 1;\n}}\n\nexport function {id}_right(x: number) {{\n  return x + 1;\n}}\n"
            )
        },
    },
    Fixture {
        language: "lua",
        extension: "lua",
        source: |id| {
            format!(
                "local function {id}_left(x)\n  return x + 1\nend\n\nlocal function {id}_right(x)\n  return x + 1\nend\n"
            )
        },
    },
    Fixture {
        language: "go",
        extension: "go",
        source: |id| {
            format!(
                "package demo\n\nfunc {id}_left(xs []int) int {{\n\treturn len(xs) + 1\n}}\n\nfunc {id}_right(xs []int) int {{\n\treturn len(xs) + 1\n}}\n"
            )
        },
    },
    Fixture {
        language: "java",
        extension: "java",
        source: |id| {
            format!(
                "class Fixture {{\n  int {id}_left(int[] xs) {{\n    return xs.length + 1;\n  }}\n\n  int {id}_right(int[] xs) {{\n    return xs.length + 1;\n  }}\n}}\n"
            )
        },
    },
    Fixture {
        language: "clojure",
        extension: "clj",
        source: |id| {
            format!(
                "(ns fixture)\n\n(defn {id}_left [xs]\n  (filter odd? xs))\n\n(defn {id}_right [xs]\n  (filter odd? xs))\n"
            )
        },
    },
];

#[test]
fn quality_commands_publish_the_same_selected_source_files() {
    for fixture in FIXTURES {
        let extension = fixture.extension;
        let files = [
            format!("src/a.{extension}"),
            format!("src/nested/b.{extension}"),
            format!("extra/c.{extension}"),
            format!("src/drop/d.{extension}"),
            format!("tests/e.{extension}"),
            format!("target/f.{extension}"),
            format!("build/g.{extension}"),
            format!("testdata/h.{extension}"),
            format!("vendor/i.{extension}"),
        ];
        let a = format!("src/a.{extension}");
        let b = format!("src/nested/b.{extension}");
        let c = format!("extra/c.{extension}");
        let d = format!("src/drop/d.{extension}");
        let f = format!("target/f.{extension}");
        let scenarios: &[(&[String], &[&String])] = &[
            (&[], &[&a, &b, &c, &d]),
            (&["-s".into(), "src".into()], &[&a, &b, &d]),
            (
                &[
                    "--source-root".into(),
                    "src".into(),
                    "-s".into(),
                    "extra".into(),
                    "-s".into(),
                    "src/nested".into(),
                ],
                &[&a, &b, &c, &d],
            ),
            (
                &[
                    "-s".into(),
                    "src".into(),
                    "--exclude".into(),
                    "src/drop".into(),
                    "--exclude".into(),
                    format!("**/b.?{}", &extension[1..]),
                ],
                &[&a],
            ),
            (&["--exclude".into(), "**".into()], &[]),
            (&["-s".into(), "missing".into()], &[]),
            (&["-s".into(), "src".into(), c.clone(), a.clone()], &[&a]),
            (
                &[
                    "-s".into(),
                    "src".into(),
                    ".".into(),
                    "--exclude".into(),
                    "src/drop".into(),
                ],
                &[&a, &b],
            ),
            (&[a.clone(), a.clone(), f.clone()], &[&a, &f]),
            (&["-s".into(), a.clone()], &[&a]),
            (&["-s".into(), "src".into(), "nested/b".into()], &[&b]),
            (&[a.clone(), "--exclude".into(), a.clone()], &[]),
        ];
        for (args, expected) in scenarios {
            for tool in TOOLS {
                if (tool.linux_only && !cfg!(target_os = "linux"))
                    || (!tool.languages.is_empty() && !tool.languages.contains(&fixture.language))
                {
                    continue;
                }
                let project = Project::new();
                project.write(
                    "Cargo.toml",
                    "[package]\nname = \"scope-fixture\"\nversion = \"0.1.0\"\n",
                );
                for file in &files {
                    let id = file
                        .trim_end_matches(&format!(".{extension}"))
                        .replace('/', "__");
                    project.write(file, &(fixture.source)(&id));
                }
                let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
                    .arg(tool.role)
                    .arg("--root")
                    .arg(&project.root)
                    .args(tool.flags)
                    .args(args.iter())
                    .current_dir(&project.root)
                    .env_remove("DISPLAY")
                    .env_remove("WAYLAND_DISPLAY")
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(0),
                    "{} {} {args:?}: {output:?}",
                    tool.role,
                    fixture.language
                );
                let selected = (tool.report)(&project.root, extension);
                let expected: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
                assert_eq!(
                    selected, expected,
                    "{} {} {args:?}",
                    tool.role, fixture.language
                );
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
}
