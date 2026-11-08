# Browser acceptance harness

The browser lane drives a real DeepSeek Harness (DSH) Web instance and the real
viewer panel. It is the seam for #22–#29: `packages/e2e/panel.spec.mjs` asserts
what the panel displays, not a recording Canvas or a stubbed panel controller.

## Prerequisites

- Node.js 22.19+ or 24, and the pnpm version pinned in `package.json`.
- The DSH 0.2.2-rc.1 Desktop CLI or an equivalent CLI on `PATH`. Set `DSH_CLI`
  to its absolute path when it is not discoverable. On this Windows host the
  harness finds the installed Desktop CLI itself.
- A real `uml-viewer` binary. Build it with `cargo build --locked -p
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

## Isolation

Every run creates a unique `DSH_HOME` under `.scratch/e2e/run-*`. That home
holds its own `web` profile, sessions, workspace registry, plugin state, mock
request log, failure screenshots and Playwright traces. The host listens on
port 0 and is started with `--no-open`. The harness rejects any URL whose host
is not loopback and rejects port 14227, which is the user's live GUI.

The only model endpoint is `packages/e2e/mock-llm.mjs`, configured through a
per-run `--patch` overlay. It speaks the Messages SSE sequence, logs requests
under the scratch home, and scripts a real two-turn panel flow:

1. `uml_ir` runs through the plugin tool against a sample policy.
2. `uml_display({path: "diagram.edn"})` refreshes an already-open panel.

No real model is called. Each DSH session gets its own browser context, so
cookies and page storage cannot make two sessions share a client-side panel
state.

## Sample projects

`packages/e2e/projects.mjs` copies two distinct TypeScript projects into the
scratch home: `shop` and `ledger`. Each has its own `policy.edn`, source files
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

This ticket does not assert survival across a DSH restart. The `uml_display`
gate is process-local (#21 residual); cross-restart restoration belongs to #23.

## Harness API for #23–#29

Import from `packages/e2e/lane.mjs`; extend this lane instead of adding another
host bootstrap.

`launchViewerLane(browser, options?)` returns:

| member | meaning |
|---|---|
| `home` | run-scoped `DSH_HOME` with projects, logs, traces |
| `host` | `{ url, stop, exited, output }` for the isolated DSH |
| `mock` | `{ url, requests, stop }`; inspect the exact model requests |
| `projects` | `{ shop, ledger }` absolute sample roots |
| `openPanel(name)` | create/select that session, open the sidebar tab, render IR, and return `{ page, panel, canvas, summary, before, problems }` |
| `pan(panel)` | real pointer drag on that panel's canvas |
| `sendPrompt(panel, text)` | send a visible composer message in that session |
| `screenshot(panel, name)` | write a full-page scratch screenshot |
| `close()` | close browser contexts, host and mock, in that order |

`panel.before` is the summary text after display; use it to assert that another
session was untouched. Add new assertions to `*.spec.mjs`; set
`UML_VIEWER_E2E_SCRATCH` for an alternate scratch root. Failure captures land in
the run home (`<project>-failure.txt` and `.png`), and the Playwright trace in
`.scratch/e2e/results/`.

Lower-level helpers are exported from `dsh-web.mjs` (`startDshWeb`,
`bootDshWeb`, `dismissOverlays`), `mock-llm.mjs` (`startMockLlm`) and
`projects.mjs` (`makeProjects`). A later ticket that needs another project
should add it to `makeProjects` and the workspace registry rather than editing
the host bootstrap.

## CI

`.github/workflows/ci.yml` has a `panel-e2e` Windows job that installs a
scratch Chromium, builds the binary and runs this suite. It is configuration
only: it has not been run on GitHub from this worktree. Local execution on this
Windows host is the evidence for #22.
