This Rust port handles **Rust and Lua projects on Linux**. Native Windows uses
per-command project copies and Job Objects, and macOS refuses mutation before
any command: see [mutation-windows.md](mutation-windows.md). `--max-workers N`
runs up to N mutants at once; without it the limit is one per core, and the actual number
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
- Fresh crapper coverage is not reused from `target/coverage/crapper-stamp.json`;
  default Rust coverage runs again in the overlay instead. No coverage or build
  output reaches the original project either way.
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
