use std::ffi::OsString;
use std::process::ExitCode;

fn run(args: &[&str]) -> ExitCode {
    mutator::run(&args.iter().map(OsString::from).collect::<Vec<_>>())
}

#[test]
fn help_wins_over_unknown_options_and_usage_errors_fail() {
    assert_eq!(run(&["--unknown", "--help"]), ExitCode::SUCCESS);
    assert_eq!(run(&["--unknown"]), ExitCode::from(1));
    assert_eq!(run(&["--root"]), ExitCode::from(1));
    assert_eq!(run(&["--max-workers", "0"]), ExitCode::from(1));
    assert_eq!(run(&["--max-workers", "many"]), ExitCode::from(1));
    // Several workers are a valid request. Parsing it is enough here; an empty
    // project proves that without running a command.
    #[cfg(target_os = "linux")]
    {
        let empty = std::env::temp_dir().join(format!("mutator-cli-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(
            run(&["--root", empty.to_str().unwrap(), "--max-workers", "2"]),
            ExitCode::SUCCESS
        );
        std::fs::remove_dir(&empty).unwrap();
    }
    assert_eq!(run(&["--root", "--help"]), ExitCode::SUCCESS);
    assert_eq!(run(&["--lines", "0"]), ExitCode::from(1));
    assert_eq!(run(&["--timeout-factor", "0"]), ExitCode::from(1));
    assert_eq!(run(&["--select-specs", "--full-suite"]), ExitCode::from(1));
    assert_eq!(
        run(&["--busted-task", "fast", "--test-command", "true"]),
        ExitCode::from(1)
    );
    assert_eq!(run(&["--scan", "--max-workers", "1"]), ExitCode::from(1));
    assert_eq!(
        run(&["--mutate-all", "--since-last-run"]),
        ExitCode::from(1)
    );
    assert_eq!(
        run(&["--no-coverage", "--coverage-command", "true"]),
        ExitCode::from(1)
    );
    assert_eq!(run(&["--exclude", "../outside"]), ExitCode::from(1));
    // `--copy` names a path outside the project root; the usage errors below are
    // the same exit as every other usage error, on every platform.
    assert_eq!(run(&["--copy"]), ExitCode::from(1));
    assert_eq!(run(&["--copy", "../.."]), ExitCode::from(1));
    assert_eq!(run(&["--copy", "contracts"]), ExitCode::from(1));
    let absolute = std::env::temp_dir();
    assert_eq!(
        run(&["--copy", absolute.to_str().unwrap()]),
        ExitCode::from(1)
    );
    let missing = format!("../mutator-cli-missing-{}", std::process::id());
    assert_eq!(run(&["--copy", &missing]), ExitCode::from(1));
    // Copy isolation recreates `output` and `mutation` and owns `project`, `home`,
    // `temp` and `appdata` in the slot, so a `--copy` target there would be deleted
    // or merged over before the command ran. Lexically reserved: the refusal does
    // not depend on the path existing, and it comes before any command.
    for name in ["project", "home", "temp", "appdata", "output", "mutation"] {
        for value in [format!("../{name}"), format!("../{name}/nested")] {
            assert_eq!(run(&["--copy", &value]), ExitCode::from(1), "{value}");
        }
    }
}
