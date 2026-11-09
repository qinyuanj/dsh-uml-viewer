//! `uml-quality.json` version 1 (ADR 0001, docs/uml-quality.schema.json),
//! validated as the frozen qinyuanj/uml-viewer `scripts/uml_quality.py`
//! `read_config` and `scope` do. All configured operations are validated
//! before any command runs.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Crap,
    Mutate,
    Dry,
}

impl Operation {
    pub fn parse(text: &str) -> Option<Operation> {
        match text {
            "crap" => Some(Operation::Crap),
            "mutate" => Some(Operation::Mutate),
            "dry" => Some(Operation::Dry),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Operation::Crap => "crap",
            Operation::Mutate => "mutate",
            Operation::Dry => "dry",
        }
    }

    /// The standard tool and its own valid exit set (`TOOLS` in
    /// `uml_quality.py`): crapper 0/2, mutator 0/3, dryer 0.
    pub fn tool(self) -> (&'static str, &'static [u8]) {
        match self {
            Operation::Crap => ("crapper", &[0, 2]),
            Operation::Mutate => ("mutator", &[0, 3]),
            Operation::Dry => ("dryer", &[0]),
        }
    }
}

/// Options taking one value inside `<tool>Options`.
pub const VALUES: &[&str] = &[
    "--coverage-command",
    "--threshold",
    "--test-command",
    "--busted-task",
    "--timeout-factor",
    "--mutation-warning",
    "--max-workers",
    "--lines",
    "--min-lines",
    "--min-nodes",
    "--format",
];
/// Scope and routing options only the dispatcher may set.
const ROUTING: &[&str] = &[
    "--root",
    "--source-root",
    "-s",
    "--exclude",
    "--full-tree",
    "--changed",
    "--mutate-all",
    "--edn",
    "--text",
];

/// `standardTools`: declared business scope and per-tool options.
#[derive(Clone, Debug, Default)]
pub struct Standard {
    /// Whether `standardTools` was declared at all.
    pub declared: bool,
    pub source_roots: Vec<String>,
    pub exclude: Vec<String>,
    pub crapper_options: Vec<String>,
    pub mutator_options: Vec<String>,
    pub dryer_options: Vec<String>,
}

impl Standard {
    pub fn options(&self, operation: Operation) -> &[String] {
        match operation {
            Operation::Crap => &self.crapper_options,
            Operation::Mutate => &self.mutator_options,
            Operation::Dry => &self.dryer_options,
        }
    }
}

/// One `operations` entry.
#[derive(Clone, Debug)]
pub struct Override {
    pub argv: Vec<String>,
    pub forward_args: bool,
    pub valid_exit_codes: Vec<u8>,
    /// `targets` or `whole-tree` for CRAP; None otherwise.
    pub crap_scope: Option<&'static str>,
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    standard: Standard,
    crap: Option<Override>,
    mutate: Option<Override>,
    dry: Option<Override>,
}

impl Config {
    pub fn standard(&self) -> &Standard {
        &self.standard
    }

    pub fn override_for(&self, operation: Operation) -> Option<&Override> {
        match operation {
            Operation::Crap => self.crap.as_ref(),
            Operation::Mutate => self.mutate.as_ref(),
            Operation::Dry => self.dry.as_ref(),
        }
    }

    /// The project's manifest; absent means the default standard dispatch.
    pub fn read(root: &Path) -> Result<Config, String> {
        let path = root.join("uml-quality.json");
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read uml-quality.json: {error}"))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("uml-quality.json is not valid JSON: {error}"))?;
        let config = parse(&value)?;
        config.check_scope(root)?;
        Ok(config)
    }

    /// `scope()`: roots are project-relative directories inside the root;
    /// exclusions are project-relative.
    fn check_scope(&self, root: &Path) -> Result<(), String> {
        for name in &self.standard.source_roots {
            if escapes(name) {
                return Err(format!(
                    "business source root must be a project-relative path: {name}"
                ));
            }
            let path = quality_core::paths::resolve(&root.join(name));
            if !path.starts_with(root) || !path.is_dir() {
                return Err(format!("invalid business source root: {name}"));
            }
        }
        for name in &self.standard.exclude {
            if escapes(name) {
                return Err(format!("invalid exclude: {name}"));
            }
        }
        Ok(())
    }
}

fn escapes(name: &str) -> bool {
    let path = PathBuf::from(name);
    path.is_absolute()
        || name.starts_with('/')
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
}

fn strings(value: &Value) -> Option<Vec<String>> {
    let list = value.as_array()?;
    list.iter()
        .map(|item| {
            item.as_str()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn parse(value: &Value) -> Result<Config, String> {
    let object = value.as_object();
    let valid = object.is_some_and(|object| {
        object.get("version").and_then(Value::as_u64) == Some(1)
            && object.get("version").is_some_and(Value::is_u64)
            && (object.contains_key("standardTools") || object.contains_key("operations"))
            && !object.contains_key("aliasResultExitCodes")
    });
    let Some(object) = object.filter(|_| valid) else {
        return Err(
            "uml-quality.json needs version 1 and standardTools or operations; \
             the Clojure alias route was removed"
                .to_string(),
        );
    };
    let mut config = Config::default();
    if let Some(standard) = object.get("standardTools") {
        config.standard = parse_standard(standard)?;
    }
    if let Some(operations) = object.get("operations") {
        let operations = operations
            .as_object()
            .filter(|map| map.contains_key("crap") && map.contains_key("mutate"))
            .ok_or("operations must declare crap and mutate")?;
        for operation in [Operation::Crap, Operation::Mutate, Operation::Dry] {
            let Some(entry) = operations.get(operation.name()) else {
                continue;
            };
            let parsed = parse_override(operation, entry)?;
            match operation {
                Operation::Crap => config.crap = Some(parsed),
                Operation::Mutate => config.mutate = Some(parsed),
                Operation::Dry => config.dry = Some(parsed),
            }
        }
    }
    Ok(config)
}

fn parse_standard(value: &Value) -> Result<Standard, String> {
    let map = value.as_object().ok_or("standardTools must be an object")?;
    let mut standard = Standard {
        declared: true,
        ..Standard::default()
    };
    for (key, value) in map {
        let list = strings(value).ok_or_else(|| format!("invalid standardTools.{key}"))?;
        if key.ends_with("Options") {
            check_options(key, &list)?;
        }
        match key.as_str() {
            "sourceRoots" => standard.source_roots = list,
            "exclude" => standard.exclude = list,
            "crapperOptions" => standard.crapper_options = list,
            "mutatorOptions" => standard.mutator_options = list,
            "dryerOptions" => standard.dryer_options = list,
            _ => return Err(format!("invalid standardTools.{key}")),
        }
    }
    Ok(standard)
}

fn check_options(key: &str, options: &[String]) -> Result<(), String> {
    let mut pending = false;
    for option in options {
        if pending {
            pending = false;
        } else if ROUTING.contains(&option.as_str()) || !option.starts_with("--") {
            return Err(format!(
                "{key} may not hold scope or positional arguments: {option}"
            ));
        } else {
            pending = VALUES.contains(&option.as_str());
        }
    }
    if pending {
        return Err(format!("{key} is missing an option value"));
    }
    Ok(())
}

fn parse_override(operation: Operation, value: &Value) -> Result<Override, String> {
    let name = operation.name();
    let invalid = || format!("invalid operations.{name}");
    let map = value.as_object().ok_or_else(invalid)?;
    let argv: Vec<String> = map
        .get("argv")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
        .and_then(|list| {
            list.iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
        })
        .filter(|argv| !argv[0].trim().is_empty())
        .ok_or_else(invalid)?;
    let forward_args = match map.get("forwardArgs").and_then(Value::as_str) {
        Some("append") => true,
        Some("none") => false,
        _ => return Err(invalid()),
    };
    let codes = map
        .get("validResultExitCodes")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
        .and_then(|list| {
            list.iter()
                .map(|code| {
                    code.as_u64()
                        .filter(|_| code.is_u64())
                        .and_then(|c| u8::try_from(c).ok())
                })
                .collect::<Option<Vec<u8>>>()
        })
        .ok_or_else(|| format!("{name} needs validResultExitCodes in 0-255"))?;
    let crap_scope = if operation == Operation::Crap {
        let scope = match map.get("scope").and_then(Value::as_str) {
            Some("targets") => "targets",
            Some("whole-tree") => "whole-tree",
            _ => "",
        };
        if scope.is_empty() || (scope == "whole-tree" && forward_args) {
            return Err(
                "whole-tree CRAP requires forwardArgs none; scope accepts targets or whole-tree"
                    .to_string(),
            );
        }
        Some(scope)
    } else {
        None
    };
    Ok(Override {
        argv,
        forward_args,
        valid_exit_codes: codes,
        crap_scope,
    })
}
