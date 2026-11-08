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
    let py = posix(&interpreter(package));
    let py = quality_core::process::shlex_quote(&py);
    let source = coverage_sources(package, files);
    let source = if source.is_empty() {
        String::new()
    } else {
        format!(" --source={}", quality_core::process::shlex_quote(&source))
    };
    let tests = if uses_pytest(package) {
        // Mutator never installs tools: without pytest the original's install
        // fallback becomes crapper's unittest fallback.
        format!(
            "if {py} -c 'import pytest' >/dev/null 2>&1; then tests='-m pytest'; else echo 'pytest is missing in {}. Falling back to unittest.' >&2; tests='-m unittest discover -s .'; fi",
            package.display()
        )
    } else {
        "tests='-m unittest discover -s .'".to_string()
    };
    let script = format!(
        "mkdir -p {directory}\nrm -f -- {report} {data}\n\
         if ! {py} -c 'import coverage' >/dev/null 2>&1; then echo 'coverage is missing in {package}; {interpreter} cannot import it. Python coverage will be N/A.' >&2; exit 0; fi\n\
         {tests}\n\
         {py} -m coverage run --data-file={data}{source} $tests\ncode=$?\n\
         if [ ! -f {data} ]; then if [ \"$code\" != 0 ]; then echo \"Python coverage exited $code in {package}. Python coverage will be N/A.\" >&2; fi; exit 0; fi\n\
         if ! {py} -m coverage lcov --data-file={data} -o {report}; then echo 'coverage lcov failed in {package}. Python coverage will be N/A.' >&2; exit 0; fi\n\
         if [ \"$code\" != 0 ]; then echo \"Python tests exited $code in {package}. Coverage was still recorded.\" >&2; fi\n",
        directory = quote(directory),
        report = quote(&report),
        data = quote(&data),
        package = package.display(),
        interpreter = posix(&interpreter(package)),
    );
    Coverage {
        script,
        report: report
            .strip_prefix(resolve(root))
            .unwrap_or(&report)
            .to_path_buf(),
    }
}
