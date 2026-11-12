# `uml-viewer quality` — the in-binary quality dispatch

qinyuanj/dsh-uml-viewer#15; ADR 0001 (with the 2026-11-01 and 2026-11-08
appendices), ADR 0012. The host tool (#20) consumes this output; the TypeScript
side is `parseQualityResult` in `packages/contracts`.

```sh
uml-viewer quality <crap|mutate|dry> [--root <path>] [args]
```

`--root` is the dispatcher's option and must directly follow the operation.
Every later argument is forwarded verbatim; in particular an `operations`
command may receive its own `--root`. A value option the dispatcher knows
(`VALUES` in `crates/quality-dispatch/src/config.rs`) keeps its value with it, so
`quality mutate --copy ../contracts app.py` forwards one option and one target
instead of reading the outside path as a target name. The plugin's
`uml_mutate` takes those paths as its `copy` array and emits exactly that
sequence — `--copy <path>` per value, before the targets (#69).

The dispatcher reads `<root>/uml-quality.json` (default root: the current
directory) and runs the requested operation:

- **`standardTools`** (and a missing manifest): the built-in `crap`, `mutate`
  and `dry` subcommands run in this process with the declared scope and
  options, exactly as `scripts/uml_quality.py` built their argv:
  - crap: `--full-tree --source-root R… --exclude G… <crapperOptions> <args>`
  - mutate: `-s R… --exclude G… <mutatorOptions> <options> <targets>`
  - dry: `-s R… --exclude G… <dryerOptions> <args>`
- **`operations.<operation>`**: the declared `argv` runs as a native executable
  at the project root (no shell); the caller's arguments are appended verbatim
  only when `forwardArgs` is `append`. Script/shim entries (`.ps1`, `.bat`,
  `.cmd`; `.exe` also on POSIX) are refused.

## The output contract

**stdout is exactly one JSON line**, the version-1 result record
(`ToolResultRecord<QualityOperation>` plus `reason`). The process's exit status
is the record's `exitCode`. The tool's own report, warnings and everything the
tool or an operation command writes to its stdout appear on **stderr**, so a
caller reads one parseable line and keeps the report separate.

```json
{"version":1,"operation":"crap","projectRoot":"/p","exitCode":0,"validResult":true,"crapScope":"whole-tree","configured":false,"tool":"crapper","reason":null}
```

| Field | Meaning |
|---|---|
| `version` | Always `1`. |
| `operation` | The requested `crap`, `mutate` or `dry`. |
| `projectRoot` | The resolved absolute project root. |
| `exitCode` | The tool's real exit code (0–255); `128+n` after signal `n`. `1` for a refusal. |
| `validResult` | Whether `exitCode` belongs to the selected tool's own valid set (below). |
| `crapScope` | `whole-tree` for a standard CRAP run; the declared `targets`/`whole-tree` for a CRAP override; `null` for mutate and dry. |
| `configured` | `true` on the `operations` route, `false` on the standard route. |
| `tool` | `crapper`, `mutator` or `dryer` on the standard route (also the tool a refusal attempted); `configured` on the `operations` route. |
| `reason` | `null` when the tool ran and succeeded; a nonempty explanation when the dispatcher refused before running it. The host plugin fills it for a failed tool too (#39) — see below. |

**`reason` distinguishes a refusal from a tool that really exited 1.** The
Python dispatch wrote the record only after a command completed and so had no
such marker: a config, launch or scope refusal raised before any record was
published. Here the record is the invocation's return value, so a refusal also
produces one, with `reason` set, `validResult: false`, `exitCode: 1` and `tool`
naming the attempted tool. Only `validResult: true` permits `uml_ir`.

The binary's line leaves `reason` null for a tool that ran and failed: the
tool's own report is on stderr, and the binary keeps the one stdout line a
record. The plugin host (`packages/host/src/quality.ts`) therefore fills the
same field from stderr when a tool really ran and `validResult` is false (#39),
so the agent does not have to run the binary directly to learn why:

```json
{"version":1,"operation":"crap","projectRoot":"/p","exitCode":1,"validResult":false,"crapScope":"whole-tree","configured":false,"tool":"crapper","reason":"uml-viewer quality crap failed (exit 1)\nlast 10 of 40 stderr lines:\n…"}
```

The wording is deliberately not the refusal wording: a refusal reads as the
dispatcher's own sentence, while a failed tool reads `uml-viewer quality
<operation> failed (exit N)` followed by at most the last 10 non-blank stderr
lines, capped at 2000 characters. A tool that wrote nothing but blank lines
keeps `reason: null`. No field is added and ADR 0001 stays version 1; a
non-null `reason` still always has `validResult: false`.

`validResult` is per tool, never a generic rule: crapper `0`/`2` (2 = threshold
finding after a valid snapshot), mutator `0`/`3` (3 = survivors with published
snapshots), dryer `0`, or the override's declared `validResultExitCodes`. An
exit `3` without such a declaration is `validResult: false` while the process
still exits `3` — a declaration is required, not a blanket acceptance.

Other statuses: `2` for a usage error (unknown/missing operation, `--root`
without a value) with usage on stderr and **no record**; `1` for every
configuration, launch, scope or unsupported-language/platform refusal, with an
error record.

Nothing writes, removes or reads `.uml-viewer/quality-result.json`: the record
is the invocation's return value, so it cannot be stale or belong to another
call. The snapshots (`.metrics/crap.edn`, `.metrics/mutate/…`,
`.metrics/dry.edn`) are written by the tools themselves, unchanged.

## Mutation refresh

`:refresh-mutate` is differential: `uml-viewer quality mutate --root P
<file>…`. Every target file is named on one call, because mutator shares one
coverage run and one baseline across them and still publishes each file's
snapshot. Named files and directories are forwarded as project-relative paths
in the order named, narrowed by the declared roots and exclusions; out-of-scope
targets are named on stderr (`uml: omitted mutation target outside business
scope: …`) and dropped, and a call with nothing left is refused without
launching. Missing paths, symlinks and paths outside the project are refused.
`:refresh-mutate-all` is the same call plus `--mutate-all`, which applies only
to the named targets. `--changed` travels as a target and cannot be combined
with named files. A caller may not pass `--root`, `-s`, `--source-root` or
`--exclude` when `standardTools` declares the scope.

A differential rerun of unchanged targets reuses their previous outcomes
(`executed=0 reused=N`, or "No mutations selected; keeping the previous
snapshots." when every target is reusable). Survived mutants are re-executed by
mutator itself, as in the original tool.

`uml_mutate` reaches the same `--copy` option (#69, ADR 0016) through its
`copy` array: each value becomes one `--copy <path>`, forwarded before the
targets, and a call without `copy` forwards exactly what it did before. A
`copy` value is a path **outside** the project root, relative to `--root` and
leaving it through a single leading `..` (`../contracts`); a value that stays
inside the root, climbs twice, is absolute, is empty, or is not normalized
is a parameter error before any quality call — the plugin checks that shape
only, and the binary decides whether the path exists and where it resolves.

## Languages and platforms

An operation whose tool cannot read a declared language is refused with an error
record, never an empty success. All three tools now read every language the
scanners know: mutator's `mutates()` accepts Rust, Lua, Python and TypeScript
(#13) and Go, Java and Clojure (#14), and crapper and dryer read all seven (#7).
Go, Java and Clojure fixture snapshots match the pinned Python originals
byte for byte, including single/multiple-worker consistency. The Linux
[CI run at `f4c12e3`](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42591228938/job/129073050014)
passed all seven snapshot/scanning tests, five report-selection tests and the
mutation-site comparison, closing #14. The optional external Clojure corpus
check remains ignored unless its separate corpus inputs are provided.

Mutation runs on Linux (namespace views,
[mutation-linux.md](mutation-linux.md)) and native Windows (per-command project
copies, #10, [mutation-windows.md](mutation-windows.md)); on macOS it refuses
with exit 1 before any command (`Mutation testing is not supported on macOS`).
A project that declares its own `operations` command is responsible for what
that command does; the dispatcher reports its real exit.

## Maintainer refresh acceptance in CI

The `linux` and native `windows-mutate` jobs run the repository-root sequence
`quality crap`, `quality mutate crates/platform/src/lib.rs`, then
`ir examples/uml-viewer.policy.edn <temporary output>`, using the job's own
binary. Linux reuses the release binary from `scripts/check.py`; Windows builds
debug against its existing integration-test artifacts to avoid a second release
compilation. The 138-line, dependency-free platform crate has small platform
decision tests and no committed mutation history, bounding the differential run
without `--mutate-all`, coverage bypasses or a custom test command.

`scripts/ci-refresh.py` runs one native command per step, checks the single
version-1 stdout record against the actual exit (including valid CRAP 2 and
mutation 3), and requires both valid records before IR. Reports stay on stderr.
The disposable checkout may acquire `.metrics/`; records and nonempty IR go
under `RUNNER_TEMP`, and neither lane uploads them. Steps are bounded to 21/6/1
minutes within the existing Linux 60-minute and Windows 45-minute job limits.
Adding this gate is not evidence that the refresh passed: criterion 6 needs a
successful CI execution on each OS.

## Samples

`fixtures/quality/records.jsonl` holds real output lines that are identical on
Linux and Windows: a mutation run's exit depends on the platform's isolation,
so the sampled mutation cases are its pre-launch refusal and the `operations`
route instead. Regenerate with
`python scripts/t15-samples.py target/debug/uml-viewer.exe
fixtures/quality/records.jsonl` after the binary is built). The Rust test
`quality::the_committed_contract_samples_are_the_binary_output` re-emits the
same scenarios and compares the lines, and `packages/contracts` parses every
line unchanged, so the two sides cannot drift apart silently.
