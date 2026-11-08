# Linux mutation isolation

This first Rust port handles **Rust projects, one worker at a time, on Linux**.
Pass `--max-workers 1`; other worker counts are an explicit usage error until a
parallel-worker ticket implements them. Other mutation languages and the
Windows and macOS isolation mechanisms are not silently substituted: they are
left to their follow-up tickets.

## Write contract (stronger than the original baseline)

Coverage commands, the baseline test command and every mutant run in a separate
child user namespace and mount namespace. Within the child, `mount --make-rprivate /`
prevents propagation, then overlayfs mounts the canonical project root over
itself. Upper and work directories live in a disposable native temp directory.
The original project is only the lower layer. Mutant source changes are copied
into the overlay only; they are never written to the lower project.

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

## Byte oracle

`fixtures/mutate` and `scripts/mutate_goldens.py` are development-only inputs.
They reproduce the Python snapshot bytes and exact help text using mutator
f772a83 and crapper 4068857. The product binary has no Python runtime.
`run_with_clock` injects a clock only for the fixed-timestamp golden tests;
normal `uml-viewer mutate` uses the real local clock, not timestamp replay.
