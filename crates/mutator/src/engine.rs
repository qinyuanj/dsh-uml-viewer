use crate::{
    cli::Options,
    isolation::View,
    metrics::{self, Decision},
    sites::{self, Form, Site},
};
use quality_core::{
    coverage::{Records, parse_lcov},
    grammar::read_source,
    runner::{modules_with, nearest},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

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
fn plan(root: &Path, options: &Options, source: &Path) -> (String, PathBuf) {
    if let Some(command) = options.value("--test-command") {
        return (command.to_string(), root.to_path_buf());
    }
    (
        "cargo test".into(),
        nearest(source, "Cargo.toml", root).unwrap_or_else(|| root.to_path_buf()),
    )
}
fn execute(
    root: &Path,
    cwd: &Path,
    command: &str,
    site: Option<(&Path, &str)>,
    timeout: Option<Duration>,
) -> Result<(i32, Duration), String> {
    let view = View::new().map_err(|e| e.to_string())?;
    let result = view
        .run(root, cwd, command, site, timeout)
        .map_err(|e| format!("Cannot enter user+mount namespace overlay: {e}"))?;
    if !result.output.is_empty() {
        eprint!("{}", result.output);
    }
    Ok((result.code, result.elapsed))
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
        if quality_core::process::which("cargo-llvm-cov").is_none() {
            eprintln!("cargo-llvm-cov is required; mutation does not install tools.");
            return Err(2);
        }
        modules_with(
            files,
            quality_core::language::Language::Rust,
            "Cargo.toml",
            root,
        )
        .into_iter()
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
        .collect()
    };
    for (cwd, command, report) in jobs {
        let view = View::new().map_err(|error| {
            eprintln!("{error}");
            2
        })?;
        let result = view
            .run(root, &cwd, &command, None, None)
            .map_err(|error| {
                eprintln!("Isolation unavailable: {error}");
                1
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
    Ok(records)
}
fn coverage_hit(records: &Records, root: &Path, file: &str, line: usize) -> bool {
    let absolute = quality_core::paths::posix(&root.join(file));
    let candidates = [file, &absolute];
    for candidate in candidates {
        for (source, lines) in records.iter() {
            if quality_core::coverage::normalize_path(source) == candidate {
                return lines.get(&(line as u64)).is_some_and(|(hit, _)| *hit > 0);
            }
        }
    }
    for (source, lines) in records.iter() {
        let key = quality_core::coverage::normalize_path(source);
        if key.ends_with(&format!("/{file}")) || file.ends_with(&format!("/{key}")) {
            return lines.get(&(line as u64)).is_some_and(|(hit, _)| *hit > 0);
        }
    }
    false
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
pub fn prepare(root: &Path, options: &Options, files: &[PathBuf]) -> Result<Vec<Prepared>, i32> {
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
    if prepared.iter().all(|item| reusable(root, item)) {
        return Ok(prepared);
    }
    let records = if options.flag("--no-coverage") {
        Records::default()
    } else {
        coverage(root, options, files)?
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
        item.decisions = metrics::decide(
            &item.sites,
            &item.covered,
            &history,
            &item.forms,
            options.flag("--mutate-all"),
            options.lines.as_ref(),
        );
        item.outcomes = if options.flag("--mutate-all") || options.lines.is_some() {
            BTreeMap::new()
        } else {
            metrics::carry_forward(&item.sites, &item.covered, &history, &item.forms)
        };
    }
    Ok(prepared)
}
pub fn mutate(
    root: &Path,
    options: &Options,
    prepared: Vec<Prepared>,
    clock: impl Fn() -> String,
) -> i32 {
    if options.flag("--scan") {
        for item in prepared {
            println!("Scan: {} mutation sites in {}", item.sites.len(), item.key);
            let history = metrics::load_history(root, &item.key);
            let changed = item.forms.iter().any(|form| {
                history
                    .forms
                    .get(&(form.namespace.clone(), form.id.clone()))
                    .is_some_and(|prior| prior.digest != form.digest)
            });
            for site in &item.sites {
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
    for mut item in prepared {
        let (command, cwd) = plan(root, options, &item.file);
        let selected = item.decisions.values().any(|d| d.action == "executed");
        if selected {
            let baseline = baselines
                .entry((command.clone(), cwd.clone()))
                .or_insert_with(|| {
                    if options.value("--test-command").is_none() {
                        let built = execute(root, &cwd, "cargo test --no-run", None, None)?;
                        if built.0 != 0 {
                            return Ok(built);
                        }
                    }
                    execute(root, &cwd, &command, None, None)
                });
            let (code, duration) = match baseline {
                Ok(value) => *value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            };
            if code != 0 {
                eprintln!("Baseline tests failed (exit {code}).");
                return 2;
            }
            let timeout =
                Duration::from_secs_f64((duration.as_secs_f64() * options.timeout_factor).max(2.0));
            for site in &item.sites {
                let decision = &item.decisions[&site.mutation_id];
                if options.flag("--verbose") {
                    println!(
                        "{}:{}: {} ({})",
                        site.file, site.line, decision.action, decision.reason
                    );
                }
                if decision.action != "executed" {
                    continue;
                }
                let mutated = sites::apply_site(&item.source, site);
                let result = execute(
                    root,
                    &cwd,
                    &command,
                    Some((&item.file, &mutated)),
                    Some(timeout),
                );
                let code = match result {
                    Ok((code, _)) => code,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                };
                let outcome = if code == 0 {
                    survived_any = true;
                    "survived"
                } else {
                    "killed"
                };
                item.outcomes
                    .insert(site.mutation_id.clone(), outcome.into());
            }
        }
        let changed = fs::read(&item.file).ok() != Some(item.raw)
            || read_source(&item.file).ok() != Some(item.source);
        if changed {
            eprintln!("Source changed during mutation: {}", item.key);
            status = 4;
            continue;
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
