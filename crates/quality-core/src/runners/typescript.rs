//! TypeScript coverage through a project's script, Vitest, or c8.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::language::{Language, is_test_file, language_of};
use crate::paths::{posix, relative_posix, resolve};
use crate::process::Program;
use crate::runner::{coverage_report, modules_with, run_shell};

const VITEST_CONFIGS: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.mts",
    "vitest.config.cts",
    "vitest.config.js",
    "vitest.config.mjs",
    "vitest.config.cjs",
];

fn json(path: &Path) -> Option<Value> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    value.is_object().then_some(value)
}

fn command(package: &Path, files: &[PathBuf], report_dir: &Path) -> Option<Vec<String>> {
    let data = json(&package.join("package.json"))?;
    let scripts = data.get("scripts").and_then(Value::as_object);
    if scripts.is_some_and(|scripts| scripts.contains_key("coverage")) {
        return Some(vec!["npm".into(), "run".into(), "coverage".into()]);
    }
    let uses_vitest = scripts
        .and_then(|scripts| scripts.get("test"))
        .is_some_and(|test| {
            test.as_str().map_or_else(
                || test.to_string().contains("vitest"),
                |s| s.contains("vitest"),
            )
        })
        || VITEST_CONFIGS
            .iter()
            .any(|name| package.join(name).is_file());
    if uses_vitest {
        let binary = package.join("node_modules/.bin/vitest");
        let mut command = if binary.is_file() {
            vec![binary.to_string_lossy().into_owned()]
        } else {
            vec!["npx".into(), "vitest".into()]
        };
        command.extend([
            "run".into(),
            "--coverage".into(),
            "--coverage.reporter=lcov".into(),
            format!("--coverage.reportsDirectory={}", report_dir.display()),
            "--coverage.reportOnFailure=true".into(),
        ]);
        let package = resolve(package);
        for file in files {
            if language_of(file) != Some(Language::TypeScript) || is_test_file(file) {
                continue;
            }
            if let Some(relative) = relative_posix(&resolve(file), &package) {
                command.push(format!("--coverage.include={relative}"));
            }
        }
        return Some(command);
    }
    if !scripts.is_some_and(|scripts| scripts.contains_key("test")) {
        return None;
    }
    Some(
        ["npx", "--yes", "c8", "--reporter=lcov", "--reports-dir"]
            .into_iter()
            .map(str::to_string)
            .chain([
                report_dir.to_string_lossy().into_owned(),
                "npm".into(),
                "test".into(),
            ])
            .collect(),
    )
}

fn ensure_vitest_coverage(package: &Path) -> io::Result<bool> {
    let provider = package.join("node_modules/@vitest/coverage-v8");
    if provider.is_dir() {
        return Ok(true);
    }
    let installed = json(&package.join("node_modules/vitest/package.json"));
    let version = installed
        .as_ref()
        .and_then(|data| data.get("version"))
        .and_then(Value::as_str);
    let spec = version.map_or_else(
        || "@vitest/coverage-v8".into(),
        |version| format!("@vitest/coverage-v8@{version}"),
    );
    eprintln!(
        "Installing {spec} so Vitest can write LCOV. package.json and the lockfile are restored afterward."
    );
    let snapshot: Vec<(PathBuf, Option<Vec<u8>>)> =
        ["package.json", "package-lock.json", "npm-shrinkwrap.json"]
            .into_iter()
            .map(|name| {
                let path = package.join(name);
                let bytes = if path.is_file() {
                    Some(fs::read(&path)?)
                } else {
                    None
                };
                Ok((path, bytes))
            })
            .collect::<io::Result<_>>()?;
    let command = vec![
        "npm".into(),
        "install".into(),
        "--no-save".into(),
        "--no-package-lock".into(),
        spec,
    ];
    let code = run_shell(&Program::Argv(&command), package);
    // Restore all three manifests even if one restoration fails.
    let mut restore_error = None;
    for (path, bytes) in snapshot {
        let restored = match bytes {
            Some(bytes) => match fs::read(&path) {
                Ok(current) if current == bytes => Ok(()),
                _ => fs::write(&path, bytes),
            },
            None if path.is_file() => fs::remove_file(&path),
            None => Ok(()),
        };
        if let Err(error) = restored {
            restore_error.get_or_insert(error);
        }
    }
    if let Some(error) = restore_error {
        return Err(error);
    }
    let ready = code == 0 && provider.is_dir();
    if !ready {
        eprintln!("Vitest's coverage provider is missing. TypeScript coverage will be N/A.");
    }
    Ok(ready)
}

/// Rewrite relative LCOV sources using the coverage command's working directory.
/// Absolute paths (including Windows drives on Unix) remain byte-for-byte intact.
pub(crate) fn anchor(report: &Path, module: &Path) -> io::Result<()> {
    let text = fs::read_to_string(report)?;
    let mut output = String::new();
    // Python str.splitlines() includes these Unicode line boundaries too.
    let normalized = text.replace("\r\n", "\n");
    let lines = normalized.split([
        '\n', '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}',
        '\u{2029}',
    ]);
    let mut lines: Vec<&str> = lines.collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    for line in lines {
        if let Some(source) = line.strip_prefix("SF:") {
            let bytes = source.as_bytes();
            let windows_drive =
                bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
            let windows_unc = source.replace('\\', "/").starts_with("//");
            if !Path::new(source).is_absolute() && !windows_drive && !windows_unc {
                output.push_str("SF:");
                let path: PathBuf = module
                    .join(source.replace('\\', "/"))
                    .components()
                    .collect();
                output.push_str(&posix(&path));
            } else {
                output.push_str(line);
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    if output.is_empty() {
        output.push('\n');
    }
    fs::write(report, output)
}

fn cover_package(root: &Path, package: &Path, files: &[PathBuf]) -> bool {
    let report = coverage_report(root, package, "typescript");
    let directory = report.parent().expect("coverage report directory");
    let Some(command) = command(package, files, directory) else {
        eprintln!(
            "No package.json test script in {}; skipping TypeScript coverage.",
            package.display()
        );
        return false;
    };
    if command.iter().any(|part| {
        Path::new(part)
            .file_name()
            .is_some_and(|name| name == "vitest")
    }) {
        match ensure_vitest_coverage(package) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                eprintln!(
                    "Cannot prepare Vitest coverage in {}: {error}. TypeScript coverage will be N/A.",
                    package.display()
                );
                return false;
            }
        }
    }
    let prepare = || -> io::Result<()> {
        if directory.exists() {
            fs::remove_dir_all(directory)?;
        }
        fs::create_dir_all(directory)
    };
    if let Err(error) = prepare() {
        eprintln!(
            "Cannot prepare {}: {error}. TypeScript coverage will be N/A.",
            report.display()
        );
        return false;
    }
    let custom = command == ["npm", "run", "coverage"];
    let code = run_shell(&Program::Argv(&command), package);
    let ran = if code != 0 {
        eprintln!(
            "TypeScript coverage exited {code} in {}. TypeScript coverage will be N/A.",
            package.display()
        );
        false
    } else if !custom && !report.is_file() {
        eprintln!(
            "TypeScript coverage wrote no {}. TypeScript coverage will be N/A.",
            report.display()
        );
        false
    } else {
        true
    };
    if !custom
        && report.is_file()
        && let Err(error) = anchor(&report, package)
    {
        eprintln!(
            "Cannot anchor {} to {}: {error}. TypeScript coverage will be N/A.",
            report.display(),
            package.display()
        );
        return false;
    }
    ran
}

/// Run every selected package, even when an earlier package fails.
pub fn cover(root: &Path, files: &[PathBuf]) -> bool {
    let packages = modules_with(files, Language::TypeScript, "package.json", root);
    if packages.is_empty() {
        eprintln!("No package.json; skipping TypeScript coverage.");
        return false;
    }
    packages
        .iter()
        .map(|package| cover_package(root, package, files))
        .fold(true, |all, result| all & result)
}
