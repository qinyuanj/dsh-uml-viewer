# Windows Herdr native observations — issue #30

> **历史记录**：本文描述的 Herdr companion、邮件与 `./uml` / `get-uml-viewer` 入口已在 qinyuanj/dsh-uml-viewer#18 删除，只在冻结的 qinyuanj/uml-viewer 中保留；本文不再是本仓库的现行契约或验收清单。

2026-10-13. This record supplements the
[automated baseline](windows-herdr-30-baseline.md). It does not close
[#30](https://github.com/qinyuanj/uml-viewer/issues/30). It covers two native
runs: the first, which reached the criteria below and failed at GUI close after
retry exhaustion, and a
[second](#native-re-observation-after-the-fix-second-run) on the fixed source
that repeated them.

## Fixture and evidence (first run)

A new exclusive fixture was installed through the public installer at
`C:\Users\Lzx_8\AppData\Local\Temp\uml-acc30-20261013-01\project A`.
The native worker reports its installed checkout is pinned to
`7de5051bf5dcafe8c3389a253d02f244b245eaa7`, using real Claude Code 2.1.296
and Herdr 0.9.3/protocol 22. Root runtime and pre-existing sessions were not
used. Environment versions are recorded in the baseline.

The worker used the public wrapper for fresh launch and both restarts,
with the existing `scripts/acc21` observer. Its input events are dispatched
inside the actual AWT Viewer; screenshots capture real desktop pixels.
This does not establish manual terminal input or detach acceptance.

Evidence root: `%TEMP%\uml-acc30-20261013-01\evidence`.
Observed root fixture ownership recorded:

- Session `uml-viewer-c15b5819-185a7a78-2163-40da-93a0-f1f366682b26`;
  workspace `w1`, pane `w1:p1`, terminal `term_65db5bf1a384f1`.
- Server PID 38620, start ticks `639274818365469368`;
  shell PID 16328, start ticks `639274818368761000`.
- Controller PID 44400, start `2026-10-13T09:50:43.634Z`.
- Initial Claude PID 28944; initial Viewer JVM PID 1424.
  These initial PIDs are worker-reported; ownership/journal corroborate
  the server, shell and controller identities above.

## Criteria and limits

| Criterion | Current evidence status |
| --- | --- |
| Named-proposal restart restoration | PARTIAL; run 1 restored main-frame rendering and post-restart state independently inspected. Run 2 restored the diagram and Inspector across `--restart`, but did not repeat the named-proposal before/after comparison. Full before/after comparison and detail-content visual verification remain incomplete. |
| Detail-window fix native revalidation | Showing Alpha frame independently confirmed in the restored snapshot. PARTIAL; detail content pixels were not separately inspected in either run. |
| Retry-budget exhaustion (A-L2) | VERIFIED natively in run 2 (S4): three retries, backoffs 1.06/2.05/4.03 seconds, terminal `:failed`, no fourth child. Run 1 confirmed only the terminal outcome. |
| Normal exit without recovery (A-L3) | VERIFIED natively in run 2 (S3), with a mail-wake limitation stated below. |
| Close during pending backoff (A-L4) | VERIFIED natively in run 2 (S2b). |
| Close after exhaustion (the failure observed in run 1) | FAILED natively in run 1, recorded below and left standing as that run's result. Fixed in source and re-observed PASS natively in run 2 (S4), subject to the S4 nuance stated there. |
| Manual terminal attach | NOT VERIFIED; no human observation recorded in either run. |
| Focus/open-layer restoration and separate metrics reload | PARTIAL; run 1 recorded the restored proposal open layer. Run 2 verified the CRAP-overlay reload without restart (S5), with `:open-layer nil` in its snapshots. Mutation-overlay reload NOT VERIFIED. |

The worker reports initial display consumed at mail cursor 1, namespace
focus `[:nested]`, zoom 1.1, declutter `:arrows`, selection/detail
`:nested.alpha`, and an actual Alpha window. Quit-for-restart mail id 2
preceded old JVM exit and public-wrapper restart; controller/initial
Claude identities remained unchanged.

For the second restart it reports proposal `:p-native30`, open layer
`:proposal.nested`, zoom 1.21, camera y 38.0165, selected/detail
`:nested.alpha`, declutter `:arrows`, and proposal discussion context
restored. Independent inspection of `07-proposal-restored.png` confirms
the visible named proposal, nested open layer, selected Alpha and declutter
mode. The image captures only the main Viewer frame and does not prove
the separate detail window. Independent inspection of fixture
`.uml-viewer/observer-result-58ddd1620c3d.edn` confirms proposal
`:p-native30`, open layer `:proposal.nested`, zoom 1.2100000000000002,
camera `(0, 38.016528925619866)`, selection/detail `:nested.alpha`,
`:arrows`, discussion context and mail cursor 3. It also records frame
names `["UML viewer" "Alpha"]` and `:detail-window? true`. This verifies
frame presence, while detail content pixels remain unverified. Namespace
restoration evidence is in `observer-result-8eeff1be9465.edn`.

Public-wrapper records are `launch-fresh-174928.json`,
`launch-restart-175321.json` and `launch-restart-175603.json`; the worker
reports each exit 0. Filenames use local time; snapshot instants use UTC.

Recovery evidence includes `recovery1`, `recovery2`, `recovery3` and
`recovery4-exhaustion` directories with identity validation and timelines.
The worker reports revalidating each Claude PID/start before abnormal-exit
injection and observing backoffs of approximately 1.06/2.10/3.99 seconds.
Independent inspection of `recovery4-exhaustion/timeline.json` confirms
attempts 3/generation 3, Claude PID 43760 with start
`2026-10-13T09:58:42.160Z`, and transition to `:failed` at 236 ms after
that injection with reason `Recovery exhausted after three retries`.
The same controller identity remains in the journal. The worker reports
no fourth child during the following 12-second observation.

## Close failure and resource status

After exhaustion, the worker dispatched normal GUI close. The fixture log
independently confirms (first run, unfixed source):

```text
UML viewer: companion cleanup: Recovery cancellation was not acknowledged; resources retained
```

The cancellation file contains the controller's matching token; the
journal inspected afterward remained `:failed`, and complete companion
ownership was retained. This is a native cleanup failure. It does not
establish the separate pending-backoff cancellation criterion.

The final worker report establishes successful verified manual teardown,
after the GUI cleanup failure:

1. Durable cancellation was already present. Controller 44400 subsequently
   exited, confirmed by `Get-Process` and `jcmd`.
2. Before cleanup, server 38620 and shell 16328 creation ticks exactly
   matched ownership; the recorded socket, pane and protocol were verified.
3. Closing owned workspace `w1` returned `ok`; stopping and deleting only
   the owned session succeeded.
4. Final PID checks and the worker's fixture process scan were empty.

Parent read-only checks independently confirm those three PIDs are absent
and the fixture session is absent from the current Herdr inventory. The
same default and two foreign sessions remain running. The ownership file
and failed journal remain unchanged as historical evidence; they do not
represent live resources. Manual teardown succeeded, while automatic GUI
cleanup remains FAILED. The worker's teardown command outputs were
returned in its transcript rather than saved as separate evidence files.

## Production fix for the close failure

Root cause: the detached controller's `-main` "controller" branch
(`src/uml_viewer/adapters/recovery.clj:252`) left its loop at a terminal
phase without ending the JVM. The runner's stream readers are non-daemon
agent threads (`src/uml_viewer/adapters/companion.clj:17-18`), so the
controller stayed alive for the agent pool's 60-second keep-alive, longer
than the ten seconds `cancel!` grants before it reports
`Recovery cancellation was not acknowledged; resources retained`. A
measured control reproduced the linger at 61.25 seconds through the real
preflight boundary, and the same JVM with an explicit exit ended
immediately.

The fix ends the process once the lifecycle is terminal: `(System/exit 0)`
after the controller loop (`recovery.clj:271-277`). This is what the
contract's close clause already requires — the controller must confirm
cancellation and exit before the workspace/server are destroyed
(`docs/herdr-backend-contract.md` §6).

Evidence is automated and is recorded separately from native observation:

| Check | Result |
| --- | --- |
| New real-process regression (`spec/uml_viewer/adapters/recovery_spec.clj`, "detached controller process") | Red on the unfixed source: the production `cancel!` threw at `recovery.clj:197` after its ten-second window. Green after the fix. |
| `clj -M:spec` | Exit 0; 531 examples, 2319 assertions, 0 failures, 1 pending Unix-shell example |
| `python -I scripts/test_restart_entries.py` | Exit 0; 6 tests, 85.261 seconds, OK |
| `python -I scripts/test_entries.py` | Exit 0; OK with 10 skips |
| `clj -M:crap` | Exit 0 |
| `clj -M:mutate src/uml_viewer/adapters/recovery.clj` | Exit 3; four uncovered mutants on `-main`; snapshot retained and not re-run |
| `clj -M:ir` | Exit 0; `examples/uml-viewer.edn` regenerated byte-identical |

The regression launches the real controller argv from `recovery/java-command`
and calls the production `cancel!`; only the external Herdr boundary
(`herdr/relaunch!`) is replaced, so it is not itself a native observation
and closes none of the criteria above. The four uncovered mutants sit on
`-main` lines that the new example executes in a separately launched JVM,
which the spec process's coverage profile cannot see.

## Native re-observation after the fix (second run)

A second exclusive fixture was installed through the same public installer at
`%TEMP%\uml-acc30-20261013-02\project A`, pinned to `28d187f`, which contains
the fix from `22c409b`; the installed `recovery.clj:277` carries the literal
`(System/exit 0)))))`. Real Claude Code 2.1.296 and Herdr 0.9.3/protocol 22 were
used again. Evidence root: `%TEMP%\uml-acc30-20261013-02\evidence`. The worker
reports the fixture policy's sha256 as
`8A37E803207CCB96B7360B0607C761CB3C5DC86AFA2FF4BBA41C0C509B4BE991` with named
proposal `p-native30`, never regenerated or re-policied during the run.

| Scenario | Result |
| --- | --- |
| S1 fresh launch | PASS; wrapper exit 0, real Claude child in the fixture-owned pane, Viewer and diagram on screen. |
| S4 close after exhaustion | PASS; see below. |
| S2b close during pending backoff (A-L4) | PASS. |
| S3 normal exit (A-L3) | PASS; see the limitation below. |
| S5 metrics overlay, restart and live-controller close | PASS for the CRAP overlay; mutation-overlay reload NOT VERIFIED. |

All six public-wrapper launches returned exit 0
(`launch-fresh-183643/184746/185124/185452/185910.json`,
`launch-restart-190207.json`).

**S4 — retry exhaustion and close (A-L2).** The worker revalidated the child
PID/start before each abnormal-exit injection and measured backoffs of 1.06,
2.05 and 4.03 seconds. After the third retry the journal
(`s4-postclose-recovery.edn`) is `:phase :failed`, generation 3, attempts 3,
reason `Recovery exhausted after three retries`, with no fourth child over the
following 14 seconds. The GUI close then produced no failure line
(`s4-viewer-log.txt`), the control file carried the journal's own cancel token,
and `s4-postclose-companion.edn` is `{}`. *Nuance:* because the fixed controller
ends itself at the terminal `:failed` phase, S4's ten-second cancellation window
was trivially satisfied; S4 does not exercise closing over a live controller.
S5's close does.

**S2b — close during pending backoff (A-L4).** `s2-timeline.json` records the
child validated at 73 ms, killed at 84 ms, the `:backoff` phase observed at
161 ms, the close dispatched at 176 ms, the journal reaching `:backoff` at
249 ms, the observer acknowledging the close at 270 ms and the journal reaching
`:cancelled` at 362 ms. The journal retains the pending `:deadline
159631362` — the close landed inside the backoff, not after it. A 45-second
watch ended with `new_child_pids: []`, so nothing revived. The control token
matches the journal token; `s2b-postclose-recovery.edn` is `:phase :cancelled`,
reason `Viewer closed; recovery revoked`; `s2b-postclose-companion.edn` is `{}`.

**S3 — normal exit (A-L3).** The companion was the real Claude TUI in the
fixture's own pane, `w1:p1` of session `uml-viewer-b4065bc0-c5aedaa5-…`. Normal
exit was produced by typing `/exit` into that pane through the documented Herdr
CLI (`pane send-text w1:p1 '/exit'`, then `pane send-keys w1:p1 Enter`); the
pane dump taken immediately before (`s3-pane-before-exit.txt`) shows an idle real
Claude Code TUI in the fixture directory with bypass permissions on. The journal
(`s3-postexhaust-normal.edn`) is `:phase :normal`, `:exit 0`, reason `Companion
exited normally`, generation 0, attempts 0. No retry and no new child followed
over a 20-second watch, the agent list was empty and the queues were unchanged.
*Limitation:* mail retention was observed only as an unchanged, undrained queue
(`s3-mail-retained.edn`). A raw `to-agent.edn` write does not trigger a wake —
wakes come from the viewer's public API/GUI — so the mail-survives-a-failed-wake
path was not exercised.

**S5 — metrics overlay, restart, live-controller close.** After a fresh launch,
`./uml crap` published a version-1 record with `validResult: true`, operation
`crap`, exit 0 and the fixture's canonical project root, in 6.77 seconds. The
pre-restart `:metrics-stamp` and the post-restart one are identical
(`[".metrics\crap.edn" 1791889135520 267]`). The restart went through the public
wrapper (exit 0; new viewer JVM 26032) and the companion was remembered rather
than re-created: controller 24764 and child 33416 were kept, journal
`:running`, generation 0. Diagram and Inspector are present in
`s5-post-restart.png`. Without any restart or `R`, re-running crap moved the
stamp to `1791889760460`; giving `demo.nested.beta/add` an `if` (complexity
1 → 2, CRAP 2.0 → 6.0, `s5-crap-content-change.txt`) changed the on-screen card
of the same selected Inspector entry to `Crap μ 6.0 max 6.0 σ 0.0`
(`s5-nested-open.png`, stamp `1791889816238`); reverting the source and
re-running crap (stamp `1791889865985`) changed it back to `Crap μ 2.0 max
2.0` (`s5-after-revert.png`). The fixture source was restored to its original
content (`s5-beta-before.clj`).

*Not verified:* a mutation-overlay reload. `./uml mutate src/demo/core.clj`
exits 1 and publishes `validResult: false`, with the tool's own message `The
test and coverage commands do not identify the same test roots. Use matching
project aliases or --test-roots.`; passing three targets answers `Unexpected
extra argument`. The fixture declares no test roots and supplying them is new
machinery, so the worker stopped there.

**Teardown (close over a live controller).** The close was acknowledged in
0.33 seconds; viewer 26032, controller 24764 and child 33416 were all gone
within 6.48 seconds. `recovery-control.edn` carries the journal's cancel token,
the final journal is `:cancelled` / `Viewer closed; recovery revoked`,
`companion.edn` is `{}`, `recovery.log` is empty and the viewer's stdout/stderr
contain no error. Fixture Herdr resources were destroyed workspace → server →
session; the fixture session is gone from `herdr session list` and its session
directory is gone. Independent inspection of
`inventory-s5-preclose.json` against `inventory-s5-postclose.json` shows every
fixture-owned process present before and absent after: Herdr session server
18920, session attach 11724, runner JVM 39108, controller 24764, viewer 26032
and launcher pwsh 9228. Fixture ids recorded before the close: workspace `w1`,
tab `w1:t1`, pane `w1:p1`, terminal `term_65db6b85b0bde1`, agent session
`99828d71-…`, cwd = fixture. One leftover file remains, the fixture's stale
`.uml-viewer\viewer-jvm.json`, which records no live process and which the
restart path identity-validates.

**Foreign resources and the root repo.** Herdr sessions `default`,
`uml-viewer-414d1586-…` and `uml-viewer-96aa7107-…` were still running at the
end; no foreign process was signalled. The root repo was clean at `28d187f`
with `.uml-viewer/companion.edn` `{}`, and no policy, generated, or global
configuration file was edited.

### Harness caveats carried with this run

1. S2's first attempt was aborted by a bug in the worker's own driver: its regex
   captured the leading colon, so the `:backoff` phase never matched, no close
   was dispatched and the controller simply retried. Its retained files
   (`s2-attempt1-harness-abort-timeline.json`, `s2-attempt1-journal.edn`) are not
   a product result; the scenario was closed cleanly and re-run fresh.
2. One `./uml crap` run launched through `Start-Process` with redirected stdout
   exited `-1073741510` (0xC000013A) after 217 seconds and published no quality
   record (`s5-crap-reload.out.txt`); the same command in the foreground
   succeeded in 6.77 seconds (`s5-crap-live.txt`). A `Start-Process`-with-redirect
   launch of the quality dispatch is not reliable in this harness. Every
   successful run prints a `--lcov` `FileNotFoundException` before retrying
   without `--lcov`.
3. Native Windows has no SIGTERM equivalent. The controlled termination used was
   `Stop-Process` (TerminateProcess), applied only to fixture `claude.exe`
   processes after validating PID, image name and start time against the
   journal's `:child`.
4. Some start ticks and cancel tokens are redacted as placeholders in the
   worker's transcript; the evidence files carry the real values.
5. Unrelated to #30: after `--restart` the viewer log writes `Watching nil` for
   that session even though the restored document path and diagram are correct.

## Native re-observation of the detail-window fix (third run)

2026-10-13. The same exclusive fixture was reused:
`C:\Users\Lzx_8\AppData\Local\Temp\uml-acc30-7f18eb83\project A`, evidence root
`…\evidence`, run record `.scratch/acc30-run.json`. Versions are unchanged from
the second run (real Claude Code 2.1.296, Herdr 0.9.3/protocol 22, native
Windows, real AWT). Fixture identities in this run: Herdr session
`uml-viewer-ba96306b-320376b5-2e5f-4f4f-aae1-7e929e446cb9`, workspace `w1`,
pane `w1:p1`, terminal `term_65db7c35ac0221`, server PID 42076, shell PID
40992, controller PID 2496, Claude child 45484, and viewer JVMs 73756, 70408,
64736, 70920, 59216 across the restarts below.

**What the installed checkout ran.** The fixture's installed checkout is pinned
to `d1e1507` with no remote. This run copied one locally modified, uncommitted
source file into it — `src/uml_viewer/adapters/sketch.clj`, carrying the
detail-window fix — and loaded the repository's modified acceptance observer
through the `scripts/acc21` `PATH` shim. This is therefore a verification of the
working tree, not of a released build or of `d1e1507` itself.

### Root cause, measured natively

`probe` on the live viewer reports `:applet class quil.Applet`,
`:surface class processing.awt.PSurfaceAWT` and
`:native class processing.awt.PSurfaceAWT$SmoothCanvas`. The native handle is a
`Canvas`, not a `Frame` or `Window`, so the previous
`(instance? Frame native)` title path and `(instance? java.awt.Window native)`
close path were both no-ops on Windows. The defect was visible before the fix:
`s7-post-crap-card.png` shows the Alpha title bar above Beta content, and
`s8-4-stale-title-alpha-content.png` records the same staleness.

### Detail card: title, content and close

`s8-fixed/` holds the post-fix observations, driven by real AWT input through
the `scripts/acc21` observer.

- `s8-1-alpha-probe.edn` / `s8-2-beta-probe.edn` record both frames
  (`["UML viewer" "Alpha"]`, then `["UML viewer" "Beta"]`), each hosting a
  `SmoothCanvas`, with `:detail-id` tracking the selection.
- `s8-1-alpha-card.png`: title bar `Alpha`, content `demo.nested.alpha`,
  `Crap μ 1.0 max 1.0 σ 0.0`, `+ scale`.
- `s8-2-beta-card.png`: title bar `Beta`, content `demo.nested.beta`,
  `Crap μ 2.8 max 4.5 σ 1.8`, `+ pick`, `+ add`. The title now follows the
  selection instead of staying on the previous class.
- `s8-4-alpha-card.png` and `s8-5-alpha-card.png`: returning to Alpha restores
  the Alpha title and content.
- `s8-native-esc-probe.edn`: after a native ESC to the card window,
  `:detail-id nil`, `:applet nil`, and the card frame reports
  `:showing false`. The card window is disposed, which the pre-fix no-op path
  could not do.

The harness sends keyboard input with `java.awt.Robot` rather than
`KeyEvent` dispatch to the canvas: a synthesized event never reached the
frame's key handler (`s8-3-esc-to-card.edn` recorded an empty result while the
dispatch target resolved). This is a harness change, not a product change.

### Named-proposal and namespace-focus restoration with non-zero pan

`.scratch/acc30-views.ps1` drives a real view, screenshots it, restarts through
the public wrapper, and screenshots the restored view. Both contexts were run
with non-zero camera coordinates, which the second run's comparisons lacked.

First proposal-context attempt was discarded: `proposal-attempt1-zero-camera.edn`
keeps `cam-x 0`/`cam-y 0` because the wheel steps did not move this deliberately
small diagram. The scenario was re-driven with a smaller zoom step and
`Shift`-scroll, and both contexts produced `cam-x 55.454545454545496`,
`cam-y 41.81818181818187`.

| Field | Proposal context pre → post | Namespace context pre → post |
| --- | --- | --- |
| `cam-x`, `cam-y` | 55.454545454545496 / 41.81818181818187 → same | 55.454545454545496 / 41.81818181818187 → same |
| `zoom` | 1.1 → 1.1 | 1.1 → 1.1 |
| `declutter` | `:classes` → `:classes` | `:full` → `:full` |
| `proposal-id` / `proposal` | `:p-native30` / true → same | `nil` / false → same |
| `open-layer` | `:proposal.nested` → `:proposal.nested` | `nil` → `nil` |
| `focus` | `[]` → `[]` | `[:nested]` → `[:nested]` |
| `selected` / `detail-id` | class `:nested.alpha` → same | class `:nested.alpha` → same |
| `discussion-context` | proposal `:p-native30` → same | `{:context :real}` → same |

Snapshots are `proposal-pre.edn`, `proposal-post.edn`, `namespace-pre.edn` and
`namespace-post.edn`; the wrapper restarts are `proposal-restart.log` and
`namespace-restart.log` (both exit 0, new JVM after the old one exited).

**Pixels.** The Alpha detail card is byte-identical before and after the
restart in both contexts: `proposal-pre-alpha.png` = `proposal-post-alpha.png`
and `namespace-pre-alpha.png` = `namespace-post-alpha.png`, SHA-256 prefix
`AF7E6DBF84BF8A8D`. This is the detail-content pixel check the earlier runs left
open.

The main frame differs only where the separate card window overlaps it.
`acc30-region-diff.ps1` reports 0 differing pixels for the diagram band
(`x 0..410, y 55..200`), 0 for the top band (`x 0..1480, y 55..100`), and 700
of 238506 for the inspector body, which the diff map
`namespace-inspector-diffmap.png` localises to a thin strip at the window's
right edge. `namespace-main-diffmap.png` shows the rest of the frame changing
only where the card window sits in one shot and not the other. So the diagram,
proposal frame, badges, titles, inspector text and both card contents are
identical; the residual difference is window z-order and decoration, not
restored state.

### Metrics overlay reload

Sequenced under the pre-fix viewer, so the numbers are fixture-local:

- A `:refresh-crap` request published `s7-crap-refresh-result.json`
  (`version 1, operation "crap", crapScope "targets", exitCode 0,
  validResult true`) and the following IR re-rendered the card without a
  restart (`s7-post-crap-card.png`, class metrics unchanged from
  `s7-card-title-anomaly-snapshot.edn`).
- A `:refresh-mutate` request published `s7-mutate-result.json`
  (`version 1, operation "mutate", exitCode 0, validResult true`), the next IR
  re-rendered the card, and the mutation figures changed in place: Beta moved
  from `:crap {:mu 2.8 …}` to `{:mu 1.0 …}` and both classes then carried
  `killed 2 / survived 0 / uncovered 0`
  (`s7-post-mutate-snapshot.edn`, `s7-post-mutate-alpha.png`). No restart and no
  `R` were used between the request and the re-rendered card.

### Source and quality checks in this change

The fix resolves title, close and front operations through the window hosting
the canvas. Reviewing the spec file also exposed an unbalanced parenthesis that
closed `describe "sketch"` early, so eleven existing lifecycle examples after it
were collected but never executed; that parenthesis is repaired and those
examples now run. Their reactivation is part of this change's evidence, not a
claim about the earlier runs.

- `clj -M:spec spec/uml_viewer/adapters/sketch_spec.clj`: 60 examples,
  0 failures, 251 assertions, 1 pending.
- `clj -M:spec`: 545 examples, 0 failures, 2360 assertions, 1 pending
  (the second run recorded 532 examples and 2322 assertions).
- `clj -M:crap`: exit 0.
- `clj -M:mutate src/uml_viewer/adapters/sketch.clj` (differential): exit 3 with
  7 of 7 differential mutants killed, 0 uncovered, retained without a re-run.
- `clj -M:ir`: exit 0.

### Fixture teardown

The viewer was closed through its own GUI close path
(`observer-result-ab56e5404372.edn`, `:close-gui true`).
`inventory-s6-preclose.json` (67 processes) and `inventory-s6-postclose.json`
(51 processes) bracket it. After the close the fixture's Herdr session has
disappeared from `herdr session list`, its
`%APPDATA%\herdr\sessions\uml-viewer-ba96306b-…` directory is gone,
`companion.edn` is `{}`, `recovery.edn` is `:phase :cancelled` with reason
`Viewer closed; recovery revoked`, and no fixture JVM, launcher, controller,
Herdr server or Claude child remains. One stale file stays behind, the
fixture's `viewer-jvm.json`, which records a dead PID and which the restart path
identity-validates.

Foreign state was not signalled. The shared `default` Herdr session and
`uml-viewer-96aa7107-443bccbd-2df5-42dd-8745-d7b3b2656326` were running when
the fixture closed; `uml-viewer-414d1586-ca726b1b-c259-4b8e-8e29-87d113e135d7`
reported `stopped` with a session directory created at 14:02 UTC, after this
run's close, so that state comes from its own project and not from this
teardown. Manual terminal attach remains NOT VERIFIED.

## Status

Automatic GUI cleanup is fixed in source and re-observed natively in the second
run; the run-1 FAILED recorded above stands as that run's result and was not
reproduced on the fixed source. A-L2, A-L3 and A-L4 are verified natively.
Named-proposal restoration, the detail-window fix and the metrics overlay remain
PARTIAL as the table states, manual terminal attach remains NOT VERIFIED, the
mutation-overlay reload is NOT VERIFIED, and macOS remains cancelled and
unverified. Historical mutation fixture resources were not accepted or cleaned
by this record.

Those statuses describe the first two runs. The third run, recorded above,
supersedes three of them. Named-proposal restoration now has a full
before/after comparison with non-zero pan, a namespace-focus comparison, and
byte-identical detail-card pixels. The detail-window fix is re-observed
natively with the correct title, the correct content and a disposed card
window. The mutation-overlay reload is verified. Manual terminal attach stays
NOT VERIFIED, the two earlier runs keep their recorded results unchanged, and
macOS remains cancelled and unverified.

The sibling sweep found the same process-exit defect in
`uml-viewer.adapters.companion/-main` (`companion.clj:111`), measured at 61.25
seconds on the wrapper's synchronous path (`scripts/uml-command.ps1:133`).
The maintainer authorized its follow-up fix: successful preflight now prints
`herdr` and explicitly exits 0. The real-process regression in
`backend_selection_spec.clj` retains production subprocess stream readers,
replacing only Herdr responses: before the fix the child failed to exit within
15 seconds; after it the nine-example file passed in 1.43 seconds. A separate
read-only native invocation against actual Herdr (`clj -M -m
uml-viewer.adapters.companion . Windows`) returned `herdr`, exit 0, in 1.47
seconds. This did not start a viewer or companion and is not another GUI
acceptance run.

Follow-up quality checks: `clj -M:crap` exited 0; differential
`clj -M:mutate src/uml_viewer/adapters/companion.clj` exited 3 with three
uncovered `-main` mutations (lines 116, 118 and 121), retained without retry;
`clj -M:ir` exited 0 and generated no diagram diff. Full specs passed with
532 examples, 2322 assertions, zero failures and one pending example; the
restart-entry suite passed six tests (97.933 seconds), and the entry suite
passed 17 tests with ten skips (41.703 seconds). Incremental code review found
no blocking findings.

One sibling risk remains unfixed: `verify!`'s completed-lifecycle branch
(`recovery.clj:180-182`) checks only PID liveness rather than the recorded start
identity.

## Maintainer closeout - 2026-10-13

The maintainer cancelled manual real-terminal attach acceptance and requested
closeout of the remaining work. That item is CANCELLED / NOT VERIFIED, not
passed. Issue #30 closes with the six other checklist items verified as recorded
above. macOS acceptance remains cancelled and unverified.

The historical fixture paths in this document and the #21 acceptance record are
paths used during testing. Before removing those temporary directories, their
contents were archived to
`C:\Users\Lzx_8\AppData\Local\uml-viewer-archives\20261013-issue30-215640\native-acceptance-fixtures.tar.gz`.
Extracting the archive restores the four original directory names. The local
acceptance harness and logs are preserved alongside it in
`acceptance-harness.tar.gz`. Unmerged local branch tips are preserved in the
verified `unmerged-branches.bundle` before their branch/worktree cleanup.
These archives are local evidence, not published repository artifacts.
