# spec203: Windows captured output deadlock

CI2 job 129055477281 ran out its 30-minute timeout. In the preserved
`D:/dev/github/spec203-notes/CI2-windows-crap.log`, lines 212–220 show four
`process_tree` tests passing, then `cmd_slash_c_switch_reaches_cmd_unquoted`
running beyond 60 seconds until the job was canceled.

## Root cause and fix

`quality-core/src/process_windows.rs::execute` waited for the child to exit
before starting either captured-pipe reader. `CreatePipe` uses the default
bounded buffer. A large CI environment makes `cmd /c set` fill stdout and
block in a write, while its parent waits for exit. Small local environments
and the four passing tests did not exceed that buffer.

Start BOTH stdout and stderr reader threads before `WaitForSingleObject`.
Retain the existing exit-code query, bounded job termination and kill-on-close
fallback. Drop the job, inherited writer handles and attributes before joining
the readers, so descendants and parent-owned write handles cannot hold EOF
open. Error returns retain the existing RAII process/job cleanup and error
propagation; a reader join is never inserted before cleanup.

The new `windows_capture_drains_large_stdout_and_stderr_before_waiting` test
uses only cmd builtins to emit exactly 131,072 bytes on each stream. It asserts
both complete byte sequences and success through the public `process::output`
boundary. A shared watchdog re-executes only the selected test in the existing
test executable; after 15 seconds it kills and reaps that helper. Closing the
helper's job ends its blocked command too. The original `cmd /c set` invocation,
exit-code assertion and PATH assertion are preserved inside the same watchdog,
so that sibling cannot hold the whole suite open on a regression. The test
fixture generates no executable and does not shrink or clear the environment.

## Installed-runtime evidence (not Rust test execution)

Ran a native Windows PowerShell 7.6.9 / .NET `System.Diagnostics.Process` probe
with both streams redirected. Its raw cmd arguments match the shell path:

```text
/d /v:off /s /c "for /l %i in (1,1,8192) do @(echo stdout-payload& 1>&2 echo stderr-payload)"
```

The three variants differed only in whether `ReadToEndAsync` started for
neither stream, stdout alone, or both streams before `WaitForExit(3000)`.
Timed-out children were killed and reaped before reading the remaining bytes.
The output contained only lengths/statuses, never environment values.

| Reader starts before wait | Exit within 3s | stdout chars | stderr chars | Exact full streams | Elapsed ms |
| --- | --- | --- | --- | --- | --- |
| Neither (red) | no; killed | 4,096 | 4,096 | no | 3,091 |
| stdout only (red) | no; killed | 4,112 | 4,096 | no | 3,042 |
| Both (green) | yes; exit 0 | 131,072 | 131,072 | yes, both | 822 |

This proves the underlying Windows pipe behavior and the cmd payload. It does
NOT execute the Rust implementation or its watchdog. Rust red/green remains
UNRUN under the user's explicit no-build/no-test resource restriction.

## Sibling sweep

- `process_windows.rs::spawn` has the sole production Windows capture-pipe
  creation path. Its `Streams::Capture` consumer is `execute`, fixed here.
- `start` / `supervise` use `Streams::File`; both streams target a file rather
  than a bounded capture pipe, so waiting does not introduce this deadlock.
- `Streams::Inherit` does not create capture pipes.
- Unix `process.rs::execute` already starts BOTH readers before waiting and
  joins after group cleanup. No change needed.
- Remaining Win32 waits are process/job cleanup waits, not undrained capture
  consumers. Rust `Command::output` / `wait_with_output` sites use the standard
  library's capture implementation. Host TypeScript delegates capture to the
  Harness subprocess service; it does not own native pipe readers.

## Validation and handoff

- PASS: installed pinned formatter, no Cargo invocation:
  `rustup run 1.99.1 rustfmt --edition 2024 --check crates/quality-core/src/process_windows.rs crates/crapper/tests/process_tree.rs`.
- PASS: `git diff --check`.
- PASS: native runtime probe above; no files, executables or profile changes.
- UNRUN: all local Cargo builds, checks, tests, clippy, quality/mutation calls.
  No new executable was generated. This worktree's `target` remains absent.
- Merged latest local `spec-203` at
  `abdcab57f663c427e10b0e348e58f9693081bfc5` before the fix commit.
- Lead's production Rust check scope: `cargo check -p quality-core --lib --locked -j 2`,
  only under the shared atomic Cargo lock and resource-policy environment.
  This library check does NOT compile the integration regression test.
- CI must run `cargo test -p crapper --test process_tree --locked`, including
  both the large-output regression and unchanged cmd environment assertions.
  No local Rust test pass is claimed.

Changed source files for the maintainer's later quality refresh:

- `crates/quality-core/src/process_windows.rs`
- `crates/crapper/tests/process_tree.rs` (integration regression; outside business scope)
