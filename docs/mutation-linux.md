# Linux mutation isolation

This first Rust port handles **Rust and Lua projects, one worker at a time, on
Linux**. Pass `--max-workers 1`; other worker counts are an explicit usage error
until a parallel-worker ticket implements them. Other mutation languages and the
Windows and macOS isolation mechanisms are not silently substituted: they are
left to their follow-up tickets.

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

## Write contract (stronger than the original baseline)

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
read `MUTATOR_VIEW` (the scratch path inside the child) to observe these
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

## Selection, differential reuse and recovery

`--changed` follows the original and uses the shared changed selection
(`Scope::select_changed`): source roots narrow the selection only when `-s` is
given, and the positional text filters and `--exclude` patterns apply
afterwards, like every other selection. Mutator still filters the resulting
files to non-test Rust sources.

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
f772a83 and crapper 4068857. The product binary has no Python runtime.
`run_with_clock` injects a clock only for the fixed-timestamp golden tests;
normal `uml-viewer mutate` uses the real local clock, not timestamp replay.

## Known deviations from the original

- The console report is this port's own compact text (per-file
  `executed=/reused=` counts, scan lines and verbose decision lines), not the
  original results table, site log and decision summary.
- Fresh crapper coverage is not reused from `target/coverage/crapper-stamp.json`;
  default Rust coverage runs again in the overlay instead. No coverage or build
  output reaches the original project either way.
- A source that is not UTF-8 aborts the run with exit 2 instead of being skipped.
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
- With one worker, the original's per-worker pristine check is the same run as
  the cached baseline, so an already-run plan is not repeated; the selected-spec
  union subset still gets its own pristine run before any mutant.
- `MUTATOR_LUA_NO_PIN=1` opts out of the LuaRocks search-path pinning as in the
  original; the original's `lua_support.lua`/fasttrace worker machinery and its
  2 GiB Lua address-space cap are not ported here (there are no workers yet).
