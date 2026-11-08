//! Public binary seam for the Rust dryer port.
use std::fs;
use std::process::{Command, Output};

use crate::crap::Project;

fn run(project: &Project, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("dry")
        .arg("--root")
        .arg(&project.root)
        .args(args)
        .current_dir(&project.root)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}

#[test]
fn help_is_the_original_and_non_rust_selection_is_explicitly_refused() {
    let project = Project::new();
    let output = run(&project, &["--help"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        include_bytes!("../../../fixtures/dry/help.txt")
    );
    assert!(output.stderr.is_empty());

    project.write("src/other.py", "def duplicate():\n    return 1\n");
    let output = run(&project, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("python is not ported yet")
    );
    assert!(!project.root.join(".metrics/dry.edn").exists());
}

#[test]
fn original_argument_and_exit_contract() {
    let project = Project::new();
    for (args, code, message) in [
        (&["--nope"][..], 1, "Unknown option: --nope\n\nUsage: dryer"),
        (&["--root"][..], 1, "--root requires a value"),
        (&["--min-lines", ""][..], 1, "--min-lines requires a value"),
        (
            &["--threshold", "nope"][..],
            1,
            "--threshold requires a number",
        ),
        (
            &["--min-nodes", "1.5"][..],
            1,
            "--min-nodes requires an integer",
        ),
        (&["--format", "csv"][..], 2, "Unknown format: csv\n"),
        (
            &["--exclude", "../outside"][..],
            1,
            "--exclude must be a path or glob inside the project root",
        ),
    ] {
        let output = run(&project, args);
        assert_eq!(output.status.code(), Some(code), "{args:?}");
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with(message),
            "{args:?}"
        );
        assert!(output.stdout.is_empty());
    }
    project.write("src/one.rs", "pub fn one() {}\n");
    let output = run(
        &project,
        &["--edn", "--text", "--min-lines", "1", "--min-nodes", "0"],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"No duplicate candidates found.\n");
}

#[test]
fn dry_publishes_empty_candidates_when_no_form_clears_size() {
    let project = Project::new();
    project.write("src/lib.rs", "pub fn small() {}\n");
    let output = run(&project, &["--edn", "--min-nodes", "999"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(output.stdout, b"{:candidates []}\n");
    assert_eq!(
        fs::read(project.root.join(".metrics/dry.edn")).unwrap(),
        output.stdout
    );
}

/// Pinned CPython 3.12 refuses more than 4300 digits in an `int()` string and
/// accepts exactly 4300 (fixtures/dry/count/limits.tsv records the boundary).
#[test]
fn count_options_take_the_interpreters_digit_limit_and_refuse_beyond_it() {
    let project = Project::new();
    let accepted = run(&project, &["--min-lines", &"1".repeat(4300)]);
    assert_eq!(accepted.status.code(), Some(0), "{accepted:?}");
    assert_eq!(accepted.stdout, b"No source files to analyze.\n");

    let refused = run(&project, &["--min-nodes", &"1".repeat(4301)]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&refused.stderr).starts_with("--min-nodes requires an integer\n\n"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
}

/// `fixtures/dry/threshold/cases.tsv` escapes non-printable ASCII as `\u{X}`.
fn decode(encoded: &str) -> String {
    let mut text = String::new();
    let mut chars = encoded.chars();
    while let Some(char) = chars.next() {
        if char != '\\' {
            text.push(char);
            continue;
        }
        match chars.next().unwrap() {
            '\\' => text.push('\\'),
            'u' => {
                assert_eq!(Some('{'), chars.next());
                let hex: String = chars.by_ref().take_while(|c| *c != '}').collect();
                text.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
            }
            other => panic!("unknown escape \\{other}"),
        }
    }
    text
}

/// Every `--threshold` spelling in the table generated from pinned dryer's
/// `float()` is accepted or refused by the binary exactly as Python does.
/// Python's `_take` refuses an empty value or one starting with `-` before
/// `float()` runs, so those rows expect `requires a value` here.
#[test]
fn threshold_accepts_and_refuses_what_pinned_python_float_does() {
    let project = Project::new();
    let help =
        String::from_utf8(include_bytes!("../../../fixtures/dry/help.txt").to_vec()).unwrap();
    let mut rows = 0;
    for line in include_str!("../../../fixtures/dry/threshold/cases.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let ["threshold", value, expected] = fields[..] else {
            panic!("unexpected case line {line:?}");
        };
        let value = decode(value);
        let output = run(&project, &["--threshold", &value]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if value.is_empty() || value.starts_with('-') {
            assert_eq!(output.status.code(), Some(1), "{value:?}");
            assert_eq!(
                stderr,
                format!("--threshold requires a value\n\n{help}"),
                "{value:?}"
            );
        } else if expected.starts_with("ok:") {
            assert_eq!(output.status.code(), Some(0), "{value:?}: {stderr}");
            assert_eq!(output.stdout, b"No source files to analyze.\n", "{value:?}");
        } else {
            assert_eq!(expected, "error:--threshold requires a number");
            assert_eq!(output.status.code(), Some(1), "{value:?}");
            assert!(output.stdout.is_empty(), "{value:?}");
            assert_eq!(
                stderr,
                format!("--threshold requires a number\n\n{help}"),
                "{value:?}"
            );
        }
        rows += 1;
    }
    assert_eq!(rows, 104);
}

/// Native Windows above the minimum build prints no platform diagnostic; the
/// pinned tests cannot reach a real older build here either.
#[cfg(windows)]
#[test]
fn native_windows_root_has_no_platform_diagnostic() {
    let project = Project::new();
    project.write("src/lib.rs", "pub fn one() {}\n");
    let output = run(&project, &[]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("Wrote "), "{stderr}");
    assert!(!stderr.contains("Windows drive"), "{stderr}");
    assert!(!stderr.contains("22H2"), "{stderr}");
}

#[cfg(unix)]
fn wsl_kernel() -> bool {
    fs::read_to_string("/proc/version")
        .map(|text| text.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

/// The delivered WSL2 platform: the real candidate binary must print the
/// original Windows-drive warning before it selects anything. Only a WSL2
/// kernel with a writable `/mnt/<drive>` runs it; other Linux hosts skip.
#[cfg(unix)]
#[test]
fn wsl_windows_drive_root_warns_on_stderr_before_the_empty_selection() {
    if !wsl_kernel() {
        eprintln!("skipping: /proc/version is not a WSL2 kernel");
        return;
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = ["/mnt/d", "/mnt/c", "/mnt"]
        .iter()
        .find_map(|base| {
            let path = std::path::Path::new(base)
                .join(format!("dsh-dry-wsl-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).ok().map(|()| path)
        })
        .expect("a writable Windows drive under /mnt");
    let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("dry")
        .arg("--root")
        .arg(&root)
        .current_dir(&root)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    let expected = format!(
        "{} is on a Windows drive. Under WSL2, clone the project under ~/ \
         for speed and working symlinks.\n",
        root.display()
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(output.stdout, b"No source files to analyze.\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), expected);
    assert!(!root.join(".metrics").exists());
    fs::remove_dir_all(&root).unwrap();
}
