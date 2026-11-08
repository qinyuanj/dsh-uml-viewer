//! Seam ① for `uml-viewer crap`: the subcommand run on scratch projects.
//! The cases are crapper's pytest cases (selection, `-s`, `--exclude`,
//! empty selection, usage errors, Rust functions), ported to the binary:
//! each asserts the published snapshot, the streams and the exit status.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// A scratch project directory, removed on drop.
pub struct Project {
    pub root: PathBuf,
}

impl Project {
    pub fn new() -> Project {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "uml-viewer-crap-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("project")).unwrap();
        Project {
            root: canonical(&base.join("project")),
        }
    }

    pub fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    /// `uml-viewer crap --root <root> args…`, run from the project root.
    pub fn crap(&self, args: &[&str]) -> Output {
        crap_in(&self.root, &self.root, args)
    }

    pub fn snapshot(&self) -> Option<String> {
        fs::read_to_string(self.root.join(".metrics").join("crap.edn")).ok()
    }

    /// `(namespace, name, complexity)` of every published entry, in order.
    pub fn entries(&self) -> Vec<(String, String, u32)> {
        let text = self.snapshot().expect("a published snapshot");
        text.lines()
            .filter(|line| line.contains(":name "))
            .map(|line| {
                let field = |key: &str| {
                    let rest = line.split(&format!(":{key} ")).nth(1).unwrap();
                    rest.split(", ")
                        .next()
                        .unwrap()
                        .trim_end_matches('}')
                        .to_string()
                };
                let unquote = |text: String| text.trim_matches('"').to_string();
                (
                    unquote(field("namespace")),
                    unquote(field("name")),
                    field("complexity").parse().unwrap(),
                )
            })
            .collect()
    }

    /// Published names, sorted, as crapper's `_names` helper reads them.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.entries().into_iter().map(|e| e.1).collect();
        names.sort();
        names
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.root.parent().unwrap());
    }
}

/// The path with symlinks resolved, without Windows' `\\?\` prefix.
pub fn canonical(path: &Path) -> PathBuf {
    let path = fs::canonicalize(path).unwrap();
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path,
    }
}

pub fn crap_in(cwd: &Path, root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("crap")
        .arg("--root")
        .arg(root)
        .args(args)
        .current_dir(cwd)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// crapper's help text: usage errors print `<message>\n\n<HELP>` on stderr.
fn help() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .args(["crap", "--help"])
        .output()
        .unwrap();
    assert_eq!(Some(0), output.status.code());
    stdout(&output)
}

const CLJ_FREE_RUST: &str = "pub fn open(ok: bool) -> i32 { if ok { 1 } else { 0 } }\n";

fn manifest(project: &Project, name: &str) {
    project.write(
        "Cargo.toml",
        &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    );
}

/// One function per file, named after the file stem (crapper's `CLJ` tree).
fn rust_tree(project: &Project, files: &[&str]) {
    manifest(project, "demo");
    for relative in files {
        let stem = Path::new(relative).file_stem().unwrap().to_string_lossy();
        project.write(relative, &format!("pub fn {stem}() {{}}\n"));
    }
}

fn tree(project: &Project) {
    rust_tree(
        project,
        &[
            "src/keep.rs",
            "src/gen/made.rs",
            "lib/util.rs",
            "lib/gen/deep.rs",
        ],
    );
}

fn scope_tree(project: &Project) {
    rust_tree(project, &["src/a.rs", "src/x/c.rs", "lib/b.rs"]);
}

// --- test_cli.py: usage errors and help ------------------------------------

#[test]
fn usage_errors_name_the_problem_and_print_help_to_stderr() {
    let help = help();
    let project = Project::new();
    let cases: &[(&[&str], &str)] = &[
        (&["--nope"], "Unknown option: --nope"),
        (&["-s"], "-s requires a value"),
        (
            &["--coverage-command", "--changed"],
            "--coverage-command requires a value",
        ),
        (&["--threshold", "high"], "--threshold requires a number"),
        (
            &["--no-coverage", "--coverage-command", "true"],
            "--no-coverage cannot be combined with --coverage-command",
        ),
        (&["--exclude"], "--exclude requires a value"),
        (
            &["--lua-shards", "0"],
            "--lua-shards requires a positive integer",
        ),
    ];
    for (argv, message) in cases {
        let output = project.crap(argv);
        assert_eq!(Some(1), output.status.code(), "{argv:?}");
        assert_eq!("", stdout(&output));
        assert_eq!(format!("{message}\n\n{help}"), stderr(&output), "{argv:?}");
    }
    let output = project.crap(&["-h", "--nope"]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!(help, stdout(&output));
}

#[test]
fn help_documents_scope_exits_and_exclusions() {
    let help = help();
    assert!(help.contains("--exclude"));
    assert!(help.contains("outside the source roots"));
    assert!(help.contains("or 3 when coverage was requested"));
    assert!(help.contains("3 when --threshold was given with coverage"));
}

#[test]
fn empty_option_values_are_rejected() {
    let project = Project::new();
    for argv in [["--threshold", ""], ["--threshold", "-1"]] {
        let output = project.crap(&argv);
        assert_eq!(Some(1), output.status.code());
        assert!(stderr(&output).starts_with("--threshold requires a value\n\n"));
    }
}

#[test]
fn scope_usage_errors_name_the_problem() {
    let help = help();
    let project = Project::new();
    fs::create_dir_all(project.root.join("src")).unwrap();
    project.write("file.rs", "fn x() {}\n");
    let cases: &[(&[&str], &str)] = &[
        (
            &["--exclude", "../x"],
            "--exclude must be a path or glob inside the project root: ../x",
        ),
        (
            &["--exclude", "/abs"],
            "--exclude must be a path or glob inside the project root: /abs",
        ),
        (
            &["--full-tree", "src"],
            "--full-tree cannot be combined with path arguments",
        ),
        (
            &["--full-tree", "--changed"],
            "--full-tree cannot be combined with --changed",
        ),
        (
            &["--full-tree", "--use-existing-coverage"],
            "--full-tree cannot be combined with --use-existing-coverage",
        ),
    ];
    for (argv, message) in cases {
        let output = project.crap(argv);
        assert_eq!(Some(1), output.status.code(), "{argv:?}");
        assert_eq!("", stdout(&output));
        assert_eq!(format!("{message}\n\n{help}"), stderr(&output));
    }
    let outside = project.root.parent().unwrap().to_path_buf();
    let roots = [
        (
            "missing".to_string(),
            format!(
                "Source root does not exist: {}",
                project.root.join("missing").display()
            ),
        ),
        (
            outside.display().to_string(),
            format!(
                "Source root must be inside project root: {}",
                outside.display()
            ),
        ),
        (
            "..".to_string(),
            format!(
                "Source root must be inside project root: {}",
                outside.display()
            ),
        ),
    ];
    for (name, message) in roots {
        let output = project.crap(&["--full-tree", "-s", &name]);
        assert_eq!(Some(1), output.status.code(), "{name}");
        assert_eq!("", stdout(&output));
        assert_eq!(
            format!("Source selection failed: {message}\n"),
            stderr(&output)
        );
    }
    assert!(!project.root.join(".metrics").exists());
}

#[test]
fn absolute_and_parent_exclusions_are_usage_errors() {
    let project = Project::new();
    for pattern in ["/abs", "..", "../x", "a/../b", r"a\..\b", r"\abs", "a/.."] {
        let output = project.crap(&["--no-coverage", "--exclude", pattern]);
        assert_eq!(Some(1), output.status.code(), "{pattern}");
        let expected =
            format!("--exclude must be a path or glob inside the project root: {pattern}\n\n");
        assert!(stderr(&output).starts_with(&expected), "{pattern}");
    }
    for pattern in ["a/b", "**/vendor", "./gen/", "..foo", "a..b/c", "*.py"] {
        let output = project.crap(&["--no-coverage", "--exclude", pattern]);
        assert_eq!(Some(0), output.status.code(), "{pattern}");
    }
}

// --- test_cli.py / test_scope.py: selection ---------------------------------

#[test]
fn selects_directories_filters_and_reports_an_empty_selection() {
    let project = Project::new();
    manifest(&project, "demo");
    project.write("src/core.rs", CLJ_FREE_RUST);
    project.write("notes.md", "not source\n");
    for argv in [
        &["--no-coverage", "--source-root", "src"][..],
        &["--no-coverage", "src"],
        &["--no-coverage", "src/core.rs", "core"],
    ] {
        let output = project.crap(argv);
        assert_eq!(Some(0), output.status.code(), "{argv:?}");
        assert_eq!(vec!["open"], project.names());
    }
    let output = project.crap(&["--no-coverage", "notes.md"]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!("No source files to analyze.\n", stdout(&output));
}

#[test]
fn glob_exclusions_follow_uml_viewer() {
    let cases = [
        ("gen", "gen/a.rs", true),
        ("gen", "src/gen/a.rs", false),
        ("gen", "generated/a.rs", false),
        ("**/gen", "src/gen/a.rs", true),
        ("**/gen", "gen/a.rs", true),
        ("src/*.rs", "src/a.rs", true),
        ("src/*.rs", "src/x/a.rs", false),
        ("src/?.rs", "src/a.rs", true),
        ("src/?.rs", "src/ab.rs", false),
        ("src/?", "src/a/b.rs", true),
        ("src/**/a.rs", "src/a.rs", true),
        ("src/**/a.rs", "src/x/y/a.rs", true),
        ("src/**", "src/x/a.rs", true),
        ("**", "a.rs", true),
        ("SRC", "src/a.rs", false),
        ("./src/", "src/a.rs", true),
        (r"src\gen", "src/gen/a.rs", true),
        ("src/a.rs", "src/a.rsx/b.rs", false),
        ("a.b", "axb/c.rs", false),
        ("a+b", "a+b/c.rs", true),
        ("*", "a.rs", true),
        ("src/[ab].rs", "src/a.rs", true),
        ("src/[!ab].rs", "src/a.rs", false),
        ("src/[a-c]x.rs", "src/bx.rs", true),
        ("src/[.rs", "src/[.rs", true),
    ];
    for (pattern, relative, expected) in cases {
        let project = Project::new();
        manifest(&project, "demo");
        project.write(relative, "pub fn chosen() {}\n");
        let output = project.crap(&["--no-coverage", "--exclude", pattern]);
        let published = project
            .snapshot()
            .is_some_and(|text| text.contains("chosen"));
        assert_eq!(
            !expected,
            published,
            "{pattern} {relative}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn business_roots_exclude_exact_directories_without_duplicate_metrics() {
    let project = Project::new();
    manifest(&project, "demo");
    for (path, name) in [
        ("client/main.rs", "client_main"),
        ("client/_trigger/event.rs", "client_trigger"),
        ("common/shared.rs", "shared"),
        ("server/main.rs", "server_main"),
        ("server/_trigger/nested/event.rs", "server_trigger"),
        ("server/_trigger_extra/event.rs", "trigger_extra"),
        ("packages/vendor.rs", "package_vendor"),
    ] {
        project.write(path, &format!("fn {name}() {{}}\n"));
    }
    let output = project.crap(&[
        "--no-coverage",
        "--source-root",
        "client",
        "--source-root",
        "common",
        "--source-root",
        "server",
        "--source-root",
        "server/_trigger_extra",
        "--source-root",
        "packages",
        "--exclude",
        "packages",
        "--exclude",
        "server/_trigger",
    ]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!(
        vec![
            "client_main",
            "client_trigger",
            "server_main",
            "shared",
            "trigger_extra"
        ],
        project.names()
    );
}

#[test]
fn glob_exclusions_match_path_segments_and_directory_descendants() {
    let project = Project::new();
    manifest(&project, "demo");
    for (path, name) in [
        ("client/generated/deep/code.rs", "client_generated"),
        ("generated/root.rs", "root_generated"),
        ("server/generated/code.rs", "server_generated"),
        ("server/private.rs", "server_private"),
        ("server/deep/private.rs", "deep_private"),
        ("common/code1.rs", "code_one"),
        ("common/code2.rs", "code_two"),
        ("common/main.rs", "main_fn"),
    ] {
        project.write(path, &format!("fn {name}() {{}}\n"));
    }
    let output = project.crap(&[
        "--no-coverage",
        "--exclude",
        "**/generated",
        "--exclude",
        "server/*.rs",
        "--exclude",
        "common/code?.rs",
    ]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!(vec!["deep_private", "main_fn"], project.names());
}

#[test]
fn exclusions_apply_to_default_explicit_and_filtered_selection() {
    for selection in [
        &[][..],
        &["client"],
        &["client/main.rs", "client/private.rs"],
        &["main"],
    ] {
        let project = Project::new();
        manifest(&project, "demo");
        project.write("client/main.rs", "fn main_fn() {}\n");
        project.write("client/private.rs", "fn private() {}\n");
        let mut argv = vec!["--no-coverage", "--exclude", "client/private.rs"];
        argv.extend_from_slice(selection);
        let output = project.crap(&argv);
        assert_eq!(Some(0), output.status.code(), "{selection:?}");
        assert_eq!(vec!["main_fn"], project.names(), "{selection:?}");
    }
}

#[test]
fn source_roots_keep_default_exclusions_and_allow_explicit_test_files() {
    let project = Project::new();
    manifest(&project, "demo");
    project.write("client/main.rs", "fn main_fn() {}\n");
    for directory in ["spec", "vendor", "target", "lua_modules", ".luarocks"] {
        project.write(&format!("{directory}/tool.rs"), "fn skipped() {}\n");
    }
    assert_eq!(Some(0), project.crap(&["--no-coverage"]).status.code());
    assert_eq!(vec!["main_fn"], project.names());
    assert_eq!(
        Some(0),
        project
            .crap(&["--no-coverage", "spec/tool.rs"])
            .status
            .code()
    );
    assert_eq!(vec!["skipped"], project.names());
}

#[test]
fn native_paths_with_spaces_and_backslash_exclusions() {
    let project = Project::new();
    let root = project.root.join("business project");
    fs::create_dir_all(&root).unwrap();
    let root = canonical(&root);
    let write = |relative: &str, name: &str| {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("fn {name}() {{}}\n")).unwrap();
    };
    write("server/main.rs", "main_fn");
    write("server/_trigger/event.rs", "trigger");
    write("server/generated/nested/event.rs", "generated");
    let server = root.join("server");
    let generated = server.join("generated");
    let output = crap_in(
        &project.root,
        &root,
        &[
            "--no-coverage",
            "--source-root",
            server.to_str().unwrap(),
            "--source-root",
            generated.to_str().unwrap(),
            "--exclude",
            r"./server\_trigger\",
            "--exclude",
            r"server\generated\**",
        ],
    );
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
    let text = fs::read_to_string(root.join(".metrics/crap.edn")).unwrap();
    assert_eq!(1, text.matches(":name ").count());
    assert!(text.contains(":name \"main_fn\""));
}

#[test]
fn exclude_filters_the_default_walk_and_source_roots() {
    let project = Project::new();
    tree(&project);
    let output = project.crap(&[
        "--no-coverage",
        "--exclude",
        "src/gen",
        "--exclude",
        "lib/g*/",
    ]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!(vec!["keep", "util"], project.names());
    let output = project.crap(&["--no-coverage", "-s", "src", "--exclude", "**/gen"]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!(vec!["keep"], project.names());
}

#[test]
fn exclude_filters_named_directories_and_reports_named_files() {
    let project = Project::new();
    tree(&project);
    let output = project.crap(&[
        "--no-coverage",
        "--exclude",
        "**/gen",
        "lib",
        "src/gen/made.rs",
    ]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!(vec!["util"], project.names());
    assert!(stderr(&output).contains("Skipping excluded src/gen/made.rs\n"));
}

#[test]
fn an_excluded_named_file_is_dropped_even_when_it_is_not_source() {
    let project = Project::new();
    project.write("gen/notes.md", "x\n");
    let output = project.crap(&["--no-coverage", "--exclude", "gen", "gen/notes.md"]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!("Skipping excluded gen/notes.md\n", stderr(&output));
    assert_eq!("No source files to analyze.\n", stdout(&output));
}

#[test]
fn without_full_tree_a_missing_source_root_is_still_allowed() {
    let project = Project::new();
    let output = project.crap(&["--no-coverage", "-s", "missing", "-s", ".."]);
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
}

#[test]
fn full_tree_accepts_existing_roots_and_a_file_root() {
    let project = Project::new();
    tree(&project);
    let output = project.crap(&[
        "--no-coverage",
        "--full-tree",
        "-s",
        "src",
        "-s",
        "lib/util.rs",
    ]);
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
    assert_eq!(vec!["keep", "made", "util"], project.names());
    let output = project.crap(&["--no-coverage", "--full-tree", "-s", "."]);
    assert_eq!(Some(0), output.status.code());
    assert_eq!(vec!["deep", "keep", "made", "util"], project.names());
}

#[test]
fn full_tree_reads_the_project_root_by_default_from_the_current_directory() {
    let project = Project::new();
    tree(&project);
    let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .args(["crap", "--full-tree", "--no-coverage", "-s", "lib"])
        .current_dir(&project.root)
        .output()
        .unwrap();
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
    assert_eq!(vec!["deep", "util"], project.names());
}

#[test]
fn a_named_file_outside_the_source_roots_is_skipped_with_a_message() {
    let project = Project::new();
    scope_tree(&project);
    let outside = project.root.parent().unwrap().join("elsewhere.rs");
    fs::write(&outside, "fn elsewhere() {}\n").unwrap();
    let output = project.crap(&[
        "--no-coverage",
        "-s",
        "src",
        "lib/b.rs",
        outside.to_str().unwrap(),
    ]);
    assert_eq!(Some(0), output.status.code());
    let shown = outside.to_string_lossy().replace('\\', "/");
    assert_eq!(
        format!(
            "Skipping lib/b.rs; it is outside the source roots\n\
             Skipping {shown}; it is outside the source roots\n"
        ),
        stderr(&output)
    );
    assert_eq!("No source files to analyze.\n", stdout(&output));
}

#[test]
fn a_named_directory_keeps_only_files_under_the_source_roots() {
    let project = Project::new();
    scope_tree(&project);
    let output = project.crap(&["--no-coverage", "-s", "src", "."]);
    assert_eq!("", stderr(&output).replace(&wrote(&project), ""));
    assert_eq!(vec!["a", "c"], project.names());
    let output = project.crap(&["--no-coverage", "-s", "src/x", "-s", "lib", "src"]);
    assert_eq!("", stderr(&output).replace(&wrote(&project), ""));
    assert_eq!(vec!["c"], project.names());
}

#[test]
fn a_named_file_inside_a_source_root_is_all_that_is_chosen() {
    let project = Project::new();
    scope_tree(&project);
    project.crap(&["--no-coverage", "-s", "src", "src/a.rs"]);
    assert_eq!(vec!["a"], project.names());
    project.crap(&["--no-coverage", "-s", "src/a.rs", "src"]);
    assert_eq!(vec!["a"], project.names());
}

#[test]
fn source_roots_alone_still_walk_the_roots_then_filter_and_exclude() {
    let project = Project::new();
    scope_tree(&project);
    for (argv, names) in [
        (&["-s", "src"][..], vec!["a", "c"]),
        (&["-s", "src", "c.rs"], vec!["c"]),
        (&["-s", "src", "--exclude", "src/x"], vec!["a"]),
        (&["-s", "src", "src", "--exclude", "src/x"], vec!["a"]),
    ] {
        let mut full = vec!["--no-coverage"];
        full.extend_from_slice(argv);
        assert_eq!(Some(0), project.crap(&full).status.code());
        assert_eq!(names, project.names(), "{argv:?}");
    }
}

fn wrote(project: &Project) -> String {
    format!(
        "Wrote {}\n",
        project.root.join(".metrics").join("crap.edn").display()
    )
}

// --- test_cli.py: coverage, thresholds and exit statuses --------------------

fn choose(project: &Project, lcov: &str) {
    manifest(project, "demo");
    project.write(
        "src/lib.rs",
        "\npub fn choose(x: bool) -> i32 {\n    if x { 1 } else { 0 }\n}\n",
    );
    project.write("target/coverage/lcov.info", lcov);
}

#[test]
fn existing_coverage_feeds_the_snapshot() {
    let project = Project::new();
    choose(&project, "SF:src/lib.rs\nDA:2,1\nDA:3,1\nend_of_record\n");
    let output = project.crap(&["--use-existing-coverage", "src/lib.rs"]);
    assert_eq!(Some(0), output.status.code());
    let text = project.snapshot().unwrap();
    assert!(text.contains(":coverage 100.0"), "{text}");
    assert!(text.contains(":crap 2.0"), "{text}");
}

#[test]
fn threshold_statuses() {
    let uncovered = "SF:src/lib.rs\nDA:2,0\nDA:3,0\nend_of_record\n";
    let project = Project::new();
    choose(&project, uncovered);
    let output = project.crap(&["--use-existing-coverage", "--threshold", "1"]);
    assert_eq!(Some(2), output.status.code());
    let output = project.crap(&["--use-existing-coverage", "--threshold", "5.25"]);
    assert_eq!(Some(2), output.status.code());
    assert!(stderr(&output).contains("CRAP threshold exceeded: 6.0 > 5.2\n"));
    let output = project.crap(&["--use-existing-coverage", "--threshold", "6"]);
    assert_eq!(
        Some(0),
        output.status.code(),
        "a threshold equal to the score passes"
    );
    let output = project.crap(&["--no-coverage", "--threshold", "1"]);
    assert_eq!(Some(0), output.status.code());
    assert!(!stderr(&output).contains("no coverage"));
}

#[test]
fn threshold_exits_3_when_existing_coverage_joins_no_function() {
    let project = Project::new();
    choose(
        &project,
        "SF:src/elsewhere/other.rs\nDA:3,1\nend_of_record\n",
    );
    let output = project.crap(&["--use-existing-coverage", "--threshold", "0"]);
    assert_eq!(Some(3), output.status.code());
    let err = stderr(&output);
    assert!(err.contains("1 of 1 functions have no coverage\n"), "{err}");
    assert!(err.contains("No function's coverage was measured"), "{err}");
    let text = project.snapshot().unwrap();
    assert!(text.contains(":coverage nil") && text.contains(":crap nil"));
}

#[test]
fn partial_coverage_keeps_its_status_and_counts_the_unscored() {
    for (threshold, status) in [("6", 0), ("5", 2)] {
        let project = Project::new();
        choose(&project, "SF:src/lib.rs\nDA:2,0\nDA:3,0\nend_of_record\n");
        project.write("src/util.rs", "pub fn one() {}\n\npub fn two() {}\n");
        let output = project.crap(&["--use-existing-coverage", "--threshold", threshold]);
        assert_eq!(Some(status), output.status.code());
        let err = stderr(&output);
        assert!(err.contains("2 of 3 functions have no coverage\n"), "{err}");
        assert!(!err.contains("No function's coverage was measured"));
        assert_eq!(
            status == 2,
            err.contains("CRAP threshold exceeded: 6.0 > 5.0")
        );
    }
}

#[test]
fn a_snapshot_that_cannot_be_published_keeps_the_previous_one() {
    let project = Project::new();
    tree(&project);
    // A directory where the snapshot belongs cannot be replaced by a file.
    fs::create_dir_all(project.root.join(".metrics/crap.edn/inside")).unwrap();
    let output = project.crap(&["--no-coverage"]);
    assert_eq!(Some(1), output.status.code());
    assert!(
        stderr(&output).starts_with("Snapshot publication failed: "),
        "{}",
        stderr(&output)
    );
    assert_eq!("", stdout(&output));
    let left: Vec<_> = fs::read_dir(project.root.join(".metrics"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        vec![std::ffi::OsString::from("crap.edn")],
        left,
        "no staged copy is left"
    );
}

#[test]
fn custom_coverage_failure_keeps_the_previous_snapshot() {
    let project = Project::new();
    tree(&project);
    project.write(".metrics/crap.edn", "{:entries []}\n");
    let output = project.crap(&["--full-tree", "--coverage-command", "exit 3"]);
    assert_eq!(Some(1), output.status.code());
    let snapshot = project.root.join(".metrics").join("crap.edn");
    assert!(stderr(&output).ends_with(&format!(
        "Coverage failed: Coverage command exited 3 in {}. {} was not replaced.\n",
        project.root.display(),
        snapshot.display()
    )));
    assert_eq!("{:entries []}\n", project.snapshot().unwrap());
}

#[test]
fn report_comes_before_the_metrics_line() {
    let project = Project::new();
    tree(&project);
    let output = project.crap(&["--no-coverage"]);
    assert_eq!(Some(0), output.status.code());
    assert!(stdout(&output).starts_with("CRAP Report\n===========\nLanguages: rust\n\n"));
    assert_eq!(wrote(&project), stderr(&output));
}

// --- test_rust.py: functions, namespaces and test code ----------------------

fn rust_entries(name: &str, relative: &str, source: &str) -> Vec<(String, String, u32)> {
    let project = Project::new();
    manifest(&project, name);
    project.write(relative, source);
    let output = project.crap(&["--no-coverage", relative]);
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
    let mut entries = project.entries();
    // The snapshot is sorted; compare as crapper's tests do, by file order.
    entries.sort_by_key(|entry| {
        source
            .find(&format!("fn {}", entry.1))
            .unwrap_or(usize::MAX)
    });
    entries
}

fn triple(namespace: &str, name: &str, complexity: u32) -> (String, String, u32) {
    (namespace.to_string(), name.to_string(), complexity)
}

#[test]
fn functions_methods_and_skipped_tests() {
    let source = "
pub fn choose(x: i32) -> i32 {
    if x > 0 && x < 10 {
        return 1;
    }
    match x {
        1 => 1,
        2 => 2,
        _ => 0,
    }
}

impl Sample {
    pub fn run(&self) -> i32 {
        if self.ok { 1 } else { 0 }
    }
}

impl Trait for Sample {
    fn draw(&mut self) {}
}

mod tests {
    fn hidden() {
        if true {}
    }
}

fn outer() {
    fn inner() {
        if true {}
    }
}
";
    assert_eq!(
        vec![
            triple("demo_game", "choose", 6),
            triple("demo_game::Sample", "run", 2),
            triple("demo_game::Sample", "draw", 1),
            triple("demo_game", "outer", 2),
        ],
        rust_entries("demo-game", "src/lib.rs", source)
    );
}

#[test]
fn nested_module_path() {
    assert_eq!(
        vec![triple("demo::game::board", "place", 2)],
        rust_entries(
            "demo",
            "src/game/board.rs",
            "pub fn place() -> i32 { if ready { 1 } else { 0 } }\n"
        )
    );
}

#[test]
fn cfg_test_items_are_test_code() {
    let source = r#"
pub fn kept() {}

#[cfg(test)]
pub fn helper() { if true {} }

#[cfg(test)]
mod policy_shape_tests {
    fn case() {}
}

#[cfg(test)]
#[allow(dead_code)]
impl Host {
    pub fn fake() {}
}

impl Host {
    pub fn real() {}
    #[cfg(test)]
    pub fn probe() {}
    #[test]
    fn checks() {}
}

#[cfg(all(test, unix))]
fn unix_only() {}

#[cfg(all(test, not(miri)))]
fn not_under_miri() {}

#[tokio::test]
async fn async_case() {}

#[cfg(not(test))]
fn shipped() {}

#[cfg(any(test, feature = "fixtures"))]
pub fn fixture() {}

#[cfg(feature = "test")]
pub fn feature_named_test() {}

mod support {
    #![cfg(test)]
    fn inner() {}
}
"#;
    let names: Vec<String> = rust_entries("demo", "src/lib.rs", source)
        .into_iter()
        .map(|entry| entry.1)
        .collect();
    assert_eq!(
        vec!["kept", "real", "shipped", "fixture", "feature_named_test"],
        names
    );
}

#[test]
fn a_tests_file_module_is_test_code() {
    assert!(rust_entries("demo", "src/compose/tests.rs", "fn case() {}\n").is_empty());
    assert!(rust_entries("demo", "src/support.rs", "#![cfg(test)]\nfn case() {}\n").is_empty());
}

#[test]
fn a_crate_named_tests_is_not_test_code() {
    let names: Vec<String> = rust_entries("tests", "src/lib.rs", "pub fn run() {}\n")
        .into_iter()
        .map(|entry| entry.1)
        .collect();
    assert_eq!(vec!["run"], names);
}

#[test]
fn impl_namespace_is_the_self_type_name() {
    let project = Project::new();
    project.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\nname = \"other\"\n",
    );
    let source = "impl<T> Widget<T> { fn a(&self) {} }\n\
                  impl<T> ui::Panel<T> { fn b(&self) {} }\n\
                  impl ui::Frame { fn c(&self) {} }\n\
                  impl<T> Trait for Vec<crate::Sample> { fn d(&self) {} }\n\
                  impl Trait for &Widget { fn e(&self) {} }\n\
                  impl union<i32> { fn f(&self) {} }\n\
                  impl From<W> for default<u8> { fn g(&self) {} }\n";
    project.write("src/lib.rs", source);
    assert_eq!(Some(0), project.crap(&["--no-coverage"]).status.code());
    let mut found: Vec<(String, String)> = project
        .entries()
        .into_iter()
        .map(|entry| (entry.0, entry.1))
        .collect();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    let expected = [
        ("demo::Widget", "a"),
        ("demo::Panel", "b"),
        ("demo::Frame", "c"),
        ("demo::Vec", "d"),
        ("demo", "e"),
        ("demo", "f"),
        ("demo", "g"),
    ];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(ns, name)| (ns.to_string(), name.to_string()))
        .collect();
    assert_eq!(expected, found);
}

#[test]
fn module_paths_follow_the_crate_layout() {
    let layouts = [
        ("src/game/mod.rs", "demo::game"),
        ("src/game/board/mod.rs", "demo::game::board"),
        ("src/bin.rs", "demo::bin"),
        ("src/bin/mod.rs", "demo::bin"),
        ("src/bin/tool.rs", "tool"),
        ("src/bin/tool/mod.rs", "tool"),
        ("src/lib/mod.rs", "demo::lib"),
        ("src/mod.rs", "demo"),
        ("src/main.rs", "demo"),
    ];
    for (relative, namespace) in layouts {
        let entries = rust_entries("demo", relative, "pub fn open() {}\n");
        assert_eq!(vec![triple(namespace, "open", 1)], entries, "{relative}");
    }
}

#[test]
fn a_file_without_a_manifest_is_crate_named_crate() {
    let project = Project::new();
    project.write("notes.rs", "pub fn bare() {}\n");
    project.write("lib.rs", "pub fn top() {}\n");
    assert_eq!(Some(0), project.crap(&["--no-coverage"]).status.code());
    let mut entries = project.entries();
    entries.sort();
    assert_eq!(
        vec![triple("crate", "top", 1), triple("crate::notes", "bare", 1)],
        entries
    );
}

#[test]
fn a_manifest_with_non_ascii_text_still_names_the_crate() {
    let project = Project::new();
    project.write(
        "Cargo.toml",
        "# Größe\n[package]\nauthors = [\"Zoë Ünïcode\"]\nname = \"démo-crate\"\n\n[dependencies]\n",
    );
    project.write("src/lib.rs", "pub fn open() {}\n");
    let output = project.crap(&["--no-coverage"]);
    assert_eq!(Some(0), output.status.code(), "{}", stderr(&output));
    assert_eq!(vec![triple("démo_crate", "open", 1)], project.entries());
}
