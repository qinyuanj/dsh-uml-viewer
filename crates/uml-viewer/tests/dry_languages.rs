//! The process-level cases of `dryer/tests/test_cli.py` (pinned b3f72ad),
//! ported to the real binary, plus the original Lua fixture's own golden.

use crate::crap::Project;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Copy `fixtures/dry/<name>` with every line ending rewritten, like the
/// original `copy_fixture`.
fn copy_fixture(name: &str, target: &Path, newline: &str) {
    fn walk(source: &Path, target: &Path, newline: &str) {
        for entry in fs::read_dir(source).unwrap() {
            let path = entry.unwrap().path();
            let copied = target.join(path.file_name().unwrap());
            if path.is_dir() {
                walk(&path, &copied, newline);
            } else {
                fs::create_dir_all(copied.parent().unwrap()).unwrap();
                let text = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
                fs::write(&copied, text.replace('\n', newline)).unwrap();
            }
        }
    }
    walk(&repo().join("fixtures/dry").join(name), target, newline);
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("dry")
        .arg("--root")
        .arg(root)
        .args(args)
        .current_dir(root)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}

fn text(output: &[u8]) -> String {
    String::from_utf8(output.to_vec()).unwrap()
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

const ALPHA: &str =
    "def alpha(xs):\n    ys = filter(xs, odd)\n    zs = map(ys, inc)\n    return list(zs)\n";
const BETA: &str = "def beta(items):\n    kept = filter(items, even)\n    out = map(kept, dec)\n    return list(out)\n";
const SMALL: &[&str] = &["--min-lines", "3", "--min-nodes", "1"];

#[test]
fn project_report_and_snapshot() {
    let project = Project::new();
    project.write("src/left.py", ALPHA);
    project.write("src/right.py", BETA);
    project.write("tests/test_left.py", ALPHA);
    let output = run(&project.root, SMALL);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let out = text(&output.stdout);
    assert!(out.contains("DUPLICATE score=1.00"), "{out}");
    assert!(out.contains("src/left.py:1-"), "{out}");
    assert!(out.contains("src/right.py:1-"), "{out}");
    assert!(!out.contains("test_left.py"), "{out}");
    let snapshot = fs::read_to_string(project.root.join(".metrics/dry.edn")).unwrap();
    assert!(snapshot.contains(":language \"python\""), "{snapshot}");
    assert!(snapshot.contains(":file \"src/left.py\""), "{snapshot}");
    assert!(text(&output.stderr).contains("Wrote"));
}

#[test]
fn edn_stdout() {
    let project = Project::new();
    project.write("one.py", "def alpha(xs):\n    return xs\n");
    let output = run(
        &project.root,
        &["--edn", "--min-lines", "1", "--min-nodes", "1"],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"{:candidates []}\n");
}

#[test]
fn an_external_source_does_not_create_a_missing_project_root() {
    let project = Project::new();
    project.write("src/one.py", "def alpha(xs):\n    return xs\n");
    let root = project.root.join("fresh").join("project");
    let source = project.root.join("src/one.py");
    // The root must not exist yet, so the process starts from its parent.
    let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .args(["dry", "--root"])
        .arg(&root)
        .arg(&source)
        .current_dir(&project.root)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(text(&output.stderr).contains("outside the project root"));
    assert!(!root.exists());
}

#[test]
fn outside_project_selection_is_rejected_without_replacing_snapshot() {
    for selection in ["file", "directory", "source-root"] {
        let base = Project::new();
        let project = base.root.join("project");
        fs::create_dir_all(&project).unwrap();
        let external = base.root.join("external");
        fs::create_dir_all(&external).unwrap();
        let source = external.join("outside.py");
        fs::write(&source, "def value():\n    return 1\n").unwrap();
        let snapshot = project.join(".metrics").join("dry.edn");
        fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
        fs::write(&snapshot, b"previous snapshot\n").unwrap();
        let output = match selection {
            "source-root" => run(&project, &["-s", external.to_str().unwrap()]),
            "directory" => run(&project, &[external.to_str().unwrap()]),
            _ => run(&project, &[source.to_str().unwrap()]),
        };
        assert_eq!(output.status.code(), Some(1), "{selection}: {output:?}");
        assert!(
            text(&output.stderr).contains("outside the project root"),
            "{selection}"
        );
        assert_eq!(output.stdout, b"", "{selection}");
        assert_eq!(
            fs::read(&snapshot).unwrap(),
            b"previous snapshot\n",
            "{selection}"
        );
    }
}

#[test]
fn path_filter() {
    let project = Project::new();
    project.write("src/board/a.py", ALPHA);
    project.write("src/board/b.py", BETA);
    project.write("src/other/a.py", ALPHA);
    project.write("src/other/b.py", BETA);
    let output = run(
        &project.root,
        &["--min-lines", "3", "--min-nodes", "1", "board"],
    );
    assert_eq!(output.status.code(), Some(0));
    let out = text(&output.stdout);
    assert!(out.contains("src/board/a.py"), "{out}");
    assert!(!out.contains("src/other/a.py"), "{out}");
}

/// The original Lua fixture's own report lines and names.
#[test]
fn lua_fixture_reports_the_copied_function() {
    let project = Project::new();
    copy_fixture("lua_project", &project.root, "\n");
    let output = run(&project.root, &[]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let snapshot = fs::read_to_string(project.root.join(".metrics/dry.edn")).unwrap();
    assert!(snapshot.contains(":language \"lua\""), "{snapshot}");
    assert!(
        snapshot.contains(
            ":left {:file \"src/calc/init.lua\", :start-line 14, :end-line 20, :namespace \"calc\", :name \"M.total\"}"
        ),
        "{snapshot}"
    );
    assert!(
        snapshot.contains(
            ":right {:file \"src/calc/stats.lua\", :start-line 5, :end-line 11, :namespace \"calc.stats\", :name \"M.sum_clamped\"}"
        ),
        "{snapshot}"
    );
}

/// `test_lua_fixture_snapshot_is_the_golden_bytes`: LF and CRLF sources
/// publish the same bytes, and those bytes are the original repository's
/// committed golden.
#[test]
fn lua_fixture_snapshot_is_the_golden_bytes() {
    let golden = fs::read(repo().join("fixtures/dry/lua_project-original.edn")).unwrap();
    for newline in ["\n", "\r\n"] {
        let project = Project::new();
        copy_fixture("lua_project", &project.root, newline);
        let output = run(&project.root, &[]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(
            fs::read(project.root.join(".metrics/dry.edn")).unwrap(),
            golden,
            "newline {newline:?}"
        );
    }
}

/// `test_edn_on_stdout_is_the_golden_bytes`: the EDN report on stdout is the
/// same bytes, from a CRLF project.
#[test]
fn edn_on_stdout_is_the_golden_bytes() {
    let project = Project::new();
    copy_fixture("lua_project", &project.root, "\r\n");
    let output = run(&project.root, &["--edn"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        output.stdout,
        fs::read(repo().join("fixtures/dry/lua_project-original.edn")).unwrap()
    );
}

#[test]
fn crlf_sources_report_the_same_lines_and_names_as_lf() {
    const CLOJURE_LEFT: &str = "(ns a.core)\n\n(defn alpha [xs]\n  (let [ys (filter odd? xs)]\n    (map inc ys)\n    (reduce + ys)))\n";
    const CLOJURE_RIGHT: &str = "(ns b.core)\n\n(defn beta [items]\n  (let [kept (filter even? items)]\n    (map dec kept)\n    (reduce * kept)))\n";
    let mut snapshots = Vec::new();
    for newline in ["\n", "\r\n"] {
        let project = Project::new();
        for (name, source) in [
            ("src/a/core.clj", CLOJURE_LEFT),
            ("src/b/core.clj", CLOJURE_RIGHT),
            ("src/left.py", &format!("\n\n{ALPHA}")),
            ("src/right.py", BETA),
        ] {
            project.write(name, &source.replace('\n', newline));
        }
        let output = run(&project.root, SMALL);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        snapshots.push(fs::read(project.root.join(".metrics/dry.edn")).unwrap());
    }
    let (lf, crlf) = (&snapshots[0], &snapshots[1]);
    assert_eq!(crlf, lf);
    let snapshot = text(lf);
    assert!(
        snapshot.contains(":left {:file \"src/a/core.clj\", :start-line 3, :end-line 6, :namespace \"a.core\", :name \"alpha\"}"),
        "{snapshot}"
    );
    assert!(
        snapshot.contains(":file \"src/left.py\", :start-line 3, :end-line 6"),
        "{snapshot}"
    );
    assert!(!lf.contains(&b'\r'));
}

/// `test_changed_finds_new_directories_under_a_subdirectory_root`: the CLI
/// half; the exact selected file list (including the `.cljd`) is asserted in
/// `crates/dryer/src/original/cli.rs`.
#[test]
fn changed_finds_new_directories_under_a_subdirectory_root() {
    let project = Project::new();
    git(&project.root, &["init", "-q"]);
    git(&project.root, &["config", "user.email", "<REDACTED_EMAIL>"]);
    git(&project.root, &["config", "user.name", "dryer"]);
    project.write("sub/old.py", "x = 1\n");
    project.write("sub/gone.py", "x = 1\n");
    git(&project.root, &["add", "."]);
    git(&project.root, &["commit", "-qm", "base"]);
    fs::remove_file(project.root.join("sub/gone.py")).unwrap();
    project.write("sub/fresh/a b.py", ALPHA);
    project.write("sub/fresh/b.cljd", "(ns b)\n");
    project.write("sub/fresh/c.py", BETA);
    project.write("sub/fresh/test_c.py", BETA);
    project.write("other/d.py", ALPHA);
    let sub = project.root.join("sub");
    let output = run(&sub, &["--changed", "--min-lines", "3", "--min-nodes", "1"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    // Exact, so a file that should not be selected (other/d.py, the test
    // file, the deleted one) cannot slip into the report unnoticed.
    assert_eq!(
        text(&output.stdout),
        "DUPLICATE score=1.00\n  fresh/a b.py:1-4\n  fresh/c.py:1-4\n",
        "{output:?}"
    );
}

#[test]
fn changed_reports_git_failure() {
    let project = Project::new();
    let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .args(["dry", "--root"])
        .arg(&project.root)
        .arg("--changed")
        .current_dir(&project.root)
        .env("LC_ALL", "C")
        // The scratch directory may sit inside a checkout; stop git there.
        .env("GIT_CEILING_DIRECTORIES", project.root.parent().unwrap())
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        text(&output.stderr).contains("not a git repository"),
        "{output:?}"
    );
    assert_eq!(output.stdout, b"No source files to analyze.\n");
}
