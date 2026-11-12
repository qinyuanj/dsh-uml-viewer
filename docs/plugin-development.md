# Plugin host development

Ticket dsh-uml-viewer#19 added the host skeleton and `uml_ir`; #21 adds the Web
panel, the typed Remote stream and `uml_display`; #20 adds `uml_crap`,
`uml_mutate`, `uml_dry`, their per-session FIFO and the quality gate. The
checkout stays private. `pnpm package:local --binary <absolute executable>
--out <absolute scratch directory>` assembles an installable main package and
one current-platform binary package; it never builds or publishes implicitly.

Use Node 24 (or Node 22.19+) and the pnpm version pinned in `package.json`:

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test
pnpm build
```

The root is the main package `dsh-uml-viewer`. `packages/host`,
`packages/client` and `packages/contracts` are private workspace packages, bundled
by `scripts/build-plugin.mjs` into `lib/host.js` (ESM, Cordis supplied by DSH at
runtime) and `lib/client.js` (the browser closure-factory bundle registered as
`window.__ModuleLoader__.load`). The client bundle requires `react` from the
host's platform module table and bundles nothing else that DSH provides: the
built artifact must not contain React, `react-dom` or
`dsh-client-ui-primitives` (`packages/client/tests/build.test.ts` asserts this on
the output). The packaged Sarasa face under `fonts/` is served by the plugin's
own host route because the `/plugins/<pkg>/assets/*` route only serves images.
The build runs without dependency lifecycle scripts; esbuild uses its platform
optional dependency. CI runs tsc, unit tests, the Cordis seam tests and the host
build on Windows, Linux and macOS alongside the existing Rust checks.

## Binary convention

The host chooses and pins the executable at activation, in this order:

1. The trusted Loader row's `binary` config, when set.
2. `UML_VIEWER_BIN`, an **absolute executable path**, with `.exe` on Windows.
3. `target/debug/uml-viewer` then `target/release/uml-viewer` under the **plugin
   checkout**, with `.exe` on Windows.

In an assembled package, activation resolves the matching exact-version
`@dsh-uml-viewer/<platform>-<arch>` optional dependency and publishes its
binary to `<plugin home>/<version>/`. All calls in an activation use the
pinned copy. Unsupported combinations and missing/mismatched dependencies
fail before tools register, with an actionable reason. Config/environment
overrides remain available for trusted development.

The package root is the nearest directory above the running module whose
`package.json` names `dsh-uml-viewer`. That search stops at the
`node_modules` directory holding an installed copy. An installed module whose
own manifest is missing fails activation; it never adopts a manifest of the
project above the profile. Without the stop, a DSH home inside a
dsh-uml-viewer checkout would run that checkout's build. CI14 hit this case.

### Version lifecycle (#32)

The plugin home holds one directory per plugin version (`1.0.0/uml-viewer.exe`
or `1.0.0/uml-viewer`) plus `current.json` — the last version an activation
pinned, with the payload's size and SHA-256. Publishing is atomic and never
rewrites a version in place: the payload is copied into a private
`.staging-<uuid>/`, its size and SHA-256 are verified against the installed
platform package, and only then is the directory renamed into place. Six
concurrent loads therefore produce one intact copy, and the losers verify and
reuse the winner.

An update only reaches a fresh open. Every activation pins its own argv once
and leases that version for as long as it may start a process: the lease file
`leases/<version>/<pid>-<uuid>.lease` is written before the first spawn, its
mtime is a heartbeat, and both residents and tool calls hold it until the
child actually exits. Open panels keep their resident across a plugin
replacement — a root-owned pool hands their actions to the old child while a
new activation starts new sessions with the new version.

Each activation also appends one line to `activations.jsonl` in the plugin
home — `at`, `pid`, the pinned `version` or `null`, the `source` (`pinned` for a
version copy, `unpinned` for a config/environment override or a checkout build),
the package `root` it resolved and the exact `argv`. A trusted override pins
nothing and leaves no other trace, so this journal is the only record of which
build an activation actually runs; it is bounded to the newest 64 lines and is
best-effort — it never fails an activation.

A panel stream ending is **not** a panel close (D13). An unmount cannot say
why it happened: a React detach, a client bundle swap, collapsing the right
sidebar, switching fullscreen or the presentation, and plugin unload are all a
**withdrawal**. The explicit close is the host's tab close handler (ADR 0015,
#43): the plugin registers `ctx.sidebarRight.registerCloseHandler` for the UML
tab kind, and the host runs it only when the user closes the tab or a
replacement (`replaceTab`) removes it. The mount disposer only ends its own
stream; it never sends a close. The resident then waits one bounded window
(`WITHDRAWAL_GRACE_MS`, 30 s), and a remount inside it joins the same child,
version and lease. Actions or tool calls that arrive during the window borrow
the child without ending the window early. A last-panel tab close does
not wait for such a borrow: it stops the child within the bounded stop timeout
before acknowledging, so reopening the tab right after the ack always starts
the current activation instead of rejoining the old child. The late action sees
that resident stopped and reports its refusal.

Each mount opens with `watchOwned(owner)`, where `owner` is `{pageId,
holderId}`. `pageId` is a random id kept as a plain string on the browser
`document`, so it stays the same across client HMR and surface moves.
`holderId` is new for every mount. The definite close is closing the
UML tab: the close handler sends `closeOwned(owner)` for that tab's holder (a
live body closes through its controller; a body that already unmounted and is
waiting out its grace is closed with its last owner), and the resident stops
once no holder is left. Reopening the tab opens fresh, so it runs the current
activation's version. The tab's refresh command is bound through
`tab.actions.bindCommands({ refresh })` to the same `reload` as **R**. The
panel registers no `main` slot and keeps no session id in `sessionStorage`;
the full-width form is the right sidebar's fullscreen. A stale `closeOwned` from an old mount
targets only its own holder; it cannot close a newer mount. The window
expiring, plugin uninstall and DSH shutdown also stop the resident, saving
the view first.

A new document gets a new `pageId`. If the session's resident has no live
holder at that moment, it stops and the new page starts vN+1. If another live
page or window still holds the session, the new page joins that resident and
stays on vN until every holder has closed or withdrawn. This is deliberate:
one resident per session, no extra processes. An app restart starts with an
empty pool. Owner ids grant nothing: actions, `uml_display` and the IR gate
still depend only on the gateway's `agent` lookup and sandbox policy.

The version-1 `watch` and `close` methods stay available. The new client falls
back to them when an older host refuses `watchOwned` before sending any event.
A legacy `close` still stops a resident that only legacy watches use, but it
is ignored once a resident has an owned holder, so an old client's close
cannot cut off a new panel; that resident ends at its window instead. These
are Node seam tests through the real `mountPanel` disposer, a new mount and
the root pool. Real browser HMR, right-sidebar presentation changes, page
reload and app restart are **not** run-verified.

Cleanup removes only version directories that are neither current nor leased
by a live owner. A lease whose heartbeat is old but whose PID is still alive
(DSH suspended) keeps its version; a lease with no live owner (crashed DSH) is
dropped, and its version is retired by renaming it to `.trash-<uuid>` and
deleting it. Windows needs no separate failure path: the plugin probes whether
anything still maps the payload (a running exe cannot be opened for writing)
and, if so, leaves the directory in place and retries at the next activation or
lease release. It never kills a process to free a directory. `current.json`,
unknown directories and DSH's own session state are never removed.

Two consequences are deliberate. A version directory whose bytes differ from
the installed payload is never written into: when another live activation holds
it, activation refuses with the holder's PID instead of replacing a running
executable (a same-version repack while a panel is open needs that panel closed
first). And `current.json` records the version the latest activation pinned, so
it follows whatever activation ran last, not necessarily the newest directory
on disk. A `<plugin home>/versions/` directory from a pre-#32 build is left
untouched; it is development-only and can be deleted by hand.

There is no PATH or legacy `.uml-viewer/` fallback and no automatic build. A
missing binary fails activation with an actionable reason. To use a separate Cargo
output directory, set `UML_VIEWER_BIN` explicitly. For example:

```powershell
$env:UML_VIEWER_BIN = 'D:\dev\github\dsh-uml-viewer-wt\t19\target\debug\uml-viewer.exe'
```

The config accepts an absolute executable string, or a trusted argv prefix such
as `["/absolute/node", "/absolute/fake-uml-viewer.mjs"]` for seam fixtures. Neither
config nor environment strings are shell commands. Model arguments can select
only policy/output; they cannot change binary, cwd, environment or authority.
The source checkout uses the development search above; only assembled packages
resolve optional dependencies. The assembler derives versions from Cargo's
workspace version, while `pnpm version:check` checks all source manifests.
`pnpm version:sync` updates npm mirrors after changing the Cargo authority.
Before creating either archive or the output directory, the assembler runs the
supplied native binary with `--version` (five-second deadline, 4 KiB output bound).
It requires exactly `uml-viewer <workspace version>` with at most one line ending,
empty stderr and successful unsignalled exit. Missing, malformed, failed,
timed-out or mismatching identities fail packaging. Older binaries without this
CLI argument must be rebuilt separately; packaging never builds implicitly.

The Node-only payload gate suite uses an explicit disposable scratch parent:

```powershell
$env:UML_VIEWER_PACKAGE_TEST_SCRATCH = 'D:\scratch\uml-package-tests'
New-Item -ItemType Directory -Force $env:UML_VIEWER_PACKAGE_TEST_SCRATCH
node --test scripts/package-version.test.mjs
```

Its positive archives contain controlled interpreter fixtures through the
imported assembly runner seam; this proves gate/assembly behavior only. The
production CLI offers no runner override. Real current-source native packing
and runtime acceptance require a separately built executable.

## Tool behavior

`uml_ir({policy?, output?})` generates IR in the current session directory from
`ctx.workingDirectory.ensure(agent, signal)`. Paths are passed as literal argv,
including spaces and shell metacharacters. Absolute paths, empty path segments
and `..` are refused before spawning, so arguments stay inside the project;
the binary's own path and policy validation still apply. With no policy argument it selects
`examples/<project-directory>.policy.edn`, otherwise the first sorted
`examples/*.policy.edn`, otherwise `policy.edn`. With no output argument the
binary uses the policy's `:out`, otherwise `diagram.edn`.

`uml_ir` also takes `request`, the 32-hex `:request` id of a panel request that
needs no quality refresh (#29: Hide sends `:omit`, 新建提案 sends
`:new-proposal`). The host issues the id (128 random bits) when it queues the
visible message, bound to that session; the first `uml_ir` that presents it
consumes it, whatever the IR outcome. It skips only the "latest quality call
failed" refusal: the IR still waits behind a quality call in progress, and
`uml_display` still needs that IR to succeed. An unknown, used or other-session
id is refused with a reason. A session keeps at most 32 pending ids (oldest
dropped), an id DSH did not admit is withdrawn, and all ids end with the plugin
activation, so after a plugin reload or a DSH restart the companion runs
`uml_ir` without one. Choosing the discussion context switches the resident
first and then queues `{:op :context …}`; it changes no file and carries no id.
All panel requests of a session (quality, Hide, context, 新建提案) are
admitted one at a time in click order through `sessionController.prompt`
with `mode: 'queue'`, so the composer draft is never touched.

Each call resolves `ctx.sandboxPolicy` with the actual session. Read-only
refuses with a reason and no child. Workspace-write uses
`ctx.sandbox.confine(argv, policy, signal)` and spawns only the returned argv;
a missing, failing or invalid confinement provider never falls back to raw
execution. Full access uses raw argv. After asynchronous preparation, the host
re-resolves and compares the entire policy before spawning. Policy changes
require a fresh invocation. The plugin never requests elevation.

A read-only session is refused **more** than the spec's four writing tools:
`uml_display` and the panel itself (`watch`) are refused too, because both need
the `uml-viewer resident` subprocess and a read-only policy withholds that
capability (D3). SPEC-203 only names the four tools that write files. The
resident writes nothing into the workspace — view state lives under the plugin
home — so a read-only viewer would be technically possible; it stays refused
deliberately, and the plugin never escalates to obtain it. This is a recorded
decision (ADR 0014), not a bug: do not relax the refusal without changing the
decision first.

The result contains ADR 0001's `version: 1`, `operation`, `projectRoot`,
`exitCode`, `validResult`, `crapScope`, `configured` and `tool`. For IR,
`operation` is `ir`, `crapScope` is null, `configured` is false and `tool` is
`uml-viewer`. `status` is `ok`, `error` or `refused`; `reason` explains failure.
A refusal or launch failure has `exitCode: null`. Only actual exit 0 without
a subprocess termination signal or tool cancellation is a valid IR result; a
signal always fails closed and its name appears in `reason`. No result files are read or written.
stdout is bounded to 256 KiB, stderr to 64 KiB, with `truncated` reporting loss.
Confinement runner failures and access denials have separate reasons.

`uml_crap({targets?})`, `uml_mutate({targets, mutateAll?})` and `uml_dry({})`
run `uml-viewer quality <operation>` in the same session cwd and under the same
sandbox rules. The binary prints exactly one version-1 record on stdout, and
that record is the tool return value: the host validates it with
`parseQualityResult`, requires its operation, canonical project root and exit
to match the invocation, and never reads a result file. A refusal, malformed
record, signal termination or unavailable output reader returns the same
fields with `validResult: false` and an explanatory reason; a host refusal has
`exitCode: null`. Mutation takes every target in one array and forwards them in
one argv, because mutator shares one coverage run and baseline across them.

A tool that really ran and failed explains itself on stderr, which the record
does not carry (#39). The host fills the same `reason` field from it — the last
10 non-blank stderr lines, capped at 2000 characters — so `reason` names the
failure without a second, direct run of the binary. The wording stays distinct
from the refusal path (`uml-viewer quality <operation> failed (exit N)`), a tool
that wrote only blank lines keeps `reason: null`, and no record field or ADR
0001 version changes.

Quality calls in one session are queued FIFO, and `uml_ir` joins the same
queue, so it sees the outcome of a quality call already in progress. A
terminated quality child is reported as terminated, never as valid, whatever
it printed first. Each completed quality result
(including a refusal or malformed argument) becomes that session's latest
quality outcome; while it has `validResult: false`, `uml_ir` refuses before
spawning and names the failing operation. With no quality call in the session,
IR remains available for requests that need no refresh. Only a later successful
quality call clears the refusal. The `uml_display`
gate is independent: it still requires the latest `uml_ir` in the session to
have succeeded, and any later refused, malformed or failed `uml_ir` revokes it.

## Quality panel requests (#28)

Right-click a class or namespace component and choose **Refresh CRAP**,
**Differential mutation**, or **Full mutation (--mutate-all)**. The menu sends
`{action: 'quality-request', request: 'refresh-crap' | 'refresh-mutate' |
'refresh-mutate-all', id}` through the panel's existing typed Remote. Its
`agentId` is the panel's bound session.
The host asks the resident's additive `quality-target` action for the element's
source files through a quiet request. The resident resolves the id against its
CURRENT loaded Rust session document and shown proposal using the existing
scene hierarchy, including namespace subtrees, proposal layers and nested groups,
`:omit`, and `:proposal.unassigned`. The browser supplies no filenames. The host
validates the returned id, kind, bounded canonical project-relative files,
deterministic order and uniqueness, then uses precisely that answer. It never
re-reads the IR from disk, parses EDN or computes membership in TypeScript.
Foreign, unknown, missing-source and escaping-path targets are explicit refusals.
See [the protocol](resident-protocol.md#quality-target) for examples and bounds.

The host admits one visible user message through DSH's `sessionController.prompt`
(the Host service backing `remote.session.prompt`), with a fresh `requestId`,
the bound `sessionId`, `mode: 'queue'` and one text content part:

```edn
UML viewer panel request:
{:op :refresh-mutate :target {:id :core :kind :component} :files ["src/core/a.ts" "src/core/b.ts"]}
```

Overlapping requests are resolved and admitted FIFO per session; DSH's queue
retains that order behind any active turn. A failed admission is reported in
the panel and later requests continue. No composer store or DOM is accessed,
so an unfinished draft remains intact. The companion handles the message using
[the companion rules](companion-rules.md): serialized quality call, then
successful `uml_ir`, then `uml_display`. These rules are embedded from that
single document and registered in the agent's own prompt scope only while its
panel stream is open (multiple panels share a ref-counted registration).

`packages/contracts/src/requests.ts` validates the structured payload and owns
its EDN rendering. `packages/host/src/requests.ts` requests the resident target and
owns delivery/FIFO; `packages/client/src/requests.ts` mounts the element menu.
The Cordis request seam covers exact messages, session isolation, overlapping
FIFO, admission failures, and rule-section lifetime. Mounted client tests
cover the three menu actions and the unchanged draft. The Playwright cases
in `packages/e2e/requests.spec.mjs` use the existing isolated lane and a
scripted model with scratch-project Node quality overrides (no Cargo); they
require a separately available existing native binary.

### Request status (#51, #52, ADR 0015)

`packages/host/src/request-status.ts` keeps each session's panel requests in
process memory (DSH restarting forgets them; at most 64 per session, finished
ones dropped first). `PanelRequests` records each admission outcome: an
admitted request is `queued`, an admitted context switch is `done`, and a
refused one is `failed` with the admission reason. A hide or new proposal is
named by its grant id, the one `uml_ir({request})` presents; any other request
by its admitted message's `requestId`. The panel stream carries the whole list
as `{kind: 'request-status', requests: [{id, op, target, state, reason,
exitCode}]}` (`packages/contracts/src/request-status.ts`). It is sent once
after the opening events and again after each change, so a reopened tab or a
refreshed page sees the unfinished entries again. A session's list never
reaches another session's stream. `RequestStatuses.record` replaces an entry
with the same id, which is the seam for the transitions below and for dismissal
(#53).

The status bar (`packages/client/src/statusbar.ts`) shows one current entry and
how many wait behind it as 「还有 N 条」; the button opens the whole queue in
list order (#53). The current entry is the list's own choice — an unseen
failure, else the running head, else the oldest queued, else the newest — so a
refreshed page and an open page agree. A completed entry fades on the client
ten seconds after it finishes or as soon as the next request starts; a failed
one stays until the user dismisses it. Dismissal is the `dismiss-request` panel
action (`{action: 'dismiss-request', id}`), answered by
`RequestStatuses.dismiss` in the host: only a finished entry goes, and the
session's new list reaches every panel. The browser evidence is
`packages/e2e/queue.spec.mjs`; the host seams are in
`packages/host/tests/requests.seam.test.ts` and
`packages/host/tests/request-status.seam.test.ts`, the standing rules in
`packages/host/tests/request-status.test.ts` and the bar's own behaviour in
`packages/client/tests/statusbar.test.ts`.

#52 attributes the companion's work to those entries in FIFO order: the head
of the list — the oldest entry still `queued` or `running` — is the request it
is handling, so

| Trigger | Transition |
| --- | --- |
| the first `uml_*` call after the head was admitted | `queued` → `running` |
| a quality result with `validResult: false` | `running` → `failed`, with the record's `reason` (a fixed sentence when the record has none) and its `exitCode` |
| a failed `uml_ir` | `running` → `failed`, with the IR record's reason and exit code |
| a successful `uml_display` | `running` → `done` |
| a turn ends with a request it owned unfinished (consumed, or admitted before it began) | → `failed`, reason *the companion ended this turn without completing the display* |

The attribution is the FIFO one the spec accepts: a quality call the user asks
for by hand is attributed to the head too, so its result can end a request the
user did not ask it for. A hide or a new proposal is matched exactly instead:
`uml_ir({request})` names one entry by the grant id it carries, so its `running`
and `failed` transitions land on that entry, not on the head. A successful
`uml_display` completes the head and reports that entry's `target` in its
result, which is what 「在面板中查看」 locates (#54); a display that answers no
request names no element. The head is read from the list itself, in request
order, never from a separately remembered "last request" record.

Turn boundaries come from DSH's published `session/event` feed, the one public
event a plugin can observe them through: `turn/start` clears the turn's claims
and numbers the turn, and `turn/end` settles it. A durable `user/message` echoes
the prompt `requestId` as its source `rpcId`, which is how the host knows a turn
consumed one of its requests; the claim is held by prompt id until the turn
ends, so a message that reaches the log before the panel's own admission
returns is still matched. `turn/end` also fails the head as it stood when the
turn ended, unless the request was admitted while that turn was already
running: a turn owns every request queued before it began, so a request the
companion never handled fails with the turn-end reason instead of staying
`queued` forever, while a request the panel queued mid-turn waits for the next
one — an admission racing a turn's end is not failed by the turn that could not
have handled it. `agent/status` is not used: it carries no turn identity, so a
per-agent idle would fail a request the next turn is about to pick up.

A refused request still names the element the browser clicked (#52 F26): the
refusal keeps the id the user clicked when the shown diagram gives its kind
(an unresolved target of a refresh, a foreign library, a class without source).
Requests whose id is not in the shown diagram any more name no element.

## Contracts and follow-on seams

`packages/contracts/src/tools.ts` owns JSON-only request/result contracts and
schemas. It has no Cordis, browser or filesystem dependencies. The package root
exports the public contracts. Later additions belong in separate modules:

- `tools.ts`: quality result parsing for the ADR 0001 version-1 fields, and
  `uml_display` requests/results. Preserve actual exit codes and the selected
  tool's valid-result rules; exit 3 is not universally valid.
- `panel.ts`: visible panel requests, discussion context and view state shared
  by host/client. FIFO remains DSH's `session.prompt` queue (#21 onward).
- `resident.ts`: validation and codecs for #17's line-delimited JSON protocol,
  schema `dsh-uml-viewer/resident`. Rust owns its wire fields; consume
  `docs/resident-protocol.md` and `crates/uml-viewer/tests/resident.rs` from
  the integrated #17 rather than guessing.
- `scene-frame.ts`: validate the Rust-owned wire format documented in
  `docs/scene-frame.md`, tested against `crates/frame/tests/fixtures/frames`.
  Do not invent a second scene model. Additive fields/kinds are compatible;
  unknown kinds are skipped and unknown roles render as `ink`.

Compatibility note: `PanelEvent.state` requires `breadcrumbs`/`status` in TS while the validator defaults absent fields to `[]`/`null` for a resident from before #45, and `DisplayResult.target` is optional in TS while `DISPLAY_RESULT_SCHEMA`/`validateDisplayResult` require it; in-repo producers always send both (F5/F11).

The host's `dsh.ts` is a narrow structural boundary to the inspected DSH
0.2.2-rc.1 services; `process.ts` owns confinement and bounded subprocess
execution; `runner.ts` composes IR arguments and the tool result; `quality.ts`
runs a quality operation and validates its stdout record; `binary.ts` owns
executable location, `versions.ts` the leased version directories;
`policy.ts` owns default policy selection; `index.ts` owns
the tool definitions, the IR handler's quality check and effect lifetime.
`gate.ts` holds both per-session gates: the latest quality outcome with its FIFO,
and the IR success `uml_display` requires. `lifetime.ts` keeps panel state on
the root context so a replaced activation's open panels keep their resident.
That root state is the plugin's one dependence on a not-yet-stable Cordis API:
`ctx.root` is `@experimental` in Cordis 4.1.0 (`context.ts`), the peer range is
`~4.1.0`, and any Cordis upgrade must re-verify it or move the pool to a
documented root-scoped service. The version seam fails loudly if it disappears;
nothing falls back silently. The state itself is keyed by
`Symbol.for('dsh-uml-viewer.panel-state.v2')`, so any change to the pooled
`ResidentPool`/`ResidentConnection` shape must bump that version. The v2 format
adds owned holders, transitions and separate panel/borrow references. It never
adopts a v1 pool: that pool remains under its original root cleanup. Resident
and lease sharing across replacements is verified only within the same format;
an incompatible JavaScript format starts a separate pool.

The `uml_display` gate is a per-session record of this plugin's own `uml_ir`
outcomes, written by the `uml_ir` tool and read by `uml_display`.
It is deliberately conservative — it is **process-local**, so after a host
restart or a session fork the first `uml_display` is refused until `uml_ir` runs
again in that session; it never reports someone else's success. The session log
carries rendered tool content only (no structured tool value), so folding it
cannot reconstruct the gate; a future ticket that needs cross-restart gating
should extend the host seam with a structured result, not parse text.
`resident.ts` owns one `uml-viewer resident` child per session, `viewer.ts` the
typed Remote, `prompt.ts` the companion prompt section that lives exactly as
long as that session's panel stream, and `state.ts` the persisted per-session
view state under the plugin home.

## Panel language (#42, ADR 0015)

Every DOM label and accessible name in the panel comes from the `panel` section
of `locale/en.json` and `locale/zh.json`; no client source hard-codes them.
`packages/client/src/locale.ts` reads the active language from DSH's
`ctx.locale` (`getLocale().active`): any `zh-*` id is Chinese, every other id
falls back to English. A switch relabels the open panel in place through
`PanelText.subscribe`; the tab is not reopened. New DOM text adds one key to
both files (a unit test keeps their key sets and `{placeholders}` equal), reads
it with `text.t(key, params)` and relabels on `subscribe`. Chinese copy uses
the GLOSSARY's interface terms (源码图, 提案, 隐藏, 类卡片, 源码窗口). Text the
resident lays out (card rows, 「无数据」, source errors, the legend's rows) is
chosen by the Rust string tables instead (#44): the panel passes
`text.language` when it opens its stream and again on every switch, and
`crates/frame/src/strings.rs` holds the Chinese and English tables behind
`Lang`, with English for anything else.

## Diagram legend (#50, ADR 0015)

The legend explains the drawing — the class and component boxes, the foreign
package ellipse, 质量色 and 无数据, the selected outline, 关系连线 and its
violating, hovered, chained and faded states. It is not a second drawing: its
rows are typeset in Rust (`crates/frame/src/legend.rs`, worded from
`crates/frame/src/strings.rs`) and ride the frame's `legend` field
(`docs/scene-frame.md`), so the panel rewords them with the next frame after a
DSH language switch and the frame goldens cover both languages.
`packages/client/src/legend.ts` lays them out as plain markup styled from
`--dsw-*` tokens; each swatch resolves its semantic roles through
`theme.ts`'s resolver, the one the canvas paints with, so a legend color is the
diagram's own color in both themes.

Where it lives follows `layout.ts`'s container measurement (#49), never the
slot the panel sits in. Below 900px the legend is a popover opened from
「更多」 (`moreMenuEntries` is extended through `mountTopBar({moreEntries})`);
**Esc** closes it, before the drawer and before going back a level
(`PanelController.escape`). From 900px it is resident: the legend takes a strip
of the canvas row below the diagram, so the canvas — and the viewport the
resident is told about — shrink with it and the legend never covers a class.
The overlay root stops above the strip, so a docked drawer never covers the
legend either. That is deliberate: a docked card reserves the right column of
the drawing stage, its canvas shrinks, and the resident is told the smaller
viewport so it refits — the card is not an overlay on the drawing (F28).
The 「更多」 legend entry is omitted while the legend is resident, since there
is no popover to open (F45).

## Conversation tool views (#54, ADR 0015)

The five `uml_*` tools register the host's keyed `tool.call.toolview` slot in
`packages/client/src/tool-views.ts`, one key per wire name, so a call in the
conversation reads as its outcome instead of the raw JSON record. The quality
tools show validity, the reason and the targets the call dispatched (`args.targets`
names explicit files, `crapScope: "targets"` names a configured target scope,
and only a whole-tree result says the whole business scope); `uml_ir` shows
success and its bounded output; `uml_display` shows the diagram it put on
screen and 「在面板中查看」. Every word comes from
the `panel` section of `locale/{en,zh}.json` and follows a DSH language switch,
like the panel's own chrome (#42): the row subscribes through `PanelText` and
re-renders in place. Rows are plain markup styled from `--dsw-*` tokens; they
never load `dsh-client-ui-primitives` or the host's Tool views (ADR 0013).

`uml_display` carries one additive field, `target: {id, kind} | null`: the
element of the shown diagram that the panel request it answered named
(`PanelRequests` remembers the target of the session's last admitted request
and `uml_display` consumes it on success). 「在面板中查看」 opens that session's
UML tab through `ctx.sidebarRight.openTab` and sends the existing `locate`
action once the panel can take actions — immediately, or on the reopened
panel's first live frame. A display that answered no request (`target: null`)
only opens the tab, and a refused or failed display does not locate anything.

## Manifests and verification

DSH reads `package.json`'s `dsh.bundle.patch`, runtime export and `engines.dsh`.
`dsh-plugin.json` is the informational community manifest, not an authorization
mechanism. The patch inserts exactly one row named like the package. The
manifest lists all five tools: `uml_ir`, `uml_crap`, `uml_mutate`, `uml_dry`
and `uml_display`. The browser half is declared where DSH reads it:
`package.json` `dsh.client` (`platform: "web"`, `immediately: true`, the right
sidebar as its only activation-order dependency, no `external` requests) plus the
`"./client"` export. `dsh-std` 0.15 has no `facets.client` key, so the
informational manifest uses the host facet and explains the Web client split
in its notes. Permissions declare subprocess execution, project reads/writes
and the plugin's own DSH home for pinned binaries and session state.
English/Chinese inventory text and original SVG artwork are included.
Local package acceptance uses only a scratch DSH home and profile.

The seam tests load the host into a real Cordis 4.1 Context, provide controlled
DSH services, and spawn a real fake binary through the subprocess seam. They
check actual argv/cwd, results for exits 0/1/3, refusal, confinement, authority
changes and unload cleanup. The version seam runs fake resident payloads from
scratch DSH homes under two plugin versions: a live vN panel and a fresh vN+1
panel in the same root, a withdrawn stream (abort plus iterator return, as the
gateway leaves it) with the same session remounting on the same vN child, the
real `mountPanel` disposer followed by a new mount keeping the vN child and
lease until the UML tab's close handler runs, after which the reopened tab
runs vN+1, the close handler stopping the resident when a replacement removes
the tab or after the body already withdrew, a close of one UML tab leaving
another alone, the tab refresh command reaching the resident as `reload`, a distinct
withdrawn document opening fresh while a live second page shares the resident,
an explicit close stopping it, root shutdown clearing a pending withdrawal window,
six simultaneous real loaders for one version, cleanup
of unleased directories only, and — on Windows — a resident running from a real
copied `node.exe` while the plugin is updated. The quality seam adds a fake
quality binary that
emits the version-1 stdout line, so exits 0/1/3, signal termination, failed
output readers, malformed records, FIFO ordering and the quality→IR→display
gate are observed at the tool boundary. `.uml-viewer/` fixtures exist **before
plugin load**; nested bytes and nanosecond mtimes are compared after calls and
unload on success, failure and refusal. These are plugin seam tests, not a
claim that the fake confinement provider enforces an OS sandbox.

## Browser acceptance lane

`#22` adds a real Playwright lane under `packages/e2e/`. It boots an isolated
DSH Web on port 0 with its own scratch home, mock model, workspace registry and
scratch Chromium, loads this plugin, opens two panels on two projects, and
asserts the displayed diagrams and the independence of their discussion
contexts. Run it after `pnpm build` and a real binary build:

```sh
pnpm e2e:install
pnpm test:e2e
```

`UML_VIEWER_BIN`, `DSH_CLI` and `UML_VIEWER_E2E_SCRATCH` select the binary, DSH
CLI and scratch root. The harness API that `#23`–`#29` should extend, the
isolation guarantees and the failure artifacts are documented in
[e2e-harness.md](e2e-harness.md). It never attaches to the live GUI or the
user's profile.

Mounting the panel in a real browser forced two production changes; the reason
for each is recorded in the
[ADR 0013 #22 appendix](adr/0013-repository-migration.md): the visible
`typertRemote` binding the Gateway requires from a hand-written Remote service,
and `uml_display` re-applying the session's persisted proposal discussion
context (`refresh(..., restoreContext)`).

For optional manual loading, use the isolated-instance recipe in the shared
`EXPLORE-dsh.md` §12: a fresh scratch `DSH_HOME`, `--profile web`, `--port 0` and
`--no-open`. Install only into that scratch instance. Never use the live GUI,
the user's profile, or scripts that kill all DSH processes. A browser or live
DSH smoke test is not part of the automated Cordis seam.

The verified native-Windows development loading route uses the Desktop CLI and a
checkout-local scratch home (replace `$DSH_CLI` with the installed `dsh.cmd`):

```sh
pnpm build
export DSH_HOME="$PWD/.scratch/dsh-home"
"$DSH_CLI" plugin --profile web add "$PWD"
"$DSH_CLI" --profile web --port 0 --no-open
```

This links the development package only into the scratch `web` profile. Use the
printed token URL for that instance, and stop only its own process tree afterward.
