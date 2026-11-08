//! Lua 5.4 + Busted/LuaCov coverage. cluacov 1.0.0 is required: its C line
//! hook determines which source lines are executable, hence the pinned contract.
use super::typescript::anchor;
use crate::coverage::{Records, read_fresh_lcov};
use crate::paths::{relative_posix, resolve};
use crate::process::{Program, capture, which};
use crate::runner::{CoverageError, coverage_report, run_shell};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const LUA_EXCLUDE: &[&str] = &[
    "^/",
    "^%a:",
    "_spec$",
    "^spec/",
    "/spec/",
    "^lua_modules/",
    "^%.luarocks/",
    "^target/",
];
fn error(message: impl Into<String>) -> CoverageError {
    CoverageError(message.into())
}
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_string()).collect()
}

pub fn modules(root: &Path, files: &[PathBuf]) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    let root = resolve(root);
    let mut memo = BTreeMap::<PathBuf, Option<PathBuf>>::new();
    let mut found = BTreeMap::<PathBuf, Vec<PathBuf>>::new();
    for file in files
        .iter()
        .filter(|file| file.extension().is_some_and(|ext| ext == "lua"))
    {
        let file = resolve(file);
        let mut directory = file.parent().unwrap_or(&file).to_path_buf();
        let mut visited = Vec::new();
        let module = loop {
            if let Some(module) = memo.get(&directory) {
                break module.clone();
            }
            if directory.join(".busted").is_file()
                || fs::read_dir(&directory)
                    .ok()
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|entry| {
                        entry
                            .path()
                            .extension()
                            .is_some_and(|ext| ext == "rockspec")
                    })
            {
                break Some(directory.clone());
            }
            if directory == root || directory.parent().is_none() {
                break None;
            }
            visited.push(directory.clone());
            directory = directory.parent().unwrap().to_path_buf();
        };
        for directory in visited {
            memo.insert(directory, module.clone());
        }
        let module = module.unwrap_or_else(|| root.clone());
        found.entry(module).or_default().push(file);
    }
    found
}

pub fn interpreter() -> Option<PathBuf> {
    ["lua5.4", "lua"].iter().find_map(|name| {
        let path = which(name)?;
        let argv = strings(&[path.to_str()?, "-e", "io.write(_VERSION)"]);
        let output = capture(&Program::Argv(&argv), &std::env::current_dir().ok()?).ok()?;
        (String::from_utf8_lossy(&output.stdout).trim() == "Lua 5.4").then_some(resolve(&path))
    })
}

pub fn require_cluacov(lua: &Path) -> Result<(), CoverageError> {
    let code = "pcall(require, 'luarocks.loader'); local ok, v = pcall(require, 'cluacov.version'); if ok and type(v) == 'string' then io.write(v) end";
    let argv = strings(&[lua.to_str().unwrap_or("lua"), "-e", code]);
    let found = capture(
        &Program::Argv(&argv),
        &std::env::current_dir().unwrap_or_default(),
    )
    .ok()
    .filter(|output| output.status.success())
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    .unwrap_or_default();
    if found != "1.0.0" {
        return Err(error(format!(
            "Lua coverage requires cluacov 1.0.0 for {} ({}): luarocks --lua-version=5.4 install cluacov 1.0.0-1",
            lua.display(),
            if found.is_empty() {
                "not installed".into()
            } else {
                format!("found {found}")
            }
        )));
    }
    Ok(())
}

fn lua_string(text: &str) -> String {
    let mut equals = "==".to_string();
    while text.contains(&format!("]{equals}]")) {
        equals.push('=');
    }
    format!("[{equals}[{text}]{equals}]")
}

fn include_pattern(path: &str) -> [String; 2] {
    let stem = path.strip_suffix(".lua").unwrap_or(path);
    let escaped: String = stem
        .chars()
        .flat_map(|c| {
            if "\\^$()%.[]*+-?".contains(c) {
                vec!['%', c]
            } else {
                vec![c]
            }
        })
        .collect();
    [format!("^{escaped}$"), format!("^%./{escaped}$")]
}

pub fn lua_coverage_config(
    stats: &Path,
    report: &Path,
    files: &[PathBuf],
    module: &Path,
) -> String {
    let selected: BTreeSet<String> = files
        .iter()
        .filter_map(|file| relative_posix(&resolve(file), module))
        .collect();
    let items = |values: Vec<String>| {
        values
            .iter()
            .map(|s| lua_string(s))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "return {{\n  statsfile = {},\n  reportfile = {},\n  includeuntestedfiles = {{{}}},\n  include = {{{}}},\n  exclude = {{{}}},\n}}\n",
        lua_string(&stats.to_string_lossy()),
        lua_string(&report.to_string_lossy()),
        items(selected.iter().cloned().collect()),
        items(
            selected
                .iter()
                .flat_map(|path| include_pattern(path))
                .collect()
        ),
        items(LUA_EXCLUDE.iter().map(|s| (*s).to_string()).collect())
    )
}

const LOADER: &str = "pcall(require, 'luarocks.loader'); ";
pub fn lua_coverage_commands(lua: &Path, config: &Path) -> (Vec<String>, Vec<String>) {
    let binary = lua.to_string_lossy();
    let run = format!("{LOADER}require('busted.runner')({{standalone = false}}); os.exit(0)");
    let convert = format!(
        "{LOADER}local r = require('luacov.runner'); local c = r.load_config(arg[1]); local real_name = r.real_name; local windows = package.config:sub(1, 1) == string.char(92); r.real_name = function(name) name = real_name(name); if windows then name = name:gsub(string.char(92), '/') end; return (name:gsub('^%./', '')) end; c.reporter = 'lcov'; r.run_report(c); os.exit(0)"
    );
    (
        vec![
            binary.to_string(),
            "-e".into(),
            run,
            "--".into(),
            "busted".into(),
            "--ignore-lua".into(),
            "-c".into(),
            format!("--coverage-config-file={}", config.display()),
        ],
        vec![
            binary.to_string(),
            "-e".into(),
            convert,
            "--".into(),
            "luacov".into(),
            config.to_string_lossy().into_owned(),
        ],
    )
}

const SPECS: &str = "pcall(require, 'luarocks.loader'); local options = require('pl.tablex').update(require('busted.options'), {standalone = false}); local args = assert(require('busted.modules.cli')(options):parse({'--ignore-lua'})); local path = require('pl.path'); assert(path.chdir(path.normpath(args.directory))); local _, _, list = require('busted.modules.test_file_loader')(require('busted.core')(), args.loaders); for _, file in ipairs(list(args.ROOT, args.pattern, {excludes = args['exclude-pattern'], recursive = args.recursive})) do io.write(file, string.char(10)) end";

pub fn module_specs(lua: &Path, module: &Path) -> Vec<String> {
    module_specs_for_task(lua, module, None)
}

/// The same listing for a named `.busted` task. Without one the listing script
/// is unchanged; with one the task is a long-bracket literal, never quoted by
/// hand, so a task name cannot become Lua source.
pub fn module_specs_for_task(lua: &Path, module: &Path, task: Option<&str>) -> Vec<String> {
    let script = match task {
        Some(task) => SPECS.replacen(
            "{'--ignore-lua'}",
            &format!("{{'--ignore-lua', '--run', {}}}", lua_string(task)),
            1,
        ),
        None => SPECS.to_string(),
    };
    let argv = strings(&[lua.to_str().unwrap_or("lua"), "-e", &script]);
    match capture(&Program::Argv(&argv), module) {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        output => {
            eprintln!(
                "Cannot list Busted spec files in {}; running one coverage shard.\n{}",
                module.display(),
                output
                    .map(|o| String::from_utf8_lossy(&o.stderr).into_owned())
                    .unwrap_or_default()
            );
            Vec::new()
        }
    }
}

pub fn shard_specs(specs: &[String], count: usize, module: &Path) -> Vec<Vec<String>> {
    let mut result = vec![Vec::new(); count];
    let mut loads = vec![0_u64; count];
    let order: BTreeMap<&str, usize> = specs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let mut sorted: Vec<&String> = specs.iter().collect();
    sorted.sort_by_key(|s| {
        (
            std::cmp::Reverse(fs::metadata(module.join(s)).map(|m| m.len()).unwrap_or(0)),
            order[s.as_str()],
        )
    });
    for spec in sorted {
        let lightest = (0..count).min_by_key(|i| (loads[*i], *i)).unwrap();
        let size = fs::metadata(module.join(spec))
            .map(|m| m.len())
            .unwrap_or(0);
        result[lightest].push(spec.clone());
        loads[lightest] += size;
    }
    for shard in &mut result {
        shard.sort_by_key(|s| order[s.as_str()]);
    }
    result
}

/// Mutator keeps the original unqualified merge error and wraps it itself.
pub fn merge_stats(parts: &[PathBuf], target: &Path) -> Result<(), CoverageError> {
    merge_stats_in(parts, target, None)
}

fn merge_stats_in(
    parts: &[PathBuf],
    target: &Path,
    module: Option<&Path>,
) -> Result<(), CoverageError> {
    let failure = |detail: String| {
        error(match module {
            Some(module) => format!("Cannot sum LuaCov stats in {}: {detail}", module.display()),
            None => format!("Cannot sum LuaCov stats: {detail}"),
        })
    };
    let mut merged = BTreeMap::<Vec<u8>, Vec<u64>>::new();
    for part in parts {
        let bytes = fs::read(part).map_err(|e| failure(format!("{e}")))?;
        let lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
        for pair in lines.chunks(2) {
            if pair.len() != 2 {
                continue;
            }
            let name = pair[0]
                .splitn(2, |b| *b == b':')
                .nth(1)
                .unwrap_or_default()
                .to_vec();
            let hits = merged.entry(name).or_default();
            for (index, count) in pair[1]
                .split(|b| *b == b' ')
                .filter(|s| !s.is_empty())
                .enumerate()
            {
                if hits.len() == index {
                    hits.push(0);
                }
                hits[index] += std::str::from_utf8(count)
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or_else(|| failure("invalid hit count".to_string()))?;
            }
        }
    }
    let mut bytes = Vec::new();
    for (name, hits) in merged {
        bytes.extend(format!("{}:{}\n", hits.len(), String::from_utf8_lossy(&name)).as_bytes());
        for hit in hits {
            bytes.extend(format!("{hit} ").as_bytes());
        }
        bytes.push(b'\n');
    }
    fs::write(target, bytes).map_err(|e| failure(format!("{e}")))
}

fn prepare(
    root: &Path,
    files: &[PathBuf],
    shards_count: u32,
) -> Result<Option<Records>, CoverageError> {
    let modules = modules(root, files);
    if modules.is_empty() {
        return Ok(None);
    }
    let reports: Vec<(PathBuf, PathBuf)> = modules
        .keys()
        .map(|module| (module.clone(), coverage_report(root, module, "lua")))
        .collect();
    let unique: BTreeSet<&PathBuf> = reports.iter().map(|(_, report)| report).collect();
    if unique.len() != reports.len() {
        return Err(error("Lua modules have colliding managed report paths."));
    }
    for (_, report) in &reports {
        let mut artifacts = vec![
            report.clone(),
            report.parent().unwrap().join("luacov.stats.out"),
        ];
        if let Ok(entries) = fs::read_dir(report.parent().unwrap()) {
            artifacts.extend(entries.flatten().map(|e| e.path()).filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("luacov.shard-"))
            }));
        }
        for path in artifacts {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(error(format!(
                        "Cannot invalidate managed coverage artifact: {e}"
                    )));
                }
            }
        }
    }
    let lua = interpreter()
        .ok_or_else(|| error("No Lua 5.4 interpreter (lua5.4 or lua) on PATH for Lua coverage."))?;
    require_cluacov(&lua)?;
    let mut records = Records::default();
    for (module, report) in reports {
        let stats = report.parent().unwrap().join("luacov.stats.out");
        let cfg = report.parent().unwrap().join("luacov.cfg.lua");
        fs::create_dir_all(report.parent().unwrap())
            .map_err(|e| error(format!("Cannot prepare LuaCov config: {e}")))?;
        fs::write(
            &cfg,
            lua_coverage_config(&stats, &report, &modules[&module], &module),
        )
        .map_err(|e| error(format!("Cannot prepare LuaCov config: {e}")))?;
        let (run, lcov) = lua_coverage_commands(&lua, &cfg);
        let specs = if shards_count > 1 {
            module_specs(&lua, &module)
        } else {
            Vec::new()
        };
        let count = (shards_count as usize).min(specs.len());
        let code = if count > 1 {
            let groups = shard_specs(&specs, count, &module);
            let mut parts = Vec::new();
            let mut exit = 0;
            let mut failed_shard = 0;
            let mut outputs = Vec::new();
            for (index, group) in groups.iter().enumerate() {
                let part = report
                    .parent()
                    .unwrap()
                    .join(format!("luacov.shard-{}.stats.out", index + 1));
                let config_path = report
                    .parent()
                    .unwrap()
                    .join(format!("luacov.shard-{}.cfg.lua", index + 1));
                fs::write(
                    &config_path,
                    lua_coverage_config(&part, &report, &modules[&module], &module),
                )
                .map_err(|e| error(format!("Cannot prepare LuaCov config: {e}")))?;
                let mut command = lua_coverage_commands(&lua, &config_path).0;
                command.extend(group.clone());
                eprintln!(
                    "+ ({}) Busted in {} coverage shards: {} -e '<busted runner>' -- busted …",
                    module.display(),
                    count,
                    lua.display()
                );
                let output = capture(&Program::Argv(&command), &module)
                    .map_err(|e| error(format!("Coverage command failed to start: {e}")))?;
                if !output.status.success() {
                    exit = output.status.code().unwrap_or(1);
                    failed_shard = index + 1;
                }
                outputs.push(output);
                parts.push(part);
            }
            for output in outputs {
                print!("{}", String::from_utf8_lossy(&output.stdout));
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
            }
            if exit != 0 {
                for path in &parts {
                    let _ = fs::remove_file(path);
                }
                let leftovers: Vec<PathBuf> = fs::read_dir(report.parent().unwrap())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("luacov.shard-"))
                    })
                    .collect();
                for path in leftovers {
                    fs::remove_file(path).map_err(|e| {
                        error(format!("Cannot invalidate managed coverage artifact: {e}"))
                    })?;
                }
                return Err(error(format!(
                    "Lua tests exited {exit} in coverage shard {failed_shard} of {count} in {}.",
                    module.display()
                )));
            }
            for part in &parts {
                if !part.is_file() {
                    return Err(error(format!(
                        "LuaCov stats missing after Lua tests in {}: {}",
                        module.display(),
                        part.display()
                    )));
                }
            }
            merge_stats_in(&parts, &stats, Some(&module))?;
            let leftovers: Vec<PathBuf> = fs::read_dir(report.parent().unwrap())
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("luacov.shard-"))
                })
                .collect();
            for path in leftovers {
                fs::remove_file(path).map_err(|e| {
                    error(format!("Cannot invalidate managed coverage artifact: {e}"))
                })?;
            }
            exit
        } else {
            run_shell(&Program::Argv(&run), &module)
        };
        if code != 0 {
            return Err(error(format!(
                "Lua tests exited {code} in {}.",
                module.display()
            )));
        }
        if !stats.is_file() {
            return Err(error(format!(
                "LuaCov stats missing after Lua tests in {}: {}",
                module.display(),
                stats.display()
            )));
        }
        let code = run_shell(&Program::Argv(&lcov), &module);
        if code != 0 {
            return Err(error(format!(
                "LuaCov LCOV conversion exited {code} in {}.",
                module.display()
            )));
        }
        if !report.is_file() {
            return Err(error(format!(
                "LuaCov LCOV report missing after conversion in {}: {}",
                module.display(),
                report.display()
            )));
        }
        anchor(&report, &module).map_err(|e| {
            error(format!(
                "LuaCov LCOV normalization failed in {}: {e}",
                module.display()
            ))
        })?;
        records.update(read_fresh_lcov(&report, &module).map_err(|e| {
            error(format!(
                "Required LCOV report invalid or unavailable: {}: {e}",
                report.display()
            ))
        })?);
    }
    Ok(Some(records))
}

pub fn cover(
    root: &Path,
    files: &[PathBuf],
    shards: Option<u32>,
) -> Result<Option<Records>, CoverageError> {
    prepare(
        root,
        files,
        shards.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1)
        }),
    )
}
