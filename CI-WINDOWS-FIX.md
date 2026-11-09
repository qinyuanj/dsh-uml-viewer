# Windows CI fixes (spec-203 continuation)

Base: `700c9af647005d8927e2bb39ff23874bb116b2c0`; branch: `tci-windows-exit`.

## Quoting root cause

The failing production trace is `child_env_tests::polluted_driver` ->
`View::run_argv` -> `copies::Slot::run_argv` -> `Program::Argv` ->
`start_supervised_with_env_removals` -> `process_windows::prepare` -> CreateProcessW.
This boundary always quoted every token, including `/c`. It emitted
`"cmd.exe" "/d" "/c" "set"`. cmd parses its command tail directly rather than
using MSVCRT argv rules; quoting `/c` leaves a quote at the start of the command
and produces the exact CI error: `'"set' is not recognized ...`.

The fixture is valid and unchanged. The minimal appropriate shared layer is
Windows argv serialization: keep program quoting and existing escape rules,
but leave nonempty arguments without whitespace or double quotes bare.
Both native argv rendering sites (ordinary executable and native Node entry)
now use that rule. Shell and batch branches already emit bare switches and
retain their separate shell syntax. Existing literal-argument tests remain.
A new regression `cmd_slash_c_switch_reaches_cmd_unquoted` uses the shared
production capture boundary and asserts successful exit plus environment output.

## Native probes (no Rust compilation)

PowerShell 7.6.9 ProcessStartInfo.Arguments passes raw command text to the native
process. A launcher path containing spaces forces program-token quoting too.
The raw `/d` and `/c` matrix showed:

| Tail | Exit | Outcome |
| --- | --- | --- |
| `"/d" "/c" "set"` | 1 | Exact CI `'"set'` error |
| `/d /c "set"` | 0 | Environment printed |
| `"/d" "/c" set` | 0 | Environment printed |
| `/d "/c" "set"` | 1 | Exact CI error |
| `"/d" /c "set"` | 0 | Environment printed |
| `/d /c set` | 0 | Environment printed |

Replaying the old/new serializer through native cmd and installed Node 24.19.0:
old cmd exit 1, no keep variable; new cmd exit 0, `MUTATOR_ENV_KEEP=keep-me`.
Both serializers round-trip the same 12 Node arguments byte-for-byte, including
empty values, spaces, ampersands, pipes, embedded quotes, trailing backslashes,
Unicode, percent expansion text and switches. No Rust executable was generated.

## CI root cause and complete workflow sweep

With default pwsh native error handling, `$ErrorActionPreference='stop'` does
not fail a step for a native nonzero result. GitHub's final LASTEXITCODE wrapper
uses the last native command, so successful integration tests hid the earlier
mutator test failure.

Split each affected Windows multiline step into one native command per step:

- ci/panel-e2e: plugin build and resident build.
- ci/windows-crap: crate tests and integration tests.
- ci/windows-mutate: llvm-tools and cargo-llvm-cov installation; crate tests and
  integration tests.
- package-release/assemble: plugin build and native release build (all runners).

The TypeScript matrix has only single-command steps. Release assemble's remaining
multiline stage step uses explicit bash plus `set -euo pipefail`. Inventory and
publish use bash on Ubuntu. All other multiline CI steps run on Linux/macOS under
the runner's fail-fast bash invocation. The Lua installation step is untouched.
Both workflow files parsed successfully with installed PyYAML 6.0.3.

A false-first/true-last probe executed the GitHub pwsh wrapper in a clean child:
`cmd /c ... exit 3` then `cmd /c ... exit 0` returned **0 (hidden failure)**.
The same failed command alone returned **3 (failure propagated)**, proving the
one-command-per-step fix. PSNativeCommandUseErrorActionPreference was False.

## Verification limits and handoff

Scoped standalone `rustfmt --check --edition 2024` passed on both changed Rust
files. No Cargo build, test, check, clippy, coverage, mutation or production
executable invocation was run. The new Rust regression and existing mutator
child-environment test are authored but UNRUN locally; native probe red/green is
not a Rust suite run. CI must exercise both after integration by the parent.

No Python path tests, release.test.mjs, generated files, dependencies, global
settings, tracker, push or integration were changed. Commits use
`git -c core.hooksPath=NUL` as requested.

Changed Rust source for maintainer quality refresh:
`crates/quality-core/src/process_windows.rs`.
Changed Rust test: `crates/crapper/tests/process_tree.rs`.
