# Windows mutation isolation, and the macOS refusal

Native Windows runs `uml-viewer mutate` on any build. No capability here is
newer than Windows 10, which the toolchain already requires, so the OS alone
decides (ADR 0018) — as in the pinned original after
[mutator#30](https://github.com/qinyuanj/mutator/issues/30), which dropped its
build floor for the same reason. macOS is still refused with a reason, because
it has neither isolation mechanism; that refusal is this port's own, not a
build-number gate. Linux keeps the overlay views described in
[mutation-linux.md](mutation-linux.md).

## A fresh copy per command, a Job Object per command

One call owns one private scratch root under the temporary directory (an
unpredictable `mutator-<pid>-<counter>-<16 hex>` name, refused when the
temporary directory is inside the project) and a pool of reusable
`worker-<n>` slots. Coverage, the baseline and every mutant each get a **fresh
copy of the project** at the slot: the whole slot is deleted before the copy is
made — the previous command's copy, its redirected home and temporary
directories, its capture files and its `--copy` placements — so a mutant's
writes reach neither the project nor the next mutant, including a write to the
copy's parent directory.

### The one exception: a Lua coverage collection

A Lua suite reports through its own instrumented collection, which needs a few
files to outlive a single command: the embedded collector that every command's
`-e` bootstrap `dofile`s, each shard's LuaCov config and stats, each shard's
spec publication, and the merged stats with the config the LCOV conversion
reads. None of them may live in the project tree — a copy is rebuilt for every
command and the project itself is never written — so a collection gets a view
of its own whose **slot is a child** directory of that view:

```text
<view>/                       the collection's own private scratch root
  lua-coverage/               published scratch: collector, per-shard config and
                              stats, spec publications, merged stats, LCOV config
  slot/                       wiped before every command
    beside/
      project/                the command's fresh copy
      home/                   the command's redirected home
```

Preparing a command deletes everything inside `<view>/slot`: the copy, the
redirected home, the capture files and the `--copy` placements, the same wipe
every other command gets. The view names that scratch
(`View::lua_coverage_scratch`), so the collection never composes the layout
itself, and `lua-coverage/` sits beside the slot, so it survives
the wipe, and `..` and `../..` from `<view>/slot/beside/project` both stay
inside the wiped region: a test cannot stumble onto that scratch, and it is read
only through the absolute path the collection itself puts in the command line
(`dofile('<view>/lua-coverage/lua_coverage.lua')`,
`--coverage-config-file=<view>/lua-coverage/luacov.shard-1.cfg.lua`).

That scratch belongs to **that one collection**. The view is removed when the
collection ends, so no later command of the same call (the baseline, a mutant,
another language's coverage command) can see it; nothing is written to the
project tree, so an empty `lua-coverage/` at the project root is neither
created nor needed; and `--reuse-coverage` skips the collection altogether.
Linux needs no such layout: its collection resets only the overlay layers, so
the scratch published in the view directory is already outside them.

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
- The declared path itself is resolved first, then copied as ordinary bytes:
  `resolve_outside` canonicalizes it, so `--copy ../contracts` where `contracts`
  is a junction places the target directory's contents and `copy_tree`'s
  linked-directory branch never fires for the declared path. That is safe — the
  bytes still land inside the call's slot, and a write through them changes only
  that command's copy — but it is what the option does, not a link refusal.
  Links **inside** the declared path still obey the project copy's rules: a
  linked directory is refused (`Copy isolation cannot freeze linked directory: …`)
  and a linked file is materialised as independent bytes.
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

## Lua suites

A Lua suite does not report through a coverage tool the project depends on. The
collection ships its own instrumented Busted bootstrap (`lua_coverage.lua`,
embedded in the binary), splits the suite into coverage shards and reads the
LuaCov LCOV report. Everything it writes — the collector, each shard's
`luacov.shard-N.stats.out`, each shard's spec publication, the merged stats and
the conversion config — is the collection's own scratch beside the slot, not
inside the project copy (`View::lua_collection`). The slot is still wiped before
every command, so the collector is openable by the shard that `dofile`s it, a
later shard still reads the earlier shards' publications, and the LCOV
conversion still reads the merged stats after the last shard. The project tree
is never a write target, and the scratch belongs to that one collection.

`MUTATOR_LUA_SHARDS=1` and the default count take the same path; only the shard
count differs, and the report prints one `Busted in N coverage shards` line per
launch. #73's LF normalisation is what makes the merged (default-count) form
work on Windows at all.

The collection reports a dead bootstrap and a failing suite with the same words,
`Lua tests exited N` (with the shard position when there is more than one), so a
caller cannot tell them apart from that line. Busted's own summary — `2 successes
/ 0 failures / 0 errors / 0 pending` — is the evidence that the suite was
entered, and `crates/uml-viewer/tests/mutate_lua.rs` judges a run by it: one
coverage shard and the default count each run without `--reuse-coverage`, the
baseline's summary and the shard launches are asserted, and the published
outcomes are compared with the committed golden, so a mutant that was not judged
fails the case. `--reuse-coverage` still skips this collection entirely, which
the same file asserts (no Busted process, no shard).

`windows-mutate` installs that toolchain with `scripts/setup-windows-lua.ps1`:
Lua 5.4.8 built with the pinned winlibs MinGW-w64, LuaRocks 3.12.0, Busted 2.3.0,
LuaCov 0.17.0, cluacov 1.0.0 and the LCOV reporter, every download SHA-256
pinned, then it proves the modules load. It exports `PATH`, `LUA_PATH` and
`LUA_CPATH` to the later steps (and to `$GITHUB_ENV` in Actions). This is the
same recipe a maintainer's Windows host uses, and `MUTATOR_REQUIRE_TOOLCHAINS=1`
makes a missing toolchain a failure rather than a skip.

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
coverage test and the whole Lua 5.4 toolchain for the Lua suite (the same rule:
a missing tool fails), and runs the mutator suite plus the `uml-viewer` mutate
tests, including the Lua cases that prove the suite is really entered. `macos-quality` (macos-latest) asserts the refusal
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
