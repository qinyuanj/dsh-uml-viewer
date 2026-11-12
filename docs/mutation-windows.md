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

## Paths outside the project root (`--copy`)

A copy holds the project root and nothing beside it, so a relative path that
leaves the root (a suite importing `../../../contracts/board-policy.json`, a
fixture next to the package) does not resolve in the copy and the **baseline
fails before any mutant runs**. `--copy <path>` (ADR 0016) places one such path
beside every command's copy at the position the copy's `..` reaches: with
`--copy ../contracts`, a command running in `<slot>/project` reads
`../contracts/value.txt` as `<slot>/contracts/value.txt`, copied from the real
`contracts/` beside the project root. The option may be repeated, in any order,
and a position declared twice is one copy. A directory is copied as a tree and a
file as one file, so a single dependency file needs no parent directory in the
project: `--copy ../server/Cargo.toml` creates `<slot>/server/`. One call can
name both kinds, as a package whose tests read the repository root does:
`--copy ../contracts --copy ../docs/release --copy ../server/Cargo.toml`.

- The slot is deleted before every command, so the outside bytes are copied
  again for each command: a mutant's write under `..` cannot greet the next
  command, and nothing is left behind when the call ends, because the whole slot
  is inside the scratch root the pool removes.
- The outside copy obeys the project copy's link rules: a linked directory is
  refused (`Copy isolation cannot freeze linked directory: …`) and a linked file
  is materialised as independent bytes.
- A path that cannot be placed fails that command with the path named
  (`Copy isolation cannot place <path> beside the copy: …`) instead of leaving a
  copy that silently lacks a dependency. Placement is not a test result, so it
  exits **1** (isolation unavailable), like every other isolation refusal — never
  2, which means a suite failed.
- The argument is a path relative to `--root` that leaves it through **one**
  leading `..`. An absolute path, a path inside the project root (the copy
  already has it), more than one leading `..` (it would reach outside the
  call's scratch slot), the bare `..` that names the project's parent directory
  itself rather than a path in it (there is no position for it beside the copy),
  a path that resolves back inside the project root, a missing path, and a path
  whose first component names one of the slot's own entries — `project`, `home`,
  `temp`, `appdata`, `output`, `mutation`, in any case — are all usage errors,
  refused before any command runs with exit 1, like every other option error.
  `--copy ..` and `--copy ../.` both fail with
  `--copy path names the project's parent directory itself, not a path in it`.
  The reserved names are lexical: `../output` is refused whether or not an
  `output` directory exists, because a copy that placed it there would delete it
  (or merge it over the project copy) before the command ran.
- Each command pays one extra tree copy per `--copy` path; name only what a
  command actually reads.

Linux accepts and validates the same option and copies nothing: its view is
mounted over the real project root, so `..` already reaches the real parent
directory ([mutation-linux.md](mutation-linux.md)).

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

## Coverage scripts run in the copy

`cargo llvm-cov` is not the only coverage a run may need: the standard path also
measures TypeScript and Python when the selected files are those languages. On
Windows both generated scripts are `cmd.exe` scripts, and both spell every path
they write relative to the package they run in — a copy lives at
`<slot>/project`, so an absolute path would name the real project and write
there. `cargo llvm-cov` already followed that rule; TypeScript and Python did
not, which is why a package that reported no coverage data at all on Windows
(the report directory was handed to `rm -rf --` as a Windows path) now measures
itself.

The shape is one `&` chain, because `cmd /c` takes a single line: `rmdir /s /q`
and `mkdir` for the report directory, `del /f /q` for the stale data file,
`|| (…)` blocks for the N/A guards, and every `if` parenthesized and last on the
chain — `if … (block) & next` makes cmd read `next` as part of the skipped
block, silently dropping the rest of the script. Arguments are quoted the way
cmd and MSVCRT read them rather than with `shlex`'s single quotes, and Vitest is
invoked through `node_modules\.bin\vitest.cmd`, the only shim cmd can execute.

A TypeScript or Python package whose coverage toolchain is missing leaves that
language uncovered with the original's `… will be N/A.` message; it never fails
the run. When crapper ran first and its stamp still covers this tree, neither
script runs at all: the reports it already wrote are reused (see
`docs/mutation-linux.md`, `crapper's coverage stamp`), and
`Reusing <languages> coverage from crapper's last run.` says so. A stale stamp
regenerates the language instead, and that language's reports already on disk
are not read: a script that quietly writes nothing leaves its sites uncovered
rather than borrowing an earlier run's hits.

## Weaker than Linux, and honest about it

A copy is a convention, not an OS-enforced sandbox: an absolute path outside
the project stays writable by a test, exactly as in the original. The original
also copies on the project's volume and block-clones on ReFS; this port copies
bytes with preserved timestamps (`fs::copy`), so a Dev Drive gives no speed
advantage here.

A relative path dependency that leaves the project (`../../extdep`) does not
resolve inside a copy, because the copy has no such sibling. This is the
original's behaviour too; its own fixture helper substitutes an absolute path
to the same directory, and so do the Windows tests. `--copy` (above) is how a
call brings such a path into the copy instead, and the bytes it brings are the
command's own: a write through them changes the copy, never the real tree.

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
toolchain fails instead of skipping, installs `coverage.py` for the Python
coverage test (the same rule: a missing tool fails), and runs the mutator suite
plus the `uml-viewer` mutate tests. `macos-quality` (macos-latest) asserts the refusal
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
