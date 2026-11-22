#![cfg(any(target_os = "linux", windows))]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn lua_project(name: &str) -> PathBuf {
    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &to.join(entry.file_name()));
            } else {
                fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutate/lua/project");
    let directory = std::env::temp_dir().join(format!(
        "mutate-lua-entry-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&directory);
    copy(&source, &directory.join("project"));
    directory.join("project")
}

fn remove_lua_project(root: &Path) {
    let directory = root.parent().unwrap();
    assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
    let name = directory.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("mutate-lua-entry-"), "{name}");
    fs::remove_dir_all(directory).unwrap();
}

fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mutate/lua/goldens")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

const TOOLCHAIN: &str = "assert(_VERSION == 'Lua 5.4', _VERSION); pcall(require, 'luarocks.loader'); assert(require('cluacov.version') == '1.0.0'); assert(require('busted.runner')); assert(require('luacov.reporter.lcov'))";

fn missing_lua_toolchain() -> Option<String> {
    for program in ["lua5.4", "lua"] {
        let loads = std::process::Command::new(program)
            .arg("-e")
            .arg(TOOLCHAIN)
            .output()
            .is_ok_and(|output| output.status.success());
        if loads {
            return None;
        }
    }
    Some(format!(
        "no Lua 5.4 with Busted, LuaCov, cluacov 1.0.0 and luacov-reporter-lcov on PATH: {} failed",
        TOOLCHAIN
    ))
}

fn skip(reason: &str) {
    assert!(
        !std::env::var_os("MUTATOR_REQUIRE_TOOLCHAINS").is_some_and(|value| value == "1"),
        "MUTATOR_REQUIRE_TOOLCHAINS=1; {reason}"
    );
    use std::io::Write;
    let _ = std::io::stderr().write_all(format!("SKIP: {reason}\n").as_bytes());
}

struct Run {
    code: Option<i32>,
    stderr: String,
}

fn run(root: &Path, flags: &[&str], environment: &[(&str, &str)]) -> Run {
    let mut args = vec![
        "mutate".to_string(),
        "--root".to_string(),
        root.to_string_lossy().into_owned(),
        "--max-workers".to_string(),
        "1".to_string(),
        "--timeout-factor".to_string(),
        "20".to_string(),
    ];
    args.extend(flags.iter().map(|flag| (*flag).to_string()));
    args.extend(["--mutate-all".to_string(), "src/calc.lua".to_string()]);
    let output = crate::binary_env(args, environment);
    Run {
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn busted_runs(stderr: &str) -> usize {
    stderr
        .lines()
        .filter(|line| {
            line.contains("success")
                && line.contains(" failures / ")
                && line.contains(" errors / ")
                && line.contains(" pending")
        })
        .count()
}

fn shard_launches(stderr: &str, count: usize) -> usize {
    stderr
        .lines()
        .filter(|line| line.contains(&format!("Busted in {count} coverage shards:")))
        .count()
}

fn never_entered(stderr: &str) -> String {
    if let Some(line) = stderr
        .lines()
        .find(|line| line.contains("Lua tests exited"))
    {
        return format!(
            "the collection command died before Busted reported anything ({}); \
             the collector, its scratch or the suite's bootstrap was unavailable, \
             which is not a test verdict",
            line.trim()
        );
    }
    if let Some(line) = stderr.lines().find(|line| {
        line.contains("Cannot prepare Lua")
            || line.contains("Invalid Lua spec coverage")
            || line.contains("Busted did not complete any spec files")
    }) {
        return format!(
            "the collection could not prepare, publish or read its scratch ({})",
            line.trim()
        );
    }
    "Busted never printed a summary; the run stopped before the suite".into()
}

fn assert_baseline_passed(stderr: &str, shards: usize) {
    let passing = |prefix: &str| {
        stderr
            .lines()
            .filter(|line| line.starts_with(prefix))
            .count()
    };
    if shards > 1 {
        let seen = passing("1 success / 0 failures");
        assert!(
            seen >= shards,
            "{shards} shards each run one passing spec file, saw {seen}:\n{stderr}"
        );
    } else {
        assert!(
            passing("2 successes / 0 failures") >= 1,
            "one shard runs both spec files in one Busted process, and it passes:\n{stderr}"
        );
    }
}

fn assert_entered(stderr: &str) {
    assert!(
        busted_runs(stderr) > 0,
        "the Lua mutation never entered the Busted suite, so no mutant was judged: {}\n--- stderr ---\n{stderr}",
        never_entered(stderr)
    );
}

fn outcomes(snapshot: &str) -> BTreeMap<String, String> {
    const OPEN: &str = ":outcomes {";
    let start = snapshot
        .find(OPEN)
        .expect("a snapshot carries an :outcomes map")
        + OPEN.len();
    let rest = &snapshot[start..];
    let end = rest.find('}').expect("the :outcomes map closes");
    let mut found = BTreeMap::new();
    let mut pieces = rest[..end].split('"');
    let _ = pieces.next();
    while let Some(key) = pieces.next() {
        let verdict = pieces.next().unwrap_or_default().trim();
        found.insert(key.to_string(), verdict.trim_start_matches(':').to_string());
    }
    found
}

fn assert_judged(root: &Path, run: &Run) {
    let path = root.join(".metrics/mutate/calc.edn");
    let snapshot = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "no mutation snapshot at {}: {error}\n--- stderr ---\n{}",
            path.display(),
            run.stderr
        )
    });
    assert!(
        snapshot.contains(":namespace \"calc\""),
        "the snapshot is not calc.lua's: {snapshot}"
    );
    let expected = outcomes(&golden("calc.edn"));
    assert!(!expected.is_empty(), "the golden carries no outcomes");
    let actual = outcomes(&snapshot);
    assert_eq!(
        actual, expected,
        "the judged mutants are not the golden's\n--- stderr ---\n{}",
        run.stderr
    );
    let survivors = actual
        .values()
        .filter(|verdict| *verdict == "survived")
        .count();
    let code = if survivors == 0 { 0 } else { 3 };
    assert_eq!(
        run.code,
        Some(code),
        "{survivors} survivors among {actual:?}\n--- stderr ---\n{}",
        run.stderr
    );
}

fn default_shards() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .clamp(1, 2)
}

#[test]
fn lua_mutation_enters_the_suite_in_one_coverage_shard() {
    if let Some(reason) = missing_lua_toolchain() {
        return skip(&reason);
    }
    let root = lua_project("one-shard");
    let run = run(&root, &[], &[("MUTATOR_LUA_SHARDS", "1")]);
    assert_entered(&run.stderr);
    assert_baseline_passed(&run.stderr, 1);
    assert!(
        !run.stderr.contains("coverage shards:"),
        "one shard runs the whole suite in one command; no shard is launched:\n{}",
        run.stderr
    );
    assert_judged(&root, &run);
    remove_lua_project(&root);
}

#[test]
fn lua_mutation_enters_the_suite_in_the_default_coverage_shards() {
    if let Some(reason) = missing_lua_toolchain() {
        return skip(&reason);
    }
    let expected = default_shards();
    let root = lua_project("default-shards");
    let run = run(&root, &[], &[("MUTATOR_LUA_SHARDS", "")]);
    assert_entered(&run.stderr);
    assert_baseline_passed(&run.stderr, expected);
    if expected > 1 {
        assert_eq!(
            shard_launches(&run.stderr, expected),
            expected,
            "the default count is {expected}, so {expected} Busted shards really launched:\n{}",
            run.stderr
        );
    } else {
        assert!(
            !run.stderr.contains("coverage shards:"),
            "one shard launches no shard of its own:\n{}",
            run.stderr
        );
    }
    assert_judged(&root, &run);
    remove_lua_project(&root);
}

#[test]
fn reuse_coverage_still_skips_the_lua_collection() {
    if let Some(reason) = missing_lua_toolchain() {
        return skip(&reason);
    }
    let root = lua_project("reuse");
    let run = run(&root, &["--reuse-coverage"], &[("MUTATOR_LUA_SHARDS", "1")]);
    assert_eq!(
        busted_runs(&run.stderr),
        0,
        "--reuse-coverage ran the suite:\n{}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("coverage shards:"),
        "--reuse-coverage launched a coverage shard:\n{}",
        run.stderr
    );
    assert!(
        matches!(run.code, Some(0) | Some(3)),
        "exit {:?}:\n{}",
        run.code,
        run.stderr
    );
    remove_lua_project(&root);
}
