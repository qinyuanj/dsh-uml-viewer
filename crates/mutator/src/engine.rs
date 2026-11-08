use crate::{
    cli::Options,
    interrupts,
    isolation::{self, Failure, View},
    lua::{self, Index},
    metrics::{self, Decision},
    sites::{self, Form, Site},
    workers::{self, Workers},
};
use quality_core::{
    coverage::{Records, parse_lcov},
    grammar::read_source,
    language::{Language, language_of},
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
) -> Result<(i32, Duration), Failure> {
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
    Ok((result.code, result.elapsed))
}

/// `os.path.relpath`: `path` as seen from `base`, both absolute.
fn relative_from(base: &Path, path: &Path) -> Option<PathBuf> {
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

/// Everything one run's selection reads: the LCOV records of the languages
/// that report through LCOV, and the Lua spec index per module.
#[derive(Default)]
struct Coverage {
    records: Records,
    indexes: BTreeMap<PathBuf, Index>,
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
        return Ok(coverage);
    }
    if !standard {
        let command = options.value("--coverage-command");
        let Some(command) = command else {
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
        return Ok(coverage);
    }
    let rust_files: Vec<PathBuf> = files
        .iter()
        .filter(|file| language_of(file) == Some(Language::Rust))
        .cloned()
        .collect();
    if rust_files.is_empty() {
        return Ok(coverage);
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
        coverage.records.update(parse_lcov(&text));
    }
    Ok(coverage)
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

/// The lines this file's tests hit: the Lua index where one exists, else the
/// LCOV records. None means the file has no coverage data at all.
fn covered_lines(
    coverage: &Coverage,
    root: &Path,
    file: &Path,
    key: &str,
) -> Option<BTreeSet<usize>> {
    if language_of(file) == Some(Language::Lua)
        && let Some(index) = coverage.indexes.get(&lua_module(root, file))
    {
        return index.hits(file).cloned();
    }
    record_lines(&coverage.records, root, key)
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
        let (sites, forms) = sites::discover(&source, file, root, &key);
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

pub fn mutate(
    root: &Path,
    options: &Options,
    prepared: Vec<Prepared>,
    clock: impl Fn() -> String,
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
    let mut baselines: BTreeMap<(Command, PathBuf), Result<(i32, Duration), Failure>> =
        BTreeMap::new();
    let mut status = 0;
    let mut survived_any = false;
    // One pool for the whole call: its slots are the workers every target
    // file's mutants reuse, and dropping it removes them.
    let selected_counts: Vec<usize> = prepared
        .iter()
        .map(|item| {
            item.decisions
                .values()
                .filter(|decision| decision.action == "executed")
                .count()
        })
        .collect();
    let limit = workers::worker_count(usize::MAX, options.max_workers);
    let pool_size = limit.min(selected_counts.iter().copied().max().unwrap_or(0));
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
    for mut item in prepared {
        let (command, cwd) = plan(root, options, &item.file);
        let selected: Vec<Site> = item
            .sites
            .iter()
            .filter(|site| {
                item.decisions
                    .get(&site.mutation_id)
                    .is_some_and(|decision| decision.action == "executed")
            })
            .cloned()
            .collect();
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
                        .or_insert(Ok((0, Duration::from_secs_f64(index.seconds))));
                }
            }
            let run_cwd = if standard_lua {
                item.index
                    .as_ref()
                    .map_or_else(|| cwd.clone(), |index| index.directory.clone())
            } else {
                cwd.clone()
            };
            // One pristine run per command and working directory, shared by
            // every target file in this call.
            let mut pristine = |command: &Command,
                                build: bool,
                                timeout: Option<Duration>|
             -> Result<f64, Failure> {
                let key = (command.clone(), run_cwd.clone());
                if !baselines.contains_key(&key) {
                    let started = std::time::Instant::now();
                    let check = workers.view(0);
                    let result = (|| {
                        if build {
                            let built = execute(
                                &check,
                                root,
                                &run_cwd,
                                &Command::Shell("cargo test --no-run".into()),
                                None,
                                None,
                            )?;
                            if built.0 != 0 {
                                return Ok(built);
                            }
                        }
                        execute(&check, root, &run_cwd, command, None, timeout)
                    })();
                    timings.add("baseline", started.elapsed());
                    baselines.insert(key.clone(), result);
                }
                match baselines.get(&key) {
                    Some(Ok((0, duration))) => Ok(duration.as_secs_f64()),
                    Some(Ok((code, _))) => Err(Failure::Message(format!("exit {code}"))),
                    Some(Err(Failure::Unavailable(error))) => {
                        Err(Failure::Unavailable(error.clone()))
                    }
                    Some(Err(Failure::Message(error))) => Err(Failure::Message(error.clone())),
                    Some(Err(Failure::Interrupted)) => Err(Failure::Interrupted),
                    None => Err(Failure::Message("no pristine run".into())),
                }
            };
            let build = !standard_lua && options.value("--test-command").is_none();
            let seconds = match pristine(&command, build, None) {
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
                        &mut |plan: &Command| pristine(plan, false, None),
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
            if let Err(error) = pristine(
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
                .map(|(code, _)| code)
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
                        pristine(&full, false, Some(Duration::from_secs_f64(full_timeout)))
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
                            Ok((code, _)) => code,
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
                if outcome == "survived" {
                    survived_any = true;
                }
                item.outcomes.insert(mutation_id, outcome);
            }
        }
        let changed = fs::read(&item.file).ok() != Some(item.raw)
            || read_source(&item.file).ok() != Some(item.source);
        if changed {
            eprintln!("Source changed during mutation: {}", item.key);
            status = 4;
            continue;
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
        for form in &item.forms {
            if form.survived > 0 {
                survived_any = true;
            }
        }
        if let Err(error) = metrics::write_results(
            root,
            &item.key,
            &item.forms,
            &item.outcomes,
            &clock(),
            isolation::level(),
        ) {
            eprintln!("Cannot publish mutation snapshot: {error}");
            return 2;
        }
        let executed = item
            .decisions
            .values()
            .filter(|d| d.action == "executed")
            .count();
        let reused = item
            .decisions
            .values()
            .filter(|d| d.action == "reused")
            .count();
        println!("{}: executed={executed} reused={reused}", item.key);
    }
    if status == 4 {
        4
    } else if survived_any {
        3
    } else {
        0
    }
}
