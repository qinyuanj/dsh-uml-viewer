//! Lua mutation: the pinned original's site discovery and its snapshots.
use mutator::sites;
use quality_core::serde_json::Value;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mutate/lua")
        .canonicalize()
        .expect("the Lua fixtures are checked in")
}

/// The pinned mutator's `sites_in_file` output for the same bytes: offsets,
/// originals, mutants, categories, owners and mutation identities.
#[test]
fn lua_site_discovery_matches_the_pinned_original() {
    let root = fixtures().join("project");
    let oracle: Value = quality_core::serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("sites.json"))
            .expect("the site oracle is checked in"),
    )
    .expect("the site oracle is JSON");
    let Value::Object(files) = oracle else {
        panic!("the site oracle is a map");
    };
    let mut compared = 0;
    for (name, expected) in &files {
        if name.ends_with(" #digests") {
            continue;
        }
        let path = root.join(name);
        let source = quality_core::grammar::read_source(&path).expect("the fixture source reads");
        let (found, forms) =
            sites::discover(&source, &path, &root, name).expect("the Lua fixture sources parse");
        let Value::Array(expected) = expected else {
            panic!("{name} lists its sites");
        };
        assert_eq!(found.len(), expected.len(), "{name} site count");
        for (site, expected) in found.iter().zip(expected) {
            let field = |key: &str| -> String {
                match expected.get(key) {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Number(number)) => number.to_string(),
                    Some(other) => panic!("{name}: unexpected {key}: {other}"),
                    None => panic!("{name}: missing {key}"),
                }
            };
            assert_eq!(site.file, field("file"), "{name} file");
            assert_eq!(site.namespace, field("namespace"), "{name} namespace");
            assert_eq!(site.form_id, field("form_id"), "{name} form");
            assert_eq!(site.name, field("name"), "{name} name");
            assert_eq!(site.line.to_string(), field("line"), "{name} line");
            assert_eq!(site.start.to_string(), field("start"), "{name} start");
            assert_eq!(site.end.to_string(), field("end"), "{name} end");
            assert_eq!(site.original, field("original"), "{name} original");
            assert_eq!(site.mutant, field("mutant"), "{name} mutant");
            assert_eq!(site.category, field("category"), "{name} category");
            assert_eq!(
                site.mutation_id,
                field("mutation_id"),
                "{name} mutation identity"
            );
            compared += 1;
        }
        let digests = files
            .get(&format!("{name} #digests"))
            .unwrap_or_else(|| panic!("{name} digests"));
        let Value::Array(digests) = digests else {
            panic!("{name} digests are a list");
        };
        let mut expected_digests = std::collections::BTreeMap::new();
        for entry in digests {
            let Value::Array(pair) = entry else {
                panic!("{name} digest entries are pairs");
            };
            let (Value::Array(key), Some(Value::String(digest))) = (&pair[0], pair.get(1)) else {
                panic!("{name} digest entry shape");
            };
            let (Some(Value::String(namespace)), Some(Value::String(id))) =
                (key.first(), key.get(1))
            else {
                panic!("{name} form key shape");
            };
            expected_digests.insert((namespace.clone(), id.clone()), digest.clone());
        }
        let found_digests: std::collections::BTreeMap<_, _> = forms
            .iter()
            .map(|form| {
                (
                    (form.namespace.clone(), form.id.clone()),
                    form.digest.clone(),
                )
            })
            .collect();
        assert_eq!(found_digests, expected_digests, "{name} form digests");
    }
    assert_eq!(compared, 16, "every fixture site was compared");
}

/// The mutated sources of the fixture, as the oracle ran them.
#[cfg(target_os = "linux")]
const TARGETS: [&str; 3] = ["src/calc.lua", "src/odd name.lua", "nested/lib/mathx.lua"];

/// A scratch Lua module: the publication contract validates spec paths on disk.
struct Module {
    directory: PathBuf,
}
impl Module {
    fn new(name: &str) -> Module {
        let directory =
            std::env::temp_dir().join(format!("lua-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        for spec in ["a_spec.lua", "b_spec.lua", "c_spec.lua", "d_spec.lua"] {
            let path = directory.join("spec").join(spec);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "it('x', function() end)\n").unwrap();
        }
        std::fs::create_dir_all(directory.join("src")).unwrap();
        std::fs::write(directory.join("src/x.lua"), "return {}\n").unwrap();
        Module { directory }
    }
    fn source(&self) -> PathBuf {
        self.directory.join("src/x.lua")
    }
    fn canonical(&self) -> String {
        mutator::lua::canonical(&self.source())
    }
}
impl Drop for Module {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// The collector's publication: spec durations, per-spec line membership and
/// the fallback lines, exactly the fields the original reads.
#[test]
fn lua_publication_decodes_specs_membership_and_fallback() {
    let module = Module::new("decode");
    let pending = r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":0.2,"spec/b_spec.lua":0.1},"lines":{"src/x.lua":{"4":{"spec/a_spec.lua":true},"5":{"spec/a_spec.lua":true,"spec/b_spec.lua":true}}},"fallback":{"src/x.lua":{"6":true}}}"#;
    let membership = mutator::lua::decode(pending, &module.directory).unwrap();
    assert_eq!(membership.specs["spec/a_spec.lua"], 0.2);
    assert_eq!(membership.specs["spec/b_spec.lua"], 0.1);
    let source = module.canonical();
    assert_eq!(
        membership.lines[&source][&4]
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["spec/a_spec.lua"]
    );
    assert_eq!(membership.lines[&source][&5].len(), 2);
    assert!(membership.fallback[&source].contains(&6));
}

/// A malformed or incomplete publication is refused, never guessed: the
/// original raises instead of selecting a subset it cannot trust.
#[test]
fn malformed_lua_publications_are_refused() {
    let module = Module::new("refused");
    for (case, pending) in [
        ("empty", "{}"),
        ("not-json", "not json"),
        (
            "old-version",
            r#"{"version":1,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{},"fallback":{}}"#,
        ),
        (
            "incomplete",
            r#"{"version":2,"complete":false,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{},"fallback":{}}"#,
        ),
        (
            "no-specs",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{},"lines":{},"fallback":{}}"#,
        ),
        (
            "bool-duration",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":true},"lines":{},"fallback":{}}"#,
        ),
        (
            "negative-duration",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":-1},"lines":{},"fallback":{}}"#,
        ),
        (
            "missing-unsafe",
            r#"{"version":2,"complete":true,"specs":{"spec/a_spec.lua":1},"lines":{},"fallback":{}}"#,
        ),
        (
            "unknown-member",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{"src/x.lua":{"4":{"spec/other_spec.lua":true}}},"fallback":{}}"#,
        ),
        (
            "empty-membership",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{"src/x.lua":{"4":{}}},"fallback":{}}"#,
        ),
        (
            "zero-line",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{"src/x.lua":{"0":{"spec/a_spec.lua":true}}},"fallback":{}}"#,
        ),
        (
            "bad-line",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{"src/x.lua":{"x":{"spec/a_spec.lua":true}}},"fallback":{}}"#,
        ),
        (
            "spec-outside",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"../outside_spec.lua":1},"lines":{},"fallback":{}}"#,
        ),
        (
            "missing-spec",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/missing_spec.lua":1},"lines":{},"fallback":{}}"#,
        ),
        (
            "bad-fallback",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{},"fallback":{"src/x.lua":{"4":false}}}"#,
        ),
        (
            "bad-fallback-line",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{},"fallback":{"src/x.lua":{"x":true}}}"#,
        ),
        (
            "empty-source",
            r#"{"version":2,"complete":true,"unsafe":false,"specs":{"spec/a_spec.lua":1},"lines":{"":{"4":{"spec/a_spec.lua":true}}},"fallback":{}}"#,
        ),
    ] {
        assert!(
            mutator::lua::decode(pending, &module.directory).is_err(),
            "{case} must be refused"
        );
    }
}

/// Unproven attribution is never a selected subset: with `unsafe`, every
/// attributed line becomes a full-suite fallback.
#[test]
fn unsafe_lua_attribution_forces_the_full_suite() {
    let module = Module::new("unsafe");
    let pending = r#"{"version":2,"complete":true,"unsafe":true,"specs":{"spec/a_spec.lua":1},"lines":{"src/x.lua":{"4":{"spec/a_spec.lua":true}}},"fallback":{}}"#;
    let membership = mutator::lua::decode(pending, &module.directory).unwrap();
    let index = index_of(&module, membership);
    assert!(index.fallback[&module.canonical()].contains(&4));
    assert_eq!(index.select(&module.source(), 4), None);
    assert_eq!(index.select(&module.directory.join("other.lua"), 4), None);
}

fn index_of(module: &Module, membership: mutator::lua::Membership) -> mutator::lua::Index {
    let mut hits = std::collections::BTreeMap::new();
    hits.insert(
        module.canonical(),
        membership.lines[&module.canonical()]
            .keys()
            .copied()
            .collect(),
    );
    mutator::lua::Index {
        directory: module.directory.clone(),
        command: vec!["busted".into()],
        specs: membership.specs,
        lines: membership.lines,
        fallback: membership.fallback,
        hits,
        overhead: 0.0,
        seconds: 0.0,
        fresh: false,
    }
}

/// The selection contract: exact lines, fallback dominance, and covering specs
/// first with every other spec still present.
#[test]
fn lua_index_selection_is_exact_and_covering_first_keeps_the_suite() {
    let module = Module::new("select");
    let mut lines = std::collections::BTreeMap::new();
    let mut membership = std::collections::BTreeMap::new();
    for (line, specs) in [
        (3, vec!["spec/a_spec.lua"]),
        (4, vec!["spec/c_spec.lua", "spec/d_spec.lua"]),
    ] {
        let mut owners = std::collections::BTreeSet::new();
        owners.extend(specs.iter().map(|spec| spec.to_string()));
        membership.insert(line, owners);
    }
    lines.insert(module.canonical(), membership);
    let specs = ["a_spec.lua", "b_spec.lua", "c_spec.lua", "d_spec.lua"]
        .iter()
        .map(|name| (format!("spec/{name}"), 0.1))
        .collect();
    let index = mutator::lua::Index {
        directory: module.directory.clone(),
        command: vec!["busted".into()],
        specs,
        lines,
        fallback: std::collections::BTreeMap::new(),
        hits: std::collections::BTreeMap::new(),
        overhead: 1.0,
        seconds: 0.0,
        fresh: false,
    };
    assert_eq!(
        index.select(&module.source(), 3),
        Some(vec!["spec/a_spec.lua".into()])
    );
    assert_eq!(index.select(&module.source(), 9), None);
    assert_eq!(index.select(&module.directory.join("other.lua"), 3), None);
    // The duration is per distinct spec, plus the suite's overhead.
    assert_eq!(
        index.seconds_for(&["spec/a_spec.lua".into(), "spec/a_spec.lua".into()]),
        1.1
    );
    assert_eq!(
        index.seconds_for(&["spec/a_spec.lua".into(), "spec/b_spec.lua".into()]),
        1.2
    );
    assert_eq!(
        index.covering_first(&module.source(), &[3, 4, 9]),
        vec![
            "spec/a_spec.lua",
            "spec/c_spec.lua",
            "spec/d_spec.lua",
            "spec/b_spec.lua"
        ]
    );
    assert_eq!(
        index.covering_first(&module.source(), &[9]),
        vec![
            "spec/a_spec.lua",
            "spec/b_spec.lua",
            "spec/c_spec.lua",
            "spec/d_spec.lua"
        ]
    );
}

/// Shards merge into one index: durations add, memberships union, and a shard
/// that could not attribute a hit forces the full suite.
#[test]
fn lua_shards_merge_into_one_index() {
    let module = Module::new("merge");
    let pending = |spec: &str, line: usize| {
        format!(
            r#"{{"version":2,"complete":true,"unsafe":false,"specs":{{"spec/{spec}":0.5}},"lines":{{"src/x.lua":{{"{line}":{{"spec/{spec}":true}}}}}},"fallback":{{}}}}"#
        )
    };
    let merged = mutator::lua::merge(vec![
        mutator::lua::decode(&pending("a_spec.lua", 4), &module.directory).unwrap(),
        mutator::lua::decode(&pending("b_spec.lua", 4), &module.directory).unwrap(),
        mutator::lua::decode(&pending("c_spec.lua", 5), &module.directory).unwrap(),
    ]);
    assert_eq!(merged.specs.len(), 3);
    assert_eq!(merged.specs["spec/a_spec.lua"], 0.5);
    assert_eq!(merged.lines[&module.canonical()][&4].len(), 2);
    assert_eq!(merged.lines[&module.canonical()][&5].len(), 1);
    assert!(!merged.unsafe_attribution);
}

/// Ported from the original's `test_lua_scheduling.py`: a Lua selection puts
/// the sites that survived the previous run first, keeps each group in
/// discovery order, and changes nothing else about the selection.
#[test]
fn lua_scheduling_puts_prior_survivors_first_and_keeps_the_selection() {
    fn named(name: &str, line: usize) -> sites::Site {
        sites::Site {
            file: "src/calc.lua".to_string(),
            namespace: "calc".to_string(),
            form_id: format!("defn/{name}"),
            name: name.to_string(),
            line,
            start: line,
            end: line + 1,
            original: "1".to_string(),
            mutant: "0".to_string(),
            category: "number".to_string(),
            mutation_id: name.to_string(),
        }
    }
    // The selected sites in discovery order, exactly the original's list.
    let mut selected = ["survivor-a", "killed", "survivor-b", "unknown"]
        .iter()
        .enumerate()
        .map(|(index, name)| named(name, index + 1))
        .collect::<Vec<_>>();
    let outcomes = std::collections::BTreeMap::from([
        ("survivor-a".to_string(), "survived".to_string()),
        ("survivor-b".to_string(), "survived".to_string()),
        ("killed".to_string(), "killed".to_string()),
    ]);
    mutator::lua::survivors_first(&mut selected, &outcomes);
    let order: Vec<&str> = selected.iter().map(|site| site.name.as_str()).collect();
    assert_eq!(order, ["survivor-a", "survivor-b", "killed", "unknown"]);
    let mut names: Vec<&str> = order.clone();
    names.sort_unstable();
    assert_eq!(names, ["killed", "survivor-a", "survivor-b", "unknown"]);
    // Without history the order is the discovery order: the sort is stable.
    let mut untouched = vec![named("a", 1), named("b", 2), named("c", 3)];
    mutator::lua::survivors_first(&mut untouched, &std::collections::BTreeMap::new());
    assert_eq!(
        untouched
            .iter()
            .map(|site| site.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
}

/// The original's `cli._shares_queue` dispatch: full-suite Lua files share one
/// worker queue; an explicit test command or `--select-specs` takes the
/// file-by-file path, and no other language ever joins it.
#[test]
fn only_full_suite_lua_files_share_the_worker_queue() {
    let lua = Path::new("src/a.lua");
    assert!(mutator::shares_queue(lua, None, false));
    assert!(!mutator::shares_queue(lua, Some("make test"), false));
    assert!(!mutator::shares_queue(lua, None, true));
    assert!(!mutator::shares_queue(Path::new("src/a.rs"), None, false));
    assert!(!mutator::shares_queue(Path::new("src/a.py"), None, false));
}

/// Ported from the original's `test_busted_task.py`: a named Busted task is a
/// single argv entry, exactly like a spaced interpreter or project path, so a
/// task name with a space is never re-split by a shell.
#[test]
fn a_busted_task_is_one_argv_entry_even_with_spaces() {
    let module = Path::new("/tmp/project with spaces");
    let command = mutator::lua::plan(module, Some("task with spaces"));
    let start = command
        .iter()
        .position(|argument| argument == "--")
        .expect("the interpreter's arguments end at --");
    assert_eq!(
        &command[start..],
        ["--", "busted", "--ignore-lua", "--run", "task with spaces"]
    );
    assert_eq!(
        command
            .iter()
            .filter(|argument| argument.as_str() == "task with spaces")
            .count(),
        1
    );
    // The default task adds no `--run` at all.
    let plain = mutator::lua::plan(module, None);
    assert!(!plain.iter().any(|argument| argument == "--run"));
}
/// One committed snapshot, byte for byte.
#[cfg(target_os = "linux")]
fn assert_goldens(root: &Path) {
    for name in ["calc.edn", "nested.lib.mathx.edn", "odd name.edn"] {
        let actual = std::fs::read(root.join(".metrics/mutate").join(name))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let expected = std::fs::read(fixtures().join("goldens").join(name)).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&actual),
            String::from_utf8_lossy(&expected),
            "{name}"
        );
        // No normalization decides the comparison: the bytes themselves match.
        assert_eq!(actual, expected, "{name}");
        assert!(!actual.contains(&b'\r'), "{name} is LF-only");
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::ffi::OsString;
    use std::process::ExitCode;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The pinned oracle's fixed clock.
    fn fixed_clock() -> String {
        "2026-11-07T12:00:00+00:00".into()
    }

    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &to.join(entry.file_name()));
            } else {
                std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }

    /// A private copy of the committed fixture: the project under test is
    /// never mutated, and nothing is written into this checkout.
    fn project(name: &str) -> (PathBuf, PathBuf) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "lua-mutate-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        copy(&fixtures().join("project"), &directory.join("project"));
        (directory.clone(), directory.join("project"))
    }

    fn remove(directory: &Path) {
        assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
        let name = directory.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("lua-mutate-"), "{name}");
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn arguments<'a>(root: &'a Path, flags: &[&'a str]) -> Vec<String> {
        let mut arguments = vec![
            "--root".to_string(),
            root.to_string_lossy().into_owned(),
            "--max-workers".into(),
            "1".into(),
            "--timeout-factor".into(),
            "20".into(),
        ];
        arguments.extend(flags.iter().map(|flag| flag.to_string()));
        arguments.extend(TARGETS.iter().map(|target| target.to_string()));
        arguments
    }

    fn run_arguments(arguments: &[String]) -> ExitCode {
        let values: Vec<OsString> = arguments.iter().map(OsString::from).collect();
        mutator::run_with_clock(&values, fixed_clock)
    }

    /// The acceptance criterion: a real Busted/cluacov run, in overlay views,
    /// publishes the pinned original's snapshot bytes. One `//` mutant of the
    /// fixture survives, so the run reports exit 3.
    #[test]
    fn lua_snapshots_are_the_pinned_originals_bytes() {
        let (directory, root) = project("bytes");
        let first = arguments(&root, &["--mutate-all"]);
        assert_eq!(run_arguments(&first), ExitCode::from(3));
        assert_goldens(&root);
        // The differential rerun reuses the killed sites, re-runs the survivor
        // and republishes the same bytes.
        let rerun = arguments(&root, &[]);
        assert_eq!(run_arguments(&rerun), ExitCode::from(3));
        assert_goldens(&root);
        for target in TARGETS {
            assert_eq!(
                std::fs::read(root.join(target)).unwrap(),
                std::fs::read(fixtures().join("project").join(target)).unwrap(),
                "{target} was mutated"
            );
        }
        remove(&directory);
    }

    /// `--select-specs` runs only the covering specs of a mutant and confirms
    /// survivors on the full suite, so the outcomes and bytes do not move.
    #[test]
    fn selected_lua_specs_keep_the_full_suite_snapshot_bytes() {
        let (directory, root) = project("select-specs");
        let arguments = arguments(&root, &["--mutate-all", "--select-specs"]);
        assert_eq!(run_arguments(&arguments), ExitCode::from(3));
        assert_goldens(&root);
        remove(&directory);
    }

    /// `--busted-task first` runs only that task's spec, so the functions no
    /// spec covers stay uncovered. Byte-exact against the pinned original.
    #[test]
    fn a_named_busted_task_filters_the_suite_and_selection() {
        let (directory, root) = project("task");
        let mut arguments = arguments(&root, &["--mutate-all", "--busted-task", "first"]);
        arguments.truncate(arguments.len() - TARGETS.len());
        arguments.push("src/calc.lua".into());
        assert_eq!(run_arguments(&arguments), ExitCode::from(0));
        let actual = std::fs::read(root.join(".metrics/mutate/calc.edn")).unwrap();
        let expected = std::fs::read(fixtures().join("goldens/task/calc.edn")).unwrap();
        assert_eq!(actual, expected);
        assert!(!root.join(".metrics/mutate/odd name.edn").exists());
        remove(&directory);
    }

    /// The other selection route: `--no-coverage` runs the covered sites
    /// anyway. The untested source has no spec at all, so its mutant survives.
    #[test]
    fn an_untested_lua_source_survives_without_coverage() {
        let (directory, root) = project("no-coverage");
        let arguments = arguments(&root, &["--mutate-all", "--no-coverage", "--lines", "4"]);
        assert_eq!(run_arguments(&arguments), ExitCode::from(3));
        let calc = std::fs::read_to_string(root.join(".metrics/mutate/calc.edn")).unwrap();
        assert!(calc.contains(":killed 2"), "{calc}");
        assert!(calc.contains(":uncovered 0"), "{calc}");
        let untested = std::fs::read_to_string(root.join(".metrics/mutate/odd name.edn")).unwrap();
        assert!(untested.contains(":survived 1"), "{untested}");
        // No coverage report is ever published into the project.
        assert!(!root.join("target/coverage").exists());
        assert!(!root.join("coverage").exists());
        remove(&directory);
    }

    /// A scratch project this test owns; every file is written relative to its
    /// root, and no fixture or checkout is ever mutated.
    fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "lua-queue-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    fn remove_scratch(root: &Path) {
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        let name = root.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("lua-queue-"), "{name}");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A marker file beside the project, so concurrent tests and repeated
    /// runs cannot share one.
    fn marker_for(root: &Path) -> PathBuf {
        root.with_file_name(format!(
            "{}-marker",
            root.file_name().unwrap().to_string_lossy()
        ))
    }

    /// The full argument vector for one run: the root, the worker count, flags
    /// and the target files, in the order the original's CLI takes them.
    fn arguments_with(
        root: &Path,
        workers: usize,
        flags: &[&str],
        targets: &[&str],
    ) -> Vec<String> {
        let mut arguments = vec![
            "--root".to_string(),
            root.to_string_lossy().into_owned(),
            "--max-workers".into(),
            workers.to_string(),
            "--timeout-factor".into(),
            "20".into(),
        ];
        arguments.extend(flags.iter().map(|flag| flag.to_string()));
        arguments.extend(targets.iter().map(|target| target.to_string()));
        arguments
    }

    fn run_with_arguments(arguments: &[String]) -> ExitCode {
        let values: Vec<OsString> = arguments.iter().map(OsString::from).collect();
        mutator::run_with_clock(&values, fixed_clock)
    }

    /// One spec invocation, as a marked spec writes it: the spec's own name,
    /// Busted's `arg`, the worker slot the command ran in and whether it was
    /// the instrumented coverage run.
    struct Invocation {
        spec: String,
        argc: usize,
        args: Vec<String>,
        view: String,
        coverage: bool,
    }

    impl Invocation {
        /// A mutant run is the full suite plus `--no-keep-going`.
        fn mutant(&self) -> bool {
            self.args.iter().any(|arg| arg == "--no-keep-going")
        }
    }

    /// Make every named spec append one marker line per invocation. The marker
    /// path is absolute and outside the project, exactly like the fixture's
    /// `MUTATOR_LUA_SPEC_MARKER`, but it needs no process-wide environment.
    fn mark_specs(root: &Path, marker: &Path, specs: &[&str]) {
        for spec in specs {
            let path = root.join(spec);
            let text = std::fs::read_to_string(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let code = format!(
                "do\n  local marker = io.open([[{marker}]], 'a')\n  if marker and arg then\n    marker:write([[{name}]] .. '\\t' .. tostring(#arg) .. '\\t' .. table.concat(arg, string.char(31)) .. '\\t' .. tostring(os.getenv('MUTATOR_VIEW')) .. '\\t' .. (package.loaded['luacov.runner'] and 'coverage' or 'run') .. '\\n')\n    marker:close()\n  end\nend\n",
                marker = marker.display(),
                name = name,
            );
            std::fs::write(&path, code + &text).unwrap();
        }
    }

    fn invocations(marker: &Path) -> Vec<Invocation> {
        std::fs::read_to_string(marker)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split('\t').collect();
                if fields.len() < 5 {
                    return None;
                }
                Some(Invocation {
                    spec: fields[0].to_string(),
                    argc: fields[1].parse().ok()?,
                    args: fields[2]
                        .split('\u{1f}')
                        .map(str::to_string)
                        .filter(|arg| !arg.is_empty())
                        .collect(),
                    view: fields[3].to_string(),
                    coverage: fields[4] == "coverage",
                })
            })
            .collect()
    }

    /// Acceptance: every snapshot of a multi-worker Lua run is byte-identical
    /// to the single-worker run with the same injected clock, and both are the
    /// pinned original's bytes.
    #[test]
    fn lua_multi_worker_snapshots_match_one_worker_and_the_goldens() {
        let targets = ["src/calc.lua", "src/odd name.lua", "nested/lib/mathx.lua"];
        let single = project("queue-single");
        let run = run_with_arguments(&arguments_with(&single.1, 1, &["--mutate-all"], &targets));
        assert_eq!(run, ExitCode::from(3), "{}", single.1.display());
        assert_goldens(&single.1);
        let parallel = project("queue-parallel");
        let run = run_with_arguments(&arguments_with(&parallel.1, 3, &["--mutate-all"], &targets));
        assert_eq!(run, ExitCode::from(3), "{}", parallel.1.display());
        assert_goldens(&parallel.1);
        let snapshots = |root: &Path| -> std::collections::BTreeMap<String, Vec<u8>> {
            std::fs::read_dir(root.join(".metrics/mutate"))
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    (name, std::fs::read(&path).unwrap())
                })
                .collect()
        };
        let expected = snapshots(&single.1);
        assert_eq!(expected.len(), 3);
        assert_eq!(
            snapshots(&parallel.1),
            expected,
            "worker count must not change bytes"
        );
        for target in targets {
            assert_eq!(
                std::fs::read(parallel.1.join(target)).unwrap(),
                std::fs::read(fixtures().join("project").join(target)).unwrap(),
                "{target} was mutated"
            );
        }
        remove(&single.0);
        remove(&parallel.0);
    }

    /// A spec file name with a space stays one argv entry in every mutant
    /// command, and the covering spec runs first: both are visible in Busted's
    /// own `arg`, not inferred from a plausible snapshot.
    #[test]
    fn a_spaced_spec_is_one_argv_and_covering_specs_come_first() {
        let (directory, root) = project("argv");
        let marker = directory.join("spec-marker");
        mark_specs(
            &root,
            &marker,
            &["spec/add_spec.lua", "spec/double unit_spec.lua"],
        );
        // One line at a time, so the covering spec is a strict subset of the
        // suite: line 4 is M.add (covered by add_spec) and line 8 M.double
        // (covered by the spec whose file name contains a space).
        let mut commands: Vec<Vec<String>> = Vec::new();
        for line in ["4", "8"] {
            let _ = std::fs::remove_file(&marker);
            let run = run_with_arguments(&arguments_with(
                &root,
                2,
                &["--mutate-all", "--lines", line],
                &["src/calc.lua"],
            ));
            assert!(
                run == ExitCode::SUCCESS || run == ExitCode::from(3),
                "line {line}: {run:?}"
            );
            let mutants: Vec<Invocation> = invocations(&marker)
                .into_iter()
                .filter(|invocation| invocation.mutant())
                .collect();
            assert!(!mutants.is_empty(), "line {line}: the mutants really ran");
            for invocation in &mutants {
                assert_eq!(
                    invocation.argc,
                    invocation.args.len(),
                    "Busted received the vector it was given: {:?}",
                    invocation.args
                );
                assert_eq!(invocation.argc, 4, "{:?}", invocation.args);
                assert!(
                    !invocation
                        .args
                        .iter()
                        .any(|arg| arg == "unit_spec.lua" || arg == "spec/double"),
                    "the spaced spec was split: {:?}",
                    invocation.args
                );
            }
            let distinct: std::collections::BTreeSet<Vec<String>> = mutants
                .into_iter()
                .map(|invocation| invocation.args)
                .collect();
            assert_eq!(distinct.len(), 1, "one command per file's selection");
            commands.push(distinct.into_iter().next().unwrap());
        }
        assert_eq!(
            commands,
            vec![
                vec![
                    "--ignore-lua".to_string(),
                    "--no-keep-going".to_string(),
                    "spec/add_spec.lua".to_string(),
                    "spec/double unit_spec.lua".to_string(),
                ],
                vec![
                    "--ignore-lua".to_string(),
                    "--no-keep-going".to_string(),
                    "spec/double unit_spec.lua".to_string(),
                    "spec/add_spec.lua".to_string(),
                ],
            ],
            "the covering spec comes first"
        );
        remove(&directory);
    }

    /// Acceptance: the Lua files of one call share one worker queue, so the
    /// next file starts while the previous file's slow mutant is still
    /// running. A file-by-file scheduler cannot publish b before a finished.
    #[test]
    fn lua_files_share_one_worker_queue() {
        let root = scratch("interleave", &[]);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("spec")).unwrap();
        std::fs::write(
            root.join(".busted"),
            "return { default = { lpath = 'src/?.lua' } }\n",
        )
        .unwrap();
        let a = "local M = {}\n\nfunction M.add(x)\n  return x + 1\nend\n\nfunction M.halve(x)\n  return x // 2\nend\n\nreturn M\n";
        let b = "local M = {}\n\nfunction M.triple(x)\n  return x * 3\nend\n\nreturn M\n";
        std::fs::write(root.join("src/a.lua"), a).unwrap();
        std::fs::write(root.join("src/b.lua"), b).unwrap();
        let marker = marker_for(&root);
        std::fs::write(
            root.join("spec/queue_spec.lua"),
            format!(
                "local a = require('a')\nlocal b = require('b')\nlocal marker = io.open([[{marker}]], 'a')\nif marker then\n  local source = io.open('src/a.lua')\n  local text = source and source:read('*a') or ''\n  if source then source:close() end\n  local slow = text:find('x / 2', 1, true) ~= nil and text:find('x // 2', 1, true) == nil\n  marker:write((slow and 'slow' or 'fast') .. ' ' .. tostring(os.getenv('MUTATOR_VIEW')) .. '\\n')\n  marker:close()\n  if slow then os.execute('sleep 4') end\nend\ndescribe('queue', function()\n  it('adds', function() assert.equal(2, a.add(1)) end)\n  it('halves', function() assert.equal(3, a.halve(7)) end)\n  it('triples', function() assert.equal(6, b.triple(2)) end)\nend)\n",
                marker = marker.display()
            ),
        )
        .unwrap();
        let arguments = arguments_with(&root, 2, &["--mutate-all"], &["src/a.lua", "src/b.lua"]);
        let run = std::thread::spawn(move || run_with_arguments(&arguments));
        let a_snapshot = root.join(".metrics/mutate/a.edn");
        let b_snapshot = root.join(".metrics/mutate/b.edn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        let mut overlapped = false;
        while std::time::Instant::now() < deadline {
            if b_snapshot.is_file() {
                overlapped = !a_snapshot.is_file() && !run.is_finished();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let run = run.join().unwrap();
        let text = std::fs::read_to_string(&marker).unwrap();
        assert!(
            text.lines().any(|line| line.starts_with("slow ")),
            "the slow mutant never ran: {text}"
        );
        assert!(
            overlapped,
            "b.lua was not published while a.lua was still running: {text}"
        );
        assert_eq!(run, ExitCode::SUCCESS, "{text}");
        assert!(a_snapshot.is_file(), "the file publishes when it finishes");
        let views: std::collections::BTreeSet<&str> = text
            .lines()
            .filter_map(|line| line.split_once(' ').map(|(_, view)| view))
            .collect();
        assert!(views.len() >= 2, "{text}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.lua")).unwrap(), a);
        assert_eq!(std::fs::read_to_string(root.join("src/b.lua")).unwrap(), b);
        remove_scratch(&root);
    }

    /// Acceptance: worker slots belong to the call, not to the file. With three
    /// workers every mutant of every Lua file runs in the same three slots,
    /// each slot is reused, and the pool is removed afterwards.
    #[test]
    fn lua_queue_workers_are_reused_and_private() {
        let (directory, root) = project("workers");
        let marker = directory.join("spec-marker");
        mark_specs(
            &root,
            &marker,
            &["spec/add_spec.lua", "spec/double unit_spec.lua"],
        );
        // `--no-coverage` executes every site, so the three slots are reused
        // across several mutants instead of one site each.
        let run = run_with_arguments(&arguments_with(
            &root,
            3,
            &["--no-coverage", "--mutate-all"],
            &["src/calc.lua", "src/odd name.lua", "nested/lib/mathx.lua"],
        ));
        assert_eq!(run, ExitCode::from(3));
        let invocations = invocations(&marker);
        let views: Vec<&str> = invocations
            .iter()
            .filter(|invocation| invocation.mutant())
            .map(|invocation| invocation.view.as_str())
            .collect();
        assert!(!views.is_empty());
        let counts = views.iter().fold(
            std::collections::BTreeMap::<&str, usize>::new(),
            |mut counts, view| {
                *counts.entry(view).or_default() += 1;
                counts
            },
        );
        assert_eq!(counts.len(), 3, "one slot per worker: {counts:?}");
        for (view, count) in &counts {
            assert!(*count >= 2, "{view} was reused: {counts:?}");
            let parent = Path::new(view).parent().unwrap();
            assert!(
                parent
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("mutator-"),
                "{view}"
            );
            assert!(
                !parent.exists(),
                "the pool is removed with the call: {view}"
            );
        }
        let parents: std::collections::BTreeSet<PathBuf> = counts
            .keys()
            .map(|view| Path::new(view).parent().unwrap().to_path_buf())
            .collect();
        assert_eq!(
            parents.len(),
            1,
            "one scratch root for the call: {parents:?}"
        );
        remove(&directory);
    }

    /// Acceptance: one coverage run and one pristine baseline per Lua module,
    /// shared by every target file of the call, instead of one of each per file.
    #[test]
    fn one_coverage_and_one_baseline_are_shared_by_the_lua_modules() {
        let (directory, root) = project("shared-runs");
        let marker = directory.join("spec-marker");
        mark_specs(
            &root,
            &marker,
            &[
                "spec/add_spec.lua",
                "spec/double unit_spec.lua",
                "nested/spec/mathx_spec.lua",
            ],
        );
        let run = run_with_arguments(&arguments_with(
            &root,
            2,
            &["--mutate-all"],
            &["src/calc.lua", "src/odd name.lua", "nested/lib/mathx.lua"],
        ));
        assert_eq!(run, ExitCode::from(3));
        let invocations = invocations(&marker);
        assert!(!invocations.is_empty(), "the specs marked their runs");
        let nested = |invocation: &Invocation| invocation.spec.starts_with("mathx_spec.lua");
        let count = |nested_module: bool, filter: &dyn Fn(&Invocation) -> bool| {
            invocations
                .iter()
                .filter(|invocation| nested(invocation) == nested_module)
                .filter(|invocation| filter(invocation))
                .count()
        };
        let mutant = |invocation: &Invocation| invocation.mutant();
        let coverage = |invocation: &Invocation| invocation.coverage;
        let pristine = |invocation: &Invocation| !invocation.mutant() && !invocation.coverage;
        assert!(count(false, &mutant) > 0, "the root module's mutants ran");
        assert!(count(true, &mutant) > 0, "the nested module's mutants ran");
        // The root module has two specs, so its fresh coverage is two shards;
        // the nested module has one spec and one shard.
        assert_eq!(count(false, &coverage), 2, "one coverage run per root spec");
        assert_eq!(count(true, &coverage), 1);
        // The fresh coverage run already proved the suite passes, so it seeds
        // the baseline (its duration is the timeout base) and no second
        // pristine run happens for either module, however many target files it
        // has. `without_a_fresh_coverage_run_the_pristine_baseline_runs` proves
        // the other side of that contract.
        assert_eq!(
            count(false, &pristine),
            0,
            "the coverage run is both root-module target files' baseline"
        );
        assert_eq!(count(true, &pristine), 0);
        remove(&directory);
    }

    /// Without fresh coverage there is no seed: the pristine baseline really
    /// runs once before the mutants. (This build does not persist a Lua spec
    /// index, so `--no-coverage` is the reused/no-fresh path.)
    #[test]
    fn without_a_fresh_coverage_run_the_pristine_baseline_runs() {
        let root = scratch("baseline", &[]);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("spec")).unwrap();
        std::fs::write(
            root.join(".busted"),
            "return { default = { lpath = 'src/?.lua' } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/calc.lua"),
            "local M = {}\n\nfunction M.add(x)\n  return x + 1\nend\n\nreturn M\n",
        )
        .unwrap();
        let marker = marker_for(&root);
        std::fs::write(
            root.join("spec/calc_spec.lua"),
            format!(
                "local calc = require('calc')\nlocal marker = io.open([[{marker}]], 'a')\nif marker and arg then\n  marker:write(tostring(package.loaded['luacov.runner'] ~= nil) .. '\\t' .. table.concat(arg or {{}}, ' ') .. '\\n')\n  marker:close()\nend\ndescribe('calc', function()\n  it('adds', function() assert.equal(2, calc.add(1)) end)\nend)\n",
                marker = marker.display()
            ),
        )
        .unwrap();
        let run = run_with_arguments(&arguments_with(
            &root,
            1,
            &["--no-coverage", "--mutate-all"],
            &["src/calc.lua"],
        ));
        assert_eq!(run, ExitCode::SUCCESS);
        let text = std::fs::read_to_string(&marker).unwrap();
        let pristine: Vec<&str> = text
            .lines()
            .filter(|line| !line.contains("--no-keep-going"))
            .collect();
        assert_eq!(
            pristine.len(),
            1,
            "exactly one pristine baseline before the mutants: {text}"
        );
        assert!(
            pristine[0].starts_with("false\t"),
            "the baseline is not the instrumented coverage run: {text}"
        );
        remove_scratch(&root);
    }

    /// A baseline failure stays with its file: the other Lua module's snapshot
    /// is still published, and the call exits 2.
    #[test]
    fn a_lua_baseline_failure_keeps_the_other_files_snapshot() {
        let root = scratch("baseline-failure", &[]);
        for path in ["src", "spec", "nested/src", "nested/spec"] {
            std::fs::create_dir_all(root.join(path)).unwrap();
        }
        std::fs::write(
            root.join(".busted"),
            "return { default = { lpath = 'src/?.lua' } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("nested/.busted"),
            "return { default = { lpath = 'src/?.lua' } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/a.lua"),
            "local M = {}\n\nfunction M.add(x)\n  return x + 1\nend\n\nreturn M\n",
        )
        .unwrap();
        std::fs::write(
            root.join("nested/src/b.lua"),
            "local M = {}\n\nfunction M.double(x)\n  return x * 2\nend\n\nreturn M\n",
        )
        .unwrap();
        std::fs::write(
            root.join("spec/a_spec.lua"),
            "local a = require('a')\ndescribe('a', function()\n  it('is broken', function() assert.equal(99, a.add(1)) end)\nend)\n",
        )
        .unwrap();
        std::fs::write(
            root.join("nested/spec/b_spec.lua"),
            "local b = require('b')\ndescribe('b', function()\n  it('doubles', function() assert.equal(4, b.double(2)) end)\nend)\n",
        )
        .unwrap();
        let run = run_with_arguments(&arguments_with(
            &root,
            2,
            &["--no-coverage", "--mutate-all"],
            &["src/a.lua", "nested/src/b.lua"],
        ));
        assert_eq!(run, ExitCode::from(2), "a baseline failure exits 2");
        assert!(
            !root.join(".metrics/mutate/a.edn").exists(),
            "a failed baseline publishes nothing"
        );
        let text = std::fs::read_to_string(root.join(".metrics/mutate/nested.src.b.edn"))
            .expect("the healthy file still publishes");
        assert!(text.contains(":killed 1"), "{text}");
        remove_scratch(&root);
    }

    /// Ported from the original `test_engine_edit_during_the_queue_keeps_it_and_
    /// the_other_snapshots`: something outside the run — here this process, as
    /// the original's editing runner — edits or deletes a source while one of
    /// its mutants is held in a view. That file publishes nothing and the call
    /// exits 4, the other file still publishes, and the edit is never undone.
    #[test]
    fn a_source_edited_during_the_queue_keeps_the_other_files_snapshot() {
        for (case, delete) in [("edited", false), ("deleted", true)] {
            let root = scratch(&format!("source-change-{case}"), &[]);
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::create_dir_all(root.join("spec")).unwrap();
            std::fs::write(
                root.join(".busted"),
                "return { default = { lpath = 'src/?.lua' } }\n",
            )
            .unwrap();
            let a = root.join("src/a.lua");
            let original = "local M = {}\n\nfunction M.add(x)\n  return x + 1\nend\n\nreturn M\n";
            std::fs::write(&a, original).unwrap();
            std::fs::write(
                root.join("src/b.lua"),
                "local M = {}\n\nfunction M.triple(x)\n  return x * 3\nend\n\nreturn M\n",
            )
            .unwrap();
            // A write made inside a view only reaches that view's overlay, so
            // the change has to come from outside the run. The spec asks for it
            // only when the view holds the `a.lua` mutant and then waits for
            // it, so the edit is certain to land before that file's last
            // mutant finishes and its snapshot is published.
            let request = marker_for(&root);
            let acknowledgement = request.with_extension("ack");
            let _ = std::fs::remove_file(&request);
            let _ = std::fs::remove_file(&acknowledgement);
            std::fs::write(
                root.join("spec/ab_spec.lua"),
                format!(
                    concat!(
                        "local view = os.getenv('MUTATOR_VIEW')\n",
                        "local held = view and io.open(view .. '/upper/src/a.lua')\n",
                        "if held then\n",
                        "  held:close()\n",
                        "  local request = io.open([[{request}]], 'a')\n",
                        "  if request then request:write('change a.lua\\n') request:close() end\n",
                        "  local waited, done = 0, false\n",
                        "  while not done and waited < 2000 do\n",
                        "    local ack = io.open([[{acknowledgement}]])\n",
                        "    if ack then\n",
                        "      ack:close()\n",
                        "      done = true\n",
                        "    else\n",
                        "      waited = waited + 1\n",
                        "      os.execute('sleep 0.01')\n",
                        "    end\n",
                        "  end\n",
                        "  if not done then error('the test never changed the source') end\n",
                        "end\n",
                        "local a = require('a')\n",
                        "local b = require('b')\n",
                        "describe('both', function()\n",
                        "  it('adds', function() assert.equal(2, a.add(1)) end)\n",
                        "  it('triples', function() assert.equal(6, b.triple(2)) end)\n",
                        "end)\n",
                    ),
                    request = request.display(),
                    acknowledgement = acknowledgement.display(),
                ),
            )
            .unwrap();
            let edited = format!("{original}-- edited by the test\n");
            let watcher = {
                let request = request.clone();
                let acknowledgement = acknowledgement.clone();
                let target = a.clone();
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        if request.exists() {
                            if delete {
                                std::fs::remove_file(&target).unwrap();
                            } else {
                                std::fs::write(&target, &edited).unwrap();
                            }
                            std::fs::write(&acknowledgement, "changed\n").unwrap();
                            return true;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    false
                })
            };
            let run = run_with_arguments(&arguments_with(
                &root,
                1,
                &["--no-coverage", "--mutate-all"],
                &["src/a.lua", "src/b.lua"],
            ));
            assert!(
                watcher.join().unwrap(),
                "{case}: the run really held a.lua's mutant"
            );
            let _ = std::fs::remove_file(&request);
            let _ = std::fs::remove_file(&acknowledgement);
            assert_eq!(run, ExitCode::from(4), "{case}");
            assert!(
                !root.join(".metrics/mutate/a.edn").exists(),
                "{case}: an edited source publishes no snapshot"
            );
            assert!(
                root.join(".metrics/mutate/b.edn").is_file(),
                "{case}: the untouched file still publishes"
            );
            if delete {
                assert!(!a.exists(), "{case}: the deletion is kept, not undone");
            } else {
                assert!(
                    std::fs::read_to_string(&a)
                        .unwrap()
                        .contains("edited by the test"),
                    "{case}: the edit is kept, not undone"
                );
            }
            remove_scratch(&root);
        }
    }

    /// Ported from the original `test_lua_shared_state.py`: one spec caches a
    /// result another spec asserts, so the covering spec on its own cannot
    /// judge a mutant. `--select-specs` runs that spec, leaves the mutant
    /// surviving, and survivor confirmation re-runs the full suite and kills
    /// it, so the outcomes and the published bytes stay the full suite's.
    #[test]
    fn shared_state_keeps_full_suite_kills_under_spec_selection() {
        let root = scratch("shared-state", &[]);
        std::fs::create_dir_all(root.join("spec")).unwrap();
        std::fs::write(
            root.join(".busted"),
            "return { default = { lpath = './?.lua;' .. package.path, helper = 'helper.lua', sort = true } }\n",
        )
        .unwrap();
        // The cached result the asserting spec expects, so that spec also
        // passes when it is the only spec of an instrumented coverage shard.
        std::fs::write(
            root.join("helper.lua"),
            "package.loaded['shared'] = {value = 11}\n",
        )
        .unwrap();
        let source = "local shared = require('shared')\nlocal M = {}\nfunction M.first(x)\n  shared.value = x + 1\n  return shared.value\nend\nreturn M\n";
        std::fs::write(root.join("calc.lua"), source).unwrap();
        // Only the preparing spec runs the mutated line; the asserting spec
        // never calls it.
        std::fs::write(
            root.join("spec/a_spec.lua"),
            "local calc = require('calc')\nlocal shared = require('shared')\nit('prepares a cached result', function() shared.value = calc.first(10) end)\n",
        )
        .unwrap();
        std::fs::write(
            root.join("spec/b_spec.lua"),
            "local shared = require('shared')\nit('checks the cached result', function() assert.equal(11, shared.value) end)\n",
        )
        .unwrap();
        let marker = marker_for(&root);
        mark_specs(&root, &marker, &["spec/a_spec.lua", "spec/b_spec.lua"]);
        let snapshot = root.join(".metrics/mutate/calc.edn");
        let full = run_with_arguments(&arguments_with(
            &root,
            1,
            &["--no-coverage", "--mutate-all"],
            &["calc.lua"],
        ));
        assert_eq!(full, ExitCode::SUCCESS, "the full suite kills the mutant");
        let expected = std::fs::read(&snapshot).unwrap();
        let text = String::from_utf8_lossy(&expected).into_owned();
        // Both sites of the function — the operator and the literal — are
        // judged by the asserting spec, not by the covering one.
        assert!(text.contains(":killed 2 :survived 0"), "{text}");
        // The premise of the case: the preparing spec really runs first.
        let marked = invocations(&marker);
        let order: Vec<&str> = marked
            .iter()
            .map(|invocation| invocation.spec.as_str())
            .collect();
        assert!(!order.is_empty());
        assert!(
            order
                .chunks(2)
                .all(|pair| pair == ["a_spec.lua", "b_spec.lua"]),
            "the cached result only exists when a_spec runs first: {order:?}"
        );
        let _ = std::fs::remove_file(&marker);
        let selected = run_with_arguments(&arguments_with(
            &root,
            1,
            &["--mutate-all", "--select-specs"],
            &["calc.lua"],
        ));
        assert_eq!(selected, ExitCode::SUCCESS, "confirmation keeps the kill");
        assert_eq!(
            std::fs::read(&snapshot).unwrap(),
            expected,
            "spec selection publishes the full suite's outcome"
        );
        let marked = invocations(&marker);
        let mutants: Vec<&Invocation> = marked
            .iter()
            .filter(|invocation| invocation.mutant())
            .collect();
        assert!(!mutants.is_empty(), "the selected route really ran mutants");
        let (confirmation, earlier) = mutants.split_last().unwrap();
        assert!(
            !earlier.is_empty()
                && earlier
                    .iter()
                    .all(|invocation| !invocation.args.iter().any(|arg| arg == "spec/b_spec.lua")),
            "the covering spec ran alone before the confirmation: {:?}",
            mutants
                .iter()
                .map(|invocation| &invocation.args)
                .collect::<Vec<_>>()
        );
        assert!(
            !confirmation.args.iter().any(|arg| arg.starts_with("spec/")),
            "the confirmation ran the whole suite, not a subset: {:?}",
            confirmation.args
        );
        // The confirmation really executed both specs, the asserting one last:
        // Busted's default suite order names no spec files at all.
        let tail: Vec<&str> = marked
            .iter()
            .rev()
            .take(2)
            .map(|invocation| invocation.spec.as_str())
            .collect();
        assert_eq!(
            tail,
            vec!["b_spec.lua", "a_spec.lua"],
            "the last run was the full suite: {:?}",
            marked
                .iter()
                .map(|invocation| invocation.spec.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("calc.lua")).unwrap(),
            source
        );
        let _ = std::fs::remove_file(&marker);
        remove_scratch(&root);
    }

    /// The other half of the original's shared-state pair: selection assumes
    /// independent specs, so with shared state the subset can kill a mutant the
    /// full suite lets survive. Selection is opt-in (`--select-specs`), which
    /// is the documented caveat; the full-suite default keeps its verdict.
    #[test]
    fn spec_selection_can_kill_a_full_suite_survivor_with_shared_state() {
        let root = scratch("shared-caveat", &[]);
        std::fs::create_dir_all(root.join("spec")).unwrap();
        std::fs::write(
            root.join(".busted"),
            "return { default = { lpath = './?.lua;' .. package.path, helper = 'helper.lua', sort = true } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("helper.lua"),
            "package.loaded['shared'] = {offset = 1}\n",
        )
        .unwrap();
        let source = "local shared = require('shared')\nlocal M = {}\nfunction M.first(x)\n  return x + shared.offset\nend\nreturn M\n";
        std::fs::write(root.join("calc.lua"), source).unwrap();
        std::fs::write(
            root.join("spec/a_spec.lua"),
            "local shared = require('shared')\nit('prepares shared state', function() shared.offset = 0 end)\n",
        )
        .unwrap();
        std::fs::write(
            root.join("spec/b_spec.lua"),
            "local shared = require('shared')\nlocal calc = require('calc')\nit('checks the calculation', function()\n  assert.equal(10 + shared.offset, calc.first(10))\nend)\n",
        )
        .unwrap();
        let marker = marker_for(&root);
        mark_specs(&root, &marker, &["spec/a_spec.lua", "spec/b_spec.lua"]);
        let snapshot = root.join(".metrics/mutate/calc.edn");
        let full = run_with_arguments(&arguments_with(
            &root,
            1,
            &["--no-coverage", "--mutate-all"],
            &["calc.lua"],
        ));
        assert_eq!(full, ExitCode::from(3), "the full suite leaves a survivor");
        let text = std::fs::read_to_string(&snapshot).unwrap();
        assert!(text.contains(":killed 0 :survived 1"), "{text}");
        // The premise of the case: the preparing spec really runs first.
        let marked = invocations(&marker);
        let order: Vec<&str> = marked
            .iter()
            .map(|invocation| invocation.spec.as_str())
            .collect();
        assert!(!order.is_empty());
        assert!(
            order
                .chunks(2)
                .all(|pair| pair == ["a_spec.lua", "b_spec.lua"]),
            "the shared state only differs when a_spec runs first: {order:?}"
        );
        let _ = std::fs::remove_file(&marker);
        let selected = run_with_arguments(&arguments_with(
            &root,
            1,
            &["--mutate-all", "--select-specs"],
            &["calc.lua"],
        ));
        assert_eq!(selected, ExitCode::SUCCESS, "the covering spec kills it");
        let text = std::fs::read_to_string(&snapshot).unwrap();
        assert!(text.contains(":killed 1 :survived 0"), "{text}");
        let mutants: Vec<Invocation> = invocations(&marker)
            .into_iter()
            .filter(|invocation| invocation.mutant())
            .collect();
        assert!(
            !mutants.is_empty(),
            "the selected route really ran the mutant"
        );
        assert!(
            mutants
                .iter()
                .all(|invocation| !invocation.args.iter().any(|arg| arg == "spec/a_spec.lua")),
            "the subset ran the covering spec alone, with no full-suite confirmation: {:?}",
            mutants
                .iter()
                .map(|invocation| &invocation.args)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("calc.lua")).unwrap(),
            source
        );
        let _ = std::fs::remove_file(&marker);
        remove_scratch(&root);
    }

    /// Ported from the original `test_busted_task.py`: the default task's
    /// failing slow spec stops the run before any mutant, while the named task
    /// excludes it and publishes survivors; the plain rerun then kills them.
    #[test]
    fn a_named_busted_task_excludes_the_failing_slow_spec() {
        let root = scratch("busted task", &[]);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("spec")).unwrap();
        std::fs::write(
            root.join(".busted"),
            "return { default = { lpath = 'src/?.lua', ROOT = {'spec'} },\n fast = { ['exclude-tags'] = 'slow' } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/calc.lua"),
            "local M = {}\nfunction M.add(x)\n  return x + 1\nend\nreturn M\n",
        )
        .unwrap();
        let spec = root.join("spec/calc_spec.lua");
        let failing = "local calc = require('calc')\ndescribe('calc', function()\n  it('returns a number', function()\n    assert.are.equal('number', type(calc.add(2)))\n  end)\n  it('adds accurately #slow', function()\n    error('slow test was executed')\n  end)\nend)\n";
        let passing = failing.replace(
            "error('slow test was executed')",
            "assert.are.equal(3, calc.add(2))",
        );
        std::fs::write(&spec, failing).unwrap();
        let run = run_with_arguments(&arguments_with(
            &root,
            1,
            &["--no-coverage", "--mutate-all"],
            &["src/calc.lua"],
        ));
        assert_eq!(run, ExitCode::from(2), "a failing baseline exits 2");
        assert!(!root.join(".metrics/mutate").exists());
        let run = run_with_arguments(&arguments_with(
            &root,
            2,
            &["--no-coverage", "--mutate-all", "--busted-task", "fast"],
            &["src/calc.lua"],
        ));
        assert_eq!(run, ExitCode::from(3), "the fast task leaves survivors");
        let text = std::fs::read_to_string(root.join(".metrics/mutate/calc.edn")).unwrap();
        assert!(text.contains(":killed 0 :survived 2"), "{text}");
        std::fs::write(&spec, passing).unwrap();
        let run = run_with_arguments(&arguments_with(
            &root,
            2,
            &["--no-coverage", "--mutate-all"],
            &["src/calc.lua"],
        ));
        assert_eq!(run, ExitCode::SUCCESS, "the full task kills both");
        let text = std::fs::read_to_string(root.join(".metrics/mutate/calc.edn")).unwrap();
        assert!(text.contains(":killed 2 :survived 0"), "{text}");
        remove_scratch(&root);
    }

    /// The publication guard holds for Lua snapshots too: a symlinked
    /// `.metrics/mutate` is refused (exit 2) instead of written or deleted
    /// through.
    #[test]
    fn a_symlinked_snapshot_directory_is_refused_for_lua() {
        let root = scratch("symlink", &[]);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("spec")).unwrap();
        std::fs::write(
            root.join(".busted"),
            "return { default = { lpath = 'src/?.lua' } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/calc.lua"),
            "local M = {}\n\nfunction M.add(x)\n  return x + 1\nend\n\nreturn M\n",
        )
        .unwrap();
        std::fs::write(
            root.join("spec/calc_spec.lua"),
            "local calc = require('calc')\ndescribe('calc', function()\n  it('adds', function() assert.equal(2, calc.add(1)) end)\nend)\n",
        )
        .unwrap();
        let victim = std::env::temp_dir().join(format!("lua-queue-victim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&victim);
        std::fs::create_dir_all(victim.join("empty")).unwrap();
        std::fs::write(victim.join("keep.txt"), "keep\n").unwrap();
        std::fs::create_dir_all(root.join(".metrics")).unwrap();
        std::os::unix::fs::symlink(&victim, root.join(".metrics/mutate")).unwrap();
        let run = run_with_arguments(&arguments_with(
            &root,
            1,
            &["--no-coverage", "--mutate-all"],
            &["src/calc.lua"],
        ));
        assert_eq!(run, ExitCode::from(2));
        assert!(victim.join("empty").is_dir());
        assert_eq!(
            std::fs::read_to_string(victim.join("keep.txt")).unwrap(),
            "keep\n"
        );
        remove_scratch(&root);
        std::fs::remove_dir_all(&victim).unwrap();
    }
}
