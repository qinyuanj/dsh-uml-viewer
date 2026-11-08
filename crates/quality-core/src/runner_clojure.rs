//! Cloverage uses its HTML form counters when `--lcov` is unavailable.
use crate::process::Program;
use crate::runner::run_shell;
use std::path::Path;

pub(super) fn cover(root: &Path) -> bool {
    if !root.join("deps.edn").is_file() && !root.join("bb.edn").is_file() {
        eprintln!("No deps.edn or bb.edn; skipping Clojure coverage.");
        return false;
    }
    let keep = [
        "typescript",
        "rust",
        "go",
        "python",
        "lua",
        "crapper-stamp.json",
    ];
    let coverage = root.join("target/coverage");
    if let Ok(entries) = std::fs::read_dir(coverage) {
        for entry in entries.flatten() {
            if keep.contains(&entry.file_name().to_string_lossy().as_ref()) {
                continue;
            }
            let path = entry.path();
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(error) = result {
                eprintln!("Cannot clean {}: {error}.", path.display());
                return false;
            }
        }
    }
    let mut code = run_shell(
        &Program::Argv(&["clj".into(), "-M:cov".into(), "--lcov".into()]),
        root,
    );
    if code != 0 {
        eprintln!("clj -M:cov --lcov failed; retrying without --lcov.");
        code = run_shell(&Program::Argv(&["clj".into(), "-M:cov".into()]), root);
    }
    if code != 0 {
        eprintln!("Clojure coverage exited {code}. Clojure coverage will be N/A.");
    }
    code == 0
}
