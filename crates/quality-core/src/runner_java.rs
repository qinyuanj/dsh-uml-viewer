//! Maven's JaCoCo coverage runner; reports remain for the coverage loader.

use std::fs;
use std::path::{Path, PathBuf};

use crate::language::Language;
use crate::process::Program;
use crate::runner::{modules_with, run_shell};

pub(crate) const MAVEN: &[&str] = &[
    "mvn",
    "-q",
    "org.jacoco:jacoco-maven-plugin:0.8.12:prepare-agent",
    "test",
    "org.jacoco:jacoco-maven-plugin:0.8.12:report",
];

fn run_reported(module: &Path, report: &Path) -> bool {
    let argv: Vec<String> = MAVEN.iter().map(|arg| (*arg).to_string()).collect();
    let code = run_shell(&Program::Argv(&argv), module);
    if code != 0 {
        eprintln!(
            "Java coverage exited {code} in {}. Java coverage will be N/A.",
            module.display()
        );
        return false;
    }
    if !report.is_file() {
        eprintln!(
            "Java coverage wrote no {}. Java coverage will be N/A.",
            report.display()
        );
        return false;
    }
    true
}

pub(super) fn cover(root: &Path, files: &[PathBuf]) -> bool {
    let modules = modules_with(files, Language::Java, "pom.xml", root);
    if modules.is_empty() {
        eprintln!("No pom.xml; skipping Java coverage.");
        return false;
    }
    let mut results = Vec::new();
    for module in modules {
        let report = module.join("target/site/jacoco");
        if report.exists()
            && let Err(error) = fs::remove_dir_all(&report)
        {
            eprintln!(
                "Cannot clean {}: {error}. Java coverage will be N/A.",
                report.display()
            );
            results.push(false);
            continue;
        }
        if let Err(error) = fs::remove_file(module.join("target/jacoco.exec"))
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("Cannot remove stale JaCoCo data: {error}. Java coverage will be N/A.");
            results.push(false);
            continue;
        }
        results.push(run_reported(&module, &report.join("jacoco.xml")));
    }
    results.into_iter().all(|ok| ok)
}
