# Windows mutation isolation, and the macOS refusal

Native Windows 10 22H2 (build 19045) and later run `uml-viewer mutate`; older
builds are refused before any work, as in the original. Linux keeps the
overlay views described in [mutation-linux.md](mutation-linux.md).

## A fresh copy per command, a Job Object per command

One call owns one private scratch root under the temporary directory (an
unpredictable `mutator-<pid>-<counter>-<16 hex>` name, refused when the
temporary directory is inside the project) and a pool of reusable
`worker-<n>` slots. Coverage, the baseline and every mutant each get a **fresh
copy of the project** at the slot: the previous command's copy and redirected
home are deleted first, so a mutant's writes reach neither the project nor the
next mutant, including a write to the copy's parent directory.

Build output named in the original's `HIDDEN` list (`.git`, `target`,
`coverage`, `.metrics`, …) becomes an empty directory in the copy. A directory
symlink or junction is **refused** with exit 1 before any command runs: it
would be a route back into the checkout. A file symlink is materialised as an
independent file, so writing through it cannot change its target.

`HOME`, `USERPROFILE`, `TEMP`, `TMP`, `APPDATA` and `LOCALAPPDATA` point at a
fresh home beside the copy; `CARGO_HOME` and `RUSTUP_HOME` keep the installed
toolchains reachable and `CARGO_TARGET_DIR` is pinned beneath the copy.

Every command is created suspended and joins a non-inheritable
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` job through
`PROC_THREAD_ATTRIBUTE_JOB_LIST` before its primary thread is resumed — the
same creation path in `quality-core::process` that #3 added for `crap`, now
with a supervised entry (environment overrides, one output file, 25 ms
poll/timeout/stop). Closing or terminating that job is what ends descendants
on a normal leader exit, on a timeout (exit 124, the mutant is killed) and when
`uml-viewer` is killed abruptly. Nothing reimplements containment.

Ctrl+C and Ctrl+Break are recorded by a console control handler; the poll loop
terminates the job, the run removes its copies and exits `128 + signal`
(SIGINT 130, SIGBREAK 149) after reporting
`Interrupted by SIG…; unfinished files keep their previous snapshots.` An
abruptly killed CLI leaves its copies on disk; the owner file's exclusive
`LockFileEx` lock dies with the process, so the next call sweeps the abandoned
root. A live concurrent run's root keeps its lock and is left alone.

Snapshots record `:isolation :copy`, as the pinned original does on Windows;
every other byte is the committed golden's, which the Windows tests assert
with the same injected clock. `Isolation: copy (a fresh copy of the project
per command)` names the mechanism on stdout.

Default coverage needs `cargo-llvm-cov` and the pinned toolchain's
`llvm-tools`. Note that `cargo llvm-cov` itself auto-installs
`llvm-tools-preview` through rustup when it is missing (its `CARGO_LLVM_COV_SETUP`
default), on every platform including under the Python original, so a host
without the component gains it on the first default-coverage run. CI installs
it up front.

## Go, Java and Clojure

The language dispatch now accepts all seven mutation languages. Go uses the
same per-module profiles and `go test -count=1 <package>` argument vectors as
Linux; Java uses Maven/JaCoCo; Clojure uses the project's existing CLI aliases
or babashka tasks. The site reader and operator rules are shared with Linux,
including the custom Clojure reader. Every coverage, baseline and mutant
command continues through the same copy and Job Object boundary.

Maven and Clojure commands seed an independent copy of the cached `.m2`
repository beside the copy and set Java's home there. They refuse linked
repository directories through the same copy-tree guard, so dependency writes
cannot follow a cache directory back into the user's profile.

The Go golden test is enabled on native Windows and compares the pinned
Linux oracle with the original's Windows isolation field (`:copy`). Its first
run was interrupted under the user's Rust resource restriction. Subsequent
CI supplied runtime evidence: the Windows mutation job passed, and Linux
Go/Java/Clojure goldens (7), reports (5) and site comparisons passed (#14 closed).
See the pinned jobs below; this does not claim a new WSL acceptance run.

## Weaker than Linux, and honest about it

A copy is a convention, not an OS-enforced sandbox: an absolute path outside
the project stays writable by a test, exactly as in the original. The original
also copies on the project's volume and block-clones on ReFS; this port copies
bytes with preserved timestamps (`fs::copy`), so a Dev Drive gives no speed
advantage here.

A relative path dependency that leaves the project (`../../extdep`) does not
resolve inside a copy, because the copy has no such sibling. This is the
original's behaviour too; its own fixture helper substitutes an absolute path
to the same directory, and so do the Windows tests.

## macOS

`uml-viewer mutate` on macOS exits 1 before selection and before any command:

```text
Mutation testing is not supported on macOS: mutator runs every mutant in a private overlay view on Linux or in a per-worker project copy on native Windows, and neither mechanism exists on macOS. Mutator never runs a weaker isolation. CRAP and dry work on this machine.
```

The pinned Python original has no macOS path at all: its `NamespaceViews`
calls `unshare` through `ctypes`, which does not exist there, so it fails with
a traceback rather than a message. The one-line refusal is a deliberate
improvement, not parity. `crap` and `dry` are unchanged.

## CI evidence

`windows-mutate` (windows-latest) installs the pinned toolchain's `llvm-tools`
and `cargo-llvm-cov`, sets `MUTATOR_REQUIRE_TOOLCHAINS=1` so a missing
toolchain fails instead of skipping, and runs the mutator suite plus the
`uml-viewer` mutate tests. `macos-quality` (macos-latest) asserts the refusal
and runs the crap and dry suites.

In [main CI6 at `6147980`](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42595826593),
[Windows mutation](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42595826593/job/129088076784),
[Windows CRAP](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42595826593/job/129088077023)
(including the pipe-backpressure regression) and
[macOS quality](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42595826593/job/129088076761)
all succeeded. All three TypeScript jobs also succeeded.

The [Linux job](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42595826593/job/129088076802)
passed the Go/Java/Clojure goldens, reports and site comparisons, with coverage
tool prerequisites installed and the real namespace probe configured for CI.
Its full CLI suite finished with 157 passed, 2 failed and 1 ignored; the two
test-only corrections are in `7fbace6`. The overall CI6 workflow failed, so
these passing jobs are partial evidence, not a claim that the full suite passed.
