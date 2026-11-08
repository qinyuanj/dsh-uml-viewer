//! The discovery and argument parts of `dryer/tests/test_cli.py` at pinned
//! b3f72ad. The process-level cases (report bytes, CRLF, `--changed`, usage
//! exits) are ported at the binary seam in
//! `crates/uml-viewer/tests/dry_languages.rs`.

use crate::discover::{is_source_file, is_test_file, iter_source_files, language_of, select};
use crate::{Options, Parsed, parse};
use quality_core::language::Language;
use quality_core::select::Scope;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    super::scratch(name)
}
fn write(root: &Path, name: &str) {
    super::write_source(root, name, "x = 1\n");
}
/// `parse_args`: the scope and the positionals of one argument list.
fn scope_and_positionals(root: &Path, args: &[&str]) -> (Scope, Vec<String>) {
    let mut scope = Scope {
        root: root.to_path_buf(),
        ..Scope::default()
    };
    let mut positionals = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            "-s" | "--source-root" => {
                scope.source_roots.push(args[index + 1].to_string());
                index += 1;
            }
            "--exclude" => {
                scope.exclusions.push(args[index + 1].to_string());
                index += 1;
            }
            positional => positionals.push(positional.to_string()),
        }
        index += 1;
    }
    (scope, positionals)
}

/// `select_files(parse_args(...))`, as project-relative posix paths, with
/// the selection notes (Python's stderr) captured.
fn chosen_with_notes(root: &Path, args: &[&str]) -> (Vec<String>, String) {
    let (scope, positionals) = scope_and_positionals(root, args);
    let mut notes = Vec::new();
    let files = select(&scope, &positionals, &mut notes);
    (
        files
            .iter()
            .map(|path| quality_core::paths::relative_posix(path, root).unwrap())
            .collect(),
        String::from_utf8(notes).unwrap(),
    )
}

fn chosen(root: &Path, args: &[&str]) -> Vec<String> {
    let (files, notes) = chosen_with_notes(root, args);
    assert_eq!(notes, "", "unexpected selection notes");
    files
}

#[test]
fn language_detection() {
    for (name, expected) in [
        ("src/app/core.clj", Some(Language::Clojure)),
        ("src/app/core.cljd", Some(Language::Clojure)),
        ("Widget.java", Some(Language::Java)),
        ("board.go", Some(Language::Go)),
        ("ui/view.tsx", Some(Language::TypeScript)),
        ("src/lib.rs", Some(Language::Rust)),
        ("src/dryer/cli.py", Some(Language::Python)),
        ("src/calc/init.lua", Some(Language::Lua)),
        ("types.d.ts", None),
        ("notes.md", None),
    ] {
        assert_eq!(language_of(Path::new(name)), expected, "{name}");
    }
}

#[test]
fn names_that_are_tests() {
    for name in [
        "pkg/foo_test.go",
        "src/app_test.clj",
        "src/app_test.cljc",
        "src/app_test.cljs",
        "src/app_test.cljd",
        "src/app_test.bb",
        "ui/a.test.ts",
        "ui/a.spec.ts",
        "ui/a.test.tsx",
        "ui/a.spec.tsx",
        "ui/a.test.mts",
        "ui/a.spec.mts",
        "tests/conftest.py",
        "pkg/foo_test.py",
        "pkg/test_foo.py",
        "tests/app.py",
    ] {
        assert!(is_test_file(Path::new(name)), "{name}");
    }
    assert!(!is_test_file(Path::new("src/app.py")));
}

#[test]
fn cljd_is_clojure_source_and_its_tests_are_tests() {
    assert!(is_source_file(Path::new("src/app/core.cljd")));
    assert!(!is_source_file(Path::new("src/app/core_test.cljd")));
    assert!(!is_source_file(Path::new("README.md")));
    assert!(!is_source_file(Path::new("src/app_spec.lua")));
    assert!(is_test_file(Path::new("src/app_spec.lua")));
    assert!(!is_test_file(Path::new("src/app.cljd")));
}

#[test]
fn test_files_and_skipped_directories_are_left_out() {
    let root = scratch("cli-skipped");
    for name in [
        "src/app.py",
        "src/foo_test.py",
        "target/app.py",
        "tests/app.py",
    ] {
        write(&root, name);
    }
    let relative: Vec<String> = iter_source_files(std::slice::from_ref(&root))
        .iter()
        .map(|path| quality_core::paths::relative_posix(path, &root).unwrap())
        .collect();
    assert_eq!(relative, vec!["src/app.py"]);
}

#[test]
fn a_named_file_is_walked_as_is() {
    let root = scratch("cli-named");
    write(&root, "notes.md");
    assert_eq!(
        iter_source_files(&[root.join("notes.md")]),
        vec![quality_core::paths::resolve(&root.join("notes.md"))]
    );
    assert_eq!(
        iter_source_files(&[root.join("missing")]),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn installed_tooling_is_not_project_source() {
    let root = scratch("cli-tooling");
    write(&root, "src/a.py");
    write(&root, ".uml-viewer/dryer/src/b.py");
    assert_eq!(chosen(&root, &[]), vec!["src/a.py"]);
}

#[test]
fn selection_follows_crapper_and_adds_cljd() {
    let root = scratch("cli-cljd-selection");
    for name in [
        "src/a.py",
        "src/b.cljd",
        "src/b_test.cljd",
        "src/c.clj",
        "gen/d.py",
        "spec/e.lua",
        "src/f_spec.lua",
    ] {
        write(&root, name);
    }
    let mine = iter_source_files(std::slice::from_ref(&root));
    // `gen` is added to SKIP_DIRS in the original case; quality-core's table
    // is fixed, so the same exclusion is expressed with --exclude instead.
    let theirs = quality_core::select::iter_source_files(std::slice::from_ref(&root));
    assert_eq!(
        mine.iter()
            .filter(|path| path.extension().is_none_or(|ext| ext != "cljd"))
            .collect::<Vec<_>>(),
        theirs.iter().collect::<Vec<_>>()
    );
    let scope = Scope {
        root: root.clone(),
        exclusions: vec!["gen".to_string()],
        ..Scope::default()
    };
    let mut notes = Vec::new();
    let selected = select(&scope, &[], &mut notes);
    assert_eq!(
        selected
            .iter()
            .map(|path| quality_core::paths::relative_posix(path, &root).unwrap())
            .collect::<Vec<_>>(),
        vec!["src/a.py", "src/b.cljd", "src/c.clj"]
    );
}

#[test]
fn source_root_limits_the_walk() {
    let root = scratch("cli-source-root");
    write(&root, "src/a.py");
    write(&root, "extra/b.py");
    assert_eq!(chosen(&root, &["--source-root", "src"]), vec!["src/a.py"]);
}

#[test]
fn exclude_filters_the_walk_and_source_roots() {
    let root = scratch("cli-exclude");
    for name in [
        "src/a.py",
        "src/gen/b.py",
        "src/deep/gen/c.py",
        "lib/d.py",
        "lib/e.py",
    ] {
        write(&root, name);
    }
    assert_eq!(
        chosen(
            &root,
            &["--exclude", "src/**/gen", "--exclude", "./lib/e.py"]
        ),
        vec!["lib/d.py", "src/a.py"]
    );
    assert_eq!(
        chosen(&root, &["-s", "src", "--exclude", "src/gen/"]),
        vec!["src/a.py", "src/deep/gen/c.py"]
    );
    assert_eq!(
        chosen(&root, &["src", "--exclude", "src/*/gen"]),
        vec!["src/a.py", "src/gen/b.py"]
    );
    assert_eq!(
        chosen(&root, &["--exclude", "SRC"]),
        vec![
            "lib/d.py",
            "lib/e.py",
            "src/a.py",
            "src/deep/gen/c.py",
            "src/gen/b.py"
        ]
    );
}

#[test]
fn an_excluded_file_named_explicitly_is_skipped() {
    let root = scratch("cli-explicit-excluded");
    write(&root, "src/gen/a.py");
    write(&root, "src/b.py");
    let (files, notes) =
        chosen_with_notes(&root, &["--exclude", "src/gen", "src/gen/a.py", "src/b.py"]);
    assert_eq!(files, vec!["src/b.py"]);
    assert_eq!(notes, "Skipping excluded src/gen/a.py\n");
}

fn scope_tree(root: &Path) {
    for name in ["src/a.py", "src/x/c.py", "lib/b.py"] {
        write(root, name);
    }
}

#[test]
fn a_named_file_outside_the_source_roots_is_skipped_with_a_message() {
    let root = scratch("cli-outside-roots");
    scope_tree(&root);
    let outside = root.parent().unwrap().join(format!(
        "{}-elsewhere.py",
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&outside, "x = 1\n").unwrap();
    let (files, notes) =
        chosen_with_notes(&root, &["-s", "src", "lib/b.py", outside.to_str().unwrap()]);
    assert_eq!(files, Vec::<String>::new());
    assert_eq!(
        notes,
        format!(
            "Skipping lib/b.py; it is outside the source roots\nSkipping {}; it is outside the source roots\n",
            quality_core::paths::posix(&quality_core::paths::resolve(&outside))
        )
    );
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn a_named_directory_keeps_only_files_under_the_source_roots() {
    let root = scratch("cli-named-directory");
    scope_tree(&root);
    assert_eq!(
        chosen(&root, &["-s", "src", "."]),
        vec!["src/a.py", "src/x/c.py"]
    );
    assert_eq!(
        chosen(&root, &["-s", "src/x", "-s", "lib", "src"]),
        vec!["src/x/c.py"]
    );
}

#[test]
fn a_named_file_inside_a_source_root_is_all_that_is_chosen() {
    let root = scratch("cli-named-inside");
    scope_tree(&root);
    assert_eq!(chosen(&root, &["-s", "src", "src/a.py"]), vec!["src/a.py"]);
    assert_eq!(chosen(&root, &["-s", "src/a.py", "src"]), vec!["src/a.py"]);
}

#[test]
fn source_roots_alone_still_walk_the_roots_then_filter_and_exclude() {
    let root = scratch("cli-roots-alone");
    scope_tree(&root);
    assert_eq!(
        chosen(&root, &["-s", "src"]),
        vec!["src/a.py", "src/x/c.py"]
    );
    assert_eq!(chosen(&root, &["-s", "src", "c.py"]), vec!["src/x/c.py"]);
    assert_eq!(
        chosen(&root, &["-s", "src", "--exclude", "src/x"]),
        vec!["src/a.py"]
    );
    assert_eq!(
        chosen(&root, &["-s", "src", "src", "--exclude", "src/x"]),
        vec!["src/a.py"]
    );
}

#[test]
fn parse_defaults_and_flags() {
    let args: Vec<String> = [
        "--threshold",
        "0.9",
        "--min-lines",
        "5",
        "--min-nodes",
        "30",
        "--edn",
        "spec",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let Parsed::Scan(options) = parse(&args) else {
        panic!("expected a scan")
    };
    assert_eq!(options.threshold, 0.9);
    assert_eq!(options.min_lines, 5);
    assert_eq!(options.min_nodes, 30);
    assert_eq!(options.format, "edn");
    assert_eq!(options.positionals, vec!["spec"]);
}

#[test]
fn zero_is_a_valid_count() {
    let args: Vec<String> = ["--min-lines", "0", "--min-nodes", "0"]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    let Parsed::Scan(Options {
        min_lines,
        min_nodes,
        ..
    }) = parse(&args)
    else {
        panic!("expected a scan")
    };
    assert_eq!(min_lines, 0);
    assert_eq!(min_nodes, 0);
}

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn git_repo(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "<REDACTED_EMAIL>"]);
    git(root, &["config", "user.name", "dryer"]);
}

/// `test_changed_finds_new_directories_under_a_subdirectory_root`: the exact
/// selected list, `.cljd` included. The report half is at the binary seam.
#[test]
fn changed_finds_new_directories_under_a_subdirectory_root() {
    let root = scratch("cli-changed-subdir");
    git_repo(&root);
    write(&root, "sub/old.py");
    write(&root, "sub/gone.py");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "base"]);
    std::fs::remove_file(root.join("sub/gone.py")).unwrap();
    super::write_source(
        &root,
        "sub/fresh/a b.py",
        "def alpha(xs):\n    return filter(xs, odd)\n",
    );
    write(&root, "sub/fresh/b.cljd");
    write(&root, "sub/fresh/c.py");
    write(&root, "sub/fresh/test_c.py");
    write(&root, "other/d.py");
    let sub = root.join("sub");
    let scope = Scope {
        root: sub,
        ..Scope::default()
    };
    let mut notes = Vec::new();
    let files = crate::discover::select_changed(&scope, &[], &mut notes);
    assert_eq!(
        files
            .iter()
            .map(|path| quality_core::paths::relative_posix(path, &root).unwrap())
            .collect::<Vec<_>>(),
        vec!["sub/fresh/a b.py", "sub/fresh/b.cljd", "sub/fresh/c.py"]
    );
}

#[test]
fn exclude_filters_changed_files() {
    let root = scratch("cli-changed-exclude");
    git_repo(&root);
    write(&root, "src/a.py");
    write(&root, "src/gen/b.py");
    let scope = Scope {
        root: root.clone(),
        exclusions: vec!["src\\gen".to_string()],
        ..Scope::default()
    };
    let mut notes = Vec::new();
    let files = crate::discover::select_changed(&scope, &[], &mut notes);
    assert_eq!(
        files
            .iter()
            .map(|path| quality_core::paths::relative_posix(path, &root).unwrap())
            .collect::<Vec<_>>(),
        vec!["src/a.py"]
    );
}
