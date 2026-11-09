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

`packages/e2e/projects.mjs` copies three TypeScript projects into the
scratch home: `shop`, `ledger` and `atlas` (the panel-primitives sample of
#24: an interface, a CJK-named module, two architecture levels and `react` as
a foreign package, all unmeasured). Each has its own `policy.edn`, source files
and proposal. The scripted agent runs the real binary's `ir` subcommand, so the
scene comes from real IR and the real resident, not a checked-in SceneFrame.
The Rust scene-frame goldens remain the contract for the frame validator and
Canvas tests; the lane does not inject their JSON.

The two workspaces are pre-registered in the scratch home. The first session
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
ports appear only in a drilled-in view, which the panel cannot reach until
#23 adds drill-in, so they are asserted against the `core-port-selected`
golden in that unit suite.

This ticket does not assert survival across a DSH restart. The `uml_display`
gate is process-local (#21 residual); cross-restart restoration belongs to #23.

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
| `projects` | `{ shop, ledger }` absolute sample roots |
| `openPanel(name)` | create/select that session, open the sidebar tab, render IR, and return `{ page, panel, canvas, summary, before, problems }` |
| `pan(panel)` | real pointer drag on that panel's canvas |
| `wheel(panel, notches)` | real wheel notches over the canvas centre; positive zooms out |
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
oracle). A later ticket that needs another project
should add it to `makeProjects` and the workspace registry rather than editing
the host bootstrap.

## CI

`.github/workflows/ci.yml` has a `panel-e2e` Windows job that installs a
scratch Chromium, builds the binary and runs this suite. It is configuration
only: it has not been run on GitHub from this worktree. Local execution on this
Windows host is the evidence for #22.
