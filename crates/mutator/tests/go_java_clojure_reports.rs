//! Selection through the public CLI from reports produced by the original's
//! coverage formats. These tests need no language toolchain: the test command
//! accepts pristine code and rejects every mutant. Expected covered lines are
//! from f772a83 tests/test_coverage.py, with Cloverage precedence edge cases.
#![cfg(any(target_os = "linux", windows))]
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mutator-native-reports-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn run(&self, file: &str) -> ExitCode {
        let command = if cfg!(windows) {
            "if exist \"%MUTATOR_VIEW%\\mutation\" (exit /b 1) else (exit /b 0)"
        } else {
            "test ! -f \"$MUTATOR_VIEW/mutation\""
        };
        mutator::run_with_clock(
            &[
                OsString::from("--root"),
                self.0.clone().into_os_string(),
                "--mutate-all".into(),
                "--use-existing-coverage".into(),
                "--max-workers".into(),
                "1".into(),
                "--test-command".into(),
                command.into(),
                file.into(),
            ],
            || "2026-11-07T12:00:00+00:00".into(),
        )
    }
    fn snapshot(&self, namespace: &str) -> String {
        fs::read_to_string(
            self.0
                .join(".metrics/mutate")
                .join(format!("{namespace}.edn")),
        )
        .unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn tally(snapshot: &str, name: &str, killed: usize, uncovered: usize) {
    let form = snapshot
        .split(&format!(":id \"{name}\""))
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    assert!(
        form.contains(&format!(
            ":killed {killed} :survived 0 :uncovered {uncovered} :sites {}",
            killed + uncovered
        )),
        "{form}"
    );
}
#[test]
fn jacoco_selects_covered_instructions_or_branches() {
    let p = Project::new();
    p.write("src/demo/Board.java", "package demo;\nclass Board {\nboolean a() { return true; }\nboolean b() { return false; }\nboolean c() { return true; }\n}\n");
    p.write("target/site/jacoco/jacoco.xml", "<?xml version=\"1.0\"?><report><package name=\"demo\"><sourcefile name=\"Board.java\"><line nr=\"3\" mi=\"0\" ci=\"2\" mb=\"0\" cb=\"0\"/><line nr=\"4\" mi=\"4\" ci=\"0\" mb=\"1\" cb=\"0\"/><line nr=\"5\" mi=\"0\" ci=\"0\" mb=\"0\" cb=\"1\"/></sourcefile></package></report>");
    assert_eq!(p.run("src/demo/Board.java"), ExitCode::SUCCESS);
    let snapshot = p.snapshot("demo.Board");
    tally(&snapshot, "defn/a", 1, 0);
    tally(&snapshot, "defn/b", 0, 1);
    tally(&snapshot, "defn/c", 1, 0);
}
#[test]
fn go_profile_selects_every_line_in_a_hit_segment() {
    let p = Project::new();
    p.write(
        "demo/widget.go",
        "package demo\nfunc Run() bool {\n return true\n}\nfunc Stop() bool {\n return false\n}\n",
    );
    p.write(
        "target/coverage/go/coverage.out",
        "mode: set\ndemo/widget.go:2.1,4.2 1 1\ndemo/widget.go:5.1,6.2 1 0\n",
    );
    assert_eq!(p.run("demo/widget.go"), ExitCode::SUCCESS);
    let snapshot = p.snapshot("demo");
    tally(&snapshot, "defn/Run", 1, 0);
    tally(&snapshot, "defn/Stop", 0, 1);
}
#[test]
fn clojure_prefers_html_forms_to_lcov_and_falls_back_when_missing() {
    let p = Project::new();
    p.write(
        "src/demo/core.clj",
        "(ns demo.core)\n(defn a [] true)\n(defn b [] false)\n",
    );
    p.write(
        "target/coverage/lcov.info",
        "SF:src/demo/core.clj\nDA:2,1\nDA:3,0\nend_of_record\n",
    );
    p.write("target/coverage/demo/core.clj.html", "<span title=\"0 out of 1 forms covered\">2&nbsp;</span><span title=\"1 out of 2 forms covered\">3&nbsp;</span>");
    assert_eq!(p.run("src/demo/core.clj"), ExitCode::SUCCESS);
    let snapshot = p.snapshot("demo.core");
    tally(&snapshot, "defn/a", 0, 1);
    tally(&snapshot, "defn/b", 1, 0);
    fs::remove_file(p.0.join("target/coverage/demo/core.clj.html")).unwrap();
    assert_eq!(p.run("src/demo/core.clj"), ExitCode::SUCCESS);
    let snapshot = p.snapshot("demo.core");
    tally(&snapshot, "defn/a", 1, 0);
    tally(&snapshot, "defn/b", 0, 1);
}
#[test]
fn missing_reports_and_lcov_do_not_cover_go_or_java() {
    for (file, source, namespace, id) in [
        (
            "a.go",
            "package demo\nfunc Run() bool { return true }\n",
            "demo",
            "defn/Run",
        ),
        (
            "src/demo/Board.java",
            "package demo;\nclass Board { boolean run() { return true; } }\n",
            "demo.Board",
            "defn/run",
        ),
    ] {
        let p = Project::new();
        p.write(file, source);
        p.write(
            "coverage/lcov.info",
            &format!("SF:{file}\nDA:2,1\nend_of_record\n"),
        );
        assert_eq!(p.run(file), ExitCode::SUCCESS);
        tally(&p.snapshot(namespace), id, 0, 1);
    }
}
#[test]
fn malformed_jacoco_is_skipped_but_malformed_go_keeps_the_previous_snapshot() {
    let p = Project::new();
    p.write(
        "src/demo/Board.java",
        "package demo;\nclass Board { boolean run() { return true; } }\n",
    );
    p.write("target/site/jacoco/jacoco.xml", "broken <");
    assert_eq!(p.run("src/demo/Board.java"), ExitCode::SUCCESS);
    tally(&p.snapshot("demo.Board"), "defn/run", 0, 1);
    let p = Project::new();
    p.write("a.go", "package demo\nfunc Run() bool { return true }\n");
    p.write(".metrics/mutate/demo.edn", "previous snapshot\n");
    p.write("target/coverage/go/coverage.out", "mode: set\nbroken\n");
    assert_eq!(p.run("a.go"), ExitCode::from(2));
    assert_eq!(p.snapshot("demo"), "previous snapshot\n");
}
