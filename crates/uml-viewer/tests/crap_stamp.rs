//! Stamp/coverage gate ports from crapper 4068857, observed at the CLI seam.
use super::crap::{Project, stderr};
use std::fs;
use std::process::Output;

const STAMP: &str = "target/coverage/crapper-stamp.json";

#[test]
fn fresh_custom_report_does_not_assign_a_siblings_suffix_coverage() {
    let p = project();
    p.write("other/a.rs", "pub fn other() {}\n");
    p.write(
        "report-input",
        "SF:other/a.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    );
    let result = p.crap(&[
        "-s",
        "a.rs",
        "--coverage-command",
        &command(),
        "--threshold",
        "8",
    ]);
    assert_eq!(result.status.code(), Some(3), "{}", stderr(&result));
    assert!(p.snapshot().unwrap().contains(":coverage nil, :crap nil"));
}

// The test executable is a portable, Rust-only coverage fixture command.
// Tests invoke this exact test in a fresh process at the scratch project root.
#[test]
fn coverage_fixture_command() {
    if !std::path::Path::new("a.rs").is_file() {
        return;
    }
    let mode = fs::read_to_string("mode").unwrap_or_default();
    if mode == "fail" {
        std::process::exit(3);
    }
    fs::create_dir_all("coverage").unwrap();
    if mode == "edit" {
        fs::write("tests/test_a.rs", "changed during coverage\n").unwrap();
    }
    if mode == "caches" {
        for name in [
            ".pytest_cache/x",
            ".hypothesis/x",
            ".mypy_cache/x",
            ".ruff_cache/x",
            ".tox/x",
            ".nox/x",
            ".cpcache/x",
            ".nyc_output/x",
            "htmlcov/x",
            "sub/.coverage.host.1",
        ] {
            fs::create_dir_all(std::path::Path::new(name).parent().unwrap()).unwrap();
            fs::write(name, "cache\n").unwrap();
        }
    }
    let report = match mode.as_str() {
        "missing" => return,
        "empty" => "",
        "invalid" => "SF:a.rs\nDA:1,1\nDA:1,0\nLF:2\nLH:1\nend_of_record\n",
        "unjoined" => "SF:coverage/x\nLF:0\nLH:0\nend_of_record\n",
        _ => "SF:a.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    };
    fs::write("coverage/x", "").unwrap();
    let report = fs::read_to_string("report-input").unwrap_or_else(|_| report.to_string());
    fs::write("coverage/lcov.info", report).unwrap();
    if mode == "count" {
        fs::write("a.rs", "pub fn a() {}\npub fn b() {}\n").unwrap();
    }
}

#[test]
fn a_new_file_during_coverage_is_not_stamped() {
    let p = project();
    p.write("mode", "count");
    let result = run(&p);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert!(stderr(&result).contains("Files changed while coverage ran"));
    assert!(!p.root.join(STAMP).exists());
}

fn command() -> String {
    let exe = std::env::current_exe().unwrap();
    let path = super::crap::canonical(&exe).to_str().unwrap().to_owned();
    let quoted = if cfg!(windows) {
        format!("\"{path}\"")
    } else {
        format!("'{}'", path.replace('\'', "'\"'\"'"))
    };
    format!("{quoted} --exact crap_stamp::coverage_fixture_command --nocapture")
}

fn project() -> Project {
    let p = Project::new();
    p.write("a.rs", "pub fn a() {}\n");
    p
}

fn run(p: &Project) -> Output {
    p.crap(&["--coverage-command", &command()])
}

#[test]
fn successful_custom_coverage_publishes_a_sorted_version_one_stamp() {
    let p = project();
    let result = run(&p);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    let text = fs::read_to_string(p.root.join(STAMP))
        .expect("successful coverage stamps the selected files");
    assert!(text.contains("\"version\": 1"), "{text}");
    assert!(
        text.contains("\"rust\": [\n      \"a.rs\"\n    ]"),
        "{text}"
    );
    assert!(text.find("\"command\"").unwrap() < text.find("\"files\"").unwrap());
    assert!(text.find("\"files\"").unwrap() < text.find("\"languages\"").unwrap());
    assert!(text.ends_with('\n') && !text.contains('\r'));
    assert!(p.snapshot().unwrap().contains(":coverage 100.0, :crap 1.0"));
}

#[test]
fn failed_custom_coverage_drops_a_prior_stamp_and_keeps_snapshot() {
    let p = project();
    assert!(run(&p).status.success());
    let old = p.snapshot();
    p.write("mode", "fail");
    let result = run(&p);
    assert_eq!(result.status.code(), Some(1));
    assert!(stderr(&result).contains("Coverage command exited 3"));
    assert!(!p.root.join(STAMP).exists());
    assert_eq!(p.snapshot(), old);
}

#[test]
fn edits_during_coverage_drop_stamp_but_tool_caches_do_not() {
    let p = project();
    p.write("tests/test_a.rs", "original\n");
    p.write("mode", "edit");
    let result = run(&p);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert!(stderr(&result).contains("Files changed while coverage ran"));
    assert!(!p.root.join(STAMP).exists());
    p.write("mode", "caches");
    let result = run(&p);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert!(p.root.join(STAMP).is_file());
    assert!(!stderr(&result).contains("Files changed while coverage ran"));
}

#[test]
fn no_coverage_and_existing_coverage_leave_the_stamp_untouched() {
    let p = project();
    p.write(STAMP, "old\n");
    for flag in ["--no-coverage", "--use-existing-coverage"] {
        let result = p.crap(&[flag]);
        assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
        assert_eq!(fs::read_to_string(p.root.join(STAMP)).unwrap(), "old\n");
    }
}

#[test]
fn invalid_or_missing_custom_reports_fail_closed_even_with_stale_coverage() {
    for mode in ["missing", "empty", "invalid"] {
        let p = project();
        p.write("mode", mode);
        p.write(
            "coverage/lcov.info",
            "SF:a.rs\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        );
        p.write("coverage/luacov.stats.out", "stale");
        p.write("luacov.stats.out", "stale");
        p.write(".metrics/crap.edn", "old\n");
        let result = run(&p);
        assert_eq!(result.status.code(), Some(1), "{mode}: {}", stderr(&result));
        assert!(stderr(&result).contains("Required LCOV report invalid or unavailable"));
        assert_eq!(p.snapshot().as_deref(), Some("old\n"));
        assert!(!p.root.join(STAMP).exists());
        assert!(!p.root.join("coverage/luacov.stats.out").exists());
        assert!(!p.root.join("luacov.stats.out").exists());
    }
}

#[test]
fn valid_but_unjoined_coverage_publishes_and_returns_threshold_exit_three() {
    let p = project();
    p.write("mode", "unjoined");
    let result = p.crap(&["--coverage-command", &command(), "--threshold", "8"]);
    assert_eq!(result.status.code(), Some(3), "{}", stderr(&result));
    assert!(p.snapshot().unwrap().contains(":coverage nil, :crap nil"));
    assert!(stderr(&result).contains("No function's coverage was measured"));
    assert!(p.root.join(STAMP).is_file());
}
