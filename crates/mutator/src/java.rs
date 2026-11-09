//! Java mutation: the per-file test plan.
use quality_core::{paths::resolve, runner::nearest};
use std::path::{Path, PathBuf};

/// `test_plan(root, source, "java", None)`: Maven's tests in the nearest
/// `pom.xml` directory, skipping tests tagged `no-mutate`.
pub fn test_plan(root: &Path, source: &Path) -> (Vec<String>, PathBuf) {
    let root = resolve(root);
    let directory = nearest(source, "pom.xml", &root).unwrap_or(root);
    (
        ["mvn", "-q", "test", "-DexcludeTags=no-mutate"]
            .map(String::from)
            .to_vec(),
        directory,
    )
}

#[cfg(test)]
mod tests {
    //! Ported from the original `tests/test_runner.py` default plan cases.
    use super::*;
    use std::fs;

    #[test]
    fn java_plans_run_maven_at_the_nearest_pom_or_the_root() {
        let root = resolve(&std::env::temp_dir())
            .join(format!("mutator-java-plan-{}", std::process::id()));
        fs::create_dir_all(root.join("module/src")).unwrap();
        let source = root.join("module/src/A.java");
        fs::write(&source, "class A {}\n").unwrap();
        let maven = ["mvn", "-q", "test", "-DexcludeTags=no-mutate"].map(String::from);
        assert_eq!(test_plan(&root, &source), (maven.to_vec(), root.clone()));
        fs::write(root.join("module/pom.xml"), "<project/>\n").unwrap();
        assert_eq!(test_plan(&root, &source).1, root.join("module"));
        fs::remove_dir_all(&root).unwrap();
    }
}
