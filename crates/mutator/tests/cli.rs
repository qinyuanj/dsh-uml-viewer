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
    assert_eq!(run(&["--max-workers", "2"]), ExitCode::from(1));
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
}
