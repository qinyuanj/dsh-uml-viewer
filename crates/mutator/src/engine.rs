use crate::{
    cli::Options,
    interrupts,
    isolation::{Completed, Failure, View},
    metrics::{self, Decision},
    python, sites,
    sites::{Form, Site},
    typescript,
    workers::{self, Workers},
};
use quality_core::{
    coverage::{Records, parse_lcov},
    grammar::read_source,
    language::{Language, language_of},
    process::shlex_quote,
    runner::{modules_with, nearest},
};
use std::{
    collections::BTreeMap,
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
}
/// `runner.Command`: a user override stays a shell string; every generated
/// command is an argument vector, so a package path is one argument.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Command {
    Shell(String),
    Argv(Vec<String>),
}
impl Command {
    /// The view runs `/bin/sh -c`; an argument vector is quoted with shlex, as
    /// the original's `display_command` renders it.
    fn script(&self) -> String {
        match self {
            Command::Shell(command) => command.clone(),
            Command::Argv(argv) => argv
                .iter()
                .map(|arg| shlex_quote(arg))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
    /// `build_command`: only a generated `cargo test` has a compile step.
    fn build(&self) -> Option<Command> {
        match self {
            Command::Argv(argv) if argv.starts_with(&["cargo".into(), "test".into()]) => {
                let mut build = vec!["cargo".into(), "test".into(), "--no-run".into()];
                build.extend(argv.iter().skip(2).cloned());
                Some(Command::Argv(build))
            }
            _ => None,
        }
    }
}
fn plan(root: &Path, options: &Options, source: &Path) -> (Command, PathBuf) {
    if let Some(command) = options.value("--test-command") {
        return (Command::Shell(command.to_string()), root.to_path_buf());
    }
    let (argv, cwd) = match language_of(source) {
        Some(Language::Python) => python::test_plan(root, source),
        Some(Language::TypeScript) => typescript::test_plan(root, source),
        _ => (
            vec!["cargo".into(), "test".into()],
            nearest(source, "Cargo.toml", root).unwrap_or_else(|| root.to_path_buf()),
        ),
    };
    (Command::Argv(argv), cwd)
}
fn execute(
    view: &View,
    root: &Path,
    cwd: &Path,
    command: &Command,
    site: Option<(&Path, &str)>,
    timeout: Option<Duration>,
) -> Result<Completed, Failure> {
    let result = view
        .run(root, cwd, &command.script(), site, timeout)
        .map_err(|failure| match failure {
            Failure::Interrupted => Failure::Interrupted,
            Failure::Message(error) => Failure::Message(format!(
                "Cannot enter user+mount namespace overlay: {error}"
            )),
        })?;
    if !result.output.is_empty() {
        eprint!("{}", result.output);
    }
    Ok(result)
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
fn coverage(root: &Path, options: &Options, files: &[PathBuf]) -> Result<Records, i32> {
    let mut records = Records::default();
    if options.flag("--use-existing-coverage") || options.flag("--reuse-coverage") {
        collect_reports(&root.join("coverage"), &mut records);
        collect_reports(&root.join("target/coverage"), &mut records);
        return Ok(records);
    }
    let jobs = if let Some(command) = options.value("--coverage-command") {
        vec![(
            root.to_path_buf(),
            format!(
                "rm -f coverage/lcov.info coverage/luacov.stats.out luacov.stats.out\n{command}"
            ),
            PathBuf::from("coverage/lcov.info"),
        )]
    } else {
        let rust = modules_with(files, Language::Rust, "Cargo.toml", root);
        if !rust.is_empty() && quality_core::process::which("cargo-llvm-cov").is_none() {
            eprintln!("cargo-llvm-cov is required; mutation does not install tools.");
            return Err(2);
        }
        rust.into_iter()
            .map(|module| {
                let report = quality_core::runner::coverage_report(root, &module, "rust");
                let relative = report.strip_prefix(root).unwrap().to_path_buf();
                let command = format!(
                    "mkdir -p {}\ncargo llvm-cov --lcov --output-path {}",
                    quote(report.parent().unwrap()),
                    quote(&report)
                );
                (module, command, relative)
            })
            .collect::<Vec<(PathBuf, String, PathBuf)>>()
    };
    for (cwd, command, report) in jobs {
        let view = View::new(root).map_err(|error| {
            eprintln!("{error}");
            2
        })?;
        let result =
            view.run(root, &cwd, &command, None, None)
                .map_err(|failure| match failure {
                    // The interrupt path reports and cleans up; no exit here.
                    Failure::Interrupted => 2,
                    Failure::Message(error) => {
                        eprintln!("Isolation unavailable: {error}");
                        1
                    }
                })?;
        eprint!("{}", result.output);
        if result.code != 0 {
            eprintln!("Coverage command exited {}.", result.code);
            return Err(2);
        }
        let report = view.directory.join("upper").join(report);
        if options.value("--coverage-command").is_some() {
            let fresh =
                quality_core::coverage::read_fresh_lcov(&report, root).map_err(|error| {
                    eprintln!("Required LCOV report invalid or unavailable: {error}");
                    2
                })?;
            records.update(fresh);
        } else {
            let text = fs::read_to_string(report).map_err(|error| {
                eprintln!("Coverage report unavailable: {error}");
                2
            })?;
            records.update(parse_lcov(&text));
        }
    }
    python_coverage(root, files, &mut records)?;
    typescript_coverage(root, files, &mut records)?;
    Ok(records)
}

/// One generated command in a package's own overlay view.
fn step(view: &View, root: &Path, cwd: &Path, argv: &str) -> Result<Completed, i32> {
    let command = Command::Shell(argv.to_string());
    view.run(root, cwd, &command.script(), None, None)
        .map_err(|failure| match failure {
            Failure::Interrupted => 2,
            Failure::Message(error) => {
                eprintln!("Isolation unavailable: {error}");
                1
            }
        })
        .inspect(|result| eprint!("{}", result.output))
}

/// Read a report from the view's upper layer, anchored as crapper's runner
/// anchors it, and merge it into the records.
fn merge_report(records: &mut Records, view: &View, report: &Path, package: &Path, anchor: bool) {
    let path = view.directory.join("upper").join(report);
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
            2
        })?;
        step(&view, root, &package, &coverage.script)?;
        merge_report(records, &view, &coverage.report, &package, true);
    }
    Ok(())
}

/// `runners._typescript_coverage` per package, inside views.
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
            2
        })?;
        step(&view, root, &package, &coverage.script)?;
        merge_report(records, &view, &coverage.report, &package, !coverage.custom);
    }
    Ok(())
}
fn file_lines<'a>(
    records: &'a Records,
    root: &Path,
    file: &str,
) -> Option<&'a quality_core::coverage::Lines> {
    let absolute = quality_core::paths::posix(&root.join(file));
    let candidates = [file, &absolute];
    for candidate in candidates {
        for (source, lines) in records.iter() {
            if quality_core::coverage::normalize_path(source) == candidate {
                return Some(lines);
            }
        }
    }
    for (source, lines) in records.iter() {
        let key = quality_core::coverage::normalize_path(source);
        if key.ends_with(&format!("/{file}")) || file.ends_with(&format!("/{key}")) {
            return Some(lines);
        }
    }
    None
}
fn coverage_hit(records: &Records, root: &Path, file: &str, line: usize) -> bool {
    file_lines(records, root, file)
        .is_some_and(|lines| lines.get(&(line as u64)).is_some_and(|(hit, _)| *hit > 0))
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
        });
    }
    if options.flag("--scan") {
        // Scanning inventories sites: no coverage command runs and nothing runs.
        return Ok(prepared);
    }
    if prepared.iter().all(|item| reusable(root, item)) {
        return Ok(prepared);
    }
    let records = if options.flag("--no-coverage") {
        Records::default()
    } else {
        let started = std::time::Instant::now();
        let records = coverage(root, options, files)?;
        timings.add("coverage", started.elapsed());
        records
    };
    for item in &mut prepared {
        let history = metrics::load_history(root, &item.key);
        item.covered = item
            .sites
            .iter()
            .map(|site| {
                (
                    site.mutation_id.clone(),
                    options.flag("--no-coverage")
                        || coverage_hit(&records, root, &item.key, site.line),
                )
            })
            .collect();
        // `_warn_uncovered`: a file absent from every report has no coverage.
        if !options.flag("--no-coverage")
            && !item.sites.is_empty()
            && file_lines(&records, root, &item.key).is_none()
        {
            eprintln!(
                "No coverage data for {}; its sites are uncovered. Pass --no-coverage to run them anyway.",
                item.key
            );
        }
        item.decisions = metrics::decide(
            &item.sites,
            &item.covered,
            &history,
            &item.forms,
            options.flag("--mutate-all"),
            options.lines.as_ref(),
        );
        // Outcomes of sites this run does not execute survive, including the
        // covered sites a --lines filter excludes.
        item.outcomes = metrics::carry_forward(&item.sites, &item.covered, &history, &item.forms);
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
    let mut baselines = BTreeMap::new();
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
        let selected = item.decisions.values().any(|d| d.action == "executed");
        if selected {
            if interrupts::pending().is_some() {
                return 2;
            }
            let workers = pool.as_ref().expect("a selected site needs a worker");
            // One pristine run per command and working directory, shared by
            // every target file in this call.
            let baseline = baselines
                .entry((command.clone(), cwd.clone()))
                .or_insert_with(|| {
                    let started = std::time::Instant::now();
                    let check = workers.view(0);
                    let result = if let Some(build) = command.build() {
                        let built = execute(&check, root, &cwd, &build, None, None)?;
                        if built.code != 0 {
                            Ok(built)
                        } else {
                            execute(&check, root, &cwd, &command, None, None)
                        }
                    } else {
                        execute(&check, root, &cwd, &command, None, None)
                    };
                    timings.add("baseline", started.elapsed());
                    result
                });
            let (code, duration, output) = match baseline {
                Ok(value) => (value.code, value.elapsed, value.output.clone()),
                Err(Failure::Interrupted) => return 2,
                Err(Failure::Message(error)) => {
                    eprintln!("{error}");
                    return 2;
                }
            };
            // unittest before 3.12 exits 0 when discovery finds nothing;
            // every mutant would then survive a suite that tests nothing.
            let no_tests = output
                .lines()
                .any(|line| line.starts_with("Ran 0 tests in "));
            if code != 0 || no_tests {
                eprintln!("Baseline tests failed (exit {code}).");
                return 2;
            }
            let timeout =
                Duration::from_secs_f64((duration.as_secs_f64() * options.timeout_factor).max(2.0));
            let sites: Vec<Site> = item
                .sites
                .iter()
                .filter(|site| item.decisions[&site.mutation_id].action == "executed")
                .cloned()
                .collect();
            let count = workers::worker_count(sites.len(), options.max_workers);
            let source = &item.source;
            let file = &item.file;
            let outcomes = workers.run_selected(count, &sites, |view, site| {
                let mutated = sites::apply_site(source, site);
                execute(
                    view,
                    root,
                    &cwd,
                    &command,
                    Some((file, &mutated)),
                    Some(timeout),
                )
                .map(|completed| completed.code)
            });
            match outcomes {
                Ok(outcomes) => {
                    for (mutation_id, outcome) in outcomes {
                        if outcome == "survived" {
                            survived_any = true;
                        }
                        item.outcomes.insert(mutation_id, outcome);
                    }
                }
                Err(Failure::Interrupted) => return 2,
                Err(Failure::Message(error)) => {
                    eprintln!("{error}");
                    return 2;
                }
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
            "namespace-view",
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
