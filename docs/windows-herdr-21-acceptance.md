# Windows Herdr native acceptance — ticket #21

> **历史记录**：本文描述的 Herdr companion、邮件与 `./uml` / `get-uml-viewer` 入口已在 qinyuanj/dsh-uml-viewer#18 删除，只在冻结的 qinyuanj/uml-viewer 中保留；本文不再是本仓库的现行契约或验收清单。

Status: **partial acceptance**. Wrap-up was ordered before the full matrix
completed. Every criterion below is marked PASS, FAIL, or NOT VERIFIED;
unverified items are gaps, not passes. Do not read this document as "#21
passed".

## Environment (recorded)

| Item | Value |
|---|---|
| Platform | Windows 11 Pro 10.0.26220, native (no WSL) |
| Herdr | official herdrdev/herdr 0.9.3, socket protocol 22 |
| Claude Code | 2.1.296 (`C:\Users\Lzx_8\.local\bin\claude.exe`, real CLI) |
| PowerShell | pwsh 7.6.6 |
| JDK | Temurin 21.0.12.1 |
| Branch | `work/spec13-ticket21` (worktree), base `integration/spec-13-herdr` f9bce46 |
| Last code SHA verified | **f5ceb0b** (gates below) |

Evidence root: `C:\Users\Lzx_8\AppData\Local\Temp\uml-acc21\evidence\`
(screenshots, observer snapshots, herdr inventories, recovery timelines,
negative-case results). No credentials or API keys appear in any captured
artifact; prompts/argv contain no secrets.

Fixture projects A (`proj A`, space in path) and B were install-only clones
under `%TEMP%\uml-acc21\fixtures\` with the viewer at the tested SHA, driven
exclusively through the public wrapper `scripts/uml-command.ps1` (`./uml`,
`./uml --restart`) and GUI/observer ops.

## Gate results at last verified SHA (f5ceb0b)

- `clj -M:spec`: 576 examples / 2400 assertions / 0 failures / 1 pre-existing
  Unix pending — exit 0.
- `clj -M:crap`: exit 0.
- Differential `clj -M:mutate` per changed file: herdr.clj 89/89 killed
  (+7 uncovered retained per policy), sketch.clj 12/12 killed — exit 0/3 as
  applicable, snapshots committed.
- `clj -M:ir`: exit 0.
- Integration merge: `Already up to date` against f9bce46.

## Per-criterion results

### Recorded facts — PASS

Checkout/versions/platform recorded above; automated evidence (files,
timelines, JSON inventories) is separated from manual GUI/terminal
observations (only items explicitly observed are claimed).

### Complex argv/cwd — PASS

Project A path contains a space (`fixtures\proj A`); companion ownership
records the exact argv (`claude.exe --dangerously-skip-permissions
--session-id … --append-system-prompt …`) and exact cwd. Companion behaved
correctly in that cwd across fresh launch, recovery relaunch, and restart.

### Missing/incompatible dependency refusal — PASS

Evidence `p1-missing-herdr.md`, `p1a-missing-herdr.md`,
`p1-incompatible-herdr.md` (fake/shim herdr, `evidence\fake-herdr`,
`shim-log`): launcher refuses with an actionable install/version message
before creating any Herdr resource; no session/pane created.

### Fresh create exactly once — PASS

Duplicate fresh launch refuses; second start is routed to restart instead of
spawning a second companion. Real defect found and fixed during acceptance:
JVM identity was recorded before startup completed (fixed 9b5385e + 3c6955a,
`start-recorded!`). Launch records `launch-fresh-*.json`.

### Terminal attach (manual observation) — NOT VERIFIED

No explicit manual terminal-attach observation was performed; automated
pane/foreground checks passed but that is not the manual criterion.

### Initial display — PASS

Real GUI observed: `a2-baseline.png` screenshot plus observer snapshot;
fresh start waited for `:display` mail before showing the diagram
(mail-seen progression recorded in `a1-view`).

### On-diagram FIFO mail consumed with display returned — PASS

`:context` and `:regen` mail consumed oldest-first with `:display`
returned each cycle (earlier A run: regen loop evidence; this lifecycle:
mail-seen 9→10→11 across proposal/view interactions, observer timeline
`acc21-ready.ps1` polls). Cursor preserved across JVM restarts
(mail-seen 6 → 10 → 11 across two restarts).

### Failed wake preserves queue — PASS (automated) / earlier lifecycle evidence

Wake-failure handling covered by spec and by earlier lifecycle evidence;
no new native failure injected this run beyond the recovery relaunch path
(which re-queued and completed). Recorded as PASS from earlier accepted
runs, not re-executed in the final lifecycle.

### A/B true parallelism — PASS

Evidence `a1-parallel`, `inventory-a1-parallel.json`: two projects, two
sessions, controllers 25008/47388, distinct Claude children; operations on
one never touched the other; default session untouched throughout.

### Abnormal recovery — relaunch — PASS

Real Claude child 35668 killed (PID+start revalidated immediately before
kill, `acc21-kill.ps1` + `kill-validation.json`). Timeline
`a3-recovery-conout\timeline.json` at SHA a4b6534:

- t+210 ms: exit detected, `:phase :backoff`, deadline set (1 s backoff).
- t+1270 ms: `:generation 1, :attempts 1, :phase :submitting` — new runner
  state file `-runner-1.edn`.
- t+5482 ms: `:phase :running`.
- t+7841 ms: new child 46528 recorded, PID/start verified, same session
  `…-b91edbcc-…`, same pane, same controller 47164.

Two real defects were found by native runs and fixed (see Defects): killed-TUI
ConPTY residue (3902e1b) and the reset writing to a redirected stdout instead
of CONOUT$ (a4b6534). The timeline above is the PASS after both fixes.

### Abnormal recovery — budget exhaustion (1/2/4 s ×3, then stop) — NOT VERIFIED

Spec-covered deterministically (recovery_spec); no native run exhausted all
three retries. GAP.

### Abnormal recovery — normal exit does not recover — NOT VERIFIED

Spec-covered; no native normal-exit observation. GAP.

### Close cancels pending retry — NOT VERIFIED (native)

Close with terminal phases (`:failed` journal) verified natively
(`a-close-failed-recovery`); close during a pending backoff window was not
observed natively. GAP.

### Public-wrapper restart — old JVM exit / new JVM bind / companion continuity — PASS

- Recovery→restart: old JVM 28656 exited; new JVM bound; companion child
  46528 and controller 47164 unchanged; recovery attempts reset per 60 s
  uptime policy. Evidence `a2-recovery-fixed`.
- Namespace restart at f5ceb0b (`launch-restart-*.json`, observer
  snapshots): restored zoom 1.1, declutter `:arrows`, camera
  (cam-x 55.4545, cam-y 41.8182 — nonzero via set-zoom, scroll limits
  clamp plain pan on small diagrams), selected class, mail-seen 10;
  companion child 38192 + controller 34044 unchanged (herdr process-info
  verified). Namespace-tree restart restore: PASS.

### Restart restores named-proposal view state — NOT VERIFIED

Saved state was recorded completely (session.edn after quit-for-restart
mail id 11: `:proposal-id :p-demo`, discussion context
`{:context :proposal :proposal-id :p-demo :name "Demo proposal"}`,
zoom 1.21, declutter `:arrows`, cam-x 50.413 / cam-y 38.017, selected
`:util`, `:detail-id :util`, `:proposal true`; screenshot
`a5-proposal-front.png` — note `a5-proposal.png` is INVALID, an occluding
foreign window was captured; recaptured with `:front`). The post-restart GUI
restore of the proposal pan/zoom/declutter/focus/selection/detail was never
observed — wrap-up was ordered with the restart not yet launched. GAP.

### Restart reopens the detail card window — spec PASS / native NOT VERIFIED

Real defect found natively (detail window absent after restart), first fix
7e9bebb patched the public `setup` fn which production never calls
(production `start!` uses an inline `:setup` closure) — native revalidation
correctly still failed. Proper fix at the production boundary f5ceb0b with a
spec invoking the captured `q/sketch` `:setup` callback (576/0). Native GUI
revalidation of f5ceb0b: NOT VERIFIED. GAP.

### Recovery→restart combination (context, FIFO mail, display, mtime reload, metrics) — PASS (partial scope)

Context, FIFO cursor, display, and EDN mtime reload observed across the
recovery→restart sequence (`a2-recovery-fixed`); CRAP/mutation metrics
overlay reload after restart not separately exercised this run — covered by
the unchanged mtime-reload path, recorded honestly as not separately
re-observed.

### Final GUI close cleans only own resources — PASS (earlier lifecycle) + final teardown

- Project B full GUI close: companion.edn → `{}`, recovery journal
  `:cancelled`, owned session deleted from herdr session list; project A
  kept working; default session untouched
  (`inventory-post-b-close-2.json`).
- Ambiguous/missing identity: no replacement, explicit errors — see
  negative matrix below.
- Final A teardown (this wrap-up): the last lifecycle ended via
  quit-for-restart mail (viewer JVM exited, mail-seen 11), so GUI close
  was unavailable; performed manual teardown instead — durable recovery
  cancel (`{:cancel <token>}`) → controller 34044 exited, journal
  `:phase :cancelled` ("Viewer closed; recovery revoked") →
  `herdr session stop` + `delete` of owned session
  `uml-viewer-693aff7c-788fa4c2-…` → child 38192 and shell 32820 gone →
  companion.edn reset to `{}`. No stray fixture processes
  (Win32 scan for `uml-acc21` in java/claude/pwsh command lines: none).
  No revival observed.

### Negative identity matrix — PASS

`scripts/acc21/acc21-negative.ps1`, `evidence\negative-identities\results.json`
(junction-linked installed clones, wrapper invoked exactly as a user would):

| Case | Result |
|---|---|
| missing launcher record | exit 1, `uml: No launcher process record; launch through ./uml before restarting.`, 0 launches |
| missing viewer-jvm.json | exit 1, `uml: No viewer JVM process record; cannot safely restart without authoritative JVM identity.`, 0 launches |
| incomplete ownership | exit 1, `uml: Companion ownership is incomplete, provisional or belongs to another project; file retained`, 0 launches |
| recorded session no longer exists | wrapper async-spawned JVM (exit 0 boundary), JVM failed closed with explicit "Herdr endpoint is not the verified 0.9.3/protocol 22 server; ownership retained" in uml-viewer-log.txt; no replacement session created; herdr session inventory unchanged |

### Legacy backend retained — PASS (by policy at the recorded #21 run)

tmux/psmux untouched; Herdr remains opt-in. #21 is not claimed complete, so
legacy backends stay.

This is the historical state of that acceptance run. The maintainer later
cancelled the full dual-platform retirement prerequisite and authorized #23;
the current implementation uses Herdr as the sole backend. See the
[current contract and local wrap-up](herdr-backend-contract.md#8-原生验收边界).
The seven native gaps below remain unverified; retirement adds no acceptance
evidence to this record.

## Real defects found by native acceptance and fixed (TDD, all gates green per commit)

1. cea48cc — atomic state/mail replacement raced transient Windows readers.
2. cec167a — journal reads raced replacement; unchanged rewrites churned mtimes.
3. 9b2931f — fresh launch after completed close did not retire the terminal
   recovery journal.
4. 9b5385e + 3c6955a — viewer JVM identity recorded before startup succeeded
   (ordering; `start-recorded!`).
5. 4c63eb8 — bounded grace for the runner to record a child exit.
6. 3902e1b + a4b6534 — recovery relaunch into a killed-TUI pane: ConPTY mode
   residue (alt-screen/bracketed-paste/kitty keyboard) mangled the runner
   command; fixed with a Windows-gated terminal reset via CONOUT$ (first
   attempt wrote to redirected stdout and failed natively — the a4b6534
   correction is what the PASS timeline above ran). Lesson: scratch proof of
   the recipe was not sufficient; only the real killed pane exposed the
   stdout-redirection defect in the fix itself.
7. 7e9bebb → f5ceb0b — detail card window not reopened on restart; first fix
   patched the non-production entry point, corrected at the production
   `start!` boundary. Lesson recorded in spec: stub specs must exercise the
   production entry path.

## Resource status at wrap-up

- Owned: A controller cancelled/exited, session stopped+deleted, child/shell
  gone, companion `{}`; B closed earlier with full cleanup; scratch session
  stopped+deleted earlier; three negative fixture dirs deleted. No owned
  processes remain.
- NOT mine, left running and untouched: default Herdr session
  (`…\Roaming\herdr\herdr.sock`), pre-existing herdr PIDs 8852/37144/42044,
  and two foreign uml-viewer sessions that appeared mid-run
  (`uml-viewer-1d954c3e-130dd795-…`, `uml-viewer-dd6e8fe6-201be54b-…`),
  confirmed not parent-owned; per instruction they were not stopped,
  signalled, or treated as pollution. No interference from them was observed
  beyond one invalid screenshot (recaptured).
- Fixture `%TEMP%\uml-acc21` retained as the evidence store.

## Gaps handed back to #13 (not verified natively)

These gaps were subsequently transferred to [#30](https://github.com/qinyuanj/uml-viewer/issues/30).
The list below records the historical #21 outcome; later runs do not change it.

1. Named-proposal restart restore (pan/zoom/declutter/focus/selection/detail
   + discussion context) — state save verified, restore not observed.
2. Detail-window native revalidation of f5ceb0b.
3. Recovery budget exhaustion (A-L2).
4. Normal exit → no recovery (A-L3).
5. Close cancels pending backoff retry (A-L4).
6. Manual terminal attach observation.
7. Focus/open-layer restore after restart; metrics overlay reload separately
   re-observed after restart.
