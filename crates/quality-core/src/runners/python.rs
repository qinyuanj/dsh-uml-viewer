//! Python's project interpreter runs coverage.py; runner policy stays in Rust.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::language::is_test_file;
use crate::paths::{posix, resolve};
use crate::process::Program;
use crate::runner::{coverage_report, nearest, run_shell};

use super::typescript::anchor;

const MARKERS: &[&str] = &[
    "pyproject.toml",
    "setup.cfg",
    "setup.py",
    "pytest.ini",
    "tox.ini",
];

fn roots(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let root = resolve(root);
    let found: BTreeSet<String> = files
        .iter()
        .filter(|file| file.extension().is_some_and(|ext| ext == "py"))
        .map(|file| {
            // Marker priority is intentional: walk to the root for each marker
            // before trying the next marker, matching the original.
            let package = MARKERS
                .iter()
                .find_map(|marker| nearest(file, marker, &root))
                .unwrap_or_else(|| root.clone());
            posix(&resolve(&package))
        })
        .collect();
    found.into_iter().map(PathBuf::from).collect()
}

fn executable(package: &Path) -> String {
    for venv in [".venv", "venv"] {
        let candidate = package.join(venv).join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        });
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    if cfg!(windows) { "python" } else { "python3" }.into()
}

fn uses_pytest(package: &Path) -> bool {
    if package.join("pytest.ini").is_file() || package.join("conftest.py").is_file() {
        return true;
    }
    ["pyproject.toml", "setup.cfg", "tox.ini"]
        .iter()
        .any(|name| {
            fs::read(package.join(name))
                .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains("pytest"))
        })
}

fn sources(package: &Path, files: &[PathBuf]) -> String {
    let package = resolve(package);
    let tops: BTreeSet<String> = files
        .iter()
        .filter(|file| file.extension().is_some_and(|ext| ext == "py") && !is_test_file(file))
        .filter_map(|file| {
            let path = resolve(file);
            path.strip_prefix(&package)
                .ok()?
                .components()
                .next()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
        })
        .collect();
    tops.into_iter().collect::<Vec<_>>().join(",")
}

fn shell(args: &[&str], package: &Path) -> i32 {
    run_shell(
        &Program::Argv(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
        package,
    )
}

fn ensure_module(py: &str, package: &Path, module: &str, install: bool) -> bool {
    if shell(&[py, "-c", &format!("import {module}")], package) == 0 {
        return true;
    }
    if !install {
        return false;
    }
    eprintln!(
        "Installing {module} for Python coverage. The project requirements are left unchanged."
    );
    shell(
        &[
            py,
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            module,
        ],
        package,
    ) == 0
}

fn cover_package(root: &Path, package: &Path, files: &[PathBuf]) -> bool {
    let report = coverage_report(root, package, "python");
    let py = executable(package);
    if !ensure_module(&py, package, "coverage", false) {
        eprintln!(
            "coverage is missing in {}; {py} cannot import it. Python coverage will be N/A.",
            package.display()
        );
        return false;
    }
    let directory = report.parent().expect("coverage report directory");
    let data = directory.join(".coverage");
    let prepare = || -> std::io::Result<()> {
        fs::create_dir_all(directory)?;
        for path in [&report, &data] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    };
    if let Err(error) = prepare() {
        eprintln!(
            "Cannot prepare {}: {error}. Python coverage will be N/A.",
            report.display()
        );
        return false;
    }
    let detected_pytest = uses_pytest(package);
    let pytest = detected_pytest && ensure_module(&py, package, "pytest", true);
    if detected_pytest && !pytest {
        eprintln!(
            "pytest is missing in {}. Falling back to unittest.",
            package.display()
        );
    }
    let data_flag = format!("--data-file={}", data.display());
    let mut command = vec![
        py.clone(),
        "-m".into(),
        "coverage".into(),
        "run".into(),
        data_flag.clone(),
    ];
    let source = sources(package, files);
    if !source.is_empty() {
        command.push(format!("--source={source}"));
    }
    command.extend(
        if pytest {
            vec!["-m", "pytest"]
        } else {
            vec!["-m", "unittest", "discover", "-s", "."]
        }
        .into_iter()
        .map(str::to_string),
    );
    let code = run_shell(&Program::Argv(&command), package);
    let data_written = data.exists();
    let recorded = if !data_written {
        if code != 0 {
            eprintln!(
                "Python coverage exited {code} in {}. Python coverage will be N/A.",
                package.display()
            );
        }
        false
    } else {
        let lcov_code = shell(
            &[
                &py,
                "-m",
                "coverage",
                "lcov",
                &data_flag,
                "-o",
                &report.to_string_lossy(),
            ],
            package,
        );
        if lcov_code != 0 {
            eprintln!(
                "coverage lcov exited {lcov_code} in {}. Python coverage will be N/A.",
                package.display()
            );
            false
        } else {
            let converted = report.is_file();
            if code != 0 {
                eprintln!(
                    "Python tests exited {code} in {}. Coverage was still recorded.",
                    package.display()
                );
            }
            code == 0 && converted
        }
    };
    // Failing tests may write a useful report too.
    let anchored = if report.is_file() {
        match anchor(&report, package) {
            Ok(()) => true,
            Err(error) => {
                eprintln!(
                    "Cannot anchor {} to {}: {error}. Python coverage will be N/A.",
                    report.display(),
                    package.display()
                );
                false
            }
        }
    } else {
        true
    };
    if (recorded || data_written) && !report.is_file() {
        eprintln!(
            "Python coverage wrote no {}. Python coverage will be N/A.",
            report.display()
        );
        return false;
    }
    recorded && anchored
}

/// Run every selected Python package, retaining LCOV even on test failure.
pub fn cover(root: &Path, files: &[PathBuf]) -> bool {
    roots(root, files)
        .iter()
        .map(|package| cover_package(root, package, files))
        .fold(true, |all, result| all & result)
}
