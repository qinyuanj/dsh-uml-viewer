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

`quality.spec.mjs` converts the quality/highlight scenarios to the existing lane:
independent C/M dots and quality fills, missing CRAP/mutation/both data, red no-data
ink, unchanged quality/violation hues with changed host skin tokens in both modes,
resident selection/chain/dimming, local relation/class hover, and the overview
class-name popup clearing on leave. `edges.spec.mjs` also checks that dimming
preserves opaque hollow-head interiors. The producer oracle accepts `hover` and
`pointer` as well as selection. No frames are injected.

Under the current resource restriction these cases are **authored, NOT RUN**:
there is no existing matching binary. Do not follow the build commands above to
make one. A later authorized run must provide an existing matching executable and
set **both** environment variables for browser installation and tests:

```powershell
$env:UML_VIEWER_E2E_SCRATCH = 'D:/dev/github/spec203-notes/scratch-t26'
$env:PLAYWRIGHT_BROWSERS_PATH = 'D:/dev/github/spec203-notes/scratch-t26/browsers'
$env:UML_VIEWER_BIN = 'D:/absolute/path/to/existing/uml-viewer.exe'
# Only when a matching existing binary is available:
pnpm e2e:install
pnpm --filter @dsh-uml-viewer/e2e test:e2e quality.spec.mjs
```

Changed host tokens exercise the skin boundary without installing a skin in the
user's live profile. This does not claim an installed dsh-claude-style browser
run. Edge-hover dependency-list popups and intermediate `via-ids` hover remain
unavailable because the current producer frame carries neither metadata field;
raw child-row hover also lacks a separate hit record (see ADR 0013 #26 appendix).

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

The only model endpoint is `packages/e2e/mock-llm.mjs`, configured through a
per-run `--patch` overlay. It speaks the Messages SSE sequence, logs requests
under the scratch home, and scripts a real two-turn panel flow:

1. `uml_ir` runs through the plugin tool against a sample policy.
2. `uml_display({path: "diagram.edn"})` refreshes an already-open panel.

No real model is called. Each DSH session gets its own browser context, so
cookies and page storage cannot make two sessions share a client-side panel
state.

## Sample projects

`packages/e2e/projects.mjs` copies five TypeScript projects into the
scratch home: `shop`, `ledger`, `deep`, `atlas` and `nexus`. `deep` (#23) has
nested namespaces, cross-namespace dependencies and same-named CJK classes
for navigation and search. `atlas` is the panel-primitives sample of
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

The summary is an accessibility surface. It is visually clipped but is a
`role="status"` with class `uml-viewer-summary`, `data-kind` (`frame` or a
failure kind), `data-context` (`real` or `proposal`), and text of the form
`Diagram: … Discussion: … Camera: x, y, zoom`. It is populated from the same
frame and context events that paint the Canvas. It gives later tickets a
user-readable assertion seam without reaching into client internals.

`primitives.spec.mjs` (#24) is the Playwright form of the egui class-box
driver scenarios (`scripts/drivers/class_boxes.edn`, `font_parity.edn`,
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
locate. It also expands the sidebar to the root-scoped `main` page, asserts
its original session id, and returns to the conversation.

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
accessible declutter control to reach triangles mode; no frame is injected.
The Ledger proposal case checks the proposal banner, its real `api → orders`
shaft at the full and frames tiers, and its disappearance in blocks after
real wheel gestures. The producer supplies geometry for each camera and tier.
Failure evidence and the Playwright trace stay under the run's scratch root.
Native fixed-input screenshot scripts remain available for the egui shell;
these cases cover the corresponding panel behavior.


## Harness API for #23–#29

Import from `packages/e2e/lane.mjs`; extend this lane instead of adding another
host bootstrap.

`launchViewerLane(browser, options?)` accepts `packageMain` and
`packagePlatform` (equivalent to the two environment variables), plus an async
`prepareProjects(projects)` hook called after sample creation and before CLI
installation. The hook lets lifecycle acceptance seed and snapshot protected
project data before either install or plugin load. It returns:

| member | meaning |
|---|---|
| `home` | run-scoped `DSH_HOME` with projects, logs, traces |
| `host` | `{ url, stop, exited, output }` for the isolated DSH |
| `mock` | `{ url, requests, stop }`; inspect the exact model requests |
| `projects` | `{ shop, ledger, deep, atlas, nexus }` absolute sample roots |
| `openPanel(name, options?)` | create/select that session, open the sidebar tab, render IR, and return `{ page, panel, canvas, summary, sessionId, before, problems }`; optional `state` overrides the seeded persisted session state before panel open |
| `pan(panel)` | real pointer drag on that panel's canvas |
| `wheel(panel, notches)` | real Ctrl+wheel notches over the canvas centre; positive zooms out (plain wheel now pans, matching egui) |
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
`UML_VIEWER_E2E_SCRATCH` for an alternate scratch root. Failure captures land in
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
$env:PLAYWRIGHT_BROWSERS_PATH = 'D:/dev/github/spec203-notes/scratch-t27/browsers'
$env:UML_VIEWER_E2E_SCRATCH = 'D:/dev/github/spec203-notes/scratch-t27'
$env:UML_VIEWER_BIN = 'D:/dev/github/dsh-uml-viewer-wt/t27/target/debug/uml-viewer.exe'
pnpm --filter @dsh-uml-viewer/e2e test:e2e overlays.spec.mjs
```

These are authored acceptance cases; adding them does not claim a browser run.

## CI

`.github/workflows/ci.yml` has a `panel-e2e` Windows job that installs a
scratch Chromium, builds the binary and runs this suite. It is configuration
only: it has not been run on GitHub from this worktree. Local execution on this
Windows host is the evidence for #22.
