# Windows Herdr #17 implementation checkpoint

> **历史记录**：本文描述的 Herdr companion、邮件与 `./uml` / `get-uml-viewer` 入口已在 qinyuanj/dsh-uml-viewer#18 删除，只在冻结的 qinyuanj/uml-viewer 中保留；本文不再是本仓库的现行契约或验收清单。

2026-10-11; Windows 11, Herdr 0.9.3 / protocol 22. Implementation branch: `work/windows-herdr`. Windows fresh/display/close has automated evidence below and user confirmation of normal visible operation. Full platform acceptance remains tracked separately in #21 and #22.

## Verified

- Full specs: 547 examples, 0 failures, 2251 assertions, 1 pending.
- Differential mutation: herdr 116/116 killed, companion 43/43, sketch 43/43, core 3/3. Coverage gaps: 13, 1, 9, 0 respectively. First three commands exited 3 for coverage gaps; snapshots retained. The initial foreground Herdr mutation run was interrupted with exit 137; the subsequent background run completed. No full mutation was forced.
- CRAP completed earlier in this implementation session. `clj -M:ir` regenerated `examples/uml-viewer.edn` after the mutation snapshots.
- Native A/B fixture previously passed complex argv/cwd, runner exit 37, and isolated cleanup checks.
- A serial real-Claude acceptance run completed with exit 0: Claude wrote display mail, Viewer consumed it, then Claude consumed context mail. Fixture: `target/herdr-claude-9d5e9cac-e873-40eb-9092-c4d19eed229a`. `to-agent.edn` ended with `{:next-id 2 :queue []}`; ownership ended as `{}`. The recorded session endpoint was independently confirmed not running.
- `git diff --check` passed.

## Limits and remaining acceptance

- The captured `viewer.png` is occluded by the terminal and is not visual proof. After the automated run, the user reported “我看已经都正常了”, confirming normal visible operation on Windows. This does not imply a separate keyboard/detach or macOS acceptance run.
- Earlier startup acknowledgement timeouts occurred while mutation and native acceptance were running concurrently. Runner state and display mail subsequently existed. The serial run passed; resource contention is a plausible contributor, not a proven exclusive cause. Startup uses a bounded 30-second acknowledgement and retains ownership on timeout.
- Windows full `session attach` is used; direct `terminal attach` remains unsupported. Human keyboard/detach acceptance remains open.
- macOS native acceptance and Viewer JVM restart continuity were not performed in this checkpoint.
- Automatic recovery remains unsupported; it belongs to #19.
- Other named Herdr sessions observed at the end were not established as this fixture's resources and were left untouched.

## Reproduction

From PowerShell, the helper changes to the worktree root and preserves clj's exit status:

```powershell
& ./scripts/run-check.ps1 '-M:spec'
& ./scripts/run-check.ps1 '-Sdeps' '{:deps {quil/quil {:mvn/version "4.3.1563"}}}' '-M' 'scripts/herdr-claude-check.clj'
& ./scripts/run-check.ps1 '-M:ir'
```

Run native acceptance separately from mutation/coverage. The real-Claude fixture launches a billable Claude session and a visible Viewer, uses its own target directory, and cleans up its recorded companion.
