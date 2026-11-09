//! Seam A (#134): the binary's process boundary. Each role is a
//! subcommand; a missing or unknown subcommand and unknown `--` options are
//! refused with usage (exit 2).

use std::process::Output;

fn run(args: &[&str]) -> Output {
    super::binary(args)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The subcommands `--help` lists, in table order.
fn listed_subcommands() -> Vec<String> {
    let output = run(&["--help"]);
    assert_eq!(Some(0), output.status.code());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip_while(|line| *line != "subcommands:")
        .skip(1)
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

#[test]
fn the_subcommands_are_the_headless_roles_and_the_quality_tools() {
    // #18 removed the Herdr companion roles with the companion itself;
    // dsh-uml-viewer#30 removed the standalone `viewer` window.
    assert_eq!(
        vec![
            "ir", "scene", "resident", "crap", "mutate", "dry", "quality"
        ],
        listed_subcommands()
    );
}

#[test]
fn the_removed_viewer_window_is_an_unknown_subcommand() {
    // dsh-uml-viewer#30: `viewer` and a bare diagram path no longer open a
    // window; both are usage errors naming the argument.
    for argument in ["viewer", "diagram.edn"] {
        let output = run(&[argument]);
        assert_eq!(Some(2), output.status.code(), "{argument}");
        let err = stderr(&output);
        assert!(err.contains(&format!("未知子命令 {argument}")), "{err}");
        assert!(err.contains("usage: uml-viewer"), "{err}");
    }
}

#[test]
fn no_arguments_is_a_usage_error() {
    let output = run(&[]);
    assert_eq!(Some(2), output.status.code());
    let err = stderr(&output);
    assert!(err.contains("缺少子命令"), "{err}");
    assert!(err.contains("usage: uml-viewer"), "{err}");
}

#[test]
fn ir_is_ported_and_reports_bad_arguments_on_stderr() {
    // #136: the ir role runs headless; an unknown option is a usage failure
    // (exit 1) naming the option, not EX_UNAVAILABLE.
    let output = run(&["ir", "--bogus"]);
    assert_eq!(Some(1), output.status.code());
    let err = stderr(&output);
    assert!(err.contains("IR generation failed"), "{err}");
    assert!(err.contains("--bogus"), "{err}");
}

#[test]
fn ir_refuses_an_ir_document_as_the_policy_with_a_usage_hint() {
    // #166: a generated diagram passed as the sole positional argument is not
    // a policy; ir fails fast instead of writing the default output.
    let dir = std::env::temp_dir().join(format!("uml-dispatch-166-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("diagram.edn"),
        "{:title \"UML\" :prefix \"app\" :policy-file \"policy.edn\" :classes [] :edges []}",
    )
    .unwrap();
    let output = super::binary([
        "ir".as_ref(),
        "--root".as_ref(),
        dir.as_os_str(),
        "diagram.edn".as_ref(),
    ]);
    assert_eq!(Some(1), output.status.code());
    let err = stderr(&output);
    assert!(err.contains("generated IR document"), "{err}");
    assert!(
        err.contains("usage: ir [policy [output]] [--root directory]"),
        "{err}"
    );
}

#[test]
fn an_unknown_leading_option_is_refused_with_usage() {
    let output = run(&["--bogus"]);
    assert_eq!(Some(2), output.status.code());
    let err = stderr(&output);
    assert!(err.contains("未知选项 --bogus"), "{err}");
    assert!(err.contains("usage"), "{err}");
}

#[test]
fn scene_is_ported_and_reports_usage_errors() {
    // #137: no `--ir` is a usage error (exit 2), not "unported" (69).
    let output = run(&["scene"]);
    assert_eq!(Some(2), output.status.code());
    assert!(stderr(&output).contains("usage"), "{}", stderr(&output));
    let output = run(&["scene", "--bogus"]);
    assert_eq!(Some(2), output.status.code());
    assert!(stderr(&output).contains("usage"), "{}", stderr(&output));
    // A missing IR file is a runtime failure (exit 1), named on stderr.
    let output = run(&["scene", "--ir", "no-such-file.edn"]);
    assert_eq!(Some(1), output.status.code());
    assert!(
        stderr(&output).contains("no-such-file.edn"),
        "{}",
        stderr(&output)
    );
}
