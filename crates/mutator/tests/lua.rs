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
        let (found, forms) = sites::discover(&source, &path, &root, name);
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

    fn run(args: &[&str]) -> ExitCode {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        mutator::run_with_clock(&args, fixed_clock)
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
}
