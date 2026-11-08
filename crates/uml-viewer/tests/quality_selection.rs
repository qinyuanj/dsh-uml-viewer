//! The same scope is exercised at each tool's CLI and read from its public
//! report, for every language. Two duplicate functions per fixture file make
//! every selected file observable even when the selection has only one file.
//! A future mutate adapter can join TOOLS by reading its published forms'
//! :file fields.
//!
//! dryer's one intentional discovery difference is `.cljd` (its
//! `discover.py`); the fixtures use `.clj`, so both tools must agree exactly.
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use crate::crap::Project;

struct Tool {
    role: &'static str,
    flags: &'static [&'static str],
    snapshot: &'static str,
    files: fn(&str, &str) -> BTreeSet<String>,
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
        files: |text, _| quoted_fields(text, "file"),
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
            format!(
                "def {id}_left(xs):\n    return filter(xs, odd)\n\ndef {id}_right(xs):\n    return filter(xs, odd)\n"
            )
        },
    },
    Fixture {
        language: "typescript",
        extension: "ts",
        source: |id| {
            format!(
                "export function {id}_left(xs: number[]): number[] {{\n  return filter(xs, odd);\n}}\n\nexport function {id}_right(xs: number[]): number[] {{\n  return filter(xs, odd);\n}}\n"
            )
        },
    },
    Fixture {
        language: "lua",
        extension: "lua",
        source: |id| {
            format!(
                "local function {id}_left(xs)\n  return filter(xs, odd)\nend\n\nlocal function {id}_right(xs)\n  return filter(xs, odd)\nend\n"
            )
        },
    },
    Fixture {
        language: "go",
        extension: "go",
        source: |id| {
            format!(
                "package demo\n\nfunc {id}_left(xs []int) []int {{\n\treturn filter(xs, odd)\n}}\n\nfunc {id}_right(xs []int) []int {{\n\treturn filter(xs, odd)\n}}\n"
            )
        },
    },
    Fixture {
        language: "java",
        extension: "java",
        source: |id| {
            format!(
                "class Fixture {{\n  int[] {id}_left(int[] xs) {{\n    return filter(xs, odd);\n  }}\n\n  int[] {id}_right(int[] xs) {{\n    return filter(xs, odd);\n  }}\n}}\n"
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
                let snapshot =
                    fs::read_to_string(project.root.join(tool.snapshot)).unwrap_or_default();
                let selected = (tool.files)(&snapshot, extension);
                let expected: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
                assert_eq!(
                    selected, expected,
                    "{} {} {args:?}",
                    tool.role, fixture.language
                );
                if expected.is_empty() {
                    assert_eq!(output.stdout, b"No source files to analyze.\n");
                    assert!(!project.root.join(tool.snapshot).exists());
                }
            }
        }
    }
}
