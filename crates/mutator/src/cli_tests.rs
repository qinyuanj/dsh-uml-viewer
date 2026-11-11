//! `--copy` parsing: the option takes one path per occurrence and may repeat.
//!
//! What the path is allowed to be is not a parsing question; the validation
//! matrix is in `isolation_outside_tests.rs`, next to `resolve_outside`.
use crate::cli;

fn parse(args: &[&str]) -> Result<cli::Options, String> {
    cli::parse(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
}

#[test]
fn copy_is_repeatable_and_keeps_every_value_in_order() {
    let options = parse(&[
        "--copy",
        "../contracts",
        "-s",
        "src",
        "--copy",
        "../shared/contracts",
        "app.py",
    ])
    .unwrap();
    assert_eq!(options.copies, ["../contracts", "../shared/contracts"]);
    // Two spellings of one position stay two arguments here; the resolver is
    // what collapses them, so a caller can still see exactly what it passed.
    let repeated = parse(&["--copy", "../contracts", "--copy", "../contracts"]).unwrap();
    assert_eq!(repeated.copies, ["../contracts", "../contracts"]);
}

#[test]
fn a_run_without_copy_declares_no_outside_path() {
    assert!(
        parse(&["--no-coverage", "app.py"])
            .unwrap()
            .copies
            .is_empty()
    );
}

#[test]
fn copy_requires_a_value() {
    for args in [
        vec!["--copy"],
        vec!["--copy", "", "--root", "."],
        vec!["--copy", "--root", "."],
    ] {
        match parse(&args) {
            Ok(_) => panic!("{args:?} parsed without a value"),
            Err(error) => assert_eq!(error, "--copy requires a value", "{args:?}"),
        }
    }
}
