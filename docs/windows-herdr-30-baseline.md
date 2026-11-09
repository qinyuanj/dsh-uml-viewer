# Windows Herdr acceptance baseline — issue #30

> **历史记录**：本文描述的 Herdr companion、邮件与 `./uml` / `get-uml-viewer` 入口已在 qinyuanj/dsh-uml-viewer#18 删除，只在冻结的 qinyuanj/uml-viewer 中保留；本文不再是本仓库的现行契约或验收清单。

2026-10-13, checkout `7de5051bf5dcafe8c3389a253d02f244b245eaa7` on `lua`.
This baseline records automated checks and read-only environment inspection.
It does not complete any of the seven native criteria in
[#30](https://github.com/qinyuanj/uml-viewer/issues/30). This is the baseline
before the separate [native observations](windows-herdr-30-observations.md); later
native results do not change the automated results recorded here. The
automated figures below are pre-fix; the post-fix automated re-run is in the
[close-fix section](windows-herdr-30-observations.md#production-fix-for-the-close-failure).

## Environment

| Item | Observed value |
| --- | --- |
| Platform | Native Windows 11 Pro 10.0.26220 |
| PowerShell | 7.6.6 |
| Herdr | 0.9.3; bundled schema protocol 22 |
| Claude Code | 2.1.296 |
| JDK | Temurin 21.0.12.1 |
| Current source commit | `7de5051bf5dcafe8c3389a253d02f244b245eaa7` |
| Root public entry's installed viewer | `9604b65a9a90b91b50f73f7b11fa60adabc55d00` |

The installed viewer differs from the tested source checkout. A native run
must record its fixture's installed commit rather than assume the root
public entry runs current source.

## Automated checks

| Command | Result |
| --- | --- |
| `clj -M:spec` | Exit 0; 530 examples, 2311 assertions, 0 failures, 1 pending Unix-shell integration example |
| `python -I scripts/test_restart_entries.py` | Exit 0; 6 tests, 87.801 seconds, OK |
| `python -I scripts/test_entries.py` | Exit 0; 17 tests, 45.783 seconds, OK with 10 skips |
| `pwsh -NoProfile -File scripts/test_windows_entries.ps1` | Exit 1; fixture setup is incomplete, described below |

The restart entry suite uses real JVM/process fixtures with controlled
Herdr boundaries. It does not establish real Herdr end-to-end acceptance.
The entry suite's skipped cases are not passes.

The native entry test fails at its first alias result assertion:
`Expected: 23 Actual: 1`. The wrapper reports that the fixture's installed
`scripts/uml-quality-config.ps1` cannot be resolved. The test setup at
`scripts/test_windows_entries.ps1:57` copies only `uml.ps1` and
`uml-command.ps1`; the latter requires the omitted helper. No production
runtime defect is established by this failure. This check remains failed;
it was not rerun after a fixture modification.

An initial invocation of the PowerShell test through Python failed with a
syntax error before executing tests. The PowerShell invocation above is the
actual check result.

Local command logs are retained under
`C:\Users\Lzx_8\AppData\Local\Temp\claude\C--Users-Lzx-8-Desktop-dev-github-uml-viewer\d055227c-6807-4e79-82f1-d433cb185c87\tasks\`:
`bxjmx33vo.output` (spec), `buxtujja2.output` (restart entries),
`bqe97hd3d.output` (entries), and `b8brd8c3p.output` (native entry failure).

## Root ownership inspection

The root `.uml-viewer/companion.edn` was `{}`. The separate
`.uml-viewer/viewer-jvm.json` recorded PID 26540 with start
`2026-10-12T08:12:04.004Z`; no matching live process existed.
The existing root Viewer was therefore neither started nor restarted.
Herdr inventory contained the default session and two running named
sessions whose ownership was not established by this checkout. They were
left untouched during this inspection; their existence is not evidence of
an owned companion.

The historical #21 evidence store remains at `%TEMP%\uml-acc21`.
Inspection did not resume or clean its fixtures or historical mutation
resources. No historical evidence was relabeled as a result of this run.

## Native criteria

The baseline leaves all seven criteria open: named-proposal restore,
detail-window revalidation, retry-budget exhaustion, normal exit without
recovery, close during pending backoff, manual terminal attach, and
focus/open-layer plus separate metrics-overlay reload after restart.
Native outcomes must be recorded separately with current ownership,
process identities, event order, mailbox/view state and actual visual
observations. macOS acceptance remains cancelled and unverified.
