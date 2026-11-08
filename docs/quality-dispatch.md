# `uml-viewer quality` — the in-binary quality dispatch

qinyuanj/dsh-uml-viewer#15; ADR 0001 (with the 2026-11-01 and 2026-11-08
appendices), ADR 0012. The host tool (#20) consumes this output; the TypeScript
side is `parseQualityResult` in `packages/contracts`.

```sh
uml-viewer quality <crap|mutate|dry> [--root <path>] [args]
```

`--root` is the dispatcher's option and must directly follow the operation.
Every later argument is forwarded verbatim; in particular an `operations`
command may receive its own `--root`.

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
| `reason` | `null` when the tool ran; a nonempty explanation when the dispatcher refused before running it. |

**`reason` distinguishes a refusal from a tool that really exited 1.** The
Python dispatch wrote the record only after a command completed and so had no
such marker: a config, launch or scope refusal raised before any record was
published. Here the record is the invocation's return value, so a refusal also
produces one, with `reason` set, `validResult: false`, `exitCode: 1` and `tool`
naming the attempted tool. Only `validResult: true` permits `uml_ir`.

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

## Not ported yet

An operation whose tool does not read a selected language is refused with an
error record, never an empty success: mutator reads Rust and Lua (Go/Java/Clojure
follow with #14); crapper and dryer read all seven (#7). Mutation also
refuses off Linux (`Mutation isolation is currently supported only on Linux`).
A project that declares its own `operations` command is responsible for what
that command does; the dispatcher reports its real exit.

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
