use crate::binary;
use std::fs;

#[test]
fn mutate_binary_exposes_original_help_and_usage_exit() {
    let output = binary(["mutate", "--nope", "--help"]);
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/mutate/help.txt"
        ))
        .unwrap()
    );
    assert!(output.stderr.is_empty());
    let output = binary(["mutate", "--nope"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output
            .stderr
            .starts_with(b"Unknown option: --nope\n\nUsage:")
    );
}
#[cfg(not(target_os = "linux"))]
#[test]
fn non_linux_mutation_refuses_without_running_tests() {
    let output = binary(["mutate"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("only on Linux"));
}

#[cfg(target_os = "linux")]
fn fixture_project() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &to.join(entry.file_name()));
            } else {
                fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/rust");
    let directory = std::env::temp_dir().join(format!(
        "mutate-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    copy(&source, &directory);
    directory.join("project")
}

#[cfg(target_os = "linux")]
fn remove_fixture(root: &std::path::Path) {
    let directory = root.parent().unwrap();
    let name = directory.file_name().unwrap().to_str().unwrap();
    assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
    assert!(name.starts_with("mutate-cli-"), "{name}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn mutate_usage_errors_are_the_original_bytes_and_messages() {
    let help = fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/mutate/help.txt"
    ))
    .unwrap();
    for (arguments, message) in [
        (vec!["--nope"], "Unknown option: --nope"),
        (
            vec!["--timeout-factor", "fast"],
            "could not convert string to float: 'fast'",
        ),
        (
            vec!["--mutation-warning", "many"],
            "invalid literal for int() with base 10: 'many'",
        ),
        (
            vec!["--lines", "x"],
            "invalid literal for int() with base 10: 'x'",
        ),
        (
            vec!["--scan", "--max-workers", "2"],
            "--scan cannot be combined with --max-workers",
        ),
    ] {
        let output = binary(std::iter::once("mutate").chain(arguments.iter().copied()));
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let mut expected = format!("{message}\n\n").into_bytes();
        expected.extend_from_slice(&help);
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&expected),
            "{arguments:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn mutate_scan_reports_sites_without_coverage_or_snapshot_writes() {
    let root = fixture_project();
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--scan",
        "app/src/lib.rs",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Scan: 1 mutation sites in app/src/lib.rs"),
        "{stdout}"
    );
    assert!(!root.join(".metrics").exists());
    assert!(!root.join("target").exists());
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--scan",
        "--lines",
        "999",
        "app/src/lib.rs",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Scan: 0 mutation sites"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    remove_fixture(&root);
}

#[cfg(target_os = "linux")]
#[test]
fn mutate_timings_warning_and_verbose_report_like_the_original() {
    let root = fixture_project();
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--no-coverage",
        "--mutate-all",
        "--timings",
        "--mutation-warning",
        "0",
        "--verbose",
        "--test-command",
        "true",
        "app/src/lib.rs",
    ]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("WARNING: 1 covered mutations selected in app/src/lib.rs."),
        "{stderr}"
    );
    assert!(stderr.contains("timing baseline: "), "{stderr}");
    assert!(stderr.contains("timing mutation-run: "), "{stderr}");
    assert!(!stderr.contains("timing coverage: "), "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "app/src/lib.rs:4 app/twice + -> - executed reason=explicit-all previous=none result=survived id=["
        ),
        "{stdout}"
    );
    remove_fixture(&root);
}

#[cfg(target_os = "linux")]
#[test]
fn mutate_binary_refuses_stale_backups_before_any_test() {
    let root = fixture_project();
    let backup = root.join("target/mutator-backup/app.rs");
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    fs::write(&backup, "stale\n").unwrap();
    let marker = root.parent().unwrap().join("ran");
    let output = binary([
        "mutate",
        "--root",
        root.to_str().unwrap(),
        "--no-coverage",
        "--mutate-all",
        "--test-command",
        &format!("touch {}", marker.display()),
        "app/src/lib.rs",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists(), "no test may run while backups exist");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Found backups from an older mutator under"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    remove_fixture(&root);
}
