# Plugin host development

Ticket dsh-uml-viewer#19 adds the host skeleton and `uml_ir`. The quality tools
and their gates follow in #20; the Web panel, Remote stream and `uml_display`
follow in #21. This checkout is a development package, marked private until the
release packaging tickets assemble the main and platform packages.

Use Node 24 (or Node 22.19+) and the pnpm version pinned in `package.json`:

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test
pnpm build
```

The root is the main package `dsh-uml-viewer`. `packages/host` and
`packages/contracts` are private workspace packages, bundled into
`lib/host.js` by `scripts/build-plugin.mjs`. Cordis is supplied by DSH at runtime.
The build runs without dependency lifecycle scripts; esbuild uses its platform
optional dependency. CI runs tsc, unit tests, the Cordis seam tests and the host
build on Windows, Linux and macOS alongside the existing Rust checks.

## Binary convention

The host chooses the executable at call time, in this order:

1. The trusted Loader row's `binary` config, when set.
2. `UML_VIEWER_BIN`, an **absolute executable path**, with `.exe` on Windows.
3. `target/debug/uml-viewer` then `target/release/uml-viewer` under the **plugin
   checkout**, with `.exe` on Windows.

There is no PATH or legacy `.uml-viewer/` fallback and no automatic build. A
missing binary produces an actionable tool result. To use a separate Cargo
output directory, set `UML_VIEWER_BIN` explicitly. For example:

```powershell
$env:UML_VIEWER_BIN = 'D:\dev\github\dsh-uml-viewer-wt\t19\target\debug\uml-viewer.exe'
```

The config accepts an absolute executable string, or a trusted argv prefix such
as `["/absolute/node", "/absolute/fake-uml-viewer.mjs"]` for seam fixtures. Neither
config nor environment strings are shell commands. Model arguments can select
only policy/output; they cannot change binary, cwd, environment or authority.
Platform optional dependencies and DSH-home version pinning belong to the
release packaging ticket; this development locator does not implement them.

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
execution; `runner.ts` composes IR arguments and the tool result;
`binary.ts` owns executable location; `policy.ts` owns default policy selection;
`index.ts` owns the tool definition and effect lifetime. #20 can extend the
runner with operation-specific args/result parsing and per-session
serialization, then wrap the IR handler with quality gating. #21 can add the
resident owner, session projection of tool-result events, panel Remote and
`uml_display` gating. Gate state must derive from the session log so it survives
resume/fork; a process-local last-result map is not authoritative.

## Manifests and verification

DSH reads `package.json`'s `dsh.bundle.patch`, runtime export and `engines.dsh`.
`dsh-plugin.json` is the informational community manifest, not an authorization
mechanism. The patch inserts exactly one row named like the package. The
manifest lists `uml_ir` as implemented and records the four follow-on tools as
planned; #21 adds the client facet/export and `dsh.client` metadata when a real
client bundle exists. English/Chinese inventory text and original SVG artwork
are included. No live profile was installed or changed for this ticket.

The seam tests load the host into a real Cordis 4.1 Context, provide controlled
DSH services, and spawn a real fake binary through the subprocess seam. They
check actual argv/cwd, results for exits 0/1/3, refusal, confinement, authority
changes and unload cleanup. `.uml-viewer/` fixtures exist **before plugin
load**; nested bytes and nanosecond mtimes are compared after calls and unload
on success, failure and refusal. These are plugin seam tests, not a claim that
the fake confinement provider enforces an OS sandbox.

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
