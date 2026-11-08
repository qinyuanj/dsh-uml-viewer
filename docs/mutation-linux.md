# Linux mutation isolation

This first Rust port handles **Rust projects, one worker at a time, on Linux**.
Pass `--max-workers 1`; other worker counts are an explicit usage error until a
parallel-worker ticket implements them. Other mutation languages and the
Windows and macOS isolation mechanisms are not silently substituted: they are
left to their follow-up tickets.

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
