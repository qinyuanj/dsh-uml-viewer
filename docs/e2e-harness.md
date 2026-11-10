# Browser acceptance harness

The browser lane drives a real DeepSeek Harness (DSH) Web instance and the real
viewer panel. It is the seam for #22–#29: `packages/e2e/panel.spec.mjs` asserts
what the panel displays, not a recording Canvas or a stubbed panel controller.

## Prerequisites

- Node.js 22.19+ or 24, and the pnpm version pinned in `package.json`.
- The DSH 0.2.2-rc.1 Desktop CLI or an equivalent CLI on `PATH`. Set `DSH_CLI`
  to its absolute path when it is not discoverable. On this Windows host the
  harness finds the installed Desktop CLI itself.
- For checkout mode, a real `uml-viewer` binary. Build it with `cargo build --locked -p
  uml-viewer`, or set `UML_VIEWER_BIN` to an absolute executable. The lane
  passes the path in the plugin config of its scratch profile.
- Chromium in the repository's scratch prefix. `pnpm e2e:install` explicitly
  sets `PLAYWRIGHT_BROWSERS_PATH` to `.scratch/e2e/browsers`; it does not use
  or update the global browser cache.

```sh
pnpm install --frozen-lockfile
pnpm build
cargo build --locked -p uml-viewer
pnpm e2e:install
pnpm test:e2e
```

`UML_VIEWER_BIN` may point at a binary built in another checkout. The lane
passes `PLAYWRIGHT_BROWSERS_PATH` to its host and browser, so an alternative
scratch root works:

```sh
export UML_VIEWER_E2E_SCRATCH="$PWD/.scratch/e2e-isolated"
export UML_VIEWER_BIN="$PWD/target/debug/uml-viewer"
pnpm e2e:install
pnpm test:e2e
```

On native Windows those paths must be **Windows-absolute** (`D:/…` or `D:\…`).
An MSYS/Git Bash value such as `/d/dev/…` is not absolute to Windows: `$PWD`
expands to `/d/…`, the host resolves it against the current drive (`D:\d\…`),
and the confinement runner answers `CreateProcessAsUserW failed (Win32 2)`
with `exitCode 127`, so `uml_ir` fails and the panel ends on
`data-kind="closed"` instead of `"frame"`. Only the display assertion catches
it.

## Resource-limited #26 verification

`quality.spec.mjs` converts the quality/highlight scenarios of the deleted egui drivers
(`class_boxes.edn`, `lod_shots.edn` and `metrics.edn` for 质量色, C/M 圆点 and
无数据; `navigation.edn` for 依赖链高亮 and 关系淡化, whose resident selection
`navigation.spec.mjs` already exercises) to the existing lane:
independent C/M dots and quality fills, missing CRAP/mutation/both data, red no-data
ink, unchanged quality/violation hues with changed host skin tokens in both modes,
resident selection/chain/dimming, local relation/class hover with the producer's
dependency tooltip rows, and the overview class-name popup clearing on leave.
`edges.spec.mjs` also checks that dimming preserves opaque hollow-head interiors.
The producer oracle accepts `hover`, `pointer` and `--hover-metadata`, so tooltip
expectations come from Rust; no frames are injected.

`queue.spec.mjs` (#53) drives the request queue in the status bar: three
admitted refreshes show the current entry and 「还有 N 条」, the button opens
the queue in list order, a refused refresh stays as a failed entry past the
ten-second fade, and its dismissal shrinks the list, adds no message to the
conversation and survives a page refresh (the host dropped the entry). Its
scripted companion only acknowledges, so every admitted request stays queued;
the completed fade needs the transitions of #52 and is covered in
`packages/client/tests/statusbar.test.ts`.

`toolviews.spec.mjs` covers the conversation side (#54): a finished turn keeps
its steps collapsed, so the spec opens the turn and its tool group the way a
user does, then reads the quality rows (validity, the refusal's reason, the
targets) in English and after a DSH language switch to Chinese. Its second case
closes the UML tab and clicks 「在面板中查看」 on a display result twice: once
for a display that answered no panel request (the tab opens with nothing
selected) and once for a display that answered a refresh asked for on
`:core.order` (the tab opens and locates that class). The quality operations
are a Node fixture declared in the scratch project, so no Cargo work runs
inside the lane; IR, resident and display are the real binary's.

These cases have run in remote CI; see the [partial results below](#ci).
The lane never builds, so a run needs an existing matching executable supplied from outside.
Do not follow the build commands above to make one inside the lane. A run sets
**both** environment variables for browser installation and tests:

```powershell
$env:UML_VIEWER_E2E_SCRATCH = '<scratch>/e2e-uml-viewer'
$env:PLAYWRIGHT_BROWSERS_PATH = '<scratch>/e2e-uml-viewer/browsers'
$env:UML_VIEWER_BIN = 'D:/absolute/path/to/existing/uml-viewer.exe'
# Only when a matching existing binary is available:
pnpm e2e:install
pnpm --filter @dsh-uml-viewer/e2e test:e2e quality.spec.mjs
```

Changed host tokens exercise the skin boundary without installing a skin in the
user's live profile. This does not claim an installed dsh-claude-style browser
run. Hover metadata (bundled-edge `viaIds`, dependency tooltip rows) is an
optional frame field (`docs/scene-frame.md`), so an older binary degrades to
endpoint-only hover; raw child-row hover still lacks a separate hit record (see
ADR 0013 #26 appendix).

## Packaged install/remove acceptance (#31)

The same bootstrap also accepts a local main tarball and the matching local
platform tarball. Supply **both** absolute paths; a missing or relative input
fails before installation. On Windows use Windows-absolute paths:

```powershell
$env:UML_VIEWER_E2E_SCRATCH = 'D:/scratch/uml-viewer-e2e'
$env:UML_VIEWER_PACKAGE_MAIN = 'D:/scratch/uml-viewer-pack/dsh-uml-viewer-1.0.0.tgz'
$env:UML_VIEWER_PACKAGE_PLATFORM = 'D:/scratch/uml-viewer-pack/dsh-uml-viewer-win32-x64-1.0.0.tgz'
pnpm --filter @dsh-uml-viewer/e2e test:e2e package.spec.mjs
```

Use the host platform's tarball (`win32-x64`, `linux-x64`, `darwin-arm64` or
`darwin-x64`) at the same version as the main package. Assembly and binary
builds happen separately; this lane does not build or pack either artifact.
With neither variable set, the regular checkout lane remains available and
`package.spec.mjs` skips with an explicit reason.

Packaged mode installs both tarballs in one `dsh plugin --profile web add
--ignore-scripts --store-dir <scratch-store> <main> <platform>` invocation under the unique scratch
`DSH_HOME`. It removes inherited `UML_VIEWER_BIN` and omits the Loader `binary`
config, exercising the shipped platform dependency and DSH-home binary locator.
The mock model still invokes real `uml_ir` and `uml_display`; acceptance opens
the real Shop panel and requires a displayed frame and visible canvas.

Before installation the spec seeds proposal and nested binary metrics fixtures
in every sample project and captures policy, proposal and `.metrics/` bytes,
directory membership and nanosecond mtimes. After display it closes the browser
contexts and host, then runs `dsh plugin --profile web remove --store-dir <scratch-store>
dsh-uml-viewer @dsh-uml-viewer/<platform>` in the same isolated home. CLI failure
fails the test. The final snapshot must match the pre-install snapshot exactly,
including inline policy proposals and their formatting. The run home retains
`package-preservation.json` with both snapshots; the screenshot is
`<scratch>/evidence/packaged-shop.png`. No live profile is consulted or changed.

## Installed live upgrade acceptance (#32)

`upgrade.spec.mjs` is a separate Windows CI case. Alongside the initial main and
platform tarballs above it requires absolute `UML_VIEWER_UPGRADE_MAIN` and
`UML_VIEWER_UPGRADE_PLATFORM` paths for disposable version 1.0.1. With no upgrade
pair it skips; the ordinary panel and package lifecycle cases are unchanged.

The pinned DSH 0.2.2-rc.1 manager documents updates as removal followed by
installation, and only Plugin Manager operations reconcile a running host: an
external `dsh plugin add` writes files and selection and leaves activation to
HMR's manifest watch, which can miss a removal and re-add that restore the
same bundle list (CI run 42628045954 stayed on 1.0.0). The sequence is
therefore three steps, each answered through the scratch page's **Allow once**
button when approval is requested:

1. a scripted prompt calls the public `plugin_manager` tool (`list_bundles`,
   then `remove_bundle`); the real result must be `application: applied`;
2. the CLI adds only the 1.0.1 platform tarball, a plain dependency that no
   selected bundle maps any more, so it cannot cause `package-moved`;
3. a second scripted prompt calls `plugin_manager` `install_bundle` with the
   absolute 1.0.1 main tarball; the real result must be `application: applied`
   for bundle `dsh-uml-viewer` version 1.0.1, after which `current.json` must
   read 1.0.1. The manager refuses an installation that changes more than one
   direct dependency, and activation requires the exact 1.0.1 platform package.

The scripted replies live in `upgrade-reply.mjs` with offline unit tests.
Directly replacing a mapped platform package while the bundle is selected
remains unsupported. The host and document keep running throughout.

The case requires the exact old resident PID, process creation identity,
executable path and lease to survive; its canvas pixels and camera state must
survive remount and another canvas gesture must work. It verifies both installed
executables with `--version`, then closes the UML tab with the host's page
close command and reopens it, requires
a new PID on 1.0.1 and checks that the 1.0.0 lease and directory disappear.
Windows write access to the mapped old executable must fail before the update.
The platform CLI installation is bounded at 20 seconds, and the whole
removal-to-install sequence must finish soon enough for the real panel to rejoin within the unchanged 30-second withdrawal grace. A slow
or failed transition fails acceptance; neither the grace nor global test/job
budgets are increased. Evidence is `upgrade-acceptance.json` and the two upgrade
screenshots, under the existing explicit CI artifact whitelist.

CI runs `scripts/ci-upgrade-package.mjs` only after ordinary panel/package checks.
It copies tracked sources to an owned disposable directory, changes that copy's
Cargo authority to 1.0.1, runs the existing npm version writer, rebuilds both
plugin libraries against the copy's contracts and builds the real second native
binary with jobs 4, debug information off and incremental compilation off. Cargo
updates only the copy's lockfile. Reusing the CI target is safe at this point:
all ordinary binary consumers have finished and the 1.0.0 tarballs are already
immutable. The real package producer rejects a binary whose version differs.
The source copy is removed afterward. No release versions, tags or registry
publication are involved. The script refuses execution outside GitHub Actions.

CI12 and CI13 both failed after `install_bundle` answered `applied` 1.0.1: the
lane's `plugin-state/current.json` stayed 1.0.0. CI13's evidence falsified the
CI12 suspects. After the install the profile's `dsh-uml-viewer` and
`@dsh-uml-viewer/win32-x64` links are real directories (realpath = themselves,
not junctions) holding 1.0.1, the virtual store holds no dsh-uml-viewer copy at
all, and the only version store is the lane's `plugin-state` with
`current.json` 1.0.0 and the single `1.0.0` directory, before and after the
install. There is no default `<DSH_HOME>/dsh-uml-viewer` store, so the `--patch`
`home` overlay survives every reconcile (`profileContext.overlays`), and no
surviving 1.0.0 copy exists for a cached resolution to load.

What is left is a forced contract. `plugin_manager` answers `applied` only for
the bundle's required rows, app-boot reports a required row that is not active
as a failure, and a Cordis fiber reaches that state only by running the plugin's
`apply`. The plugin pins `<home>/<version>/` and `current.json` as its first act
in `apply`, so an `applied` install of a packaged 1.0.1 must have pinned 1.0.1.
The store shows it did not, and the artifacts cannot separate the three
remaining causes: the activation took the unversioned branch of
`activationBinary` (`config.binary`, `UML_VIEWER_BIN`, or a private checkout
manifest with a `target/<profile>` binary — these pin nothing and leave the
store exactly like this); the row never re-activated and the tools and prompt
section that returned to the session have another source; or a pin happened and
was undone.

So each activation appends one line to `<home>/activations.jsonl`
(`packages/host/src/activation.ts`; the newest 64 lines, best-effort, never
failing an activation): `at`, `pid`, the pinned `version` or `null`, the
`source` (`pinned` or `unpinned`), the package `root` it resolved and the exact
`argv`. That is a product diagnostic — it answers which build an open panel runs
and why an activation did not pin — and it is the one fact the store cannot
record. `upgrade-evidence.mjs` (offline tests in `upgrade-evidence.test.mjs`)
records `afterRemoval`, `afterPlatform`, `afterInstall` and, when the 1.0.1 poll
fails, `afterPoll` into `upgrade-acceptance.json` before the assertion fails.
Each record lists every version store directly under the scratch DSH home (its
`current.json`, version directories, live leases and activation journal,
including a default `dsh-uml-viewer` store), the real targets and versions of
the profile's main and platform links, every dsh-uml-viewer copy in the virtual
store, and the installed main manifest (`version`, `private`, `main`,
`optionalDependencies`, and which `target/<profile>` binaries exist) — the facts
that decide the packaged versus checkout branch of `activationBinary`. The spec
also keeps the isolated host's own redacted stdout/stderr (`lane.hostOutput()`),
which carries activation warnings and errors that never reach the page, the
session or the DOM. Assertions, polls and budgets are unchanged; a pin that
never happened still fails this case, and the host restart that would produce
one is not performed.

This case has been authored and checked offline; installed-upgrade browser
acceptance remains **NOT RUN until CI executes it**. Source or Node fixture
checks do not establish this outcome.

## Isolation

Every run creates a unique `DSH_HOME` under `.scratch/e2e/run-*`. That home
holds its own `web` profile, sessions, workspace registry, plugin state, mock
request log, failure screenshots and Playwright traces. The host listens on
port 0 and is started with `--no-open`. npm, pnpm store, Corepack and XDG
caches are explicitly rooted under the run home's `cache/`; Chromium uses
the separate scratch browser prefix. The store path is passed explicitly to
DSH's bundled pnpm, because environment-only store configuration was ignored
by Desktop 0.2.2-rc.1. Removal omits `--ignore-scripts`, which pnpm 11 rejects;
the assembled packages contain no lifecycle scripts. Inherited `DSH_PROFILE` and
`DSH_PROFILE_DIR` are removed from CLI/host environment. The harness rejects
any URL whose host is not loopback and rejects port 14227, which is the user's
live GUI.

Before a lane creates a run home, it gives the scratch root its own package
scope: `isolateScratch` writes `<scratch>/package.json`, named
`dsh-uml-viewer-e2e-scratch`, with no `exports` (`scratch-scope.mjs`, offline
tests in `scratch-scope.test.mjs`). By default the scratch root lies inside
this checkout. The checkout's root manifest is the `dsh-uml-viewer` package,
and it has an `exports` map. A user's DSH home never sits under such a package.
DSH continues some profile lookups above the profile, at
`<home>/profiles/package.json`, and runs its own self-reference check from the
profile's base. Without the scope, both resolve `dsh-uml-viewer` through Node's
package self-reference to the checkout's `lib/host.js`. CI14 recorded this
result: after `install_bundle` applied 1.0.1, the re-activation journalled the
checkout root and its `target/debug` binary, unpinned. With the scope, those
lookups miss, as they do for a user. What the upgrade case proves does not
change.

The only model endpoint is `packages/e2e/mock-llm.mjs`, configured through a
per-run `--patch` overlay. It speaks the Messages SSE sequence, logs requests
under the scratch home, and scripts a real two-turn panel flow:

1. `uml_ir` runs through the plugin tool against a sample policy.
2. `uml_display({path: "diagram.edn"})` refreshes an already-open panel.

No real model is called. Each DSH session gets its own browser context, so
cookies and page storage cannot make two sessions share a client-side panel
state.

## CI failure diagnostics

The `panel-e2e` job always uploads `panel-e2e-diagnostics` (seven-day
retention). Its explicit file list includes Playwright results/traces,
evidence screenshots, producer oracle frames, mock request/reply pairs and
lane snapshots. It excludes installed scratch profiles, caches and binaries.

Each lane writes `diagnostics/run-*/page-<n>-open-failed.json` when opening
fails and `page-<n>-close.json` before closing an open page, with screenshots
and `host.log`. JSON records the startup stage, requested project, visible
conversation session ids, panel session ids, summary markup, canvas CSS and
bitmap bounds, recent pointer down/up targets, console/request errors and
mock request count. This also captures failures after `openPanel` succeeds.
The numbered `mock-requests/<n>-reply.json` records the exact reply emitted
for `<n>.json`; input/reply numbering stays paired across concurrent requests.

For a missing generated-response assertion, compare the selected workspace
and conversation in the snapshot with the mock inputs, tool results and
emitted reply. For a selection failure, compare the pointer target and canvas
bounds with the saved producer frame and panel camera summary. Keep the
existing readiness, selection and pixel assertions: these observations are
for diagnosing a failed run, never an alternative acceptance condition.

## Offline fixture checks

`pnpm --filter @dsh-uml-viewer/e2e test` runs Node-only fixture regressions;
it starts no DSH host, browser or native viewer. Workspace selection waits for
the visible conversation's session id to change when the chosen workspace
changes. The chooser label alone updates before the new conversation mounts.
The delayed-transition check keeps those two states separate.

The Shop DPI and metrics-reload cases pan left by the measured inspector width
before asking the producer for a fresh frame. Their previous target was under
the floating inspector, so a raw page click reached the inspector rather than
the canvas. The gesture check replays the CI CSS geometry at both DPR values;
the browser cases still require real selection, resize and metrics changes.
Node fixture checks do not establish browser acceptance.

## Sample projects

`packages/e2e/projects.mjs` copies six TypeScript projects into the
scratch home: `shop`, `ledger`, `deep`, `atlas`, `levels` and `nexus`. `deep` (#23) has
nested namespaces, cross-namespace dependencies and same-named CJK classes
for navigation and search. `levels` (#45) has two namespace levels to drill
into and a proposal whose layer holds a nested group, so the top bar's
breadcrumb path has the same depth in the source tree and in a proposal.
`atlas` is the panel-primitives sample of
#24: an interface, a CJK-named module, two architecture levels and `react` as
a foreign package, all unmeasured. Each has its own `policy.edn`, source files
and proposal where needed. `nexus` (#25) scans six imports from a hub and uses
the supported policy `:edge-kinds` overrides to produce association, dependency,
inheritance, implements, aggregation and composition. A reverse dependency
violates the declared architecture levels; an unrelated observer supplies a
selection that dims every relation. Policy `:edges` is deliberately not used:
the IR command rejects it as generated-IR input. The scripted agent runs the real binary's `ir` subcommand, so the
scene comes from real IR and the real resident, not a checked-in SceneFrame.
The Rust scene-frame goldens remain the contract for the frame validator and
Canvas tests; the lane does not inject their JSON.

The sample workspaces are pre-registered in the scratch home. The first session
uses Shop. The second selects Ledger in DSH's new-session workspace chooser.
For Ledger the lane restores a proposal discussion context from persisted
session state before opening the panel. The panel summary therefore proves the
two sessions are discussing different contexts, not merely displaying the same
default twice.

## Acceptance

`panel.spec.mjs` opens both panels and asserts:

- Shop's panel displays “Shop”; Ledger's displays “Ledger”.
- Shop's context is the source diagram; Ledger's is “Ledger proposal”.
- A real pointer drag on Shop's canvas changes Shop's camera and repaints it.
- Ledger's full summary, including project, discussion context and camera, is
  byte-for-byte unchanged after Shop is driven.
- Neither page emitted a `pageerror`.
- The three-section skeleton (#46): the top bar, the canvas and the status bar
  stack in the narrow sidebar, the canvas sits between the bars and is neither
  behind them nor smaller than the diagram needs; the bar holds the breadcrumb
  path, the discussion context's name, the search button and 「更多」; **Tab**
  reaches the search button and then 「更多」 from the canvas; 「更多」 lists
  Fit view with `Ctrl+0` and the Remove arrows toggle, keeps `role="menu"`
  only while open (a hidden one would be read as a modal by the host's
  shortcut service, #43) and closes on **Esc**.
- A panel opened before any display (`openPanel(name, { display: false })`,
  lane option) shows the waiting canvas and puts the localized
  `waiting-display` text in the status bar, with `data-status` on the
  summary; no chip of text is painted into the canvas.
- Both bars resolve the host's own `--dsw-alias-*` tokens: their computed
  background equals the token's resolved colour in the light theme and in
  the dark theme, so a host theme switch repaints them and neither carries
  a raw colour of its own.

The summary is an accessibility surface. It is visually clipped but is a
`role="status"` with class `uml-viewer-summary`, `data-kind` (`frame` or a
failure kind), `data-context` (`real` or `proposal`), `data-view` (`source` or
`proposal`), `data-depth` (`root` or `drilled`), `data-breadcrumbs` (the path's
length), `data-status` (the canvas status code, empty when there is none),
`data-search`/`data-results`, `data-selected`, `data-chain` and `data-dimmed`,
and text of the form `Diagram: … Discussion: … Camera: x, y, zoom`. It is
populated from the same frame, context and state events that paint the Canvas.
The view and the depth come from the `state` event (#46), not from chrome the
frame no longer carries: `data-view` is `proposal` exactly while the state's
`proposalId` names a proposal layer (the resident drops it on the way back to
the source tree), and `data-depth` is `drilled` once the breadcrumb path has
more than one level. A proposal's root is therefore `proposal`/`root`, a
drilled namespace is `source`/`drilled`, and a proposal viewed from a source
discussion reads `proposal` until **P**. It gives later tickets a user-readable assertion seam
without reaching into client internals.

`primitives.spec.mjs` (#24) is the Playwright form of the deleted egui
class-box driver scenarios (`class_boxes.edn`, `font_parity.edn`,
`lod_shots.edn`). It reads the real panel canvas pixel by pixel and compares
it with the real producer frame for the same view: `canvas.mjs` runs
`uml-viewer scene --frame --wide` with the panel's viewport and the camera
from its summary, so the expected geometry comes from Rust, not from the
painter under test. It asserts the translucent, dashed component box; the
dashed unmeasured class frames; class names, level numbers, the I marker and
«interface» inked at their producer positions; no label past a box edge; the
foreign-package ellipse (canvas in its bounding-box corner, solid ring);
Sarasa advances (CJK 1 em, Latin 0.5 em); and, after real wheel notches to the
`frames` tier, names without the full tier's band and member lines. The egui
italic glyph-vertex assertion has no Canvas counterpart; the italic font
choice is covered by `packages/client/tests/primitives.test.ts`. Dependency
ports appear in drilled-in views: `navigation.spec.mjs` checks the painted
port after a real drill-in, and the unit suite checks the `core-port-selected`
golden.

`navigation.spec.mjs` (#23) replaces the navigation/search driver scenarios:
real double-click drill-in, an in-browser painted dependency-port assertion,
selection with dependency-chain highlighting and dimming, Escape/back, P,
Ctrl+F, CJK typing/paste, same-name namespace disambiguation and Down/Enter
locate. It also covers the top bar's breadcrumb path (#45): two namespace
levels down and clicked back to the middle and root level (with a painted
check that the level's box is what is drawn), then the same in a proposal,
where the path starts at the proposal and its root returns to the proposal
rather than to the source diagram.

`lifecycle.spec.mjs` (#43) drives the host tab lifecycle through the host's
own controls: the tab refresh command and **R** each send `reload`; collapsing
the right sidebar and toggling fullscreen keep the same resident process and
the same view; the UML tab's own close button (`[data-dockkit-tab-close]`)
runs the tab's close handler and the resident exits at once; reopening the tab
restores the view and, for `ledger`, the proposal discussion context. It also
asserts the panel has no `Expand`, `Conversation` or `Close viewer` button.
Residents are observed through `/proc`, so the spec runs on Linux only.

Two host facts the lane had to meet (`packages/e2e/lifecycle.mjs`). DSH
0.2.2-rc.1 binds `page.refresh` and `page.close` only on Desktop and on Windows
and macOS Web (Ctrl+Alt+R / Ctrl+Alt+W); Linux Web has no default, and its
shortcut service admits only Ctrl+/, Ctrl+Shift+, and Ctrl+Shift+., so the page
cannot reach the refresh command until the user saves an override. The lane
therefore writes one into the page's own `dsh.keybindings.v1` preferences
before it loads (`bindRefresh`, Ctrl+Shift+.), which is the document the
shortcut reference writes, and it closes the tab through the host's close
button instead of a key. Note that the host counts any `[role="menu"]` in the
document as an open modal even when it is `hidden`, and a modal blocks the
refresh command, whose modal list is empty; the panel's element menu therefore
carries that role only while it is open (#43, `client/tests/quality-menu`).

`session.spec.mjs` replaces the session/restore scenarios: navigate, select,
zoom and pan; reload the Web client and compare its restored summary; then
stop and boot the real isolated DSH host on the same home and compare again,
without another IR/display call. A proposal case proves discussion-context
restoration too. The process-local IR→display gate stays conservative, as
explained in the ADR 0013 #23 appendix; restoration only replays the saved IR
when its persisted SHA-256 still matches. The host state seam also checks
changed and missing IR, which must reopen waiting.

`edges.spec.mjs` (#25) is the Playwright form of the edge-related egui
settings in `route_library.edn`, `route_proposal.edn`, `route_shots.edn` and
the edge lines visible across `lod_shots.edn` / the triangles half of
`navigation.edn`. It reads the real Nexus panel canvas against an independent
`scene --frame` producer oracle. It checks all six head shapes, continuous
curved shafts, normal relation ink and violating red. Closed-head interiors
must mask the shaft with opaque canvas/ink fill, including when the relation
is dimmed and at the native mesh scale below 0.6. Triangles mode must paint
both incoming and outgoing dependency indicators, retaining red for violations.
The test clicks the isolated Observer class to dim relations, then uses the
「更多」 menu's Remove arrows toggle to reach triangles mode (it names
`:triangles` instead of cycling, #46); no frame is injected.
The Ledger proposal case checks that the frame carries no proposal banner (the
proposal's name is the top bar's discussion context and the path's root, #46),
the proposal's real `api → orders`
shaft at the full and frames tiers, and its disappearance in blocks after
real wheel gestures. The producer supplies geometry for each camera and tier.
Failure evidence and the Playwright trace stay under the run's scratch root.
These cases cover the panel behavior of the deleted fixed-input screenshot
scripts.

## Deleted window drivers (#30)

The standalone egui window, its `viewer` crate, the EDN drivers under
`scripts/drivers/` and `scripts/run_viewer_driver.py` were deleted in
dsh-uml-viewer#30. Each driver maps to the Playwright case that now owns its
behavior. "Retired" marks window-only behavior with no panel counterpart.

| deleted driver | Playwright case | not carried over |
|---|---|---|
| `class_boxes.edn` | `primitives.spec.mjs` › paints class boxes, component boxes, α/I, levels, CJK names and the foreign ellipse like the producer | egui italic glyph-vertex shear (italic font choice: `packages/client/tests/primitives.test.ts`) |
| `detail_scroll.edn` | `overlays.spec.mjs` › class card wheel steps clamp at both ends, keep inspector visible, and resize columns; › inspector follows class selection, switches proposal, and P returns to the namespace tree (empty-inspector help) | right-click Rename/Delete menu: retired, the panel edits no policy; the New Proposal row is now a panel request, `view-requests.spec.mjs` › choosing a proposal tells the companion; 新建提案 sends a timestamp name; all in click order (#29) |
| `dpi_shots.edn`, `dpi_window.edn` | `display.spec.mjs` › scale-2 canvas rasterizes at device pixels, hits at CSS points and follows a resized page | window size limits and the 280 → 340 sidebar drag: retired, the host owns the panel size |
| `font_parity.edn`, `font_parity_baseline.edn`, `fonts.edn` | `primitives.spec.mjs` (names inked inside their producer slots, no label past a box edge, Sarasa CJK/Latin advances) | window font lookup chain and the glyph-coverage probe: retired with the window's font loader |
| `inspector.edn` | `overlays.spec.mjs` › inspector follows class selection, switches proposal, and P returns to the namespace tree; `session.spec.mjs` › restores the proposal discussion context after a DSH restart | proposal rename, its Chinese text input and policy/IR write-back: retired, the companion edits the policy in place (`docs/companion-rules.md`) |
| `lod_shots.edn` | `primitives.spec.mjs` (full → frames tier); `edges.spec.mjs` › proposal relation stays solid in full and frames tiers and disappears in blocks | — |
| `metrics.edn` | `reload.spec.mjs` › a metric-only snapshot edit reloads quality without rewriting the IR | — |
| `navigation.edn` | `navigation.spec.mjs` › drills in and back, selects a dependency chain, searches and returns with P; › anchors Ctrl+wheel zoom, pans by drag and wheel, fits with Ctrl+0 and toggles 移除箭头 from the 「更多」 menu in place | — |
| `review_fixes.edn` | `overlays.spec.mjs` › module source, exact member line, native copy, same-file reuse and independent source windows | per-window retained selections after copying from another widget: the browser owns one document selection |
| `route_library.edn`, `route_proposal.edn`, `route_shots.edn` | `edges.spec.mjs` (all four cases) | before/after screenshots only; they asserted nothing |
| `search.edn` | `navigation.spec.mjs` › drills in and back, selects a dependency chain, searches and returns with P | — |
| `source_jump.edn`, `source_window.edn` | `overlays.spec.mjs` › module source, exact member line, native copy, same-file reuse and independent source windows | — |
| `standalone.edn` | `reload.spec.mjs` › an edited diagram.edn hot-reloads into the open panel | — |
| `terminate.edn`, `window_controls.edn` | none | retired: WM_CLOSE/SIGTERM and undecorated-window close buttons belong to the deleted window; the panel's lifecycle is the host seams' and `package.spec.mjs` |

`display.spec.mjs`, `reload.spec.mjs` and the camera/declutter case of
`navigation.spec.mjs` were authored in #30 under the resource policy and later
ran in CI. #30 is closed; the aggregate browser results remain [partial](#ci).


## Deterministic companion policy acceptance (#29)

`packages/e2e/view-requests.spec.mjs` has three bounded cases. Its test-only
`view-companion.mjs` binds to the lane-owned Deep scratch project during
`prepareProjects`. It implements the companion protocol deterministically:
source/proposal Hide splices the requested `:omit`, and New Proposal inserts
one fresh id, the requested timestamp name and `:layers []`. The fixture edits
once per request before returning `uml_ir({request})`; only a correlated
successful IR result permits `uml_display`. Unknown messages, mismatched
contexts, unsafe names and malformed timestamps refuse without a write.

`view-policy.mjs` contains the deliberately spaced, commented policy. The
fixture recognizes only its literal anchors and known panel request forms;
it is not a general EDN parser or a production policy writer. Prefix and suffix
UTF-8 bytes around each splice are compared explicitly, preserving unrelated
comments, ordering, line layout and the existing proposal. Node tests compare
exact expected policy bytes and exercise retries, refusal, context and failure
gating. Run them offline with `node --test packages/e2e/view-policy.test.mjs`.

Browser assertions cover source/proposal class absence in the real producer
frame after IR/display, source visibility after a proposal-only omit, the new
proposal's actual inspector row, preserved discussion context until an explicit
choice, and untouched composer drafts. A malformed scratch policy exercises
failed real IR: the mock emits no display, the generated IR bytes remain
unchanged, and the displayed summary stays unchanged. These strengthened
browser cases require CI runtime acceptance; local Node tests and Playwright
discovery do not establish a browser pass.

This tests a deterministic companion implementation of the documented
protocol. No external model is called; arbitrary real LLM compliance with
in-place editing or context instructions remains unproven by this harness.

## Harness API for #23–#29

Import from `packages/e2e/lane.mjs`; extend this lane instead of adding another
host bootstrap.

`launchViewerLane(browser, options?)` accepts `packageMain` and
`packagePlatform` (equivalent to the two environment variables), plus an async
`prepareProjects(projects)` hook called after sample creation and before CLI
installation, and `deviceScaleFactor` for a high-DPI browser context. The hook lets lifecycle acceptance seed and snapshot protected
project data before either install or plugin load. It returns:

| member | meaning |
|---|---|
| `home` | run-scoped `DSH_HOME` with projects, logs, traces |
| `host` | `{ url, stop, exited, output }` for the isolated DSH |
| `mock` | `{ url, requests, stop }`; inspect the exact model requests |
| `projects` | `{ shop, ledger, deep, atlas, levels, nexus, inspector }` absolute sample roots |
| `openPanel(name, options?)` | create/select that session, open the sidebar tab, render IR, and return `{ page, panel, canvas, summary, sessionId, before, problems }`; optional `state` overrides the seeded persisted session state before panel open; `display: false` stops before the display prompt, so the panel shows the waiting canvas and its status-bar text (#46) |
| `pan(panel)` | real pointer drag on that panel's canvas |
| `wheel(panel, notches)` | real Ctrl+wheel notches over the canvas centre; positive zooms out (plain wheel pans) |
| `point(panel, [x,y], clicks?)` | real canvas-local click or double-click |
| `reopenPanel(panel)` | reload the Web client and reopen the same session, without IR/display |
| `restart(panel)` | stop and boot the isolated DSH host on the same home, reopen the same session |
| `sendPrompt(panel, text)` | send a visible composer message in that session |
| `screenshot(panel, name)` | write a full-page scratch screenshot |
| `close()` | idempotently close browser contexts, host and mock, in that order |
| `packages` | `{ main, platform }` tarball paths in packaged mode; otherwise `null` |
| `uninstall()` | close the lane, then remove the plugin (and direct platform package in packaged mode) through isolated CLI management |

`panel.before` is the summary text after display; use it to assert that another
session was untouched. Add new assertions to `*.spec.mjs`; set
`UML_VIEWER_E2E_SCRATCH` for an alternate scratch root. On a host with a flaky
registry, set `UML_VIEWER_E2E_COREPACK_SEED` to a known-good Corepack home (for
example `~/.cache/node/corepack`); each lane copies it into its fresh
`COREPACK_HOME` before the plugin install. Failure captures land in
the run home (`<project>-failure.txt` and `.png`), and the Playwright trace in
`.scratch/e2e/results/`.

Lower-level helpers are exported from `dsh-web.mjs` (`startDshWeb`,
`bootDshWeb`, `dismissOverlays`), `mock-llm.mjs` (`startMockLlm`),
`projects.mjs` (`makeProjects`) and `canvas.mjs` (`producerFrame`,
`panelCamera`, `sample`, `inkCount`: canvas pixel reads against the producer
oracle; `producerFrame` accepts optional `focus`, `extra` and `select` EDN
for the same drilled-in, selected and declutter view). A later ticket that needs another project
should add it to `makeProjects` and the workspace registry rather than editing
the host bootstrap.

## Overlay acceptance (#27)

`packages/e2e/overlays.spec.mjs` extends the same isolated lane. Its
`inspector` project is created by `packages/e2e/inspector-project.mjs` and
registered through `makeProjects`; the workspace registry consumes every key
of that mapping automatically. The real mock-model `uml_ir` flow scans two
TypeScript modules, 60 exported operations plus `target`, a dependency, and a
named proposal. The target is a top-level exported function at **line 82**;
class methods would not exercise the current TypeScript declaration lookup.
The file has 84 displayed rows including the trailing empty row.

The cases port the applicable behavior of `source_jump.edn`,
`source_window.edn`, `detail_scroll.edn` and `inspector.edn`:

- The inspector explains an empty selection, refreshes to the selected class
  and its no-data summary, selects the named proposal, and accepts **P** with
  focus still on that proposal button to return to the source namespace tree.
- A real canvas double-click opens the class card at coordinates from
  `producerFrame`, without injecting a frame or invoking a private controller.
  Wheel gestures advance by 48 pixels and clamp at both ends while the
  inspector and diagram camera stay put. End/Home native scrolling followed by
  another wheel notch verifies the DOM and resident offsets stay synchronized.
  Native CSS resize gestures exercise
  horizontal overflow below the 560-pixel plan minimum and column layout at
  the available sidebar width.
- The module row opens the whole file without a marked line. The member row
  reuses that source window, marks exactly line 82, and puts the full row
  inside a genuinely scrolled source viewport. A native word selection and
  copy put `function` on Chromium's clipboard; the line-number gutter remains
  unselectable. A relationship row locates the second module; its source can
  coexist with the first and close independently.
- Switching `body[data-ds-dark-theme]` checks computed inspector, card, source,
  gutter, keyword and border colors against resolved host tokens in both
  modes, and requires the actual surface colors to change.

Proposal creation, rename/delete and regeneration remain #28/#29 scope. Source
window coexistence uses distinct files; repeated opens of the same file reuse
its existing DOM window. The source window is resized to expose card controls
while testing reuse. Browser clipboard permissions are scoped to that isolated
context. No case attaches to the live GUI.

Run only these cases after the plugin and binary have been built and scratch
Chromium has been installed. For the native Windows t27 scratch lane:

```powershell
$env:PLAYWRIGHT_BROWSERS_PATH = '<scratch>/e2e-uml-viewer/browsers'
$env:UML_VIEWER_E2E_SCRATCH = '<scratch>/e2e-uml-viewer'
$env:UML_VIEWER_BIN = '<absolute path to an existing uml-viewer.exe>'
pnpm --filter @dsh-uml-viewer/e2e test:e2e overlays.spec.mjs
```

These cases have run in CI; see the [aggregate results and remaining failures](#ci).

## CI

`.github/workflows/ci.yml` has a `panel-e2e` Windows job that installs scratch
Chromium, builds the binary and runs this suite. All acceptance uses scratch
profiles, not the user's live profile.

| Pinned evidence | Result and boundary |
| --- | --- |
| [CI5 at `f4c12e3`](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42591228938) | Both real-binary `requests.spec.mjs` cases passed (#28 closed). Real two-session isolation and restart/view restoration also passed (#22 closed). |
| [CI6 panel job at `6147980`](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42595826593/job/129088076561) | Ordinary suite: 22 passed, 3 failed, 1 skipped. The three test-only corrections are in `7fbace6`; this result does not establish their pass. |
| Same CI6 job, dedicated `package.spec.mjs` step | Real current-source tarball install/display/uninstall: 1/1 passed, including protected project bytes and mtimes (#31 closed). |
| [CI7 at `7fbace6`](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42600985418) | Main workflow still in progress when checked on 2026-11-09; no final browser result claimed. It also includes the strengthened #29 policy-edit cases described above. |

Real installed-profile update acceptance remains **UNVERIFIED** (#32).
The scripted companion proves only its deterministic protocol behavior;
actual LLM compliance has not been tested.
