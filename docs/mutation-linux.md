# Linux mutation isolation

This Rust port handles **all seven languages on Linux: Rust, Lua, Python,
TypeScript, Go, Java and Clojure**.
Native Windows uses per-command project copies and Job Objects, and macOS
refuses mutation before any command: see [mutation-windows.md](mutation-windows.md).
`--max-workers N` runs up to
N mutants at once; without it the limit is one per core, and the actual number
is the smaller of that limit, the core count and the file's selected sites, as
in the original. Other mutation languages are not silently substituted.

A snapshot is byte-identical whatever the worker count: outcomes are keyed by
mutation id and forms are sorted, so parallel execution cannot reorder the EDN.
The acceptance test compares every published snapshot under `--max-workers 3`
## Lua suites (Busted + LuaCov/cluacov)

For a `.lua` source the test command is the generated Busted argv, not a shell
string: `lua5.4 -e "pcall(require, 'luarocks.loader'); require('busted.runner')
({standalone = false}); os.exit(0)" -- busted --ignore-lua [--run <task>]`. The
module directory is the nearest ancestor with a `.busted` file or a rockspec, so
a project root and a nested module each get their own suite, their own coverage
collection and their own baseline. `MUTATOR_LUA_SHARDS` pins the shard count for
tests; by default the instrumented suite splits into one Busted coverage shard
per CPU, capped by the number of spec files. One `Busted in N coverage shards`
line is printed per launched shard, so the split is observable in stderr. Each
shard writes its own LuaCov stats and collector publication, and the host merges
them: spec durations add, line memberships union, and an unattributable hit in
any shard makes those lines full-suite fallbacks.

`lua_coverage.lua` is shipped with the build and loaded before `busted.runner`.
It subscribes to Busted's file start/end, drains LuaCov's per-line counters at
each transition, and publishes `{"version":2,"complete":true,"unsafe":…,
"specs":…,"lines":…,"fallback":…}`. A malformed or incomplete publication — a
spec outside the module, a non-finite duration, a line that is not a positive
decimal, a membership naming an unknown spec, a missing safety flag — aborts the
run with exit 2 instead of guessing a subset, and a failing suite does the same.

cluacov 1.0.0 is required before any test runs: its C hook decides which lines
are executable, so another version would not produce this baseline. Since the
view redirects `HOME`, the suite and the LCOV conversion get `LUA_PATH` and
`LUA_CPATH` confined to `luarocks --lua-version 5.4 config rocks_trees` trees, as
the original's `_pinned_lua_paths` does. The collector, the shard configs, the
LuaCov stats and the LCOV report all live in the disposable view scratch: no
coverage artifact is written to, or removed from, the production project.

A site's line covered in the LCOV report is selected; a line with no hit is
uncovered and counted, not executed. A source absent from the report warns
(`No coverage data for <file>; its sites are uncovered. Pass --no-coverage to run
them anyway.`), and `--no-coverage` runs those sites anyway without collecting
anything — an untested source's mutants therefore survive, which is the expected
exit 3.

Mutants run the full suite with `--no-keep-going` and the covering specs first
(ADR 0003), so a killing spec fails fast while every spec still runs. With
`--select-specs`, each mutant runs only the specs that covered its line, with a
timeout from those specs' durations and their pristine run (never below 2
seconds); a line with no known covering spec, or a subset whose pristine run
fails, falls back to the full suite, and every survivor is confirmed against the
full suite before it counts. `--busted-task` selects a named `.busted` task for
both collection and mutation, and a `--test-command` string bypasses the whole
Lua plan, as the original does.

## Worker slots and the write contract (stronger than the original baseline)

One call creates one private scratch root and a pool of worker slots
`worker-0…worker-N` inside it. Every slot is reused by every mutant of every
target file in that call, and every command still runs in a fresh overlay over
the same slot path: the previous command's layers are removed before the next
one. A mutant's writes therefore reach neither the project nor the next mutant,
while `MUTATOR_VIEW` names the same few slots for the whole call. The pool and
its root are removed when the call ends.

Coverage commands, the baseline test command and every mutant run in a separate
child user+mount+PID namespace. Within the child, `mount --make-rprivate /`
prevents propagation, then overlayfs mounts the canonical project root over
itself. Upper and work directories live in a disposable native temp directory.
The original project is only the lower layer. Mutant source changes are copied
into the overlay only; they are never written to the lower project.

The scratch directory is created with mode `0700` and an unpredictable
`mutator-<pid>-<counter>-<16 hex>` name, and `TMPDIR` is refused when it points
inside the canonical project. The mutation input and captured output files are
created `0600`. Cleanup walks the tree without following links, restores the
zero permission overlayfs leaves on its work directory and reports a removal
failure on stderr instead of ignoring it. Each command's shell is the PID
namespace's first process (`unshare --pid --fork --kill-child`), so when it
exits every remaining descendant dies with the namespace, including a `setsid`
escape; the unshare parent also takes SIGKILL when `uml-viewer` dies. Tests can
read `MUTATOR_VIEW` (the worker slot's path inside the child) to observe these
properties.

Commands run with `HOME`, `XDG_CACHE_HOME` and `TMPDIR` inside the view's own
`target/mutator-home`, with `CARGO_TARGET_DIR` pinned beneath the view and
`CARGO_BUILD_TARGET_DIR`, `CARGO_ENCODED_RUSTFLAGS` and `LLVM_PROFILE_FILE`
unset. `CARGO_HOME` and `RUSTUP_HOME` stay at the installed toolchains' homes so
the pinned toolchain and registry cache remain usable.

Using the canonical root in the child (rather than a differently named project
copy) retains absolute paths and sibling Cargo path dependencies. Cargo output
is pinned beneath the view's own project `target/mutator-build`, even when the
caller supplied `CARGO_TARGET_DIR`. Only `.metrics/mutate` snapshots are
published by the host process. No coverage report, Cargo output, lockfile or
test marker is copied into the production project.

The Python original ran its baseline in the real tree and could therefore leave
baseline test markers there. The ticket deliberately requires isolation for the
baseline as well; this is a stronger write contract, not a snapshot-format
change. A namespace/mount view is not a security sandbox: absolute paths outside
the project remain reachable, as in the original.

`cargo-llvm-cov` and its LLVM tools must already exist. Mutator never installs
coverage tools or components. Timeouts and command exit status select outcomes;
snapshot publication is atomic and the normal clock records local ISO/RFC3339
wall time with seconds and a `+HH:MM` offset.

## Python and TypeScript

Site discovery follows the original's shared leaf walk: binary operators swap
through the Java table plus the bitwise/shift/remainder swaps, `!`/`not` and
unary `-` are deleted, booleans swap and `0`/`1` flip. Python skips bitwise
operators inside `type` annotations and spells booleans `True`/`False`;
TypeScript adds `===`/`!==` and `>>>`→`<<`, and `.tsx` parses with the TSX
grammar. A Python function is private when its name starts with `_` (dunders
excepted); a TypeScript one when its name starts with `#` or its declaration
contains the word `private`. `fixtures/mutate/sites/sites.json` records the
original's sites, offsets, categories, owners, form spans, digests and privacy
for a 30-file corpus, and the parity test compares every field.

The per-file test plan is the original's: Python runs at the nearest
`pyproject.toml`, then `pytest.ini`, then `setup.cfg` directory (marker order
beats nearness) with the project's `.venv`/`venv` interpreter — absolute, its
symlink unresolved — using pytest when the project declares it and
`unittest discover` otherwise. TypeScript runs `npm test` at the nearest
`package.json`. A baseline whose output contains `Ran 0 tests in` fails, so an
empty suite cannot let every mutant survive. Only a generated `cargo test` has
a separate untimed build step.

Python and TypeScript coverage uses #4's runner rules — the same package
roots, interpreter, pytest detection, `--source` tops, `npm run coverage` /
Vitest / c8 choice and LCOV anchoring — but every step runs inside one overlay
view per package, so the coverage data file, the report, any Vitest provider
install and all test writes stay out of the project. A view command starts
from a fresh upper layer, which is why each package's coverage is a single
script. A missing `coverage` module, a failed run or a missing report leaves
that language's files uncovered with the original's N/A message; a file absent
from every report is reported as
`No coverage data for <file>; its sites are uncovered. Pass --no-coverage to run them anyway.`
A custom `npm run coverage` report is read from the root's `coverage/lcov.info`
without anchoring, as the original's `load_bundle` does. A run with only
`--test-command` still measures Python and TypeScript coverage with the
project's own suite, as the original's `_prepare_coverage` does; the Rust path
keeps this port's earlier behavior.

`fixtures/mutate/{python,typescript}` hold the oracle projects and the pinned
original's raw snapshot bytes; `scripts/mutate_goldens_python_ts.py` regenerates
them (see `fixtures/mutate/README.md`). The acceptance tests inject the fixed
clock and compare bytes, rerun unchanged to prove the snapshots are kept, and
compare one worker against two.

## Go, Java and Clojure

Go and Java use the shared language tables and function discovery from
quality-core. Their mutation sites use the original's Java operator table;
Go's `*` → `/` is one way, and neither language adds the bitwise, shift or
remainder swaps used by Rust, Python and TypeScript. Java's `private` header
and a lowercase first character of a Go name give the form a `defn-/` id.

Clojure function ownership, namespaces and spans still use #5's custom
function reader. Its mutation sites use a separate custom reader for the
syntax observed by mutator f772a83: the first symbol directly inside a list,
booleans and exactly `0`/`1`, metadata spans, quotes, reader conditionals,
discarded forms and error recovery. It does not link a Clojure grammar. The
reader was compared with the pinned original on 132 Clojure library sources
(7,033 raw sites), and the committed 21-file corpus compares every field of
288 sites and their forms against the pinned original.

The per-file test plans are argument vectors, passed to the existing worker
pool. Go runs `go test -count=1 <package>` at the nearest `go.mod` (`.` for
that module, otherwise a single `./relative/path` argument). Java runs
`mvn -q test -DexcludeTags=no-mutate` at the nearest `pom.xml`. Clojure prefers
the nearest `deps.edn` over a nearer `bb.edn`: Speclj uses
`clj -M:spec --tag ~no-mutate`, a plain `:spec` uses `clj -M:spec`, and the
fallback is `clj -M:test`. A babashka-only project uses
`bb spec --tag ~no-mutate` when its task file mentions `spec`, otherwise
`bb test`. A `--test-command` retains the common shell override.

Coverage also runs inside disposable views: Go writes its built-in statement
profile, Java uses #5's pinned JaCoCo Maven command, and Clojure runs
`clj -M:cov --lcov` with a fallback without `--lcov`. Reports are read before
their view is dropped. Go selects every line of a hit segment; Java uses the
sourcefile's covered instructions **or** branches; Clojure prefers Cloverage
HTML form counts before LCOV. A source absent from its native report stays
uncovered; LCOV cannot cover a Go or Java source. Reused reports use the same
formats. A malformed Go report stops before publication, while Java's line
reader skips malformed XML, as mutator's original reader does.

Generated Go and Java runs consume only the exact report written for that
module, scoped to that module's sources. Stale sibling reports in a Windows
project copy cannot override a fresh sibling result, and a fresh zero-hit
record stays uncovered. A custom `--coverage-command` reads native reports
from its live disposable view alongside its required LCOV report, as the
original reads them from the project after running the command in place.
Reports the command left unchanged count as well: on Linux the overlay upper
layer is read through to the project below it, and a report the command
deleted (an overlay whiteout, or an opaque directory) stays deleted; a Windows
copy holds the whole project. Absolute copy-local Go source paths map back to
the project. A Go or Java module whose own report is visible scopes its
sources to that report, so module-relative keys keep their module; a module
without one reads the root-level reports (`target/...`, `*/target/...`,
`*/*/target/...`) the original globs. A selected native source that no report
names is not a failure: `No coverage data for …; its sites are uncovered.` is
printed, those sites stay uncovered, and the run continues.

JVM commands get an independent copy of the caller's cached Maven repository
inside their redirected home, and `JAVA_TOOL_OPTIONS` adds `-Duser.home` for
that home. The copy is preflighted recursively before any byte moves: a link at
the `.m2` root or at any entry below it, dangling or not, and any special file
refuse the command before it is launched. Copying then opens every entry with
`O_NOFOLLOW` through held directory descriptors, so a link renamed in after the
preflight cannot redirect it. Only bytes reach a new `0600` file in the `0700`
seed: no symlink, hard link or caller inode permission is carried over, and a
failed seed is removed whole. The seed is bind-mounted over the view's
`$HOME/.m2` and discarded with the view's layers before the next command, so
dependency downloads and lockfiles remain inside the view. The option itself is
the JVM's own quoting, not shell quoting, so a project path with spaces, `'` or
`"` stays one property and is never spliced into shell source. Ordinary-file
writes by another process during the copy can still produce an inconsistent
seed, and the preflight guards the seed rather than the caller's home: an
absolute path outside the project, including the real `~/.m2`, stays reachable,
as noted above for every view command.
The same pool, process containment, interrupt handling and snapshot publication
serve these languages; no language adds another scheduler.

`fixtures/mutate/{go,java,clojure}` contain seven raw fixed-clock snapshot
oracles generated by the unmodified pins. Regeneration and versions are
recorded by `scripts/mutate_goldens_go_java_clojure.py` and
`fixtures/mutate/go-java-clojure-evidence.json`. The Rust golden suites and
report-selection tests are present. Their final execution is pending: the
user restricted Rust validation to scoped `cargo check` during #14, before
the final adapters could be run. See the ticket's progress log for the exact
completed evidence and pending commands; these fixtures alone do not prove
the final runtime acceptance.

## Interrupt, kill and abandoned scratch

`SIGINT` and `SIGTERM` stop a run instead of killing the process mid-cleanup.
The handler only records the signal; the command poll loop kills that command's
process group, the worker loop stops taking sites, and the run removes every
scratch root it owns before reporting
`Interrupted by SIGTERM; unfinished files keep their previous snapshots.` and
exiting `128 + signal`. Files that finished keep their new snapshots; the
unfinished file publishes nothing.

`SIGKILL` cannot be handled. It still leaves nothing running or mounted: each
child is the first process of its own PID namespace and takes `SIGKILL` when
`uml-viewer` dies (`PR_SET_PDEATHSIG`), so its overlay vanishes with the
namespace. The scratch root survives. The next call reclaims it: the owner
writes a `0600` owner file and holds its `flock` for the process's lifetime, so
a root whose lock can be taken belongs to a process that no longer exists, and
`mutate` sweeps it before it starts. A live concurrent run's root keeps its
lock and is never touched. Only this user's `mutator-*` directories directly
under `TMPDIR` are considered, links are never followed, and the removal
reports a failure instead of ignoring it.

## Selection, differential reuse and recovery

`--mutate-all` is a decision, never a wider selection: it executes every
covered site of the files named on the command line (or the changed set), and
no other file gains a snapshot. Source roots, positional filters and
`--exclude` narrow the selection exactly as they do without it.

`--changed` follows the original and uses the shared changed selection
(`Scope::select_changed`): source roots narrow the selection only when `-s` is
given, and the positional text filters and `--exclude` patterns apply
afterwards, like every other selection. Mutator still filters the resulting
files to non-test sources of the languages this build mutates.

The differential fast path ("No mutations selected; keeping the previous
snapshots.") mirrors the original `reused_run` guard per file: the file must
still own mutation sites, every current function digest must equal the recorded
one, and no site may be selected while every site is treated as covered. A
function added or rewritten with no mutation sites of its own is therefore not
reusable: the run takes the normal path and republishes that file's snapshot,
carrying the prior outcomes, without executing a test.

Before any test, the non-`--scan` path refuses to run with exit 2 while
`target/mutator-backup` holds files, as the original does; nothing is written
back and an empty backup directory is not a refusal.

`--scan` inventories sites only: it runs no coverage command and applies the
`--lines` filter. `--mutation-warning` (default 50) warns per file above its
threshold, `--timings` prints the original's per-phase `timing <phase>: ...`
summary on stderr, and `--verbose` prints the original's per-site decision line.
Usage errors print the original message, a blank line and the exact help bytes.

Snapshot publication stays under the canonical project: a symlinked `.metrics`,
`.metrics/mutate`, namespace directory or snapshot file is refused (exit 2)
rather than read, written or deleted through, and directory walking never
follows links.

## Byte oracle

`fixtures/mutate` and `scripts/mutate_goldens.py` are development-only inputs.
They reproduce the Python snapshot bytes and exact help text using mutator
f772a83 and crapper 4068857. The Lua case (`--lua`) additionally needs Lua 5.4,
Busted, LuaCov, cluacov 1.0.0 and dkjson; it writes `fixtures/mutate/lua`
(project, goldens, `sites.json`, `evidence.json`). The product binary has no
Python runtime.
`run_with_clock` injects a clock only for the fixed-timestamp golden tests;
normal `uml-viewer mutate` uses the real local clock, not timestamp replay.

## Known deviations from the original

- The console report is this port's own compact text (per-file
  `executed=/reused=` counts, scan lines and verbose decision lines), not the
  original results table, site log and decision summary.
- Go, Java and Clojure consult the shared crapper freshness stamp before
  collecting native reports. The earlier Rust, Python, TypeScript and Lua
  paths retain their existing behavior and collect again in the overlay.
  Mutation itself writes no coverage stamp or report to the project.
- Python's runner installs pytest when it is missing; mutator never installs
  tools, so a detected but unavailable pytest falls back to
  `unittest discover` with the original's message. Vitest's coverage provider
  install still happens, but only inside the view.
- A source that is not UTF-8 aborts the run with exit 2 instead of being skipped.
- The original's Rust worker slots also keep a Cargo target directory per slot
  across files and runs (`WorkerSlots`). This port reuses the slot but not its
  build output: every command builds in the overlay's own
  `target/mutator-build`, exactly as the single-worker port did. Snapshot bytes
  are unaffected; only build time is.
- `--paranoid-workers` is accepted and ignored. One pristine run per command
  and working directory is the pool's baseline, and every worker's view is
  built the same way, so a per-worker check would repeat the same run.
- Only `HOME`, `XDG_CACHE_HOME` and `TMPDIR` are redirected; the Windows-only
  profile and app-data variables of the original's copy isolation have no Linux
  counterpart here.
- The Lua suite is collected in coverage shards (default one per CPU, capped by
  the spec count) instead of the original's single instrumented run; the merged
  index, the selection and the snapshot bytes are the same.
- The Lua spec index is not persisted: `--use-existing-coverage` reuses the
  LCOV reports the shared path finds, and a spec index needed by
  `--select-specs` is recollected in a fresh view. Persisting it would write
  into the project, which this build never does.
- Pristine runs (the baseline, each selected-spec subset and their union) run
  in the pool's first slot and are cached per command and directory, instead of
  the original's per-worker pristine checks; a subset that fails its pristine
  run falls back to the full suite, and survivors are confirmed on it.
- `MUTATOR_LUA_NO_PIN=1` opts out of the LuaRocks search-path pinning as in the
  original; the original's `lua_support.lua`/fasttrace machinery and its 2 GiB
  Lua address-space cap are not ported here. Lua mutants use the same worker
  slots as Rust ones, with the covering specs first or the selected subset.
