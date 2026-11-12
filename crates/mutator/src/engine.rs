use crate::{
    cli::Options,
    clojure, golang, interrupts,
    isolation::{self, Failure, View},
    java,
    lua::{self, Index},
    metrics::{self, Decision},
    python,
    sites::{self, Form, Site},
    typescript,
    workers::{self, Workers},
};
use quality_core::{
    coverage::{Records, parse_lcov},
    grammar::read_source,
    language::{Language, language_of},
    paths::resolve,
    runner::{modules_with, nearest},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

/// The original `--timings` totals: one line per phase in first-run order.
#[derive(Default)]
pub struct Timings {
    phases: Vec<(String, f64, usize)>,
}
impl Timings {
    pub fn add(&mut self, phase: &str, elapsed: Duration) {
        let seconds = elapsed.as_secs_f64();
        if let Some(entry) = self.phases.iter_mut().find(|(name, _, _)| name == phase) {
            entry.1 += seconds;
            entry.2 += 1;
        } else {
            self.phases.push((phase.to_string(), seconds, 1));
        }
    }
    pub fn summary(&self) -> String {
        self.phases
            .iter()
            .map(|(name, seconds, count)| {
                format!(
                    "timing {name}: {seconds:.1}s total, {count} run(s), {:.2}s each\n",
                    seconds / *count as f64
                )
            })
            .collect()
    }
}

/// A test command: the original's shell string, or the Lua suite's argv, so a
/// spec file name with a space or a quote stays one argument.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Command {
    Shell(String),
    Argv(Vec<String>),
}
impl Command {
    /// The command as the original echoes it.
    fn display(&self) -> String {
        match self {
            Command::Shell(command) => command.clone(),
            Command::Argv(argv) => argv
                .iter()
                .map(|argument| {
                    if !argument.is_empty()
                        && argument
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || "-_./=:@,+".contains(ch))
                    {
                        argument.clone()
                    } else {
                        format!("'{}'", argument.replace('\'', "'\\''"))
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
    /// Whether this is the generated `cargo test` whose compile step runs
    /// untimed before the baseline (the original's `build_command`).
    fn is_cargo_test(&self) -> bool {
        match self {
            Command::Shell(command) => command == "cargo test",
            Command::Argv(argv) => argv.starts_with(&["cargo".to_string(), "test".to_string()]),
        }
    }
    /// `[*command, *tail]` for both command shapes.
    fn extend(&self, tail: &[String]) -> Self {
        match self {
            Command::Shell(command) => {
                let mut joined = command.clone();
                for argument in tail {
                    joined.push(' ');
                    joined.push_str(argument);
                }
                Command::Shell(joined)
            }
            Command::Argv(argv) => {
                let mut extended = argv.clone();
                extended.extend(tail.iter().cloned());
                Command::Argv(extended)
            }
        }
    }
}

/// One baseline run's outcome: exit code, duration and captured output, keyed
/// by the command and directory it ran in.
type Baselines = BTreeMap<(Command, PathBuf), Result<(i32, Duration, String), Failure>>;

pub struct Prepared {
    pub file: PathBuf,
    pub key: String,
    pub raw: Vec<u8>,
    pub source: String,
    pub sites: Vec<Site>,
    pub forms: Vec<Form>,
    pub decisions: BTreeMap<String, Decision>,
    pub outcomes: BTreeMap<String, String>,
    pub covered: BTreeMap<String, bool>,
    /// The module's Lua spec index, when this file has one.
    pub index: Option<Index>,
}

/// The test command and the directory it runs in.
fn plan(root: &Path, options: &Options, source: &Path) -> (Command, PathBuf) {
    if let Some(command) = options.value("--test-command") {
        return (Command::Shell(command.to_string()), root.to_path_buf());
    }
    if language_of(source) == Some(Language::Lua) {
        let module = lua_module(root, source);
        return (
            Command::Argv(lua::plan(&module, options.value("--busted-task"))),
            module,
        );
    }
    // A generated Python or TypeScript plan is an argument vector too, so a
    // package path stays one argument.
    let planned = match language_of(source) {
        Some(Language::Python) => Some(python::test_plan(root, source)),
        Some(Language::TypeScript) => Some(typescript::test_plan(root, source)),
        Some(Language::Go) => Some(golang::test_plan(root, source)),
        Some(Language::Java) => Some(java::test_plan(root, source)),
        Some(Language::Clojure) => Some(clojure::test_plan(root, source)),
        _ => None,
    };
    if let Some((argv, directory)) = planned {
        return (Command::Argv(argv), directory);
    }
    (
        Command::Shell("cargo test".into()),
        nearest(source, "Cargo.toml", root).unwrap_or_else(|| root.to_path_buf()),
    )
}

/// The Lua module directory a source belongs to: the nearest ancestor with a
/// `.busted` file or a rockspec, or the project root.
fn lua_module(root: &Path, source: &Path) -> PathBuf {
    quality_core::runners::lua::modules(root, &[source.to_path_buf()])
        .into_keys()
        .next()
        .unwrap_or_else(|| root.to_path_buf())
}

fn execute(
    view: &View,
    root: &Path,
    cwd: &Path,
    command: &Command,
    site: Option<(&Path, &str)>,
    timeout: Option<Duration>,
) -> Result<(i32, Duration, String), Failure> {
    let run = match command {
        Command::Shell(text) => view.run(root, cwd, text, site, timeout),
        Command::Argv(argv) => view.run_argv(root, cwd, argv, site, timeout),
    };
    let result = run.map_err(|failure| match failure {
        Failure::Interrupted => Failure::Interrupted,
        Failure::Unavailable(text) => Failure::Unavailable(text),
        Failure::Message(error) => {
            Failure::Message(format!("Cannot enter the mutation isolation view: {error}"))
        }
    })?;
    if !result.output.is_empty() {
        eprint!("{}", result.output);
    }
    Ok((result.code, result.elapsed, result.output))
}

/// `os.path.relpath`: `path` as seen from `base`, both absolute.
///
/// The generated commands of a copied project use it to spell a path relative
/// to their own directory, never the real project's absolute path.
pub(crate) fn relative_from(base: &Path, path: &Path) -> Option<PathBuf> {
    let base: Vec<_> = base.components().collect();
    let path: Vec<_> = path.components().collect();
    let common = base
        .iter()
        .zip(&path)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in common..base.len() {
        relative.push("..");
    }
    relative.extend(path[common..].iter());
    Some(relative)
}

/// A package as a generated Windows script's diagnostic names it: relative to
/// the project root, so the copy never prints the real project's absolute path,
/// and quoted for cmd.
///
/// cmd parses an `echo`'s text before it runs the line: an unquoted `&` ends the
/// `echo` and runs the rest as a command, and an unquoted `(`/`)` makes cmd
/// reject the whole `/c` line, so the script never reaches its own coverage
/// command and the report silently goes missing. The quotes do not stop
/// `%VAR%` expansion; a project path containing a percent sign would still
/// expand, quoted or not.
pub(crate) fn diagnostic_label(root: &Path, package: &Path) -> String {
    let root = resolve(root);
    let package = resolve(package);
    let relative = relative_from(&root, &package).unwrap_or(package);
    let label = if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    };
    quality_core::process::cmd_quote(&label.display().to_string())
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn collect_reports(directory: &Path, records: &mut Records) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_reports(&path, records);
        } else if path.file_name().is_some_and(|name| name == "lcov.info")
            && let Ok(text) = fs::read_to_string(path)
        {
            records.update(parse_lcov(&text));
        }
    }
}

/// crapper's LCOV reports for the languages its stamp calls fresh.
///
/// Only a fresh language may be read from disk: a stale language's report is
/// an earlier run's result, and reading it would let an old `lcov.info` decide
/// this run's coverage when the regeneration silently wrote nothing. Lua is
/// never gated — the stamp does not speak for it; its own instrumented suite
/// ran in this run.
fn collect_crapper_reports(directory: &Path, fresh: &[&str], records: &mut Records) {
    let mut crapper = Records::default();
    collect_reports(directory, &mut crapper);
    for (source, lines) in crapper.iter() {
        let reusable = language_of(Path::new(source))
            .is_some_and(|language| language == Language::Lua || fresh.contains(&language.name()));
        if reusable {
            records.insert(source.clone(), lines.clone());
        }
    }
}

/// Everything one run's selection reads: the LCOV records of the languages
/// that report through LCOV, and the Lua spec index per module.
#[derive(Default)]
struct Coverage {
    records: Records,
    indexes: BTreeMap<PathBuf, Index>,
    /// Go, Clojure and the LCOV of every language, as `load_bundle` reads the
    /// reports the views produced.
    native: Option<quality_core::native_coverage::NativeReports>,
    /// JaCoCo's covered source lines per report key (`package/File.java`).
    java: Option<Vec<(String, BTreeSet<usize>)>>,
    /// Fresh module reports have authority only over that module's sources.
    modules: Vec<ModuleCoverage>,
}

struct ModuleCoverage {
    root: PathBuf,
    language: Language,
    native: quality_core::native_coverage::NativeReports,
    java: Vec<(String, BTreeSet<usize>)>,
}

fn coverage(root: &Path, options: &Options, files: &[PathBuf]) -> Result<Coverage, i32> {
    let mut coverage = Coverage::default();
    let reuse = options.flag("--use-existing-coverage") || options.flag("--reuse-coverage");
    let standard =
        options.value("--test-command").is_none() && options.value("--coverage-command").is_none();
    if standard && !reuse {
        // Lua suites report through their own instrumented collection.
        let lua_files: Vec<PathBuf> = files
            .iter()
            .filter(|file| language_of(file) == Some(Language::Lua))
            .cloned()
            .collect();
        for (module, sources) in quality_core::runners::lua::modules(root, &lua_files) {
            let started = std::time::Instant::now();
            let view = View::new(root).map_err(|error| {
                eprintln!("Cannot create a mutation view: {error}");
                1
            })?;
            let index = lua::collect(
                &view,
                &lua::Request {
                    root,
                    module: &module,
                    sources: &sources,
                    busted_task: options.value("--busted-task"),
                    shards: None,
                },
            )
            .map_err(|error| {
                eprintln!("{error}");
                2
            })?;
            eprintln!(
                "timing lua-coverage: {:.1}s",
                started.elapsed().as_secs_f64()
            );
            coverage.indexes.insert(module, index);
        }
    }
    if reuse {
        collect_reports(&root.join("coverage"), &mut coverage.records);
        collect_reports(&root.join("target/coverage"), &mut coverage.records);
        collect_native(root, files, None, &mut coverage)?;
        return Ok(coverage);
    }
    if !standard {
        let command = options.value("--coverage-command");
        let Some(command) = command else {
            // Only --test-command was given: the original still measures the
            // languages crapper measures, so Python and TypeScript do, and
            // crapper's stamp decides which of them are regenerated.
            let (stale, fresh) = stale_coverage_files(root, files, None);
            report_reused(&fresh);
            collect_crapper_reports(&root.join("coverage"), &fresh, &mut coverage.records);
            collect_crapper_reports(&root.join("target/coverage"), &fresh, &mut coverage.records);
            python_coverage(root, &stale, &mut coverage.records)?;
            typescript_coverage(root, &stale, &mut coverage.records)?;
            collect_native(root, files, Some(&stale), &mut coverage)?;
            return Ok(coverage);
        };
        // Stale reports are removed before the command, in the host shell's
        // own spelling; a fresh copy has an empty coverage directory anyway.
        let removal = if cfg!(windows) {
            "del /f /q coverage\\lcov.info coverage\\luacov.stats.out luacov.stats.out 2>nul & "
        } else {
            "rm -f coverage/lcov.info coverage/luacov.stats.out luacov.stats.out\n"
        };
        let job = (
            root.to_path_buf(),
            format!("{removal}{command}"),
            PathBuf::from("coverage/lcov.info"),
        );
        let view = View::new(root).map_err(|error| {
            eprintln!("{error}");
            2
        })?;
        let result = view
            .run(root, &job.0, &job.1, None, None)
            .map_err(|error| {
                eprintln!("Isolation unavailable: {error}");
                1
            })?;
        eprint!("{}", result.output);
        if result.code != 0 {
            eprintln!("Coverage command exited {}.", result.code);
            return Err(2);
        }
        let report = isolation::report_path(&view, &job.2);
        let fresh = quality_core::coverage::read_fresh_lcov(&report, root).map_err(|error| {
            eprintln!("Required LCOV report invalid or unavailable: {error}");
            2
        })?;
        coverage.records.update(fresh);
        collect_custom_native(root, files, &mut coverage, &view)?;
        return Ok(coverage);
    }
    // The original's runner order: TypeScript, Python, Rust, Go, Java,
    // Clojure, then Lua (already collected above). crapper's stamp decides
    // which languages are regenerated; the rest are read from its reports, and
    // the run says so, as the original and the help text promise.
    let (stale, fresh) = stale_coverage_files(root, files, None);
    report_reused(&fresh);
    collect_crapper_reports(&root.join("coverage"), &fresh, &mut coverage.records);
    collect_crapper_reports(&root.join("target/coverage"), &fresh, &mut coverage.records);
    rust_coverage(root, &stale, &mut coverage.records)?;
    python_coverage(root, &stale, &mut coverage.records)?;
    typescript_coverage(root, &stale, &mut coverage.records)?;
    collect_native(root, files, Some(&stale), &mut coverage)?;
    Ok(coverage)
}

/// Files to regenerate, and the languages crapper's stamp says are fresh.
///
/// A language is measured for all of its files or none, as the original's
/// `_stale` decides: one stale file makes every file of that language stale.
/// Lua is left out: it reports through its own instrumented collection, which
/// this run just redid.
fn stale_coverage_files(
    root: &Path,
    files: &[PathBuf],
    command: Option<&str>,
) -> (Vec<PathBuf>, Vec<&'static str>) {
    let remaining: Vec<PathBuf> = files
        .iter()
        .filter(|file| language_of(file) != Some(Language::Lua))
        .cloned()
        .collect();
    let stale_languages: BTreeSet<Language> =
        quality_core::runner::stale_files(root, &remaining, command)
            .iter()
            .filter_map(|file| language_of(file))
            .collect();
    let fresh: Vec<&'static str> = remaining
        .iter()
        .filter_map(|file| language_of(file))
        .filter(|language| !stale_languages.contains(language))
        .map(Language::name)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let stale = remaining
        .iter()
        .filter(|file| language_of(file).is_some_and(|l| stale_languages.contains(&l)))
        .cloned()
        .collect();
    (stale, fresh)
}

/// The original prints one line when crapper's reports are reused, naming the
/// fresh languages sorted, and nothing at all when every language is stale.
fn report_reused(fresh: &[&str]) {
    if let Some(line) = reuse_message(fresh) {
        eprintln!("{line}");
    }
}

fn reuse_message(fresh: &[&str]) -> Option<String> {
    (!fresh.is_empty()).then(|| {
        format!(
            "Reusing {} coverage from crapper's last run.",
            fresh.join(", ")
        )
    })
}

/// Every Rust module's LCOV report, each in its own view. A missing tool or a
/// failed run is the original's fatal coverage error.
fn rust_coverage(root: &Path, files: &[PathBuf], records: &mut Records) -> Result<(), i32> {
    let rust_files: Vec<PathBuf> = files
        .iter()
        .filter(|file| language_of(file) == Some(Language::Rust))
        .cloned()
        .collect();
    if rust_files.is_empty() {
        return Ok(());
    }
    if quality_core::process::which("cargo-llvm-cov").is_none() {
        eprintln!("cargo-llvm-cov is required; mutation does not install tools.");
        return Err(2);
    }
    let jobs: Vec<(PathBuf, String, PathBuf)> =
        modules_with(&rust_files, Language::Rust, "Cargo.toml", root)
            .into_iter()
            .map(|module| {
                let report = quality_core::runner::coverage_report(root, &module, "rust");
                let relative = report.strip_prefix(root).unwrap_or(&report).to_path_buf();
                // Windows runs this in a copy of the project: the report path
                // must be relative to the command's own directory, or it would
                // name the real project and write there.
                let command = if cfg!(windows) {
                    let parent = relative_from(&module, report.parent().unwrap())
                        .unwrap_or_else(|| report.parent().unwrap().to_path_buf());
                    let report = relative_from(&module, &report).unwrap_or_else(|| report.clone());
                    format!(
                        "mkdir \"{}\" 2>nul & cargo llvm-cov --lcov --output-path \"{}\"",
                        parent.display(),
                        report.display()
                    )
                } else {
                    format!(
                        "mkdir -p {}\ncargo llvm-cov --lcov --output-path {}",
                        quote(report.parent().unwrap()),
                        quote(&report)
                    )
                };
                (module, command, relative)
            })
            .collect();
    for (cwd, command, class) in jobs {
        let view = View::new(root).map_err(|error| {
            eprintln!("{error}");
            2
        })?;
        let result =
            view.run(root, &cwd, &command, None, None)
                .map_err(|failure| match failure {
                    // The interrupt path reports and cleans up; no exit here.
                    Failure::Interrupted => 2,
                    Failure::Message(error) | Failure::Unavailable(error) => {
                        eprintln!("Isolation unavailable: {error}");
                        1
                    }
                })?;
        eprint!("{}", result.output);
        if result.code != 0 {
            eprintln!("Coverage command exited {}.", result.code);
            return Err(2);
        }
        let report = isolation::report_path(&view, &class);
        let text = fs::read_to_string(report).map_err(|error| {
            eprintln!("Coverage report unavailable: {error}");
            2
        })?;
        records.update(parse_lcov(&text));
    }
    Ok(())
}

/// One generated coverage command in a package's own overlay view.
fn view_step(view: &View, root: &Path, cwd: &Path, script: &str) -> Result<(i32, String), i32> {
    view.run(root, cwd, script, None, None)
        .map_err(|failure| match failure {
            Failure::Interrupted => 2,
            Failure::Message(error) | Failure::Unavailable(error) => {
                eprintln!("Isolation unavailable: {error}");
                1
            }
        })
        .inspect(|result| eprint!("{}", result.output))
        .map(|result| (result.code, result.output))
}

/// Read a report from the view's upper layer, anchored as crapper's runner
/// anchors it, and merge it into the records.
fn merge_report(records: &mut Records, view: &View, report: &Path, package: &Path, anchor: bool) {
    let path = isolation::report_path(view, report);
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let text = if anchor {
        quality_core::runners::typescript::anchor_text(&text, package)
    } else {
        text
    };
    records.update(parse_lcov(&text));
}

/// `runners._python_coverage` per package, inside views. A missing module or a
/// failed run leaves that language uncovered, as the original's N/A does.
fn python_coverage(root: &Path, files: &[PathBuf], records: &mut Records) -> Result<(), i32> {
    for package in python::coverage_roots(root, files) {
        let coverage = python::coverage(root, &package, files);
        let view = View::new(root).map_err(|error| {
            eprintln!("{error}");
            1
        })?;
        view_step(&view, root, &package, &coverage.script)?;
        merge_report(records, &view, &coverage.report, &package, true);
    }
    Ok(())
}

/// `runners._typescript_coverage` per package, inside views. `npm run coverage`
/// owns its report location and is read without anchoring; the generated
/// commands' reports are anchored to the package.
fn typescript_coverage(root: &Path, files: &[PathBuf], records: &mut Records) -> Result<(), i32> {
    for package in typescript::coverage_roots(root, files) {
        let Some(coverage) = typescript::coverage(root, &package, files) else {
            eprintln!(
                "No package.json test script in {}; skipping TypeScript coverage.",
                package.display()
            );
            continue;
        };
        let view = View::new(root).map_err(|error| {
            eprintln!("{error}");
            1
        })?;
        view_step(&view, root, &package, &coverage.script)?;
        merge_report(records, &view, &coverage.report, &package, !coverage.custom);
    }
    Ok(())
}

/// The lines a report records a hit for, or None when the file is absent.
fn record_lines(records: &Records, root: &Path, file: &str) -> Option<BTreeSet<usize>> {
    let absolute = quality_core::paths::posix(&root.join(file));
    let candidates = [file, absolute.as_str()];
    for candidate in candidates {
        for (source, lines) in records.iter() {
            if quality_core::coverage::normalize_path(source) == candidate {
                return Some(hits(lines));
            }
        }
    }
    for (source, lines) in records.iter() {
        let key = quality_core::coverage::normalize_path(source);
        if key.ends_with(&format!("/{file}")) || file.ends_with(&format!("/{key}")) {
            return Some(hits(lines));
        }
    }
    None
}

fn hits(lines: &quality_core::coverage::Lines) -> BTreeSet<usize> {
    lines
        .iter()
        .filter(|(_, (covered, _))| *covered > 0)
        .map(|(number, _)| *number as usize)
        .collect()
}

/// mutator `coverage._candidates`: a report path and its resolved absolute
/// form, normalized the way crapper normalizes report keys.
fn candidates(root: &Path, key: &str) -> Vec<String> {
    let absolute = quality_core::paths::posix(&quality_core::paths::resolve(&root.join(key)));
    let mut found = vec![quality_core::coverage::normalize_path(key)];
    let absolute = quality_core::coverage::normalize_path(&absolute);
    if !found.contains(&absolute) {
        found.push(absolute);
    }
    found
}

/// `coverage._suffix_hit` on report segments.
fn suffix_hit(key: &str, candidates: &[String]) -> bool {
    candidates.iter().any(|candidate| {
        quality_core::coverage::report_suffix_match(key, candidate)
            || quality_core::coverage::report_suffix_match(candidate, key)
    })
}

/// `coverage._lookup`: an exact candidate first, then the first record whose
/// path is a suffix of a candidate or ends with one.
fn lookup<'r>(
    records: &'r Records,
    candidates: &[String],
) -> Option<&'r quality_core::coverage::Lines> {
    if records.is_empty() {
        return None;
    }
    for candidate in candidates {
        for (source, lines) in records.iter() {
            if quality_core::coverage::normalize_path(source) == *candidate {
                return Some(lines);
            }
        }
    }
    records
        .iter()
        .find(|(source, _)| suffix_hit(source, candidates))
        .map(|(_, lines)| lines)
}

/// Native reports after a custom command, read as its disposable view showed
/// them before the view guard drops. The original runs the command in place
/// and reads every report globally from the project root, so reports the
/// command left unchanged count too. A module whose own report is visible
/// scopes its sources to that report; any other module falls back to the
/// root-level globs. A selected source no report names returns None, and the
/// caller warns that its sites are uncovered, as the original does.
fn collect_custom_native(
    root: &Path,
    files: &[PathBuf],
    coverage: &mut Coverage,
    view: &View,
) -> Result<(), i32> {
    if !files.iter().any(|file| {
        matches!(
            language_of(file),
            Some(Language::Go | Language::Java | Language::Clojure)
        )
    }) {
        return Ok(());
    }
    let base = isolation::report_path(view, Path::new(""));
    let lower = quality_core::paths::resolve(root);
    let tree = isolation::report_tree(&base, &lower);
    coverage.java = Some(quality_core::native_coverage::jacoco_lines_tree(tree));
    let mut native = if files
        .iter()
        .any(|file| matches!(language_of(file), Some(Language::Go | Language::Clojure)))
    {
        quality_core::native_coverage::NativeReports::load_tree(tree).map_err(|error| {
            eprintln!("{error}");
            2
        })?
    } else {
        // Java's own reader skips malformed XML; it never calls load_bundle.
        Default::default()
    };
    native.rebase(&base, root);
    coverage.native = Some(native);
    for (language, marker) in [(Language::Go, "go.mod"), (Language::Java, "pom.xml")] {
        for module in modules_with(files, language, marker, root) {
            let local = relative(root, &module);
            let (native, java) = if language == Language::Go {
                let Some(profile) = [
                    "target/coverage/go/coverage.out",
                    "target/coverage/coverage.out",
                    "coverage.out",
                ]
                .into_iter()
                .find_map(|path| tree.file(&local.join(path))) else {
                    continue;
                };
                let mut native = quality_core::native_coverage::NativeReports::go_report(&profile)
                    .map_err(|error| {
                        eprintln!("{error}");
                        2
                    })?;
                native.rebase(&base, root);
                (native, Vec::new())
            } else {
                let Some(report) = tree.file(&local.join("target/site/jacoco/jacoco.xml")) else {
                    continue;
                };
                (
                    Default::default(),
                    quality_core::native_coverage::jacoco_report(&report),
                )
            };
            coverage.modules.push(ModuleCoverage {
                root: module,
                language,
                native,
                java,
            });
        }
    }
    Ok(())
}

/// Read the language-native reports before their view is dropped.
fn merge_root_native(coverage: &mut Coverage, view: &View) -> Result<(), i32> {
    let base = isolation::report_path(view, Path::new(""));
    let mut reports =
        quality_core::native_coverage::NativeReports::load(&base).map_err(|error| {
            eprintln!("{error}");
            2
        })?;
    // This root command generated Clojure reports only. A copied Go/Java
    // sibling report has no provenance from this command.
    reports.invalidate(Language::Go);
    coverage
        .native
        .get_or_insert_with(Default::default)
        .extend(reports);
    Ok(())
}

/// crapper's native reports already on disk, then the stale languages among
/// `regenerate` regenerated in disposable views. Reports and build output never
/// reach the project. `None` reads crapper's reports and runs no command.
fn collect_native(
    root: &Path,
    files: &[PathBuf],
    regenerate: Option<&[PathBuf]>,
    coverage: &mut Coverage,
) -> Result<(), i32> {
    if !files.iter().any(|file| {
        matches!(
            language_of(file),
            Some(Language::Go | Language::Java | Language::Clojure)
        )
    }) {
        return Ok(());
    }
    coverage.java = Some(quality_core::native_coverage::jacoco_lines(root));
    coverage.native = Some(
        if files
            .iter()
            .any(|file| matches!(language_of(file), Some(Language::Go | Language::Clojure)))
        {
            quality_core::native_coverage::NativeReports::load(root).map_err(|error| {
                eprintln!("{error}");
                2
            })?
        } else {
            // Java's own reader skips malformed XML; it never calls load_bundle.
            quality_core::native_coverage::NativeReports::default()
        },
    );
    let Some(stale) = regenerate else {
        return Ok(());
    };
    // A language is measured for all selected files or none.
    let languages: BTreeSet<Language> = stale.iter().filter_map(|file| language_of(file)).collect();
    for language in &languages {
        if let Some(reports) = &mut coverage.native {
            reports.invalidate(*language);
        }
        if *language == Language::Java {
            coverage.java = Some(Vec::new());
        }
    }
    let native_files: Vec<PathBuf> = files
        .iter()
        .filter(|file| language_of(file).is_some_and(|language| languages.contains(&language)))
        .cloned()
        .collect();
    go_coverage(root, &native_files, coverage)?;
    java_coverage(root, &native_files, coverage)?;
    if languages.contains(&Language::Clojure) {
        clojure_coverage(root, coverage)?;
    }
    Ok(())
}

/// Bind one module's freshly generated report to that module. A sibling's
/// report in the same copied view is not this command's provenance.
fn merge_module_native(
    coverage: &mut Coverage,
    view: &View,
    root: &Path,
    module: &Path,
    language: Language,
) -> Result<(), i32> {
    let base = isolation::report_path(view, Path::new(""));
    let mut native = Default::default();
    let mut java = Vec::new();
    match language {
        Language::Go => {
            let report = isolation::report_path(
                view,
                &relative(root, &module.join("target/coverage/go/coverage.out")),
            );
            if !report.is_file() {
                return Ok(());
            }
            native = quality_core::native_coverage::NativeReports::go_report(&report).map_err(
                |error| {
                    eprintln!("{error}");
                    2
                },
            )?;
            native.rebase(&base, root);
        }
        Language::Java => {
            java = quality_core::native_coverage::jacoco_report(&isolation::report_path(
                view,
                &relative(root, &module.join("target/site/jacoco/jacoco.xml")),
            ));
        }
        _ => {}
    }
    coverage.modules.push(ModuleCoverage {
        root: module.to_path_buf(),
        language,
        native,
        java,
    });
    Ok(())
}

/// crapper's Go runner per stale module, in its own view.
fn go_coverage(root: &Path, files: &[PathBuf], coverage: &mut Coverage) -> Result<(), i32> {
    if !files
        .iter()
        .any(|file| language_of(file) == Some(Language::Go))
    {
        return Ok(());
    }
    let modules = quality_core::runner::modules_with(files, Language::Go, "go.mod", root);
    if modules.is_empty() {
        eprintln!("No go.mod; skipping Go coverage.");
        return Ok(());
    }
    for module in modules {
        let view = View::new(root).map_err(|error| {
            eprintln!("Cannot create a mutation view: {error}");
            1
        })?;
        let result = view_step(&view, root, &module, &golang::coverage_script())?;
        let report = module.join("target/coverage/go/coverage.out");
        if result.0 != 0 {
            eprintln!(
                "Go coverage exited {} in {}. Go coverage will be N/A.",
                result.0,
                module.display()
            );
            continue;
        }
        let generated = isolation::report_path(&view, &relative(root, &report));
        if !generated.is_file() {
            eprintln!(
                "Go coverage wrote no {}. Go coverage will be N/A.",
                report.display()
            );
            continue;
        }
        merge_module_native(coverage, &view, root, &module, Language::Go)?;
    }
    Ok(())
}

/// crapper's Maven JaCoCo runner per stale module: the stale report and data
/// go, the plugin prepares the agent, tests run, the report is written.
fn java_coverage(root: &Path, files: &[PathBuf], coverage: &mut Coverage) -> Result<(), i32> {
    if !files
        .iter()
        .any(|file| language_of(file) == Some(Language::Java))
    {
        return Ok(());
    }
    let modules = quality_core::runner::modules_with(files, Language::Java, "pom.xml", root);
    if modules.is_empty() {
        eprintln!("No pom.xml; skipping Java coverage.");
        return Ok(());
    }
    let script = if cfg!(windows) {
        format!(
            "rmdir /s /q target\\site\\jacoco 2>nul & del /f /q target\\jacoco.exec 2>nul & {}",
            quality_core::runner::JACOCO_MAVEN.join(" ")
        )
    } else {
        format!(
            "rm -rf target/site/jacoco target/jacoco.exec\n{}",
            quality_core::runner::JACOCO_MAVEN.join(" ")
        )
    };
    for module in modules {
        let view = View::new(root).map_err(|error| {
            eprintln!("Cannot create a mutation view: {error}");
            1
        })?;
        let result = view_step(&view, root, &module, &script)?;
        let report = module.join("target/site/jacoco/jacoco.xml");
        if result.0 != 0 {
            eprintln!(
                "Java coverage exited {} in {}. Java coverage will be N/A.",
                result.0,
                module.display()
            );
            continue;
        }
        if !isolation::report_path(&view, &relative(root, &report)).is_file() {
            eprintln!(
                "Java coverage wrote no {}. Java coverage will be N/A.",
                report.display()
            );
        }
        merge_module_native(coverage, &view, root, &module, Language::Java)?;
    }
    Ok(())
}

/// crapper's Clojure runner at the root: `clj -M:cov --lcov`, retried without
/// `--lcov` when Cloverage has no LCOV reporter.
fn clojure_coverage(root: &Path, coverage: &mut Coverage) -> Result<(), i32> {
    if !root.join("deps.edn").is_file() && !root.join("bb.edn").is_file() {
        eprintln!("No deps.edn or bb.edn; skipping Clojure coverage.");
        return Ok(());
    }
    let view = View::new(root).map_err(|error| {
        eprintln!("Cannot create a mutation view: {error}");
        1
    })?;
    let script: String = if cfg!(windows) {
        "clj -M:cov --lcov || (echo clj -M:cov --lcov failed; retrying without --lcov. 1>&2 & clj -M:cov)".into()
    } else {
        "clj -M:cov --lcov\ncode=$?\nif [ $code -ne 0 ]; then echo 'clj -M:cov --lcov failed; retrying without --lcov.' >&2; clj -M:cov; code=$?; fi\nif [ $code -ne 0 ]; then echo \"Clojure coverage exited $code. Clojure coverage will be N/A.\" >&2; fi\nexit 0".into()
    };
    let cleanup = if cfg!(windows) {
        // Keep the other languages' report directories and the stamp.
        "for /d %D in (target\\coverage\\*) do @if not \"%~nxD\"==\"go\" if not \"%~nxD\"==\"java\" if not \"%~nxD\"==\"rust\" if not \"%~nxD\"==\"python\" if not \"%~nxD\"==\"typescript\" if not \"%~nxD\"==\"lua\" rmdir /s /q \"%D\"\nfor %F in (target\\coverage\\*) do @if not \"%~nxF\"==\"crapper-stamp.json\" del /f /q \"%F\" 2>nul\n"
    } else {
        "for entry in target/coverage/* target/coverage/.[!.]* target/coverage/..?*; do\n  if [ ! -e \"$entry\" ] && [ ! -L \"$entry\" ]; then continue; fi\n  case \"${entry##*/}\" in typescript|rust|go|python|lua|crapper-stamp.json) ;; *) rm -rf -- \"$entry\" || exit 1;; esac\ndone\n"
    };
    view_step(&view, root, root, &format!("{cleanup}{script}"))?;
    merge_root_native(coverage, &view)?;
    Ok(())
}

/// A path under `root` as a relative one, for a view's upper layer.
fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(quality_core::paths::resolve(root))
        .unwrap_or(path)
        .to_path_buf()
}

/// The lines this file's tests hit: the Lua index where one exists, else the
/// language's own report. Java reads JaCoCo source lines, Go its statement
/// profile, Clojure Cloverage form counts before LCOV, and every other
/// language LCOV. None means the file has no coverage data at all.
fn covered_lines(
    coverage: &Coverage,
    root: &Path,
    file: &Path,
    key: &str,
) -> Option<BTreeSet<usize>> {
    let language = language_of(file);
    if language == Some(Language::Lua)
        && let Some(index) = coverage.indexes.get(&lua_module(root, file))
    {
        return index.hits(file).cloned();
    }
    let scoped = coverage
        .modules
        .iter()
        .filter(|scope| Some(scope.language) == language && file.starts_with(&scope.root))
        .max_by_key(|scope| scope.root.components().count());
    if let Some(scope) = scoped {
        let local = quality_core::paths::posix(&relative(&scope.root, file));
        let names = candidates(&scope.root, &local);
        let found = if language == Some(Language::Go) {
            // Exact module-relative or absolute key first; a bare basename
            // would let a subpackage's same-named file satisfy this module.
            names
                .last()
                .and_then(|absolute| scope.native.go_lines_exact(absolute))
                .or_else(|| scope.native.go_lines_exact(&local))
                .or_else(|| scope.native.go_lines_exact(key))
                .or_else(|| {
                    names
                        .last()
                        .and_then(|absolute| scope.native.go_lines(absolute))
                })
        } else {
            scope
                .java
                .iter()
                .find(|(report, _)| names.contains(report))
                .or_else(|| {
                    scope
                        .java
                        .iter()
                        .find(|(report, _)| suffix_hit(report, &names))
                })
                .map(|(_, lines)| lines.clone())
        };
        return found;
    }
    let candidates = candidates(root, key);
    match language {
        Some(Language::Java) => {
            let index = coverage.java.as_ref()?;
            index
                .iter()
                .find(|(report, _)| candidates.contains(report))
                .or_else(|| {
                    index
                        .iter()
                        .find(|(report, _)| suffix_hit(report, &candidates))
                })
                .map(|(_, lines)| lines.clone())
        }
        Some(Language::Go) => coverage.native.as_ref()?.go_lines(candidates.last()?),
        Some(Language::Clojure) => {
            let native = coverage.native.as_ref();
            if let Some(lines) = native.and_then(|reports| lookup(reports.forms(), &candidates)) {
                return Some(hits(lines));
            }
            // Custom/generated LCOV was collected from a view; existing
            // reports were loaded from the project's native report bundle.
            record_lines(&coverage.records, root, key).or_else(|| {
                native.and_then(|reports| lookup(reports.lcov(), &candidates).map(hits))
            })
        }
        _ => record_lines(&coverage.records, root, key),
    }
}

/// Python's `reused_run` contract: the file still owns sites, its form digests
/// are exactly the recorded ones and no site was selected, treating every site
/// as covered. Anything else takes the normal path so the snapshot is
/// republished even when no test runs.
fn reusable(root: &Path, item: &Prepared) -> bool {
    let history = metrics::load_history(root, &item.key);
    !item.sites.is_empty()
        && metrics::unchanged(&item.forms, &history)
        && item
            .decisions
            .values()
            .all(|decision| decision.action != "executed")
}

fn backups_hold_files(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => backups_hold_files(&path),
            _ => path.is_file(),
        }
    })
}

/// Refuse to run while an older mutator's backups are present. Mutants only
/// touch overlay views, so such a backup can only come from a run that still
/// mutated the project; it records no bytes that run last wrote, so nothing is
/// written back and no test runs.
pub fn check_backups(root: &Path) -> Result<(), String> {
    let base = root.join("target").join("mutator-backup");
    if base.is_dir() && backups_hold_files(&base) {
        return Err(format!(
            "Found backups from an older mutator under {}. Stopping before any test runs; the sources and backups were left untouched. Compare them, then remove the directory.",
            base.display()
        ));
    }
    Ok(())
}

fn decide(item: &mut Prepared, lines: Option<&BTreeSet<usize>>, root: &Path, options: &Options) {
    let ignore = options.flag("--no-coverage");
    item.covered = item
        .sites
        .iter()
        .map(|site| {
            (
                site.mutation_id.clone(),
                ignore || lines.is_some_and(|lines| lines.contains(&site.line)),
            )
        })
        .collect();
    let history = metrics::load_history(root, &item.key);
    item.decisions = metrics::decide(
        &item.sites,
        &item.covered,
        &history,
        &item.forms,
        options.flag("--mutate-all"),
        options.lines.as_ref(),
    );
    item.outcomes = metrics::carry_forward(&item.sites, &item.covered, &history, &item.forms);
}

pub fn prepare(
    root: &Path,
    options: &Options,
    files: &[PathBuf],
    timings: &mut Timings,
) -> Result<Vec<Prepared>, i32> {
    let mut prepared = Vec::new();
    for file in files {
        let raw = fs::read(file).map_err(|error| {
            eprintln!("Cannot read {}: {error}", file.display());
            2
        })?;
        let source = read_source(file).map_err(|_| 2)?;
        let key = metrics::source_key(file, root).map_err(|_| 2)?;
        let (sites, forms) = sites::discover(&source, file, root, &key).map_err(|error| {
            eprintln!(
                "Cannot discover mutation sites in {}: {error}",
                file.display()
            );
            1
        })?;
        let history = metrics::load_history(root, &key);
        let covered = sites
            .iter()
            .map(|site| (site.mutation_id.clone(), true))
            .collect();
        let decisions = metrics::decide(
            &sites,
            &covered,
            &history,
            &forms,
            options.flag("--mutate-all"),
            options.lines.as_ref(),
        );
        prepared.push(Prepared {
            file: file.clone(),
            key,
            raw,
            source,
            sites,
            forms,
            decisions,
            outcomes: BTreeMap::new(),
            covered,
            index: None,
        });
    }
    if options.flag("--scan") {
        // Scanning inventories sites: no coverage command runs and nothing runs.
        return Ok(prepared);
    }
    if prepared.iter().all(|item| reusable(root, item)) {
        return Ok(prepared);
    }
    let coverage = if options.flag("--no-coverage") {
        // The original never collects coverage for a --no-coverage run.
        Coverage::default()
    } else {
        let started = std::time::Instant::now();
        let coverage = coverage(root, options, files)?;
        timings.add("coverage", started.elapsed());
        coverage
    };
    for item in &mut prepared {
        let lua = language_of(&item.file) == Some(Language::Lua);
        if lua {
            item.index = coverage.indexes.get(&lua_module(root, &item.file)).cloned();
        }
        let lines = if options.flag("--no-coverage") {
            None
        } else {
            covered_lines(&coverage, root, &item.file, &item.key)
        };
        if !options.flag("--no-coverage") && lines.is_none() {
            eprintln!(
                "No coverage data for {}; its sites are uncovered. Pass --no-coverage to run them anyway.",
                item.key
            );
        }
        decide(item, lines.as_ref(), root, options);
        let selected = item
            .decisions
            .values()
            .filter(|decision| decision.action == "executed")
            .count();
        if selected as i64 > options.mutation_warning {
            eprintln!(
                "WARNING: {selected} covered mutations selected in {}.",
                item.key
            );
        }
    }
    Ok(prepared)
}

/// A pristine run that could not happen is not a test result: an isolation
/// refusal exits 1 before anything is decided, as the original does.
fn baseline_exit(item: &Prepared, command: &Command, error: Failure) -> i32 {
    match error {
        Failure::Unavailable(message) => {
            eprintln!("{message}");
            1
        }
        Failure::Interrupted => 2,
        Failure::Message(tail) => failure(item, command, &tail),
    }
}

fn failure(item: &Prepared, command: &Command, tail: &str) -> i32 {
    let mut message = format!("Baseline failed for {}: {}", item.key, command.display());
    if !tail.is_empty() {
        message.push('\n');
        message.push_str(tail);
    }
    eprintln!("{message}");
    2
}

/// `--select-specs`: run only the specs that cover a mutant, and fall back to
/// the full suite when an attribution is missing or its subset does not pass.
/// `pristine` runs a subset's pristine check and reports its seconds.
fn lua_plans(
    item: &Prepared,
    index: &Index,
    selected: &[Site],
    command: &Command,
    timeout: f64,
    factor: f64,
    pristine: &mut dyn FnMut(&Command) -> Result<f64, Failure>,
) -> (BTreeMap<String, (Command, f64)>, Command, f64) {
    let mut plans = BTreeMap::new();
    let mut subsets: BTreeMap<Vec<String>, Option<(Command, f64)>> = BTreeMap::new();
    let mut passed: BTreeMap<Vec<String>, f64> = BTreeMap::new();
    let mut union: BTreeSet<String> = BTreeSet::new();
    let mut full_suite = false;
    for site in selected {
        let specs = index.select(&item.file, site.line).unwrap_or_default();
        if !specs.is_empty() && !subsets.contains_key(&specs) {
            let selected_command = command.extend(&specs);
            let entry = match pristine(&selected_command) {
                Ok(seconds) => {
                    passed.insert(specs.clone(), seconds);
                    Some((
                        selected_command,
                        (index.seconds_for(&specs) * factor)
                            .max(seconds * factor)
                            .max(2.0),
                    ))
                }
                Err(_) => None,
            };
            subsets.insert(specs.clone(), entry);
        }
        match subsets.get(&specs) {
            Some(Some(plan)) => {
                union.extend(specs);
                plans.insert(site.mutation_id.clone(), plan.clone());
            }
            _ => {
                plans.insert(site.mutation_id.clone(), (command.clone(), timeout));
                full_suite = true;
            }
        }
    }
    if full_suite {
        return (plans, Command::Argv(index.command.clone()), timeout);
    }
    let specs: Vec<String> = union.into_iter().collect();
    let check_timeout = (index.seconds_for(&specs) * factor)
        .max(passed.values().sum::<f64>() * factor)
        .max(2.0);
    (plans, command.extend(&specs), check_timeout)
}

/// What one file's publication reported.
#[derive(Default)]
struct Published {
    changed: bool,
    survived: bool,
}

/// One pristine run per command and working directory, shared by every target
/// file in this call. The pool's first slot runs it; every worker's view is
/// built the same way, so a per-worker repeat would only repeat the same run.
#[allow(clippy::too_many_arguments)]
fn pristine(
    workers: &Workers,
    root: &Path,
    timings: &mut Timings,
    baselines: &mut Baselines,
    command: &Command,
    cwd: &Path,
    build: bool,
    timeout: Option<Duration>,
) -> Result<f64, Failure> {
    let key = (command.clone(), cwd.to_path_buf());
    if !baselines.contains_key(&key) {
        let started = std::time::Instant::now();
        let check = workers.view(0);
        let result = (|| {
            if build {
                let built = execute(
                    &check,
                    root,
                    cwd,
                    &Command::Shell("cargo test --no-run".into()),
                    None,
                    None,
                )?;
                if built.0 != 0 {
                    return Ok(built);
                }
            }
            execute(&check, root, cwd, command, None, timeout)
        })();
        timings.add("baseline", started.elapsed());
        baselines.insert(key.clone(), result);
    }
    match baselines.get(&key) {
        // unittest before 3.12 exits 0 when discovery finds nothing;
        // every mutant would then survive a suite that tests nothing.
        Some(Ok((0, _, output)))
            if output
                .lines()
                .any(|line| line.starts_with("Ran 0 tests in ")) =>
        {
            Err(Failure::Message(
                output
                    .lines()
                    .rev()
                    .take(20)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n"),
            ))
        }
        Some(Ok((0, duration, _))) => Ok(duration.as_secs_f64()),
        Some(Ok((code, _, _))) => Err(Failure::Message(format!("exit {code}"))),
        Some(Err(Failure::Unavailable(error))) => Err(Failure::Unavailable(error.clone())),
        Some(Err(Failure::Message(error))) => Err(Failure::Message(error.clone())),
        Some(Err(Failure::Interrupted)) => Err(Failure::Interrupted),
        None => Err(Failure::Message("no pristine run".into())),
    }
}

/// The sites a file's decisions execute, in the order the engine scheduled
/// them.
fn selected_sites(item: &Prepared) -> Vec<Site> {
    item.sites
        .iter()
        .filter(|site| {
            item.decisions
                .get(&site.mutation_id)
                .is_some_and(|decision| decision.action == "executed")
        })
        .cloned()
        .collect()
}

fn executed_count(item: &Prepared) -> usize {
    item.decisions
        .values()
        .filter(|decision| decision.action == "executed")
        .count()
}

/// Write one file's snapshot from its outcomes and report its console line.
/// A source edited or deleted during the run keeps its previous snapshot and
/// makes the call exit 4, exactly as the original's `_source_change` does.
fn publish(
    root: &Path,
    options: &Options,
    clock: &(impl Fn() -> String + Sync),
    mut item: Prepared,
) -> Result<Published, Failure> {
    let changed = fs::read(&item.file).ok() != Some(item.raw.clone())
        || read_source(&item.file).ok() != Some(item.source.clone());
    if changed {
        eprintln!("Source changed during mutation: {}", item.key);
        return Ok(Published {
            changed: true,
            survived: false,
        });
    }
    if options.flag("--verbose") {
        for site in &item.sites {
            let decision = &item.decisions[&site.mutation_id];
            let result = item
                .outcomes
                .get(&site.mutation_id)
                .map_or("none", String::as_str);
            println!(
                "{}:{} {}/{} {} {} reason={} previous={} result={result} id={}",
                site.file,
                site.line,
                site.namespace,
                site.name,
                site.description(),
                decision.action,
                decision.reason,
                decision.previous,
                site.mutation_id
            );
        }
    }
    metrics::tally_forms(
        &mut item.forms,
        &item.sites,
        &item.covered,
        &item.outcomes,
        options.lines.as_ref(),
    );
    let survived = item.forms.iter().any(|form| form.survived > 0);
    if let Err(error) = metrics::write_results(
        root,
        &item.key,
        &item.forms,
        &item.outcomes,
        &clock(),
        isolation::level(),
    ) {
        return Err(Failure::Message(format!(
            "Cannot publish mutation snapshot: {error}"
        )));
    }
    println!(
        "{}: executed={} reused={}",
        item.key,
        executed_count(&item),
        item.decisions
            .values()
            .filter(|decision| decision.action == "reused")
            .count()
    );
    Ok(Published {
        changed: false,
        survived,
    })
}

/// A worker panicking must not turn the others' results into a poisoned lock.
fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One queued file's mutants and the command that judges them. Commands differ
/// per file (covering specs first), so every item carries its own, as the
/// original's `QueuedFile` does.
struct Job {
    key: String,
    file: PathBuf,
    source: String,
    command: Command,
    cwd: PathBuf,
    timeout: f64,
    sites: Vec<Site>,
}

/// What the queued files of one call reported.
#[derive(Default)]
struct Queued {
    changed: bool,
    survived: bool,
    failed: bool,
    /// An isolation refusal is not a test result: the call exits 1, keeping the
    /// snapshots of the files that already finished.
    unavailable: Option<String>,
}

/// Run every full-suite Lua file of the call on one shared worker queue.
///
/// This is the original's `mutate_queued`: each file's baseline is checked
/// before any mutant runs and a baseline failure stays with its file, every
/// worker takes the next mutant from any file, and each file's snapshot is
/// published as soon as its own last mutant finishes. A worker failure keeps
/// the completed files' snapshots and fails the rest; an interrupt keeps the
/// finished files and leaves the unfinished ones with their previous
/// snapshots.
fn mutate_queued(
    root: &Path,
    options: &Options,
    files: Vec<Prepared>,
    workers: &Workers,
    baselines: &mut Baselines,
    clock: &(impl Fn() -> String + Sync),
    timings: &mut Timings,
) -> Queued {
    let mut jobs: Vec<Job> = Vec::new();
    let mut items: Vec<Option<Prepared>> = Vec::new();
    let mut result = Queued::default();
    for item in files {
        let (base, cwd) = plan(root, options, &item.file);
        let mut command = base;
        let mut run_cwd = cwd;
        if let Some(index) = &item.index {
            command = Command::Argv(index.command.clone());
            run_cwd = index.directory.clone();
            if index.fresh {
                // The fresh coverage run already proved the pristine suite
                // passes; its duration is the timeout base (ADR 0001). There is
                // no captured output to inspect for an empty suite.
                baselines
                    .entry((command.clone(), index.directory.clone()))
                    .or_insert(Ok((
                        0,
                        Duration::from_secs_f64(index.seconds),
                        String::new(),
                    )));
            }
        }
        let seconds = match pristine(
            workers, root, timings, baselines, &command, &run_cwd, false, None,
        ) {
            Ok(seconds) => seconds,
            Err(Failure::Unavailable(message)) => {
                // An isolation refusal stops the run: exit 1, keep finished snapshots.
                result.unavailable.get_or_insert(message);
                continue;
            }
            Err(Failure::Interrupted) => {
                result.failed = true;
                continue;
            }
            Err(Failure::Message(tail)) => {
                // A baseline failure stays with its file; the others still run.
                result.failed = true;
                failure(&item, &command, &tail);
                continue;
            }
        };
        let timeout = (seconds * options.timeout_factor).max(2.0);
        let mut selected = selected_sites(&item);
        // The original's Lua scheduling: previous survivors first.
        lua::survivors_first(&mut selected, &item.outcomes);
        let mut tail = vec!["--no-keep-going".to_string()];
        if let Some(index) = &item.index {
            let lines: Vec<usize> = selected.iter().map(|site| site.line).collect();
            tail.extend(index.covering_first(&item.file, &lines));
        }
        let command = command.extend(&tail);
        jobs.push(Job {
            key: item.key.clone(),
            file: item.file.clone(),
            source: item.source.clone(),
            command,
            cwd: run_cwd,
            timeout,
            sites: selected,
        });
        items.push(Some(item));
    }
    if jobs.is_empty() {
        return result;
    }
    let total: usize = jobs.iter().map(|job| job.sites.len()).sum();
    let count = workers::worker_count(total, options.max_workers).clamp(1, workers.len());
    let order: Vec<&[Site]> = jobs.iter().map(|job| job.sites.as_slice()).collect();
    let lookup: BTreeMap<&str, usize> = jobs
        .iter()
        .enumerate()
        .map(|(index, job)| (job.key.as_str(), index))
        .collect();
    let flags = std::sync::Mutex::new(Queued::default());
    let items = std::sync::Mutex::new(items);
    let run = crate::queue::run_queue(
        count,
        &order,
        |slot, site| {
            let job = &jobs[lookup[site.file.as_str()]];
            let mutated = sites::apply_site(&job.source, site);
            let view = workers.view(slot);
            execute(
                &view,
                root,
                &job.cwd,
                &job.command,
                Some((&job.file, &mutated)),
                Some(Duration::from_secs_f64(job.timeout)),
            )
            .map(|(code, _, _)| code != 0)
        },
        |index, outcomes| {
            let item = lock(&items)[index]
                .take()
                .expect("a queued file publishes once");
            let mut item = item;
            item.outcomes.extend(
                outcomes
                    .iter()
                    .map(|(id, outcome)| (id.clone(), outcome.clone())),
            );
            let published = publish(root, options, clock, item)?;
            let mut flags = lock(&flags);
            flags.changed |= published.changed;
            flags.survived |= published.survived;
            Ok(())
        },
    );
    {
        let flags = lock(&flags);
        result.changed |= flags.changed;
        result.survived |= flags.survived;
    }
    match run {
        Ok(()) => {
            if interrupts::pending().is_some() {
                result.failed = true;
            }
        }
        Err(Failure::Interrupted) => result.failed = true,
        Err(Failure::Unavailable(error)) => {
            eprintln!("{error}");
            result.unavailable.get_or_insert(error);
        }
        Err(Failure::Message(error)) => {
            eprintln!("{error}");
            result.failed = true;
        }
    }
    result
}

pub fn mutate(
    root: &Path,
    options: &Options,
    prepared: Vec<Prepared>,
    clock: impl Fn() -> String + Sync,
    timings: &mut Timings,
) -> i32 {
    if options.flag("--scan") {
        for item in prepared {
            let sites: Vec<&Site> = item
                .sites
                .iter()
                .filter(|site| {
                    options
                        .lines
                        .as_ref()
                        .is_none_or(|lines| lines.contains(&site.line))
                })
                .collect();
            println!("Scan: {} mutation sites in {}", sites.len(), item.key);
            let history = metrics::load_history(root, &item.key);
            let changed = item.forms.iter().any(|form| {
                history
                    .forms
                    .get(&(form.namespace.clone(), form.id.clone()))
                    .is_some_and(|prior| prior.digest != form.digest)
            });
            for site in &sites {
                let mark = if history
                    .forms
                    .get(&(site.namespace.clone(), site.form_id.clone()))
                    .is_some_and(|prior| {
                        item.forms
                            .iter()
                            .find(|form| {
                                form.namespace == site.namespace && form.id == site.form_id
                            })
                            .is_some_and(|form| form.digest != prior.digest)
                    }) {
                    "*"
                } else {
                    " "
                };
                let coverage = if item.covered.get(&site.mutation_id) == Some(&false) {
                    " uncovered"
                } else {
                    ""
                };
                println!(
                    "{mark} {}:{} {}{}  [{}]",
                    site.file,
                    site.line,
                    site.description(),
                    coverage,
                    site.form_id
                );
            }
            if changed {
                println!("* marks a function whose text changed since the last snapshot.");
            }
            println!();
        }
        return 0;
    }
    // Reuse the snapshots only when every file is unchanged in the sense of
    // `reusable`; a new or rewritten form still needs its snapshot republished.
    if prepared.iter().all(|item| reusable(root, item)) {
        println!("No mutations selected; keeping the previous snapshots.");
        return 0;
    }
    let shares_queue = |item: &Prepared| {
        crate::shares_queue(
            &item.file,
            options.value("--test-command"),
            options.flag("--select-specs"),
        )
    };
    let mut no_sites = Vec::new();
    let mut queue = Vec::new();
    let mut single = Vec::new();
    let mut queued_sites = 0;
    let mut single_sites = 0;
    for item in prepared {
        let count = executed_count(&item);
        if count == 0 {
            no_sites.push(item);
        } else if shares_queue(&item) {
            queued_sites += count;
            queue.push(item);
        } else {
            single_sites = single_sites.max(count);
            single.push(item);
        }
    }
    // One pool for the whole call: its slots are the workers every target
    // file's mutants reuse, and dropping it removes them.
    let limit = workers::worker_count(usize::MAX, options.max_workers);
    let pool_size = limit.min(queued_sites.max(single_sites));
    let pool = if pool_size == 0 {
        None
    } else {
        match Workers::new(root, pool_size) {
            Ok(pool) => Some(pool),
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        }
    };
    let mut baselines = Baselines::new();
    let mut changed_any = false;
    let mut survived_any = false;
    // A file with nothing to run still republishes its snapshot, as the
    // original does while it prepares the files.
    for item in no_sites {
        match publish(root, options, &clock, item) {
            Ok(published) => {
                changed_any |= published.changed;
                survived_any |= published.survived;
            }
            Err(Failure::Interrupted) => return 2,
            Err(Failure::Unavailable(error)) => {
                eprintln!("{error}");
                return 1;
            }
            Err(Failure::Message(error)) => {
                eprintln!("{error}");
                return 2;
            }
        }
    }
    let queued = if queue.is_empty() {
        None
    } else {
        let workers = pool.as_ref().expect("selected queued sites need workers");
        Some(mutate_queued(
            root,
            options,
            queue,
            workers,
            &mut baselines,
            &clock,
            timings,
        ))
    };
    if let Some(queued) = &queued {
        changed_any |= queued.changed;
        survived_any |= queued.survived;
    }
    for mut item in single {
        let (command, cwd) = plan(root, options, &item.file);
        let mut selected = selected_sites(&item);
        if !selected.is_empty() {
            if interrupts::pending().is_some() {
                return 2;
            }
            let workers = pool.as_ref().expect("a selected site needs a worker");
            // `language == "lua" and not test_command`: the generated Busted
            // command, with or without a coverage index.
            let standard_lua = language_of(&item.file) == Some(Language::Lua)
                && options.value("--test-command").is_none();
            let mut command = command;
            if standard_lua && let Some(index) = &item.index {
                command = Command::Argv(index.command.clone());
                if index.fresh {
                    // The fresh coverage run already proved the pristine suite
                    // passes; its duration is the timeout base (ADR 0001).
                    baselines
                        .entry((command.clone(), index.directory.clone()))
                        .or_insert(Ok((
                            0,
                            Duration::from_secs_f64(index.seconds),
                            String::new(),
                        )));
                }
            }
            let run_cwd = if standard_lua {
                item.index
                    .as_ref()
                    .map_or_else(|| cwd.clone(), |index| index.directory.clone())
            } else {
                cwd.clone()
            };
            // One pristine run per command and working directory, shared by every
            // target file in this call.
            let mut baseline = |command: &Command,
                                build: bool,
                                timeout: Option<Duration>|
             -> Result<f64, Failure> {
                pristine(
                    workers,
                    root,
                    timings,
                    &mut baselines,
                    command,
                    &run_cwd,
                    build,
                    timeout,
                )
            };
            let build = !standard_lua && command.is_cargo_test();
            let seconds = match baseline(&command, build, None) {
                Ok(seconds) => seconds,
                Err(error) => return baseline_exit(&item, &command, error),
            };
            let timeout = (seconds * options.timeout_factor).max(2.0);
            let mut plans: Option<BTreeMap<String, (Command, f64)>> = None;
            let mut check_command = command.clone();
            let mut check_timeout = timeout;
            let mut confirmation: Option<(Command, f64)> = None;
            let mut run_command = command.clone();
            if standard_lua {
                // The original's Lua scheduling: previous survivors first.
                lua::survivors_first(&mut selected, &item.outcomes);
                if options.flag("--select-specs")
                    && let Some(index) = &item.index
                {
                    run_command = command.extend(&["--no-keep-going".to_string()]);
                    let (found, check, check_seconds) = lua_plans(
                        &item,
                        index,
                        &selected,
                        &run_command,
                        timeout,
                        options.timeout_factor,
                        &mut |plan: &Command| baseline(plan, false, None),
                    );
                    plans = Some(found);
                    check_command = check;
                    check_timeout = check_seconds;
                    confirmation = Some((run_command.clone(), timeout));
                } else {
                    let mut tail = vec!["--no-keep-going".to_string()];
                    if let Some(index) = &item.index {
                        let lines: Vec<usize> = selected.iter().map(|site| site.line).collect();
                        tail.extend(index.covering_first(&item.file, &lines));
                    }
                    run_command = command.extend(&tail);
                }
            }
            // With one worker the pristine plan check is the cached baseline;
            // the selected-specs union subset still gets its own pristine run
            // before any mutant can earn a kill.
            if let Err(error) = baseline(
                &check_command,
                false,
                Some(Duration::from_secs_f64(check_timeout)),
            ) {
                return baseline_exit(&item, &check_command, error);
            }
            let count = workers::worker_count(selected.len(), options.max_workers);
            let source = &item.source;
            let file = &item.file;
            let outcomes = workers.run_selected(count, &selected, |view, site| {
                let (site_command, site_timeout) = plans
                    .as_ref()
                    .and_then(|plans| plans.get(&site.mutation_id))
                    .cloned()
                    .unwrap_or_else(|| (run_command.clone(), timeout));
                let mutated = sites::apply_site(source, site);
                execute(
                    view,
                    root,
                    &run_cwd,
                    &site_command,
                    Some((file, &mutated)),
                    Some(Duration::from_secs_f64(site_timeout)),
                )
                .map(|(code, _, _)| code)
            });
            let mut outcomes = match outcomes {
                Ok(outcomes) => outcomes,
                Err(Failure::Interrupted) => return 2,
                Err(Failure::Message(error)) => {
                    eprintln!("{error}");
                    return 2;
                }
                Err(Failure::Unavailable(error)) => {
                    eprintln!("{error}");
                    return 1;
                }
            };
            if let Some((full, full_timeout)) = confirmation {
                let survivors: Vec<&Site> = selected
                    .iter()
                    .filter(|site| {
                        outcomes.get(&site.mutation_id).map(String::as_str) == Some("survived")
                            && plans
                                .as_ref()
                                .and_then(|plans| plans.get(&site.mutation_id))
                                .is_none_or(|(command, _)| command != &full)
                    })
                    .collect();
                if !survivors.is_empty() {
                    if let Err(error) =
                        baseline(&full, false, Some(Duration::from_secs_f64(full_timeout)))
                    {
                        return baseline_exit(&item, &full, error);
                    }
                    for site in survivors {
                        let mutated = sites::apply_site(&item.source, site);
                        let result = execute(
                            &workers.view(0),
                            root,
                            &run_cwd,
                            &full,
                            Some((&item.file, &mutated)),
                            Some(Duration::from_secs_f64(full_timeout)),
                        );
                        let code = match result {
                            Ok((code, _, _)) => code,
                            Err(Failure::Interrupted) => return 2,
                            Err(Failure::Message(error)) => {
                                eprintln!("{error}");
                                return 2;
                            }
                            Err(Failure::Unavailable(error)) => {
                                eprintln!("{error}");
                                return 1;
                            }
                        };
                        outcomes.insert(
                            site.mutation_id.clone(),
                            if code == 0 { "survived" } else { "killed" }.to_string(),
                        );
                    }
                }
            }
            for (mutation_id, outcome) in outcomes {
                item.outcomes.insert(mutation_id, outcome);
            }
        }
        match publish(root, options, &clock, item) {
            Ok(published) => {
                changed_any |= published.changed;
                survived_any |= published.survived;
            }
            Err(Failure::Interrupted) => return 2,
            Err(Failure::Unavailable(error)) => {
                eprintln!("{error}");
                return 1;
            }
            Err(Failure::Message(error)) => {
                eprintln!("{error}");
                return 2;
            }
        }
    }
    if let Some(queued) = &queued
        && let Some(message) = &queued.unavailable
    {
        eprintln!("{message}");
        return 1;
    }
    if queued.is_some_and(|queued| queued.failed) {
        return 2;
    }
    if changed_any {
        4
    } else if survived_any {
        3
    } else {
        0
    }
}

#[cfg(test)]
#[path = "native_report_tests.rs"]
mod native_report_tests;

#[cfg(test)]
mod reuse_tests {
    use super::reuse_message;

    #[test]
    fn the_reuse_line_names_the_fresh_languages_sorted() {
        assert_eq!(
            reuse_message(&["python", "typescript"]).as_deref(),
            Some("Reusing python, typescript coverage from crapper's last run.")
        );
        assert_eq!(
            reuse_message(&["rust"]).as_deref(),
            Some("Reusing rust coverage from crapper's last run.")
        );
        // Every language stale: the original prints nothing at all.
        assert_eq!(reuse_message(&[]), None);
    }
}
