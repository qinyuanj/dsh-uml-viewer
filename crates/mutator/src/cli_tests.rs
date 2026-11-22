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

#[test]
fn changed_inputs_are_repeatable_and_confirmation_controls_the_scope() {
    use crate::metrics::{ChangedInput, ChangedInputs, TestInputScope};

    let global = parse(&["--no-coverage", "app.py"]).unwrap();
    assert_eq!(global.test_input_scope(), TestInputScope::Global);

    let confirmed = parse(&[
        "--changed-input",
        "src/a.rs",
        "--deleted-input",
        "old.rs",
        "--changed-input",
        "tests/a.rs",
        "--confirm-changed-inputs",
    ])
    .unwrap();
    assert_eq!(confirmed.changed_inputs, ["src/a.rs", "tests/a.rs"]);
    assert_eq!(confirmed.deleted_inputs, ["old.rs"]);
    let scope = confirmed.test_input_scope();
    let canonical = scope.canonical();
    assert!(
        canonical.contains(&format!("F {}:src/a.rs", "src/a.rs".len())),
        "{canonical}"
    );
    assert!(
        canonical.contains(&format!("D {}:old.rs", "old.rs".len())),
        "{canonical}"
    );
    assert!(
        canonical.contains(&format!("F {}:tests/a.rs", "tests/a.rs".len())),
        "{canonical}"
    );
    assert_eq!(
        scope,
        TestInputScope::changed([
            ChangedInput::changed("src/a.rs"),
            ChangedInput::changed("tests/a.rs"),
            ChangedInput::deleted("old.rs"),
        ])
    );

    let unconfirmed = parse(&["--changed-input", "src/a.rs"]).unwrap();
    assert_eq!(
        unconfirmed.test_input_scope(),
        TestInputScope::Changed(ChangedInputs {
            inputs: [ChangedInput::changed("src/a.rs")].into_iter().collect(),
            confirmed: false,
        })
    );
}

#[test]
fn a_confirmed_changed_scope_includes_copies() {
    let options = parse(&[
        "--changed-input",
        "src/a.rs",
        "--copy",
        "../contracts",
        "--confirm-changed-inputs",
    ])
    .unwrap();
    let canonical = options.test_input_scope().canonical();
    assert!(
        canonical.contains(&format!("F {}:../contracts", "../contracts".len())),
        "{canonical}"
    );
}

#[test]
fn copies_alone_keep_the_global_scope() {
    use crate::metrics::TestInputScope;

    let options = parse(&["--copy", "../contracts"]).unwrap();
    assert_eq!(options.test_input_scope(), TestInputScope::Global);

    let confirmed_without_inputs =
        parse(&["--copy", "../contracts", "--confirm-changed-inputs"]).unwrap();
    assert!(matches!(
        confirmed_without_inputs.test_input_scope(),
        TestInputScope::Changed(_)
    ));
    assert!(
        !confirmed_without_inputs
            .test_input_scope()
            .canonical()
            .contains("../contracts")
    );
}
