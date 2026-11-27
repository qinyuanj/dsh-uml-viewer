#![allow(clippy::disallowed_methods)]
#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

pub fn binary<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    binary_env(args, &[])
}

pub fn binary_env<I, S>(args: I, environment: &[(&str, &str)]) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "uml-viewer-bin-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&scratch).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .args(args)
        .current_dir(&scratch)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .envs(environment.iter().copied())
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&scratch);
    output
}

pub struct Project {
    pub root: PathBuf,
}

impl Project {
    pub fn new() -> Project {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "uml-viewer-crap-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("project")).unwrap();
        Project {
            root: canonical(&base.join("project")),
        }
    }

    pub fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    pub fn crap(&self, args: &[&str]) -> Output {
        crap_in(&self.root, &self.root, args)
    }

    pub fn snapshot(&self) -> Option<String> {
        fs::read_to_string(self.root.join(".metrics").join("crap.edn")).ok()
    }

    pub fn entries(&self) -> Vec<(String, String, u32)> {
        let text = self.snapshot().expect("a published snapshot");
        text.lines()
            .filter(|line| line.contains(":name "))
            .map(|line| {
                let field = |key: &str| {
                    let rest = line.split(&format!(":{key} ")).nth(1).unwrap();
                    rest.split(", ")
                        .next()
                        .unwrap()
                        .trim_end_matches('}')
                        .to_string()
                };
                let unquote = |text: String| text.trim_matches('"').to_string();
                (
                    unquote(field("namespace")),
                    unquote(field("name")),
                    field("complexity").parse().unwrap(),
                )
            })
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.entries().into_iter().map(|e| e.1).collect();
        names.sort();
        names
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.root.parent().unwrap());
    }
}

pub fn canonical(path: &Path) -> PathBuf {
    let path = fs::canonicalize(path).unwrap();
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path,
    }
}

pub fn crap_in(cwd: &Path, root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("crap")
        .arg("--root")
        .arg(root)
        .args(args)
        .current_dir(cwd)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}
