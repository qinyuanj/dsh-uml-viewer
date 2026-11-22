use crate::{
    interrupts,
    isolation::{self, Completed, Failure},
    scratch,
    slot::Entry,
};
use quality_core::process::{self, Ended, Program};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

const HIDDEN: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".idea",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    ".metrics",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".clj-kondo",
    ".gradle",
    ".uml-viewer",
    ".toolcache",
];

const DEPENDENCIES: &[&str] = &["node_modules"];

const REDIRECTED: &[(&str, Entry, &str)] = &[
    ("HOME", Entry::Home, ""),
    ("USERPROFILE", Entry::Home, ""),
    ("TEMP", Entry::Temp, ""),
    ("TMP", Entry::Temp, ""),
    ("APPDATA", Entry::AppData, "Roaming"),
    ("LOCALAPPDATA", Entry::AppData, "Local"),
];

pub fn describe() -> &'static str {
    "copy (a fresh copy of the project per command)"
}

pub fn remember_outside(root: &Path, outside: &[isolation::Outside]) {
    let mut registered = registry();
    if outside.is_empty() {
        registered.remove(root);
    } else {
        registered.insert(root.to_path_buf(), outside.to_vec());
    }
}

pub fn outside(root: &Path) -> Vec<isolation::Outside> {
    registry().get(root).cloned().unwrap_or_default()
}

fn registry() -> MutexGuard<'static, BTreeMap<PathBuf, Vec<isolation::Outside>>> {
    static OUTSIDE: OnceLock<Mutex<BTreeMap<PathBuf, Vec<isolation::Outside>>>> = OnceLock::new();
    OUTSIDE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct Slot {
    directory: PathBuf,
    build: Option<crate::build_cache::BuildCache>,
}
impl Slot {
    pub fn reused(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            build: None,
        }
    }

    pub fn with_build(mut self, build: Option<crate::build_cache::BuildCache>) -> Self {
        self.build = build;
        self
    }
    fn beside(&self) -> PathBuf {
        self.directory.join("beside")
    }

    fn entry(&self, entry: Entry) -> PathBuf {
        self.beside().join(entry.name())
    }

    fn redirected(&self, entry: Entry, folder: &str) -> PathBuf {
        match folder {
            "" => self.entry(entry),
            folder => self.entry(entry).join(folder),
        }
    }

    fn project(&self) -> PathBuf {
        self.entry(Entry::Project)
    }
    fn home(&self) -> PathBuf {
        self.entry(Entry::Home)
    }
    fn mark(&self) -> PathBuf {
        self.entry(Entry::Mutation)
    }

    fn copy(&self, root: &Path, command: &str, mutation: Option<(&Path, &str)>) -> io::Result<()> {
        for entry in fs::read_dir(&self.directory)? {
            scratch::remove_patiently(&entry?.path())?;
        }
        scratch::private_directory(&self.beside())?;
        let project = self.project();
        scratch::private_directory(&project)?;
        copy_tree(root, &project, &Exclusions::project(root, command))?;
        for outside in outside(root) {
            self.place(&outside)?;
        }
        for (_, entry, folder) in REDIRECTED {
            fs::create_dir_all(self.redirected(*entry, folder))?;
        }
        let mark = self.mark();
        scratch::force_remove(&mark)?;
        if let Some(build) = &self.build {
            build.force_previous(&project)?;
        }
        if let Some((path, source)) = mutation {
            scratch::private_file(&mark)?.write_all(source.as_bytes())?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| io::Error::other("the mutated source is outside the project root"))?;
            fs::write(project.join(relative), source)?;
            if let Some(build) = &self.build {
                build.remember(relative)?;
            }
        }
        Ok(())
    }

    fn cwd(&self, root: &Path, cwd: &Path) -> io::Result<PathBuf> {
        let relative = cwd
            .strip_prefix(root)
            .map_err(|_| io::Error::other("the command's directory is outside the project"))?;
        Ok(self.project().join(relative))
    }

    fn place(&self, outside: &isolation::Outside) -> io::Result<()> {
        let source = &outside.source;
        let destination = self.beside().join(&outside.relative);
        let target = format!("{} beside the copy", source.display());
        if fs::metadata(source)
            .map_err(|error| cannot_place(&target, error))?
            .is_dir()
        {
            fs::create_dir_all(&destination).map_err(|error| cannot_place(&target, error))?;
            copy_tree(source, &destination, &Exclusions::none())
                .map_err(|error| cannot_place(&target, error))?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| cannot_place(&target, error))?;
            }
            fs::copy(source, &destination).map_err(|error| cannot_place(&target, error))?;
        }
        Ok(())
    }

    pub fn run(
        &self,
        root: &Path,
        cwd: &Path,
        command: &str,
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        self.run_program(&Program::Shell(command), root, cwd, mutation, timeout)
    }

    pub fn run_argv(
        &self,
        root: &Path,
        cwd: &Path,
        argv: &[String],
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        self.run_program(&Program::Argv(argv), root, cwd, mutation, timeout)
    }

    fn run_program(
        &self,
        program: &Program,
        root: &Path,
        cwd: &Path,
        mutation: Option<(&Path, &str)>,
        timeout: Option<Duration>,
    ) -> Result<Completed, Failure> {
        let command = program.display();
        self.copy(root, &command, mutation).map_err(copy_failure)?;
        let cwd = self
            .cwd(root, cwd)
            .map_err(|error| Failure::Message(error.to_string()))?;
        let output_path = self.entry(Entry::Output);
        scratch::force_remove(&output_path).map_err(message)?;
        let output = scratch::private_file(&output_path).map_err(message)?;
        let home = self.home();
        let mut environment: Vec<(OsString, OsString)> = REDIRECTED
            .iter()
            .map(|(name, entry, folder)| {
                (
                    OsString::from(name),
                    self.redirected(*entry, folder).into_os_string(),
                )
            })
            .collect();
        let copy = self.project();
        let target = match &self.build {
            Some(build) => build.directory().to_path_buf(),
            None => copy.join("target/mutator-build"),
        };
        environment.push(("CARGO_TARGET_DIR".into(), target.into_os_string()));
        if self.build.is_some() && is_cargo_test_command(&command) {
            environment.push(("CARGO_INCREMENTAL".into(), "1".into()));
        }
        if std::env::var_os("CARGO_HOME").is_none() {
            let default = quality_core::paths::home().join(".cargo");
            if default.is_dir() {
                environment.push(("CARGO_HOME".into(), default.into_os_string()));
            }
        }
        if std::env::var_os("RUSTUP_HOME").is_none() {
            let default = quality_core::paths::home().join(".rustup");
            if default.is_dir() {
                environment.push(("RUSTUP_HOME".into(), default.into_os_string()));
            }
        }
        let rendered = program.display();
        if rendered.contains("mvn") || rendered.contains("clj") || rendered.contains("clojure") {
            let jvm_home = home.join("home");
            let repository = quality_core::paths::home().join(".m2");
            if repository.is_dir() {
                let destination = jvm_home.join(".m2");
                fs::create_dir_all(&destination).map_err(message)?;
                copy_tree(&repository, &destination, &Exclusions::none()).map_err(message)?;
            }
            let previous = std::env::var("JAVA_TOOL_OPTIONS").unwrap_or_default();
            environment.push((
                "JAVA_TOOL_OPTIONS".into(),
                format!("{previous} -Duser.home=\"{}\"", jvm_home.display()).into(),
            ));
        }
        environment.push(("MUTATOR_VIEW".into(), self.beside().into_os_string()));
        let started = Instant::now();
        let running = process::start_supervised_with_env_removals(
            program,
            &cwd,
            &environment,
            &crate::child_env::inherited_removals(),
            &output,
        )
        .map_err(message)?;
        drop(output);
        let code = match process::supervise(running, timeout, || interrupts::pending().is_some())
            .map_err(message)?
        {
            Ended::Exited(code) => code,
            Ended::Timeout => 124,
            Ended::Stopped => return Err(Failure::Interrupted),
        };
        Ok(Completed {
            code,
            elapsed: started.elapsed(),
            output: fs::read_to_string(output_path).unwrap_or_default(),
        })
    }
}

impl Slot {
    pub fn project_root(&self) -> PathBuf {
        self.project()
    }
}

fn message(error: io::Error) -> Failure {
    Failure::Message(error.to_string())
}

fn isolation_failure(text: &str) -> bool {
    text.starts_with("Copy isolation cannot")
}

fn copy_failure(error: io::Error) -> Failure {
    let text = error.to_string();
    let text = if isolation_failure(&text) {
        text
    } else {
        format!("Copy isolation cannot prepare the copy: {text}")
    };
    Failure::Unavailable(text)
}

fn cannot_place(target: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("Copy isolation cannot place {target}: {error}"),
    )
}

pub(crate) struct Exclusions {
    hidden: Vec<PathBuf>,
    dependencies: bool,
}

impl Exclusions {
    fn project(root: &Path, command: &str) -> Self {
        let dependencies = !runs_javascript(command);
        let hidden = hidden_paths(root)
            .into_iter()
            .filter(|path| dependencies || !is_dependency(path))
            .collect();
        Self {
            hidden,
            dependencies,
        }
    }

    fn none() -> Self {
        Self {
            hidden: Vec::new(),
            dependencies: true,
        }
    }
}

fn is_cargo_test_command(command: &str) -> bool {
    crate::rust_suite::is_suite(command)
}

fn runs_javascript(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    [
        "npm", "pnpm", "yarn", "bun", "deno", "node ", "node.exe", "vitest", "jest", "mocha",
        "tsx", "tsc",
    ]
    .iter()
    .any(|tool| command.contains(tool))
}

pub(crate) fn copy_tree(
    source: &Path,
    destination: &Path,
    exclusions: &Exclusions,
) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&path)?;
        if exclusions.hidden.contains(&path)
            || (exclusions.dependencies && is_dependency(&path) && metadata.is_dir())
        {
            fs::create_dir(&target)?;
            continue;
        }
        if is_link(&metadata) {
            match fs::metadata(&path) {
                Ok(resolved) if resolved.is_dir() => {
                    return Err(io::Error::other(format!(
                        "Copy isolation cannot freeze linked directory: {}",
                        path.display()
                    )));
                }
                Ok(resolved) if resolved.is_file() => {
                    let source = fs::canonicalize(&path).map_err(|_| {
                        io::Error::other(format!(
                            "Copy isolation cannot freeze unresolved link: {}",
                            path.display()
                        ))
                    })?;
                    fs::copy(source, target)?;
                }
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => {
                    return Err(io::Error::other(format!(
                        "Copy isolation cannot freeze unresolved link: {}",
                        path.display()
                    )));
                }
            }
        } else if metadata.is_dir() {
            fs::create_dir(&target)?;
            copy_tree(&path, &target, exclusions)?;
        } else if metadata.is_file() {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
}

pub fn hidden_paths(root: &Path) -> Vec<PathBuf> {
    let mut names: Vec<String> = HIDDEN.iter().map(|name| (*name).to_owned()).collect();
    for name in declared_ignored_directories(root) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
        .into_iter()
        .map(|name| root.join(name))
        .filter(|path| path.exists())
        .collect()
}

fn declared_ignored_directories(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for file in ignore_files(root) {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            let Some(name) = literal_directory(line) else {
                continue;
            };
            if root.join(&name).is_dir() && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn ignore_files(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join(".gitignore")];
    if let Some(directory) = git_directory(root) {
        files.push(directory.join("info").join("exclude"));
    }
    files
}

fn git_directory(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if fs::metadata(&dot).ok()?.is_dir() {
        return Some(dot);
    }
    let named = fs::read_to_string(&dot).ok()?;
    let directory = resolve(root, PathBuf::from(named.strip_prefix("gitdir:")?.trim()));
    match fs::read_to_string(directory.join("commondir")) {
        Ok(common) => Some(resolve(&directory, PathBuf::from(common.trim()))),
        Err(_) => Some(directory),
    }
}

fn resolve(base: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn literal_directory(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    let name = line.trim_start_matches('/').trim_end_matches('/');
    if name.is_empty() || name.contains('/') || name.contains(['*', '?', '[', ']', '\\']) {
        return None;
    }
    Some(name.to_owned())
}

fn is_dependency(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| DEPENDENCIES.contains(&name))
}

#[cfg(test)]
#[path = "copies_tests.rs"]
mod tests;
