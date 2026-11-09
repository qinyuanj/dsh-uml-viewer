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

A panel stream ending is **not** a panel close (D13). The gateway aborts the
method signal and returns the stream whenever the browser half is unloaded, a
carrier reconnects, or the panel moves between the sidebar and the main page,
so the resident is kept for one bounded window (`WITHDRAWAL_GRACE_MS`, 30 s)
waiting for the same session to re-attach. A remount inside that window joins
the waiting child: same resident, same version, still one child and one lease.
The panel's own `close` call, the window expiring with nobody attached, plugin
uninstall and DSH shutdown stop it (each saves the view first), and only then
does a later open start vN+1. So an in-session remount keeps vN; a **full page
reload or app restart is a fresh open and legitimately runs vN+1**.

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

Each call resolves `ctx.sandboxPolicy` with the actual session. Read-only
refuses with a reason and no child. Workspace-write uses
`ctx.sandbox.confine(argv, policy, signal)` and spawns only the returned argv;
a missing, failing or invalid confinement provider never falls back to raw
execution. Full access uses raw argv. After asynchronous preparation, the host
re-resolves and compares the entire policy before spawning. Policy changes
require a fresh invocation. The plugin never requests elevation.

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
`Symbol.for('dsh-uml-viewer.panel-state.v1')`, so any change to the pooled
`ResidentPool`/`ResidentConnection` shape must bump that version.

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
gateway leaves it) with the same session remounting on the same vN child, an
explicit close stopping it, root shutdown clearing a pending withdrawal window,
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
