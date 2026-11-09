//! Go mutation: the per-file test plan and the coverage command.
use quality_core::{
    paths::{relative_posix, resolve},
    runner::nearest,
};
use std::path::{Path, PathBuf};

/// `test_plan(root, source, "go", None)`: `go test -count=1 <package>` in the
/// nearest module, the package as a `./`-relative path (`.` for the module's
/// own directory), one argument even with a space in it.
pub fn test_plan(root: &Path, source: &Path) -> (Vec<String>, PathBuf) {
    let root = resolve(root);
    let directory = nearest(source, "go.mod", &root).unwrap_or_else(|| root.clone());
    let parent = resolve(source)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| directory.clone());
    let relative = relative_posix(&parent, &resolve(&directory)).unwrap_or_else(|| ".".into());
    let package = if relative == "." {
        ".".to_string()
    } else {
        format!("./{relative}")
    };
    (
        vec!["go".into(), "test".into(), "-count=1".into(), package],
        directory,
    )
}

/// crapper's Go runner for one module, as one view command: the profile goes
/// to the module's `target/coverage/go/coverage.out`, which the view keeps.
pub fn coverage_script() -> String {
    if cfg!(windows) {
        "mkdir target\\coverage\\go 2>nul & del /f /q target\\coverage\\go\\coverage.out 2>nul & go test ./... -coverprofile=target/coverage/go/coverage.out"
            .into()
    } else {
        "mkdir -p target/coverage/go\nrm -f target/coverage/go/coverage.out\nexec go test ./... -coverprofile=target/coverage/go/coverage.out"
            .into()
    }
}

#[cfg(test)]
mod tests {
    //! Ported from the original `tests/test_runner.py` Go plan cases.
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn project() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "mutator-go-plan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        resolve(&directory)
    }

    #[test]
    fn go_package_is_one_argument() {
        let root = project();
        let package = root.join("a;echo no");
        fs::create_dir_all(&package).unwrap();
        fs::write(root.join("go.mod"), "module demo\n").unwrap();
        let source = package.join("widget.go");
        fs::write(&source, "package a\n").unwrap();
        let (command, directory) = test_plan(&root, &source);
        assert_eq!(command, ["go", "test", "-count=1", "./a;echo no"]);
        assert_eq!(directory, root);
        let spaced = root.join("my dir");
        fs::create_dir_all(&spaced).unwrap();
        let spaced = spaced.join("widget.go");
        fs::write(&spaced, "package b\n").unwrap();
        assert_eq!(
            test_plan(&root, &spaced).0,
            ["go", "test", "-count=1", "./my dir"]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn go_plans_fall_back_to_the_root() {
        let root = project();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.go"), "package a\n").unwrap();
        assert_eq!(
            test_plan(&root, &root.join("src/a.go")),
            (
                vec![
                    "go".into(),
                    "test".into(),
                    "-count=1".into(),
                    "./src".into()
                ],
                root.clone()
            )
        );
        fs::write(root.join("main.go"), "package main\n").unwrap();
        assert_eq!(
            test_plan(&root, &root.join("main.go")).0,
            ["go", "test", "-count=1", "."]
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
