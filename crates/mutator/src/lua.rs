//! Lua suites: coverage shards, the spec index, and the lines the tests hit.
//!
//! The pinned original collects one instrumented Busted suite per Lua module
//! (`lua_coverage.py` with `lua_coverage.lua`) and reads a per-spec line map
//! from it; `covered_lines` then takes the LuaCov LCOV report for the line a
//! mutation site sits on. This port keeps both contracts and splits the
//! instrumented suite into Busted coverage shards, merging the shard
//! publications into one index (the shard count is on stderr).
//!
//! Everything the collection writes — the collector, shard configs, LuaCov
//! stats, the LCOV report and the shard publications — lives in the
//! disposable view scratch, so the production project is never a write target.
use crate::isolation::View;
use crate::sites::Site;
use quality_core::coverage::{Records, read_fresh_lcov};
use quality_core::paths::{relative_posix, resolve};
use quality_core::process::{Program, output, which};
use quality_core::runners::lua;
use quality_core::serde_json::{self, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The instrumented collector, shipped with this build. Embedded assets live
/// outside `crates`, the repository's declared quality business source root.
const COLLECTOR: &str = include_str!("../../../embedded-assets/mutator/lua_coverage.lua");
/// The format version the collector and this reader share.
const VERSION: u64 = 2;

/// The canonical identity of a source path, as the original's `_canonical`.
pub fn canonical(path: &Path) -> String {
    resolve(path).to_string_lossy().replace('\\', "/")
}

/// One file's share of the index: which specs cover which lines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Index {
    /// The module directory the suite runs in.
    pub directory: PathBuf,
    /// The full-suite test command, without coverage arguments.
    pub command: Vec<String>,
    /// Spec path to its share of the suite's wall time, in seconds.
    pub specs: BTreeMap<String, f64>,
    /// Canonical source to covered line to the specs whose run hit it.
    pub lines: BTreeMap<String, BTreeMap<usize, BTreeSet<String>>>,
    /// Canonical source to lines whose aggregate hit has no known owner: the
    /// full suite has to judge those, never a selected subset.
    pub fallback: BTreeMap<String, BTreeSet<usize>>,
    /// Canonical source to the lines LuaCov recorded a hit for.
    pub hits: BTreeMap<String, BTreeSet<usize>>,
    /// Seconds the suite spent outside its spec files.
    pub overhead: f64,
    /// Wall time of the collecting run; not part of the published bytes.
    pub seconds: f64,
    /// The pristine suite ran in this process, so it is known to pass.
    pub fresh: bool,
}

impl Index {
    /// The specs that covered `line`, or None when the full suite must run.
    pub fn select(&self, source: &Path, line: usize) -> Option<Vec<String>> {
        let key = canonical(source);
        if self
            .fallback
            .get(&key)
            .is_some_and(|lines| lines.contains(&line))
        {
            return None;
        }
        let found = self.lines.get(&key)?.get(&line)?;
        (!found.is_empty()).then(|| found.iter().cloned().collect())
    }

    /// The original's `seconds_for`: overhead plus each distinct spec once.
    pub fn seconds_for(&self, specs: &[String]) -> f64 {
        self.overhead
            + specs
                .iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|spec| self.specs.get(spec).copied().unwrap_or_default())
                .sum::<f64>()
    }

    /// The lines this file's tests hit, or None when it is absent from LCOV.
    pub fn hits(&self, source: &Path) -> Option<&BTreeSet<usize>> {
        self.hits.get(&canonical(source))
    }

    /// Every spec of the suite, the ones covering these mutation lines first.
    /// The full suite still runs, so outcomes keep their full-suite meaning; a
    /// killing spec just runs early under `--no-keep-going` (ADR 0003).
    pub fn covering_first(&self, source: &Path, lines: &[usize]) -> Vec<String> {
        let mut covering = BTreeSet::new();
        for line in lines {
            if let Some(specs) = self.select(source, *line) {
                covering.extend(specs);
            }
        }
        let mut order: Vec<String> = covering.iter().cloned().collect();
        order.extend(
            self.specs
                .keys()
                .filter(|spec| !covering.contains(*spec))
                .cloned(),
        );
        order
    }
}

/// One shard's instrumented publication, as the collector writes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Membership {
    pub specs: BTreeMap<String, f64>,
    pub lines: BTreeMap<String, BTreeMap<usize, BTreeSet<String>>>,
    pub fallback: BTreeMap<String, BTreeSet<usize>>,
    /// A hit outside any spec's run: its owner is unknown, so it is a
    /// fallback line rather than a selected subset.
    pub unsafe_attribution: bool,
}

/// One module's collection.
pub struct Request<'a> {
    pub root: &'a Path,
    pub module: &'a Path,
    /// The module's sources to mutate; untested ones are still reported.
    pub sources: &'a [PathBuf],
    pub busted_task: Option<&'a str>,
    /// The Busted coverage shard count; defaults to one per CPU.
    pub shards: Option<u32>,
}

fn error(message: impl Into<String>) -> String {
    message.into()
}

/// The original's `_relative_spec`: a spec inside the module, or a refusal.
fn relative_spec(name: &str, module: &Path) -> Result<String, String> {
    let path = PathBuf::from(name.replace('\\', "/"));
    let path = if path.is_absolute() {
        path
    } else {
        module.join(path)
    };
    let path = resolve(&path);
    let relative = relative_posix(&path, &resolve(module))
        .ok_or_else(|| error("Invalid spec path"))?
        .to_string();
    if relative.is_empty()
        || relative
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        || !path.is_file()
    {
        return Err(error("Invalid spec path"));
    }
    Ok(relative)
}

/// The original's `_seconds`: a finite, non-negative number, never a bool.
fn seconds(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(found) if found.is_finite() && found >= 0.0 => Ok(found),
            _ => Err(error("Invalid spec duration")),
        },
        _ => Err(error("Invalid spec duration")),
    }
}

/// The original's `_line`: a decimal string above zero.
fn line(number: &str) -> Result<usize, String> {
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error("Invalid coverage line"));
    }
    match number.parse::<usize>() {
        Ok(found) if found > 0 => Ok(found),
        _ => Err(error("Invalid coverage line")),
    }
}

fn object<'a>(
    value: &'a Value,
    message: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    match value {
        Value::Object(fields) => Ok(fields),
        _ => Err(error(message)),
    }
}

/// Read one collector publication. Malformed input is refused, never guessed.
pub fn decode(pending: &str, module: &Path) -> Result<Membership, String> {
    let raw: Value =
        serde_json::from_str(pending).map_err(|_| error("Incomplete spec coverage publication"))?;
    let fields = object(&raw, "Incomplete spec coverage publication")?;
    if fields.get("version") != Some(&Value::from(VERSION))
        || fields.get("complete") != Some(&Value::Bool(true))
    {
        return Err(error("Incomplete spec coverage publication"));
    }
    let mut membership = Membership::default();
    for (name, elapsed) in object(
        fields
            .get("specs")
            .ok_or_else(|| error("Invalid spec name"))?,
        "Invalid spec name",
    )?
    .iter()
    {
        let relative = relative_spec(name, module)?;
        let elapsed = seconds(elapsed)?;
        *membership.specs.entry(relative).or_default() += elapsed;
    }
    if membership.specs.is_empty() {
        return Err(error("Busted did not complete any spec files"));
    }
    let source_key = |name: &str| -> Result<String, String> {
        if name.is_empty() {
            return Err(error("Invalid source path"));
        }
        let path = PathBuf::from(name.replace('\\', "/"));
        let located = if path.is_absolute() {
            path
        } else {
            module.join(path)
        };
        Ok(canonical(&located))
    };
    if let Some(covered) = fields.get("lines") {
        for (name, entries) in object(covered, "Invalid coverage mapping")?.iter() {
            let target = membership.lines.entry(source_key(name)?).or_default();
            for (number, members) in object(entries, "Invalid coverage mapping")?.iter() {
                let members = object(members, "Invalid spec membership")?;
                if members.is_empty() {
                    return Err(error("Invalid spec membership"));
                }
                let mut selected = BTreeSet::new();
                for member in members.keys() {
                    if !membership.specs.contains_key(member) {
                        return Err(error("Invalid spec membership"));
                    }
                    selected.insert(member.clone());
                }
                target.entry(line(number)?).or_default().extend(selected);
            }
        }
    }
    if let Some(covered) = fields.get("fallback") {
        for (name, entries) in object(covered, "Invalid cached fallback")?.iter() {
            let target = membership.fallback.entry(source_key(name)?).or_default();
            for (number, hit) in object(entries, "Invalid fallback membership")?.iter() {
                if hit != &Value::Bool(true) {
                    return Err(error("Invalid fallback membership"));
                }
                target.insert(line(number)?);
            }
        }
    }
    let Some(unsafe_attribution) = fields.get("unsafe").and_then(Value::as_bool) else {
        return Err(error("Missing attribution safety state"));
    };
    membership.unsafe_attribution = unsafe_attribution;
    if unsafe_attribution {
        // A hit outside any spec's context cannot be attributed: every line
        // the collector saw belongs to the full suite.
        for (name, covered) in &membership.lines {
            membership
                .fallback
                .entry(name.clone())
                .or_default()
                .extend(covered.keys().copied());
        }
    }
    Ok(membership)
}

/// Merge the shards' publications: spec durations add, memberships union.
pub fn merge(parts: Vec<Membership>) -> Membership {
    let mut merged = Membership::default();
    for part in parts {
        for (name, elapsed) in part.specs {
            *merged.specs.entry(name).or_default() += elapsed;
        }
        for (source, lines) in part.lines {
            let target = merged.lines.entry(source).or_default();
            for (number, members) in lines {
                target.entry(number).or_default().extend(members);
            }
        }
        for (source, lines) in part.fallback {
            merged.fallback.entry(source).or_default().extend(lines);
        }
        merged.unsafe_attribution |= part.unsafe_attribution;
    }
    merged
}

fn lua_string(text: &str) -> String {
    let mut equals = "==".to_string();
    while text.contains(&format!("]{equals}]")) {
        equals.push('=');
    }
    format!("[{equals}[{text}]{equals}]")
}

/// The machine's LuaRocks 5.4 trees, or none when luarocks is unusable.
fn rocks_trees() -> &'static [String] {
    static TREES: OnceLock<Vec<String>> = OnceLock::new();
    TREES.get_or_init(|| {
        let Some(luarocks) = which("luarocks") else {
            return Vec::new();
        };
        let argv = vec![
            luarocks.to_string_lossy().into_owned(),
            "--lua-version".into(),
            "5.4".into(),
            "config".into(),
            "rocks_trees".into(),
        ];
        let Ok(output) = output(
            &Program::Argv(&argv),
            &std::env::current_dir().unwrap_or_default(),
        ) else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut found = Vec::new();
        for (index, piece) in text.split('"').enumerate() {
            if index % 2 == 1 && piece.starts_with('/') && !found.iter().any(|tree| tree == piece) {
                found.push(piece.to_string());
            }
        }
        found
    })
}

fn absolute(entry: &str) -> bool {
    entry.starts_with('/')
}

/// `LUA_PATH`/`LUA_CPATH` confined to the LuaRocks trees, or None to keep
/// Lua's own defaults. The view redirects HOME, so the module search path has
/// to be handed to the child explicitly (`_pinned_lua_paths`).
pub fn search_paths(module: &Path) -> Option<(String, String)> {
    // The original's opt-out: keep Lua's own search path untouched.
    if std::env::var("MUTATOR_LUA_NO_PIN").as_deref() == Ok("1") {
        return None;
    }
    let trees = rocks_trees();
    if trees.is_empty()
        || !trees.iter().any(|tree| {
            Path::new(tree)
                .join("share/lua/5.4/busted/runner.lua")
                .is_file()
        })
    {
        return None;
    }
    let keep = |entry: &str| {
        !absolute(entry)
            || trees
                .iter()
                .any(|tree| entry == tree || entry.starts_with(&format!("{tree}/")))
    };
    let inherited = |names: [&str; 2]| {
        let mut entries: Vec<String> = Vec::new();
        for name in names {
            let Ok(value) = std::env::var(name) else {
                continue;
            };
            for entry in value
                .split(';')
                .filter(|entry| !entry.is_empty() && keep(entry))
            {
                if !entries.iter().any(|found| found == entry) {
                    entries.push(entry.to_string());
                }
            }
        }
        entries
    };
    let directory = module.to_string_lossy().replace('\\', "/");
    let mut path = inherited(["LUA_PATH_5_4", "LUA_PATH"]);
    let mut cpath = inherited(["LUA_CPATH_5_4", "LUA_CPATH"]);
    for entry in [
        format!("{directory}/?.lua"),
        format!("{directory}/?/init.lua"),
    ] {
        if !path.contains(&entry) {
            path.push(entry);
        }
    }
    let library = "so";
    let entry = format!("{directory}/?.{library}");
    if !cpath.contains(&entry) {
        cpath.push(entry);
    }
    for tree in trees {
        for template in ["share/lua/5.4/?.lua", "share/lua/5.4/?/init.lua"] {
            let entry = format!("{tree}/{template}");
            if !path.contains(&entry) {
                path.push(entry);
            }
        }
        for template in [
            format!("lib/lua/5.4/?.{library}"),
            format!("lib/lua/5.4/loadall.{library}"),
        ] {
            let entry = format!("{tree}/{template}");
            if !cpath.contains(&entry) {
                cpath.push(entry);
            }
        }
    }
    Some((path.join(";"), cpath.join(";")))
}

/// The original's Lua scheduling (`engine._prepare`): the sites that survived
/// the previous run go first, and each group keeps its discovery order. Only
/// the order of execution changes — the selection and the published bytes do
/// not — so a slow surviving mutant starts before a fresh kill can idle a
/// worker.
pub fn survivors_first(sites: &mut [Site], outcomes: &BTreeMap<String, String>) {
    sites.sort_by_key(|site| {
        outcomes.get(&site.mutation_id).map(String::as_str) != Some("survived")
    });
}

/// The full-suite command: Busted in process on Lua 5.4, module paths pinned.
pub fn plan(module: &Path, busted_task: Option<&str>) -> Vec<String> {
    let interpreter = lua::interpreter()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "lua5.4".into());
    let runner = "pcall(require, 'luarocks.loader'); require('busted.runner')({standalone = false}); os.exit(0)";
    let mut command = Vec::new();
    if let Some((path, cpath)) = search_paths(module) {
        command.extend(["env".to_string(), format!("LUA_PATH={path}")]);
        command.push(format!("LUA_CPATH={cpath}"));
    }
    command.extend([
        interpreter,
        "-e".into(),
        runner.into(),
        "--".into(),
        "busted".into(),
        "--ignore-lua".into(),
    ]);
    if let Some(task) = busted_task {
        command.extend(["--run".into(), task.into()]);
    }
    command
}

fn shard_count(requested: Option<u32>, specs: usize) -> usize {
    let wanted = requested.unwrap_or_else(|| {
        std::env::var("MUTATOR_LUA_SHARDS")
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|count| *count > 0)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|count| count.get() as u32)
                    .unwrap_or(1)
            })
    });
    (wanted.max(1) as usize).min(specs.max(1))
}

fn hits_of(records: &Records) -> BTreeMap<String, BTreeSet<usize>> {
    let mut hits: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for (source, lines) in records.iter() {
        let key = canonical(Path::new(source));
        let entry = hits.entry(key).or_default();
        for (number, (covered, _total)) in lines.iter() {
            if *covered > 0 {
                entry.insert(*number as usize);
            }
        }
    }
    hits
}

/// The conversion runs inside the view too, so it needs the same pinned
/// search paths as the suite: the view redirects HOME away from the rocks.
fn pinned(module: &Path, argv: &[String]) -> Vec<String> {
    match search_paths(module) {
        Some((path, cpath)) => {
            let mut out = vec![
                "env".to_string(),
                format!("LUA_PATH={path}"),
                format!("LUA_CPATH={cpath}"),
            ];
            out.extend(argv.iter().cloned());
            out
        }
        None => argv.to_vec(),
    }
}

/// Collect one module's index in a disposable view.
pub fn collect(view: &View, request: &Request<'_>) -> Result<Index, String> {
    let root = resolve(request.root);
    let module = resolve(request.module);
    let interpreter = lua::interpreter()
        .ok_or_else(|| error("No Lua 5.4 interpreter (lua5.4 or lua) on PATH for Lua coverage."))?;
    // A missing or other cluacov version cannot produce this baseline, so the
    // refusal happens before any test runs.
    lua::require_cluacov(&interpreter).map_err(|found| found.0)?;
    let scratch = view.directory.join("lua-coverage");
    fs::create_dir_all(&scratch)
        .map_err(|err| error(format!("Cannot prepare Lua coverage: {err}")))?;
    let collector = scratch.join("lua_coverage.lua");
    fs::write(&collector, COLLECTOR)
        .map_err(|err| error(format!("Cannot prepare Lua coverage: {err}")))?;
    let stats = scratch.join("luacov.stats.out");
    let report = scratch.join("lcov.info");
    let command = plan(&module, request.busted_task);
    let sources = request.sources;
    // The shard list is the suite that will really run: a named task's specs.
    let specs = lua::module_specs_for_task(&interpreter, &module, request.busted_task);
    let count = shard_count(request.shards, specs.len());
    let mut publications = Vec::new();
    let mut parts = Vec::new();
    let groups = if count > 1 {
        lua::shard_specs(&specs, count, &module)
    } else {
        vec![Vec::new()]
    };
    let started = std::time::Instant::now();
    let mut failed = None;
    for (index, group) in groups.iter().enumerate() {
        if count > 1 {
            // One line per shard, as the coverage runner's own report: the
            // shard count and this shard's launch are both observable.
            eprintln!(
                "+ ({}) Busted in {} coverage shards: {} -e '<busted runner>' -- busted …",
                module.display(),
                count,
                interpreter.display()
            );
        }
        let part = scratch.join(format!("luacov.shard-{}.stats.out", index + 1));
        let config = scratch.join(format!("luacov.shard-{}.cfg.lua", index + 1));
        fs::write(
            &config,
            lua::lua_coverage_config(&part, &report, sources, &module),
        )
        .map_err(|err| error(format!("Cannot prepare LuaCov config: {err}")))?;
        let pending = scratch.join(format!("spec-coverage.pending-{}.json", index + 1));
        let mut argv = command.clone();
        let Some(entry) = argv.iter().position(|arg| arg == "-e") else {
            return Err(error("Unsupported Busted bootstrap"));
        };
        let marker = "require('busted.runner')";
        let Some(loader) = argv.get(entry + 1).cloned() else {
            return Err(error("Unsupported Busted bootstrap"));
        };
        let Some((before, after)) = loader.split_once(marker) else {
            return Err(error("Unsupported Busted bootstrap"));
        };
        let bootstrap = format!(
            "local mutator_coverage_finish = dofile({})({}); ",
            lua_string(&collector.to_string_lossy()),
            lua_string(&pending.to_string_lossy())
        );
        let Some(ending) = after.strip_suffix("; os.exit(0)") else {
            return Err(error("Unsupported Busted completion"));
        };
        argv[entry + 1] =
            format!("{before}{bootstrap}{marker}{ending}; mutator_coverage_finish(); os.exit(0)");
        argv.extend([
            "-c".into(),
            format!("--coverage-config-file={}", config.display()),
        ]);
        argv.extend(group.clone());
        let result = view
            .run_argv(&root, &module, &argv, None, None)
            .map_err(|err| error(format!("Coverage command failed to start: {err}")))?;
        eprint!("{}", result.output);
        if result.code != 0 {
            failed = failed.or(Some((result.code, index + 1)));
        }
        parts.push(part);
        publications.push(pending);
    }
    let elapsed = started.elapsed().as_secs_f64();
    if let Some((code, shard)) = failed {
        let position = if count > 1 {
            format!(" in coverage shard {shard} of {count}")
        } else {
            String::new()
        };
        return Err(error(format!(
            "Lua tests exited {code}{position} in {}.",
            module.display()
        )));
    }
    let mut decoded = Vec::new();
    for pending in &publications {
        let text = fs::read_to_string(pending).map_err(|err| {
            error(format!(
                "Invalid Lua spec coverage in {}: {err}",
                module.display()
            ))
        })?;
        decoded.push(decode(&text, &module).map_err(|err| {
            error(format!(
                "Invalid Lua spec coverage in {}: {err}",
                module.display()
            ))
        })?);
    }
    let membership = merge(decoded);
    lua::merge_stats(&parts, &stats)
        .map_err(|err| error(format!("Cannot sum LuaCov stats: {}", err.0)))?;
    if !stats.is_file() {
        return Err(error(format!(
            "Invalid Lua spec coverage in {}: LuaCov stats missing or empty after suite",
            module.display()
        )));
    }
    let config = scratch.join("luacov.cfg.lua");
    fs::write(
        &config,
        lua::lua_coverage_config(&stats, &report, sources, &module),
    )
    .map_err(|err| error(format!("Cannot prepare LuaCov config: {err}")))?;
    let convert = lua::lua_coverage_commands(&interpreter, &config).1;
    let result = view
        .run_argv(&root, &module, &pinned(&module, &convert), None, None)
        .map_err(|err| error(format!("Coverage command failed to start: {err}")))?;
    eprint!("{}", result.output);
    if result.code != 0 {
        return Err(error(format!(
            "LuaCov LCOV conversion exited {} in {}.",
            result.code,
            module.display()
        )));
    }
    let records = read_fresh_lcov(&report, &module).map_err(|err| {
        error(format!(
            "Required LCOV report invalid or unavailable: {}: {err}",
            report.display()
        ))
    })?;
    let hits = hits_of(&records);
    let mut fallback = membership.fallback;
    // Any aggregate hit without a known owner must force the full suite.
    for (source, covered) in &hits {
        let owned = membership.lines.get(source);
        for number in covered {
            if !owned.is_some_and(|lines| lines.contains_key(number)) {
                fallback.entry(source.clone()).or_default().insert(*number);
            }
        }
    }
    let total: f64 = membership.specs.values().sum();
    Ok(Index {
        directory: module,
        command,
        specs: membership.specs,
        lines: membership.lines,
        fallback,
        hits,
        overhead: (elapsed - total).max(0.0),
        seconds: elapsed,
        fresh: true,
    })
}
