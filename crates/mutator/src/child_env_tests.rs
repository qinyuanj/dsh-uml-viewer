//! Child-observed environment at the production View seam. Authored, not run
//! under the current no-Cargo resource policy.
#![cfg_attr(test, allow(clippy::disallowed_methods))]
use crate::{isolation::View, scratch::Root};
use std::{collections::BTreeMap, fs, process::Command, time::Duration};

const POLLUTED: &[&str] = &[
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_LLVM_COV",
    "CARGO_LLVM_COV_TARGET_DIR",
    "CARGO_LLVM_COV_FUTURE_SETTING",
    "LLVM_PROFILE_FILE",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_TARGET_DIR",
];

#[test]
fn isolated_commands_scrub_outer_coverage_environment() {
    // Pollute a separate driver, never the parallel test runner's environment.
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "child_env_tests::polluted_driver", "--nocapture"]);
    command.env("MUTATOR_ENV_TEST_DRIVER", "1");
    command.env("MUTATOR_ENV_KEEP", "keep-me");
    for name in POLLUTED {
        command.env(name, "outer-contamination");
    }
    #[cfg(windows)]
    command.env("cargo_llvm_cov_mixed_case", "outer-contamination");
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "polluted driver failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // A filter that selects nothing would make this test pass hollow.
    assert!(
        stdout.contains("1 passed"),
        "the polluted driver did not run:\n{stdout}"
    );
}

#[test]
fn polluted_driver() {
    if std::env::var_os("MUTATOR_ENV_TEST_DRIVER").is_none() {
        eprintln!("skipped: polluted_driver runs only inside its polluted driver process");
        return;
    }
    let fixture = Root::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let project = fixture.path().join("project");
    fs::create_dir(&project).unwrap();
    let project = fs::canonicalize(project).unwrap();
    let source = project.join("source.txt");
    fs::write(&source, "baseline").unwrap();
    // Worker slots sit below their owner's root; the adapter must not reset
    // the root's locked owner file while preparing a fresh command.
    let slot = fixture.path().join("slot");
    fs::create_dir(&slot).unwrap();
    let view = View::reused(&slot);
    #[cfg(windows)]
    let argv = vec!["cmd.exe".into(), "/d".into(), "/c".into(), "set".into()];
    #[cfg(target_os = "linux")]
    let argv = vec!["/usr/bin/env".into()];
    // The same adapter runs baseline and mutant commands.
    for mutation in [None, Some((source.as_path(), "mutant"))] {
        let result = view
            .run_argv(
                &project,
                &project,
                &argv,
                mutation,
                Some(Duration::from_secs(15)),
            )
            .unwrap_or_else(|error| panic!("production isolation unavailable or failed: {error}"));
        assert_eq!(result.code, 0, "{}", result.output);
        assert_environment(&view, &project, &result.output, false);
    }
    // An inner coverage command can install its own flags/profile after the
    // inherited outer coverage settings have been removed.
    #[cfg(windows)]
    let coverage = "(echo own-report)>coverage.txt&&set RUSTFLAGS=own-flags&&set LLVM_PROFILE_FILE=own-%p.profraw&&set";
    #[cfg(target_os = "linux")]
    let coverage = "printf 'own-report\\n' > coverage.txt; RUSTFLAGS=own-flags LLVM_PROFILE_FILE=own-%p.profraw env";
    let result = view
        .run(
            &project,
            &project,
            coverage,
            None,
            Some(Duration::from_secs(15)),
        )
        .unwrap_or_else(|error| panic!("production isolation unavailable or failed: {error}"));
    assert_eq!(result.code, 0, "{}", result.output);
    assert_environment(&view, &project, &result.output, true);
    let report = crate::isolation::report_path(&view, std::path::Path::new("coverage.txt"));
    assert_eq!(fs::read_to_string(report).unwrap().trim(), "own-report");
    assert!(!project.join("coverage.txt").exists());
    // No child setting or mutation leaks back to the caller.
    assert_eq!(fs::read_to_string(source).unwrap(), "baseline");
    assert_eq!(
        std::env::var("RUSTC_WRAPPER").unwrap(),
        "outer-contamination"
    );
    assert_eq!(
        std::env::var("CARGO_TARGET_DIR").unwrap(),
        "outer-contamination"
    );
}

fn assert_environment(view: &View, project: &std::path::Path, output: &str, own_coverage: bool) {
    let vars: BTreeMap<String, String> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_ascii_uppercase(), value.to_owned()))
        .collect();
    for name in POLLUTED {
        if *name == "CARGO_TARGET_DIR"
            || (own_coverage && ["RUSTFLAGS", "LLVM_PROFILE_FILE"].contains(name))
        {
            continue;
        }
        assert!(
            !vars.contains_key(*name),
            "inherited {name} reached child: {output}"
        );
    }
    assert!(
        !vars.keys().any(|key| key.starts_with("CARGO_LLVM_COV")),
        "{output}"
    );
    assert_eq!(
        vars.get("MUTATOR_ENV_KEEP").map(String::as_str),
        Some("keep-me")
    );
    #[cfg(windows)]
    let target = view.directory.join("project").join("target/mutator-build");
    #[cfg(target_os = "linux")]
    let target = {
        let _ = view;
        project.join("target/mutator-build")
    };
    #[cfg(windows)]
    let _ = project;
    assert_eq!(
        vars.get("CARGO_TARGET_DIR"),
        Some(&target.to_string_lossy().into_owned())
    );
    if own_coverage {
        assert_eq!(vars.get("RUSTFLAGS").map(String::as_str), Some("own-flags"));
        assert_eq!(
            vars.get("LLVM_PROFILE_FILE").map(String::as_str),
            Some("own-%p.profraw")
        );
    }
}
