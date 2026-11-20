//! Native Windows: every command, coverage, baseline and mutant included,
//! runs in a fresh copy of the project at its worker slot (ADR 0010).
//!
//! The copy is the original's `CopyViews`: the directories that take no part in
//! a mutation -- build output, dependency trees, and whatever the project's own
//! ignore rules declare -- become an empty directory (#102), a link to a real
//! directory is refused before a command runs, a link to nothing is skipped,
//! file links are materialised as independent files, and `HOME`, `USERPROFILE`,
//! `TEMP`, `TMP`, `APPDATA` and `LOCALAPPDATA` point beside the copy. The
//! command's process tree is #3's kill-on-close Job Object in
//! `quality_core::process`; there is no second containment implementation.
//!
//! A call's `--copy` paths are placed beside the copy at their real
//! project-relative position (ADR 0016): a dependency outside the project root,
//! such as the contracts a package imports from its repository's root, is
//! otherwise missing from the copy and the baseline fails before any mutant.
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

/// The original's `HIDDEN`: build output a copy shows as an empty directory.
///
/// These are the tools' own output names, for a project that declares nothing
/// about its build; `declared_ignored_directories` adds the project's own
/// answer, so the copy is never tied to this list (`hidden_paths`).
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

/// Dependency directories, hidden at any depth (see `hidden_paths` and
/// `is_dependency`).
///
/// A project's ignore rules name one `node_modules` at its root, but pnpm and
/// npm give every package of a workspace its own
/// (`packages/<pkg>/node_modules`), and each of those is a farm of links into
/// the package manager's store -- which a copy may not hold, because a linked
/// directory is what `copy_tree` refuses. A mutation never writes inside a
/// dependency, so the whole tree is an empty directory in the copy.
///
/// This is the one name this module contributes rather than reading from the
/// project, because the *layout*, not the project, is what puts it below a
/// package directory. The boundary: a project that keeps real sources in a
/// directory named `node_modules` loses them from the copy.
const DEPENDENCIES: &[&str] = &["node_modules"];

/// Home, profile, temp and app-data beside the copy, never inside it: each
/// environment variable and the slot entry (`crate::slot`) it points at, plus
/// the sub-path inside that entry.
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

/// The outside paths one call declared, keyed by the project root it runs on.
///
/// Every view of a call must show the same `--copy` paths, and views are created
/// in more places than the CLI reaches (coverage, baseline, mutants), so the set
/// is recorded once per call instead of threaded through each one. A root keeps
/// only its latest call: a call without `--copy` clears it, and a slot deletes
/// its previous command's copies first, so nothing is inherited.
pub fn remember_outside(root: &Path, outside: &[isolation::Outside]) {
    let mut registered = registry();
    if outside.is_empty() {
        registered.remove(root);
    } else {
        registered.insert(root.to_path_buf(), outside.to_vec());
    }
}

/// The outside paths this call declared for `root`; empty without `--copy`.
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

/// One slot of the pool: a copy per command, plus the file a mutant carries.
pub struct Slot {
    directory: PathBuf,
}
impl Slot {
    pub fn reused(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
        }
    }
    /// The directory the copy's `..` resolves to. It sits one level under the
    /// wiped slot, so both `..` and `../..` from the project land inside the
    /// slot and the next command's wipe removes them. A collection's published
    /// scratch lives outside this slot and is opened by absolute path.
    fn beside(&self) -> PathBuf {
        self.directory.join("beside")
    }

    /// One slot entry, named by the declaration that also makes it a `--copy`
    /// reservation (`crate::slot`), so a slot the copy creates and a name the
    /// option refuses can never be two separate lists.
    fn entry(&self, entry: Entry) -> PathBuf {
        self.beside().join(entry.name())
    }

    /// One redirected variable's directory: a slot entry, plus the sub-path the
    /// variable uses inside it.
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

    /// Reset the slot and copy the project into it. `command` is the command
    /// that will run (it decides which dependency trees the copy must keep, see
    /// `Exclusions`), and `mutation` is the mutated source this command must
    /// see, if any.
    fn copy(&self, root: &Path, command: &str, mutation: Option<(&Path, &str)>) -> io::Result<()> {
        // The whole slot is fresh for every command: every entry, with no
        // exemption by name. The copy sits one level down, so a `..` write and a
        // `../..` write both land here and are gone before the next command
        // starts, and a mutant's own `lua-coverage` directory is removed like
        // any other. The one entry a command keeps (`Entry::LuaScratch`) is
        // kept by a collection's view nesting this slot under it, never here.
        for entry in fs::read_dir(&self.directory)? {
            scratch::force_remove(&entry?.path())?;
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
        if let Some((path, source)) = mutation {
            scratch::private_file(&mark)?.write_all(source.as_bytes())?;
            let destination = path
                .strip_prefix(root)
                .map_err(|_| io::Error::other("the mutated source is outside the project root"))?;
            let destination = project.join(destination);
            // Cargo treats a source no newer than its last build as unchanged;
            // every copy is new, but the mutated file is rewritten last.
            fs::write(&destination, source)?;
        }
        Ok(())
    }

    /// How `cwd`, an absolute path in the project, maps into the copy.
    fn cwd(&self, root: &Path, cwd: &Path) -> io::Result<PathBuf> {
        let relative = cwd
            .strip_prefix(root)
            .map_err(|_| io::Error::other("the command's directory is outside the project"))?;
        Ok(self.project().join(relative))
    }

    /// Place one `--copy` path beside the copy, where the copy's `..` reaches it.
    ///
    /// The slot was just wiped, so the bytes are copied again from the real path
    /// for every command: a mutant's write to `..` cannot greet the next
    /// command, and an outside path that changed mid-call is seen as it is now.
    /// Nothing is left behind when the call ends, because the whole slot is
    /// inside the scratch root the pool removes.
    fn place(&self, outside: &isolation::Outside) -> io::Result<()> {
        let source = &outside.source;
        let destination = self.beside().join(&outside.relative);
        let target = format!("{} beside the copy", source.display());
        // The same link rules as the project copy: a linked directory is
        // refused, a linked file is materialised as independent bytes.
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

    /// An argv command reaches the shared MSVCRT quoting of the process
    /// boundary instead of a shell string, as the original's argv plans do.
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
        environment.push((
            "CARGO_TARGET_DIR".into(),
            copy.join("target/mutator-build").into_os_string(),
        ));
        // The installed toolchains stay reachable after the home moves.
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
        // JVM tools use java.user.home for dependency repositories. Cached
        // jars are independent copies; downloads and lockfiles stay beside
        // this worker, rather than writing to the user's Maven repository.
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
    /// The copy's root: reports a command wrote live here, never in the project.
    pub fn project_root(&self) -> PathBuf {
        self.project()
    }
}

fn message(error: io::Error) -> Failure {
    Failure::Message(error.to_string())
}

/// Whether a copy failure is an isolation failure rather than a test result.
///
/// Freezing the project and placing an outside path both fail before any command
/// runs, so neither is a test outcome: the run exits 1 (isolation unavailable)
/// instead of 2 (a failing suite). Every message either step writes starts with
/// "Copy isolation cannot", and the placement failure names its path.
fn isolation_failure(text: &str) -> bool {
    text.starts_with("Copy isolation cannot")
}

/// Every failure of `copy` is an isolation failure, never a test outcome: each
/// of its steps -- the wipe of the previous command's slot, the copy, a placed
/// `--copy` path -- runs before the command does, so a bare `io::Error` there
/// must not be reported as a red suite (#105). A message that already names
/// itself keeps its own words.
fn copy_failure(error: io::Error) -> Failure {
    let text = error.to_string();
    let text = if isolation_failure(&text) {
        text
    } else {
        format!("Copy isolation cannot prepare the copy: {text}")
    };
    Failure::Unavailable(text)
}

/// A `--copy` path that could not be placed names itself: a missing dependency
/// must never look like a broken project.
fn cannot_place(target: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("Copy isolation cannot place {target}: {error}"),
    )
}

/// What a copy leaves out: the project-root directories shown as an empty
/// directory, and whether a dependency tree anywhere in it is hidden too.
pub(crate) struct Exclusions {
    hidden: Vec<PathBuf>,
    /// `node_modules` is an empty directory at any depth, because the command
    /// that will run does not use one (`runs_javascript`).
    dependencies: bool,
}

impl Exclusions {
    /// A copy of `root` under `command`: the project's declared exclusions,
    /// minus the dependency trees the command itself needs.
    ///
    /// This is the rule #102 warned about -- an ignored path can be a build
    /// input, and `node_modules` is one for a JavaScript project: the
    /// TypeScript lane runs the package's own `node_modules/.bin` shim inside
    /// the copy. The command is what says so: the same reading that decides the
    /// JVM home below (`rendered.contains("mvn") ...`) decides this.
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

    /// A copy that leaves out nothing it did not ask to: a declared `--copy`
    /// path and a toolchain's own dependency store.
    fn none() -> Self {
        Self {
            hidden: Vec::new(),
            dependencies: true,
        }
    }
}

/// Whether `command` runs the project's JavaScript toolchain, which reads its
/// dependencies from `node_modules`.
fn runs_javascript(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    [
        "npm", "pnpm", "yarn", "bun", "deno", "node ", "node.exe", "vitest", "jest", "mocha",
        "tsx", "tsc",
    ]
    .iter()
    .any(|tool| command.contains(tool))
}

/// `copy_tree`: a hidden directory becomes empty, a linked directory is
/// refused (it could route a mutant back into the checkout), a link to nothing
/// is skipped, a file link is materialised, and every other file is copied with
/// its timestamps.
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
            // Hidden build output and dependency trees are an empty directory,
            // as in an overlay: present, so a build that expects the directory
            // finds one, and empty, so nothing inside it is frozen.
            fs::create_dir(&target)?;
            continue;
        }
        if is_link(&metadata) {
            // Resolving the link is what separates the two cases #102 turned
            // on. A link to a real directory -- a junction, or a symlink -- is
            // a route a mutant's write could take back into the checkout, so
            // it is refused and the run stops before any command. A link to
            // nothing is a route nowhere: it is skipped, because it cannot
            // carry a write out of the copy, and refusing it made one stale
            // test fixture or one broken package-manager entry price the whole
            // project's mutation run.
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
                // Neither directory nor file: nothing a mutation can use.
                Ok(_) => continue,
                // Present as a reparse point, absent as a target: dangling.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                // Any other resolution failure is still refused: this is the
                // path that must never be guessed at.
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

/// A symlink, or any reparse point (a junction too), as the original sees it.
fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
}

/// The directories a copy shows as an empty directory: the tools' own build
/// output (`HIDDEN`) plus every root directory the project's own ignore rules
/// declare.
///
/// The declaration is the project's, not this module's: `.gitignore` and the
/// repository's `info/exclude` are where a project says which directories are
/// generated or fetched, and that is where `target`, `lib`, `node_modules` and
/// `.agents` are named in this repository (#102). Reusing it is why this is not
/// a hardcoded three, and why it follows a project that ignores something else.
/// The mutation scope (`sourceRoots` in `uml-quality.json`) is deliberately not
/// the input: inverting it would drop `Cargo.toml`, `fixtures/` and every other
/// file a test needs, and the copy would fail before the first mutant.
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

/// Root directories `root`'s ignore rules declare: a literal, top-level
/// directory name in `.gitignore` or in the repository's `info/exclude`.
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

/// `.gitignore` first, then the repository's `info/exclude`.
///
/// A linked worktree -- `.agents/worktrees/<n>` is where a ticket's own tree
/// lives -- has a `.git` *file* naming its per-worktree directory, and the
/// ignore rules live in the shared repository that `commondir` names. Reading
/// only `<root>/.git/info/exclude` would lose `.agents/` exactly where the
/// ticket works.
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

/// `path` as `base` reads it; an absolute path stands as written.
fn resolve(base: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

/// One ignore-file line naming a single root directory literally.
///
/// A negation (`!build/`), a wildcard (`run-*`, `**/tmp`), a nested path
/// (`.claude/worktrees/`) or a file pattern is not this. Those are left to
/// `HIDDEN` and to the link rules: guessing at git's pattern language would
/// silently drop a directory a build needs, which is the failure this ticket
/// exists to end.
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

/// Whether `path` names a dependency directory (`DEPENDENCIES`).
fn is_dependency(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| DEPENDENCIES.contains(&name))
}

#[cfg(test)]
#[path = "copies_tests.rs"]
mod tests;
