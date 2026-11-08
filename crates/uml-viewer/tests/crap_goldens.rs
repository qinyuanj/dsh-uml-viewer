//! `uml-viewer crap` reproduces the Python crapper byte for byte (ADR 0012).
//! Each `fixtures/crap/goldens/<scenario>/` names a sample project, setup
//! steps and argv; Python crapper at the commit in `goldens/SOURCE` published
//! the committed `crap.edn`, `stdout`, `stderr` and `exit` there
//! (`scripts/crap_goldens.py`). This test only reads those files.

use std::fs;
use std::path::{Path, PathBuf};

use super::crap::{Project, crap_in};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "target" || name == "Cargo.lock" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(&name));
        } else {
            fs::copy(&path, to.join(&name)).unwrap();
        }
    }
}

fn copy_fresh_lcov() -> &'static str {
    if cfg!(windows) {
        r"mkdir coverage 2>NUL & copy /Y fixture.lcov coverage\lcov.info >NUL"
    } else {
        "mkdir -p coverage && cp fixture.lcov coverage/lcov.info"
    }
}

/// Set the scenario's project up in `project` and return its argv.
fn prepare(scenario: &Path, project: &Project) -> Vec<String> {
    let crap = repo().join("fixtures/crap");
    let mut args = Vec::new();
    for line in fs::read_to_string(scenario).unwrap().lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (word, rest) = line.split_once(' ').unwrap();
        match word {
            "project" => {
                let source = match rest {
                    "rust" | "go" | "java" | "clojure" => crap.join(rest),
                    "rust-fixture" => repo().join("rust-fixture"),
                    other => panic!("unknown project {other}"),
                };
                copy_tree(&source, &project.root);
            }
            "file" => {
                let (source, destination) = rest.split_once(' ').unwrap();
                let target = project.root.join(destination);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::copy(crap.join(source), target).unwrap();
            }
            "crlf" => {
                let path = project.root.join(rest);
                let text = fs::read_to_string(&path).unwrap();
                fs::write(&path, text.replace('\n', "\r\n")).unwrap();
            }
            "arg" => args.push(rest.replace("{COPY_FRESH_LCOV}", copy_fresh_lcov())),
            other => panic!("{}: unknown line {other}", scenario.display()),
        }
    }
    args
}

/// The generator's normalization: root → `<ROOT>`, `\` → `/`, no `+ (` echoes.
fn normalize(stderr: &str, root: &Path) -> String {
    let mut text = stderr.to_string();
    for form in [
        root.display().to_string(),
        root.display().to_string().replace('\\', "/"),
    ] {
        text = text.replace(&form, "<ROOT>");
    }
    text.replace('\\', "/")
        .split('\n')
        .filter(|line| !line.starts_with("+ ("))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_scenario_matches_the_python_crapper_byte_for_byte() {
    let goldens = repo().join("fixtures/crap/goldens");
    let mut scenarios: Vec<PathBuf> = fs::read_dir(&goldens)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("scenario").is_file())
        .collect();
    scenarios.sort();
    // Exact: 22 base scenarios and 64 behavior groups (65 pinned cases less
    // the helper-only string-blanking case). A dropped scenario must fail.
    assert_eq!(scenarios.len(), 86, "scenarios: {scenarios:?}");
    // A behavior scenario must select its materialized source; an empty
    // selection means its `-s` root does not exist and nothing was tested.
    let hollow: Vec<String> = scenarios
        .iter()
        .filter(|dir| dir.file_name().unwrap().to_string_lossy().contains("-behavior-"))
        .filter(|dir| {
            fs::read_to_string(dir.join("stdout")).unwrap() == "No source files to analyze.\n"
        })
        .map(|dir| dir.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(hollow.is_empty(), "hollow behavior scenarios: {hollow:?}");
    let mut failures = Vec::new();
    for dir in &scenarios {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let project = Project::new();
        let args = prepare(&dir.join("scenario"), &project);
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = crap_in(&project.root, &project.root, &argv);
        let exit: i32 = fs::read_to_string(dir.join("exit"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        if output.status.code() != Some(exit) {
            failures.push(format!(
                "{name}: exit {:?}, expected {exit}",
                output.status.code()
            ));
        }
        let published = fs::read(project.root.join(".metrics/crap.edn")).ok();
        let golden = fs::read(dir.join("crap.edn")).ok();
        if published != golden {
            failures.push(format!(
                "{name}: crap.edn differs\n--- published\n{}\n--- golden\n{}",
                published
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
                golden
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
            ));
        }
        if output.stdout != fs::read(dir.join("stdout")).unwrap() {
            failures.push(format!(
                "{name}: stdout differs\n{}",
                String::from_utf8_lossy(&output.stdout)
            ));
        }
        let stderr = normalize(&String::from_utf8_lossy(&output.stderr), &project.root);
        let golden_stderr = fs::read_to_string(dir.join("stderr")).unwrap();
        if stderr != golden_stderr {
            failures.push(format!(
                "{name}: stderr differs\n--- actual\n{stderr}\n--- golden\n{golden_stderr}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
