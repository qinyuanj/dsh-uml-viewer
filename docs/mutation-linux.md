# Linux mutation isolation

This Rust port handles **Rust projects on Linux**. Native Windows uses
per-command project copies and Job Objects, and macOS refuses mutation before
any command: see [mutation-windows.md](mutation-windows.md). `--max-workers N` runs up to
N mutants at once; without it the limit is one per core, and the actual number
is the smaller of that limit, the core count and the file's selected sites, as
in the original. Other mutation languages are not silently substituted.

A snapshot is byte-identical whatever the worker count: outcomes are keyed by
mutation id and forms are sorted, so parallel execution cannot reorder the EDN.
The acceptance test compares every published snapshot under `--max-workers 3`
with the single-worker run and with the original's goldens.

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
