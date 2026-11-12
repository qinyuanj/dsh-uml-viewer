//! Python mutation: sites, privacy, the per-file test plan and coverage.
use crate::operators;
use quality_core::{
    grammar::Node,
    paths::{posix, resolve},
    runner::nearest,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// `_python_private`: a leading underscore, except a dunder.
pub fn private(name: &str) -> bool {
    if name.starts_with("__") && name.ends_with("__") {
        return false;
    }
    name.starts_with('_')
}

/// `_python_annotation`: union annotations share `a | b`'s syntax tree.
fn in_annotation(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "type" {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

/// The leaf's site under the original's Python rules.
pub fn site(node: Node, token: &str) -> Option<(String, &'static str)> {
    operators::leaf(node, token, |token| {
        if let Some(mutant) = operators::bitwise(token) {
            return (!in_annotation(node)).then_some(mutant);
        }
        operators::binary(token)
    })
}

/// `_project_interpreter`: the project's virtualenv, or the interpreter on
/// PATH. The path is absolute and a symlink is not resolved: a view does not
/// copy the project's `.venv`, and resolving it would leave the virtualenv.
pub fn interpreter(directory: &Path) -> PathBuf {
    for name in [".venv", "venv"] {
        let candidate = directory.join(name).join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        });
        // os.access(X_OK) is permission-based; existence is not enough.
        if candidate.is_file() && executable(&candidate) {
            return candidate;
        }
    }
    PathBuf::from(if cfg!(windows) { "python" } else { "python3" })
}
#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    // Every file is executable to os.access on Windows; the suffix is what
    // CreateProcess needs, and the runner only asks for the interpreter.
    path.is_file()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// `_python_command`'s pytest detection.
pub fn uses_pytest(directory: &Path) -> bool {
    directory.join("pytest.ini").is_file()
        || directory.join("conftest.py").is_file()
        || directory.join("tests/conftest.py").is_file()
        || directory.join("test/conftest.py").is_file()
        || read(&directory.join("pyproject.toml")).contains("pytest")
        || (directory.join("setup.cfg").is_file()
            && read(&directory.join("setup.cfg")).contains("pytest"))
}

/// `test_plan(root, source, "python", None)`: marker order beats nearness.
pub fn test_plan(root: &Path, source: &Path) -> (Vec<String>, PathBuf) {
    let root = resolve(root);
    let directory = ["pyproject.toml", "pytest.ini", "setup.cfg"]
        .iter()
        .find_map(|marker| nearest(source, marker, &root))
        .unwrap_or_else(|| root.clone());
    let interpreter = interpreter(&directory);
    let mut command = vec![interpreter.to_string_lossy().into_owned(), "-m".into()];
    if uses_pytest(&directory) {
        command.push("pytest".into());
    } else {
        command.extend(["unittest".into(), "discover".into()]);
    }
    (command, directory)
}

/// `runners._python_roots`: every package marker before the next marker.
pub fn coverage_roots(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    const MARKERS: &[&str] = &[
        "pyproject.toml",
        "setup.cfg",
        "setup.py",
        "pytest.ini",
        "tox.ini",
    ];
    let root = resolve(root);
    let found: BTreeSet<String> = files
        .iter()
        .filter(|file| file.extension().is_some_and(|ext| ext == "py"))
        .map(|file| {
            let package = MARKERS
                .iter()
                .find_map(|marker| nearest(file, marker, &root))
                .unwrap_or_else(|| root.clone());
            posix(&resolve(&package))
        })
        .collect();
    found.into_iter().map(PathBuf::from).collect()
}

/// `runners._python_sources`: the top-level names under each package.
pub fn coverage_sources(package: &Path, files: &[PathBuf]) -> String {
    let package = resolve(package);
    let tops: BTreeSet<String> = files
        .iter()
        .filter(|file| file.extension().is_some_and(|ext| ext == "py"))
        .filter(|file| !quality_core::language::is_test_file(file))
        .filter_map(|file| {
            resolve(file)
                .strip_prefix(&package)
                .ok()?
                .components()
                .next()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
        })
        .collect();
    tops.into_iter().collect::<Vec<_>>().join(",")
}

/// `runners._python_coverage` as one view command. Every write happens in the
/// overlay: a view command starts from a fresh upper layer, so the coverage
/// data, the tests and the report belong to the same script.
pub struct Coverage {
    pub script: String,
    /// Project-relative report the caller reads from the view's upper layer.
    pub report: PathBuf,
}
pub fn coverage(root: &Path, package: &Path, files: &[PathBuf]) -> Coverage {
    let quote = |path: &Path| quality_core::process::shlex_quote(&path.to_string_lossy());
    let report = quality_core::runner::coverage_report(root, package, "python");
    let directory = report.parent().expect("coverage report directory");
    let data = directory.join(".coverage");
    // A Windows command runs in a copy of the project, so every path it writes
    // must be relative to the package it runs in: an absolute path would name
    // the real project and write there. `rust_coverage` follows the same rule.
    // The interpreter is a read, so it stays the real project's absolute path.
    let command_path = |path: &Path| {
        if cfg!(windows) {
            crate::engine::relative_from(&resolve(package), &resolve(path))
                .unwrap_or_else(|| resolve(path))
                .display()
                .to_string()
        } else {
            path.to_string_lossy().into_owned()
        }
    };
    let directory_text = command_path(directory);
    let report_text = command_path(&report);
    let data_text = command_path(&data);
    // The Windows diagnostics quote the package and name it from the project
    // root; the POSIX script's text stays as the original wrote it.
    let package_label = if cfg!(windows) {
        crate::engine::diagnostic_label(root, package)
    } else {
        package.display().to_string()
    };
    let py_path = interpreter(package);
    let py = if cfg!(windows) {
        quality_core::process::cmd_quote(&py_path.display().to_string())
    } else {
        quality_core::process::shlex_quote(&posix(&py_path))
    };
    let source = coverage_sources(package, files);
    let source = if source.is_empty() {
        String::new()
    } else if cfg!(windows) {
        format!(" --source={}", quality_core::process::cmd_quote(&source))
    } else {
        format!(" --source={}", quality_core::process::shlex_quote(&source))
    };
    // One command per branch, so the generated script never re-parses an
    // argument list: no shell word splitting of a test command.
    let run = if cfg!(windows) {
        format!("{py} -m coverage run --data-file=\"{data_text}\"{source}")
    } else {
        format!(
            "{py} -m coverage run --data-file={data}{source}",
            data = quote(&data)
        )
    };
    let tests = if uses_pytest(package) {
        // Mutator never installs tools: without pytest the original's install
        // fallback becomes crapper's unittest fallback.
        if cfg!(windows) {
            // `if not errorlevel 1` is read when it runs, unlike `%ERRORLEVEL%`,
            // which cmd expands before the first command of the line runs.
            format!(
                "{py} -c \"import pytest\" 2>nul & ( if not errorlevel 1 ({run} -m pytest) else (echo pytest is missing in {package}. Falling back to unittest. 1>&2 & {run} -m unittest discover -s .) )",
                package = package_label,
            )
        } else {
            format!(
                "if {py} -c 'import pytest' >/dev/null 2>&1; then\n  {run} -m pytest\nelse\n  echo 'pytest is missing in {package}. Falling back to unittest.' >&2\n  {run} -m unittest discover -s .\nfi",
                package = package.display(),
            )
        }
    } else {
        format!("{run} -m unittest discover -s .")
    };
    let script = if cfg!(windows) {
        // cmd has no line separators inside one `/c` string, so the script is
        // one `&` chain; each failure guard is a `||` block, which cmd keeps
        // whole, and the last check is a parenthesized `if`.
        format!(
            "mkdir \"{directory}\" 2>nul & del /f /q \"{report}\" \"{data}\" 2>nul & \
             {py} -c \"import coverage\" 2>nul || (echo coverage is missing in {package}; {interpreter} cannot import it. Python coverage will be N/A. 1>&2 & exit /b 0) & \
             {tests} & \
             {py} -m coverage lcov --data-file=\"{data}\" -o \"{report}\" & \
             ( if not exist \"{report}\" (echo Python coverage wrote no {report_echo}. Python coverage will be N/A. 1>&2) ) & exit /b 0",
            directory = directory_text,
            report = report_text,
            report_echo = quality_core::process::cmd_quote(&report_text),
            data = data_text,
            package = package_label,
            interpreter = py.as_str(),
        )
    } else {
        format!(
            "mkdir -p {directory}\nrm -f -- {report} {data}\n\
             if ! {py} -c 'import coverage' >/dev/null 2>&1; then echo 'coverage is missing in {package}; {interpreter} cannot import it. Python coverage will be N/A.' >&2; exit 0; fi\n\
             {tests}\ncode=$?\n\
             if [ ! -f {data} ]; then if [ \"$code\" != 0 ]; then echo \"Python coverage exited $code in {package}. Python coverage will be N/A.\" >&2; fi; exit 0; fi\n\
             if ! {py} -m coverage lcov --data-file={data} -o {report}; then echo 'coverage lcov failed in {package}. Python coverage will be N/A.' >&2; exit 0; fi\n\
             if [ \"$code\" != 0 ]; then echo \"Python tests exited $code in {package}. Coverage was still recorded.\" >&2; fi\n",
            directory = quote(directory),
            report = quote(&report),
            data = quote(&data),
            package = package.display(),
            interpreter = posix(&interpreter(package)),
        )
    };
    Coverage {
        script,
        report: report
            .strip_prefix(resolve(root))
            .unwrap_or(&report)
            .to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    //! Ported from the original `tests/test_runner.py` Python cases.
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Project(PathBuf);
    impl Project {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir().join(format!(
                "mutator-python-plan-{}-{}-{name}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&directory).unwrap();
            // Match test_plan's canonical root, including macOS /var aliases
            // and Windows path spelling, without resolving interpreter symlinks.
            Self(quality_core::paths::resolve(&directory))
        }
        fn write(&self, relative: &str, text: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        #[cfg(unix)]
        fn executable(&self, relative: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }
    impl Drop for Project {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn plan(root: &Path, source: &Path) -> (Vec<String>, PathBuf) {
        test_plan(root, source)
    }

    #[cfg(unix)]
    #[test]
    fn python_command_uses_an_absolute_project_interpreter() {
        let project = Project::new("proj");
        let dot = project.executable(".venv/bin/python");
        project.write("pytest.ini", "[pytest]\n");
        assert_eq!(interpreter(&project.0), dot);
        assert_eq!(
            test_plan(&project.0, &project.0.join("a.py")).0,
            vec![
                dot.to_string_lossy().into_owned(),
                "-m".into(),
                "pytest".into()
            ]
        );
        let other = Project::new("other");
        let plain = other.executable("venv/bin/python");
        assert_eq!(
            test_plan(&other.0, &other.0.join("a.py")).0,
            vec![
                plain.to_string_lossy().into_owned(),
                "-m".into(),
                "unittest".into(),
                "discover".into()
            ]
        );
        let preferred = other.executable(".venv/bin/python");
        assert_eq!(interpreter(&other.0), preferred);
    }

    #[cfg(unix)]
    #[test]
    fn python_command_keeps_the_virtualenv_symlink() {
        let project = Project::new("proj");
        let target = project.executable("real-python");
        let link = project.0.join(".venv/bin/python");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        project.write("pytest.ini", "[pytest]\n");
        // Resolving the symlink would leave the virtualenv.
        assert_eq!(interpreter(&project.0), link);
    }

    #[cfg(unix)]
    #[test]
    fn python_command_falls_back_when_the_virtualenv_cannot_run() {
        let project = Project::new("proj");
        project.write(".venv/bin/python", "");
        let fallback = interpreter(&project.0);
        assert_eq!(fallback, PathBuf::from("python3"));
        let missing = Project::new("missing");
        assert_eq!(interpreter(&missing.0), PathBuf::from("python3"));
    }

    #[test]
    fn python_commands_detect_pytest() {
        let pytest_dir = Project::new("py");
        pytest_dir.write("pytest.ini", "[pytest]\n");
        assert!(uses_pytest(&pytest_dir.0));
        let unit = Project::new("unit");
        assert!(!uses_pytest(&unit.0));
        let cfg = Project::new("cfg");
        cfg.write("setup.cfg", "[tool:pytest]\n");
        assert!(uses_pytest(&cfg.0));
        let project = Project::new("proj");
        project.write("pyproject.toml", "[project]\ndependencies=['pytest']\n");
        assert!(uses_pytest(&project.0));
        let other = Project::new("other");
        other.write("setup.cfg", "[metadata]\nname = demo\n");
        assert!(!uses_pytest(&other.0));
        let conf = Project::new("conf");
        conf.write("conftest.py", "");
        assert!(uses_pytest(&conf.0));
        let nested = Project::new("nested");
        nested.write("tests/conftest.py", "");
        assert!(uses_pytest(&nested.0));
    }

    #[test]
    fn plans_prefer_markers_in_a_fixed_order_over_nearness() {
        let root = Project::new("root");
        root.write("sub/app.py", "x = 1\n");
        root.write("sub/setup.cfg", "[metadata]\n");
        assert_eq!(
            plan(&root.0, &root.0.join("sub/app.py")).1,
            root.0.join("sub")
        );
        root.write("pytest.ini", "[pytest]\n");
        assert_eq!(plan(&root.0, &root.0.join("sub/app.py")).1, root.0);
        root.write("sub/pytest.ini", "[pytest]\n");
        root.write("pyproject.toml", "[project]\n");
        assert_eq!(plan(&root.0, &root.0.join("sub/app.py")).1, root.0);
    }

    #[test]
    fn python_plan_falls_back_to_the_root() {
        let root = Project::new("root");
        root.write("src/a.py", "x = 1\n");
        let (command, directory) = plan(&root.0, &root.0.join("src/a.py"));
        assert_eq!(directory, root.0);
        assert_eq!(command[1..], ["-m", "unittest", "discover"]);
    }
}
