//! Go's built-in statement coverage, one profile per selected module.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::language::Language;
use crate::process::Program;
use crate::runner::{modules_with, run_shell};

fn cover_module(module: &Path) -> bool {
    let profile = module
        .join("target")
        .join("coverage")
        .join("go")
        .join("coverage.out");
    let prepared = fs::create_dir_all(profile.parent().expect("the profile has a parent"))
        .and_then(|()| match fs::remove_file(&profile) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        });
    if let Err(error) = prepared {
        eprintln!(
            "Cannot prepare {}: {error}. Go coverage will be N/A.",
            profile.display()
        );
        return false;
    }
    let command = vec![
        "go".to_string(),
        "test".to_string(),
        "./...".to_string(),
        format!("-coverprofile={}", profile.display()),
    ];
    let code = run_shell(&Program::Argv(&command), module);
    if code != 0 {
        eprintln!(
            "Go coverage exited {code} in {}. Go coverage will be N/A.",
            module.display()
        );
        return false;
    }
    if !profile.is_file() {
        eprintln!(
            "Go coverage wrote no {}. Go coverage will be N/A.",
            profile.display()
        );
        return false;
    }
    true
}

pub(super) fn cover(root: &Path, files: &[PathBuf]) -> bool {
    let modules = modules_with(files, Language::Go, "go.mod", root);
    if modules.is_empty() {
        eprintln!("No go.mod; skipping Go coverage.");
        return false;
    }
    // Evaluate every module before combining results, even after a failure.
    let results: Vec<bool> = modules.iter().map(|module| cover_module(module)).collect();
    results.into_iter().all(|ok| ok)
}
