//! crapper's command line: the same options, messages and help as the
//! Python crapper (`-h` anywhere prints help; a usage error prints the
//! message, a blank line and the help on stderr, exit 1).

use std::path::PathBuf;

pub const HELP: &str = "\
Usage: crapper [options] [path-or-filter ...]

Detect the language of each source file and score it with the CRAP metric
CRAP = CC² × (1 − coverage)³ + CC. Writes .metrics/crap.edn for uml-viewer
and prints a report sorted worst first.

Languages: Clojure (.clj .cljc .cljs .bb), Java (.java), Go (.go),
TypeScript (.ts .tsx .mts .cts), Rust (.rs), Python (.py), Lua (.lua).

Options:
  -h, --help                    Print this help and exit.
  --root <path>                 Project root. Metrics are written here.
                                Default: the current directory.
  -s, --source-root <path>      Walk this tree instead of the project root.
                                May be repeated. Path arguments and --changed
                                are narrowed to the files under a source root.
  --exclude <path-or-glob>      Exclude a project-relative file or directory.
                                May be repeated; applies to every selection mode.
                                Both separators work. * and ? match within a path
                                segment; ** matches zero or more whole segments.
                                Matching directories exclude their descendants.
                                An absolute pattern or one with .. is an error.
  --full-tree                   Replace the complete configured business tree.
                                Rejects --changed, positionals and existing coverage.
                                Any language's coverage failure keeps the previous
                                snapshot and exits 1. --no-coverage publishes
                                complexity only.
  --changed                     Analyze source files git reports as added,
                                modified, renamed, or untracked under the
                                project root, outside the skipped directories.
  --no-coverage                 Score complexity only. Coverage and CRAP are N/A.
  --use-existing-coverage       Read coverage already on disk. Do not rerun tests.
  --coverage-command <cmd>      Run this command instead of the per-language
                                coverage tools, then read the reports it wrote.
  --threshold <number>          Exit 2 when the worst CRAP score is above this,
                                or 3 when coverage was requested but no
                                function's coverage was measured.
  --lua-shards <n>              Split Lua spec files across n Busted coverage
                                shards. Default: the CPU count. 1 keeps one
                                process for specs that share state.

Arguments:
  path              File or directory to analyze. Test paths are included when
                    you name them explicitly. With --source-root, a named file
                    outside every root is dropped:
                    \"Skipping <path>; it is outside the source roots\" on stderr.
  filter            When the argument is not a path, only source files whose
                    path contains this text are analyzed.

With no paths, source files under the project root are analyzed. Directories
named test, tests, spec, specs, vendor, node_modules, and target are skipped,
as are *_test.go, *.spec.ts, test_*.py, *_test.py, and *_spec.lua files.

Exit status: 0 when the analysis completed, 2 when --threshold was exceeded
after the snapshot was written, 3 when --threshold was given with coverage but
no analyzed function's coverage was measured, after the snapshot was written,
1 for a usage error, failed Lua or custom coverage, failed --full-tree coverage,
or a snapshot that could not be written. With coverage, functions whose
coverage is N/A are counted on stderr.

Coverage, when it is produced:
  Clojure      clj -M:cov --lcov, then Cloverage form counts or LCOV
  Java         Maven JaCoCo instruction coverage, for each pom.xml module
  Go           go test ./... -coverprofile=...
  TypeScript   npm run coverage, Vitest's own LCOV, or c8 around npm test
  Rust         cargo llvm-cov, or cargo tarpaulin, in the nearest Cargo.toml
  Python       coverage.py LCOV, via pytest or unittest, per project
  Lua          busted -c with luacov, then luacov -r lcov, per .busted or rockspec
Languages whose coverage succeeded are stamped in
target/coverage/crapper-stamp.json, so mutator can reuse the reports until
any file outside the skipped directories and tool caches (.pytest_cache,
.cpcache, .coverage, ...) changes. A file edited while coverage runs leaves
that run unstamped. A coverage script is judged by its exit status; the other
coverage commands fail when they write no report.

uml-viewer joins .metrics/crap.edn on :namespace and :name.
  Clojure      the ns, and the defn name
  Java         package.Class, and the method name
  Go           import path, or import.path.Receiver for methods
  TypeScript   dotted module path, or module.Class for methods
  Rust         crate::module, or crate::module::Type for methods
  Python       dotted module path, or module.Class for methods
  Lua          require path, and the name as written (M.foo, Class:method)
";

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub project_root: PathBuf,
    pub source_roots: Vec<String>,
    pub exclusions: Vec<String>,
    pub positionals: Vec<String>,
    pub no_coverage: bool,
    pub use_existing_coverage: bool,
    pub coverage_command: Option<String>,
    pub threshold: Option<f64>,
    pub changed: bool,
    pub full_tree: bool,
    pub lua_shards: Option<u32>,
}

/// What the command line asks for.
pub enum Parsed {
    Help,
    Usage(String),
    Analyze(Options),
}

fn take<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    match args.get(index + 1) {
        Some(value) if !value.is_empty() && !value.starts_with('-') => Ok(value),
        _ => Err(format!("{option} requires a value")),
    }
}

fn apply(options: &mut Options, args: &[String], index: usize) -> Result<usize, String> {
    let arg = args[index].as_str();
    match arg {
        "-s" | "--source-root" => {
            options
                .source_roots
                .push(take(args, index, arg)?.to_string());
            Ok(index + 2)
        }
        "--exclude" => {
            options.exclusions.push(take(args, index, arg)?.to_string());
            Ok(index + 2)
        }
        "--root" => {
            options.project_root = PathBuf::from(take(args, index, arg)?);
            Ok(index + 2)
        }
        "--coverage-command" => {
            options.coverage_command = Some(take(args, index, arg)?.to_string());
            Ok(index + 2)
        }
        "--threshold" => {
            let raw = take(args, index, arg)?;
            let value =
                quality_core::number::python_float(raw).ok_or("--threshold requires a number")?;
            options.threshold = Some(value);
            Ok(index + 2)
        }
        "--lua-shards" => {
            let raw = take(args, index, arg)?;
            let shards = raw
                .bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| raw.parse::<u32>().ok())
                .flatten()
                .filter(|count| *count >= 1)
                .ok_or("--lua-shards requires a positive integer")?;
            options.lua_shards = Some(shards);
            Ok(index + 2)
        }
        "--no-coverage" => {
            options.no_coverage = true;
            Ok(index + 1)
        }
        "--use-existing-coverage" => {
            options.use_existing_coverage = true;
            Ok(index + 1)
        }
        "--changed" => {
            options.changed = true;
            Ok(index + 1)
        }
        "--full-tree" => {
            options.full_tree = true;
            Ok(index + 1)
        }
        other if other.starts_with('-') => Err(format!("Unknown option: {other}")),
        other => {
            options.positionals.push(other.to_string());
            Ok(index + 1)
        }
    }
}

fn conflict(options: &Options) -> Option<String> {
    if options.no_coverage && options.coverage_command.is_some() {
        return Some("--no-coverage cannot be combined with --coverage-command".to_string());
    }
    if let Some(error) = options
        .exclusions
        .iter()
        .find_map(|pattern| quality_core::select::exclusion_error(pattern))
    {
        return Some(error);
    }
    if options.full_tree {
        let rejected = [
            ("path arguments", !options.positionals.is_empty()),
            ("--changed", options.changed),
            ("--use-existing-coverage", options.use_existing_coverage),
        ];
        if let Some((flag, _)) = rejected.iter().find(|(_, set)| *set) {
            return Some(format!("--full-tree cannot be combined with {flag}"));
        }
    }
    None
}

pub fn parse(args: &[String]) -> Parsed {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Parsed::Help;
    }
    let mut options = Options {
        project_root: PathBuf::from("."),
        ..Options::default()
    };
    let mut index = 0;
    while index < args.len() {
        match apply(&mut options, args, index) {
            Ok(next) => index = next,
            Err(message) => return Parsed::Usage(message),
        }
    }
    match conflict(&options) {
        Some(message) => Parsed::Usage(message),
        None => Parsed::Analyze(options),
    }
}
