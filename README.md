# UML viewer
## Abstract

This project displays a project as UML. The UML is *dynamic*. You can click
elements to drill down — all the way to source code. You can also play
what-if games by proposing architecture changes.

The app is coupled to an agent assistant. The two communicate, so you can
display the UML, drill down, tell the agent what you don't like, and watch
it make the changes. Describe a proposal and the agent will build a new
diagram. Keep revising that proposal until you like it, then tell the
agent to make the code match.

Color coding comes from CRAP and mutation metrics. You can adjust the
thresholds in config if you like.

The viewer and agent treat namespaces as components. Modules inside those
namespaces are the components' elements. Nesting can be arbitrarily deep.
You build an architecture by moving modules into the right namespaces with
the right dependencies.

You choose architectural levels by telling the agent what coupling and
cohesion criteria to use. For example, I tell my agent to group modules
into components by the Common Closure Principle.

Dependencies that point from a higher level to a lower level are colored
red, in accordance with the Dependency Rule of Clean Architecture.

The bottom line: you can view the structure of your code, see how well
tests cover it, play what-if games, and manipulate the system from a high
level.

## Tech

A Rust app (egui/eframe) that lays out and draws UML from an EDN IR. A
**policy** plus a language-specific scanner writes the topology; this tool
displays it, routes the arrows, colors CRAP, and lets you click. One Rust
binary runs the viewer, IR generation, the four scanners, and the companion's
headless roles. The JVM/Clojure implementation was removed in the JVM cutover
and the Lua/LOVE implementation in the #144 cutover (ADR 0008). The supported platforms are **WSL2 + Ubuntu 24.04 + WSLg** and
**macOS** (Metal, Terminal.app attach); native Windows is unsupported (ADR 0007).

The IR is **topology**: the namespace tree, classes, and edges. **Metrics**
(CC, coverage, CRAP, killed/survived/uncovered) come from `.metrics/`
snapshots produced by [crapper](https://github.com/qinyuanj/crapper) and
[mutator](https://github.com/qinyuanj/mutator). The viewer overlays
those files at load, keyed by namespace and function name. Agents edit the
policy, not the IR. See [Policy](#policy).

[dryer](https://github.com/qinyuanj/dryer) is the third quality tool. It
reports structurally duplicated forms in `.metrics/dry.edn`; the viewer does
not overlay that snapshot yet, so `./uml dry` is a report you read from the
terminal.

## Run

Needs the Rust toolchain pinned by `rust-toolchain.toml` (1.99.1 with
`rustfmt` and `clippy`), `git`, Herdr >= 0.9.3, and Python 3.11+. There is no
Java, Clojure, Lua, LOVE, cimgui or PowerShell dependency.

From **this repo** (the viewer, IR generator, scanners and companion roles
are the Cargo workspace):

```sh
cargo build --release --locked
./uml ir ./examples/uml-viewer.policy.edn
./uml ./examples/uml-viewer.edn
cargo test --workspace --locked          # pure and integration suites
git config core.hooksPath .githooks      # once per clone: enable the pre-commit hook
```

The versioned `.githooks/pre-commit` runs `cargo fmt --all -- --check` and
`cargo clippy --workspace --all-targets --locked -- -D warnings`; a failure
blocks the commit.

From **any project** you want to view (no local uml-viewer checkout
needed):

```bash
cd /path/to/the-project
curl -fsSL https://raw.githubusercontent.com/qinyuanj/uml-viewer/main/scripts/get-uml-viewer -o get-uml-viewer
chmod +x get-uml-viewer
./get-uml-viewer               # fetch, write ./uml, start
./uml                          # later fresh starts (returns immediately)
./uml --restart                # companion only: new Rust process, restore last view
./uml ir                       # policy → IR
./uml crap                     # .metrics/crap.edn for the declared tree
./uml mutate 'src/file with spaces.rs'  # differential mutation of these files
```

`scripts/get-uml-viewer` is in this repo. It clones uml-viewer into
**gitignored** `.uml-viewer/uml-viewer/` (a nested `.git` there is invisible
to the project's repo), writes `./uml`, and starts the viewer. `--install-only`
skips the start. `UML_VIEWER_REPO_URL` and `UML_VIEWER_REF` override the clone
source (default `qinyuanj/uml-viewer`, branch `main`). Installation and analysis
use the native POSIX entry (`scripts/get-uml-viewer`), which runs Python
directly and performs every step with literal argv; no PowerShell is
required. The Rust toolchain, git, Herdr, and Python 3.11+ are user-installed
prerequisites. Install the pinned toolchain with `rustup toolchain install
1.99.1`; the installer runs `cargo build --release --locked` in
`.uml-viewer/uml-viewer/` and records the resulting binary in
`.uml-viewer/install.json`. There is no LOVE, Lua, cimgui or LuaJIT
provisioning any more (#144). Upgrading from the LOVE version: every install
deletes the `cimgui-love-*` directories the old installer provisioned directly
under `.uml-viewer/` (real directories holding `cimgui.so`, `cimgui.dll` or
`cimgui/init.lua`), whatever `install.json` records, and names each removed
path on stderr. Symlinks are not followed, unmarked directories of that name
and everything else stay, and an install refused because of live runtime
state deletes nothing (#167). Live means held companion ownership, a
recovery not in a terminal phase (`failed`, `cancelled`, `normal`) or the
identity of a viewer still running; the records a normal close leaves do not
block an install. The installer leaves two things for you to
remove by hand: `~/.local/share/love/uml-viewer/` (LOVE's own save directory,
for example `imgui.ini`) and the apt `love` / `lua` packages, if nothing else
uses them.
The installer also provisions Sarasa Mono SC Regular v1.0.42: it downloads the
pinned TTF release, verifies the archive and face SHA256 (the pins from #82),
and extracts only that face into `.uml-viewer/fonts/`; any failure exits
non-zero. A download with no data for 60 seconds fails naming the URL and the
timeout, and writes no font. Upstream ships the release only as 7z, so extraction uses `py7zr` in
`.uml-viewer/installer/.venv` (created on first use) — no system 7z package is
needed. The viewer's font lookup is an explicit path, then `UML_CJK_FONT`, then
that provisioned Sarasa, then a system CJK font (Noto CJK on Ubuntu) with a one-time stderr warning naming `get-uml-viewer`.
Install Herdr >= 0.9.3 from
https://github.com/herdrdev/herdr/releases. The installer preserves existing
policies and proposals and does not create a policy or generate a diagram
during `--install-only`.

Download `scripts/get-uml-viewer` from the same ref, then run from the
project:

```sh
sh ./get-uml-viewer --install-only
./uml ir
./uml crap
./uml mutate 'src/file with spaces.lua'
```

The installer writes a POSIX `uml` entry that forwards to the installed
Python command module. Arguments are passed as a literal argv: no shell
re-parses them, so spaces, Unicode and shell metacharacters reach the tool
unchanged.
`UML_VIEWER_TOOL_ROOT` overrides the tool checkout the entry runs: point it at
a working tree (for example this repository) to launch its Cargo workspace
instead of the pinned `.uml-viewer/uml-viewer/` clone. The path must contain
`Cargo.toml`; anything else fails and names the variable. When the selected
tool root is a different git checkout at a different commit, startup prints a
one-line stderr warning with both short SHAs and never blocks the launch.
`UML_VIEWER_RUST_EXECUTABLE` selects the built binary (default
`<tool root>/target/release/uml-viewer`). The default binary is rebuilt with
`cargo build --release --locked` before use whenever a crate source, a
manifest, `Cargo.lock` or `rust-toolchain.toml` is newer than it, so a
relaunch never runs code older than the tree; the Herdr backend preflight reads one
line from its stdout. The viewer launcher starts that binary with individual
arguments and writes its output to `.uml-viewer/viewer.log`.
Fresh startup uses **Herdr as the sole companion backend** on WSL.
Historical Windows and macOS evidence remains historical.
`--backend herdr` remains accepted; `--backend tmux` and `--backend psmux` are
rejected before creating resources. Restart, wake and
close require explicit Herdr ownership and verify its recorded identities.
Legacy or unmarked ownership is retained and refused; see
[legacy migration](docs/herdr-backend-contract.md#9-旧后端退役与迁移).
Startup checks the preinstalled CLI is **>= 0.9.3** and the bundled protocol is **22**
without starting a server, terminal, companion or detached Viewer. With several
`herdr` executables on `PATH`, the highest compatible release is used for every
Herdr call ([ADR 0004](docs/adr/0004-herdr-executable-selection.md)). Install from
[the official release](https://github.com/herdrdev/herdr/releases)
or [herdr.dev](https://herdr.dev), then verify the version.
There is no download, upgrade or fallback. `--backend herdr` creates an isolated
named session and records its server, shell, pane and process creation identities.
On WSL the companion opens a Windows Terminal window running
`herdr session attach <session>` through `wt.exe` interop. Without `wt.exe`, or
when it fails to start, the viewer's status line, the ready event and the project
log carry that command to run in a separate terminal (#49). Do not
start a second companion to retry attachment. Herdr starts a detached project recovery
controller. A verified abnormal Claude exit is retried at most three times,
after 1, 2 and 4 seconds; the count resets only after 60 seconds of verified
continuous child uptime. A normal exit, unknown exit evidence, or lost pane
ownership stops recovery and retains mail with a diagnostic. Closing the
diagram revokes recovery and waits for the controller to exit before removing
only this project's Herdr resources. Recovery needs an executable that can be
`exec`ed directly (`claude`, or `node` plus the official `cli.js`); script
wrappers cannot prove the child identity. Windows native is no longer a
delivery platform ([ADR 0007](docs/adr/0007-posix-platforms-drop-windows-native.md));
its earlier Herdr records stay historical in
[the #30 baseline](docs/windows-herdr-30-baseline.md). Historical backend
evidence remains in [platform acceptance](docs/platform-acceptance.md).
A standalone downloaded installer script fetches its entry point from
`UML_VIEWER_REF`; for custom hosts, keep the checkout layout or set
`UML_VIEWER_INSTALLER_URL` explicitly.

The Rust binary must be built for the host architecture; the installer
verifies the pinned toolchain before building. There is no JVM, Clojure, Lua or
LOVE requirement. A launcher record from a removed runtime (`love` or `jvm`)
is refused; fresh-start the viewer instead.

`CLAUDE_BIN` selects the companion program; it defaults to `claude` on PATH.
The runner execs it directly so its recorded identity is the agent's own
process, and argv is passed literally, never through a shell. It also sets
`UML_COMPANION_ROOT` to the project root for the agent: inside a companion,
`./uml` refuses a fresh start and any lifecycle entry for another directory,
and allows only `./uml --restart` of its own project.

The companion can be Codex CLI instead of Claude Code:
`./uml --coding-agent codex` (fresh start; `./uml --restart` reuses the
recorded provider, and an explicit conflicting choice is refused). The option
belongs only to the lifecycle entries; `ir`, `scene`, `crap`, `mutate` and
`dry` refuse it. Before creating anything the preflight requires `CODEX_BIN`
(a literal path) or `codex` on PATH reporting `codex-cli` **>= 0.166.0**,
`codex login status` exit 0, `codex --enable hooks features list` reporting
`hooks` `true`, the selected Herdr **>= 0.9.5**, and
`herdr integration status` reporting `codex: current`. Nothing is installed,
logged in or configured for you, and a refusal never falls back to Claude.
Herdr's Codex hook must also be trusted in your own Codex config. No Stable
read-only Codex interface reports hook trust (ADR 0009), so the preflight
cannot check it: an untrusted hook shows up as a 45-second identity timeout
that names it as the likely cause, after which this start's Herdr session is
released. Run `codex` yourself, review its hooks and trust Herdr's
SessionStart hook, then start again. The companion runs the Stable interactive
TUI with `--dangerously-bypass-approvals-and-sandbox --enable hooks
--no-daemon -C <project>`, the standing rules as `developer_instructions`,
and a per-invocation trust of the project root
(`-c projects={"<root>"={trust_level="trusted"}}`), so a git project's
"Trust this folder?" screen does not stop an unattended start; Codex saves
none of these and `~/.codex/config.toml` is not modified. Ownership records
`:coding-agent :codex` and the session UUID Herdr reports. Restart rebinds
the same Codex process and UUID; recovery after a verified abnormal exit runs
`codex resume <options> <saved uuid>`, never `--last` or a picker. Starting a
new conversation in the companion pane (`/new`) breaks the companion: once the
new conversation reports its own UUID, wakes are refused and mail stays
queued until you close the viewer. WSL2 acceptance with real Codex 0.166.0 and
Herdr 0.9.5 is in
[acceptance-evidence/199-wsl-codex/](acceptance-evidence/199-wsl-codex/README.md).

Every project gets
[crapper](https://github.com/qinyuanj/crapper),
[mutator](https://github.com/qinyuanj/mutator) and
[dryer](https://github.com/qinyuanj/dryer) cloned side by side into
`.uml-viewer/` (branch `main`; override with `CRAPPER_REPO_URL`,
`CRAPPER_REF`, `MUTATOR_REPO_URL`, `MUTATOR_REF`, `DRYER_REPO_URL`,
`DRYER_REF`). The installer always
fetches all three tools, including in repositories that still contain a `deps.edn`
from the removed Clojure implementation. It then provisions each dispatcher
path: `.uml-viewer/<tool>/.venv` with the tool installed editable. Failure to
fetch any tool is fatal instead of a warning. They need Python 3.11+
(select the interpreter with `UML_PYTHON_EXECUTABLE` when needed). Re-run
`get-uml-viewer --install-only` to fetch updates; the editable installs use
the updated source. Each tool runs from its own `.venv/bin/python -m <tool>`;
sibling checkouts are not on `PATH` and are not used by the dispatcher. Keep a
WSL project in the Linux home directory rather than `/mnt/c`. The quality tools
analyze the project's own language; this repository is Rust. crapper generates
Rust coverage with `cargo llvm-cov` (installed into `~/.cargo` on first use when
absent) and mutator uses `cargo test`. No Lua, Busted or LuaCov setup is needed
for Rust analysis. See
[mutator's setup and limitations](https://github.com/qinyuanj/mutator/tree/main).

Analysis always runs from the examined project, forwards individual arguments,
and preserves the tool's exit status. Standard crapper/mutator/dryer invocation
is the primary quality route; a repository command override is an optional
extension. The installed standard tools accept CLI options rather than loading
persistent business-scope settings. `uml-quality.json` can persist these options
through `standardTools`, independently of optional executable overrides in
`operations`:

```json
{"version":1,"standardTools":{
  "sourceRoots":["business tree","common"],
  "exclude":["business tree/generated","**/vendor"],
  "crapperOptions":[],
  "mutatorOptions":["--max-workers","3"],
  "dryerOptions":["--threshold","0.9"]
}}
```

Every field is an optional array of nonempty strings. Unknown fields fail.
`standardTools` is the primary quality route. There is no Clojure alias route
after the cutover; without a declaration the standard tools inspect the whole
project. Explicit `operations` take priority
and still require complete valid command declarations; `crap` and `mutate`
remain mandatory there, while a `dry` entry is optional and leaves the standard
dryer in place. Roots resolve from the
project root and must be existing directories inside it. Exclusions are anchored
at the project root, use either separator, and forbid absolute paths or `..`.
`*` and `?` match within one segment; `**` matches whole directory segments,
including zero segments; matching a directory excludes descendants. Matching is
case-sensitive. Tool option arrays cannot declare owned routing flags (`--root`,
`--source-root`, `-s`, `--exclude`, `--full-tree`, `--changed`, `--mutate-all`,
`--edn`, `--text`).
Scope fields own roots/exclusions; only explicit refresh arguments request full
mutation. Refresh arguments cannot override configured roots/exclusions.

With standard scope configured, `./uml mutate` passes the declared roots and
exclusions to Mutator as `-s` and `--exclude` and forwards its targets without
expanding them: either `--changed`, forwarded as the flag, or named files and
directories, each forwarded once as its project-relative path, in the order
named. `--changed` together with a named target fails (exit 1) without
launching a tool or publishing a record, because Mutator would mutate only the
changed set and ignore the names; run them separately.
A named file outside every root or matching an exclusion is not forwarded, nor
is a named directory that is excluded or that neither lies inside a root nor
contains one; each gets this line on stderr:
`uml: omitted mutation target outside business scope: <path>`. Missing paths,
symbolic links and paths outside the project fail; scoped refresh does not accept textual path filters. A call with neither
`--changed` nor a remaining target fails (exit 1) without launching a tool or
publishing a record. Mutator decides which files in a forwarded directory or
changed set are sources, by crapper's selection rules, and exits 0 with `No
source files to mutate.` when none remain. Mutation retains differential
behavior and snapshots.

When it passes `-s` or `--exclude`, `./uml mutate` or `./uml dry` first runs
`<tool> --help`. If the installed Mutator or Dryer does not list `--exclude`,
the call fails (exit 1) and asks for `get-uml-viewer --install-only`; it does
not launch the tool or publish a record. An older tool would reject
`--exclude`, or mutate the whole `-s` tree instead of the named file. Without
declared roots or exclusions nothing is probed.

Without persistent configuration, supply scope flags per invocation:

```sh
./uml crap --source-root 'business tree' --source-root common --exclude 'business tree/generated'
./uml mutate 'business tree/file with spaces.rs' --max-workers 3
./uml dry --threshold 0.9
```

External `crap` adds `--full-tree`: crapper publishes every class under the
specified roots in one `.metrics/crap.edn` snapshot. Roots default to the project
root, must exist inside it, and exclusions are project-relative paths or globs.
The tool rejects positional class paths, `--changed` and existing-coverage reuse
in a full-tree refresh. A failed refresh preserves the previous snapshot rather
than replacing other classes' metrics with a partial result. `--no-coverage`
explicitly requests complexity only; it does not establish fresh coverage.

Mutator accepts repeated `-s`/`--source-root` and `--exclude` and selects files
by crapper's rules. With `-s`, named paths and `--changed` are narrowed to the
source roots: a named file outside every root is dropped with
`Skipping <path>; it is outside the source roots` on stderr, files reached through a named
directory or `--changed` are dropped silently, and `--exclude` applies
afterwards. Given both `--changed` and named paths, Mutator takes only the
changed set. Without a `standardTools` declaration no `-s` or `--exclude` is
passed and the scope is the project root; declare roots/exclusions or supply
changed business files explicitly. Crapper exclusions alone do not apply to
mutation.
`mutate` stays
differential; only explicitly passing `--mutate-all` requests all sites. Passing `--max-workers 3`
to `./uml mutate src/calc/init.lua` caps parallel workers without changing
differential selection. Mutator writes the same
`.metrics/mutate/<namespace>.edn` snapshots used by the overlay; private
functions are included as `defn-/…`.

The standard contracts were inspected read-only in local crapper `1417c43`
and Mutator `844a0d7`. CRAP exit 0 means analysis completed; exit 2 means a
threshold finding after a valid snapshot was published; exit 1 denotes invalid
arguments or analysis/coverage/publication failure. Mutation exit 0 includes
uncovered sites recorded as coverage gaps; exit 3 means survivors with published
snapshots; exit 2 means baseline failure; usage and infrastructure errors are
failures. Findings retain snapshots and never request an automatic rerun or
`--mutate-all`. Snapshot joins remain by class namespace, then class id; missing
CRAP or mutation remains red.

Dryer writes `.metrics/dry.edn` on exit 0 only; exit 1 is a usage error and exit
2 an unknown `--format`, so neither is a valid quality result. `./uml dry`
passes the declared roots and exclusions as `-s` and `--exclude`, then
`dryerOptions` and the caller's arguments, and never lists files itself. Dryer
selects files by crapper's rules, so its rows name the same sources as
`crap.edn`; without declared roots it walks the project root, and with nothing
left to analyze it exits 0 with `No source files to analyze.`. Nothing in the
viewer reads `dry.edn` yet.

Run `python -I scripts/test_posix_entries.py` for the public-entry checks: real
`./uml ir`, the four scanners, standard and configured quality scope, serial
per-file mutation, installer idempotence and the launch/preflight contract. They
do not establish real tool coverage/mutation or companion mailbox acceptance, which
remains part of consumer integration (`scripts/test_posix_e2e.py`).

Public-entry acceptance on POSIX (Python 3 + git + Rust toolchain):

- `python3 -I scripts/test_posix_entries.py` — installer, IR, quality dispatch.
- `python3 -I scripts/test_posix_restart_entries.py` — restart refusal rules.
- `python3 -I scripts/test_posix_codex_companion.py` — Codex provider mail, quality
  gate, isolation and normal close through the real worker, with stand-ins for
  Herdr and Codex (no window, login or network).
- `python3 -I scripts/test_posix_viewer.py` — real window, reload, frame capture.
- `python3 -I scripts/test_posix_e2e.py` — launch, companion ready, FIFO, close.
- `python3 -I scripts/test_posix_restart_e2e.py` and
  `scripts/test_posix_recovery_e2e.py` — restart and recovery.
- `python3 -I scripts/run_viewer_driver.py <driver>` — GUI driver evidence.

These run in isolated projects and never touch a working checkout.

`./uml ir` runs the Rust IR generator on `examples/<dir>.policy.edn` (else the
first `examples/*.policy.edn`, else `policy.edn`). `./uml crap` and
`./uml mutate` run the project-local crapper and mutator. Run `./uml crap` on
the declared tree: `.metrics/crap.edn` is replaced every run. The companion uses
these three commands for every language.

`./uml` returns immediately and starts the Rust viewer through the detached launcher.
Exceptions and viewer output append to **`uml-viewer-log.txt`** in the project
directory (gitignored).

A **Herdr** session unique to the examined project starts interactive Claude
Code in
that directory (`--dangerously-skip-permissions --append-system-prompt …`
plus a launch prompt). The name is
`uml-viewer-<project>-<hash>`, stored in `.uml-viewer/companion.edn`. On start
it writes a hierarchical policy from that project's namespaces and regenerates
the IR. Type there; Esc is the real TUI interrupt. Closing the diagram kills
**only that** companion session and its terminal window — other Claude sessions
stay
up. Verified abnormal Claude exits use the bounded recovery policy described
above; normal or unknown exits stop recovery. That
instance also runs `./uml crap`, `./uml mutate`, and `./uml ir` after later
changes. Project-wide rules live in `AGENTS.md`.

Prefer `./uml` in the examined project (from `get-uml-viewer`).

| Command | Who | What |
|---------|-----|------|
| `./uml` | anyone | Fresh Rust viewer in the background. Starts the companion. Waits for `:display`. |
| `./uml --restart` | **associated agent only** | New Rust viewer process, same companion. Restores the last view. |

Do **not** pass `--restart` unless you are that companion recycling the
window after source changes. The wrapper requires the existing launcher/viewer
records and recorded companion ownership, and refuses restart rather than
leaving a diagram with no agent or replacing a missing companion. The
companion recycles the window by writing `:quit-for-restart` to
`.uml-viewer/to-viewer.edn`, waiting for the viewer to exit, then running
`./uml --restart`. The new process restores the saved diagram path, discussion
context, depth, pan, zoom, and which proposal was showing
(`.uml-viewer/session.edn`). An explicit path after `--restart` opens that
diagram instead; omit the path to reuse the saved one. Only a legacy session
without a saved path falls back to the policy output. Policy selection matches
`./uml ir`: `examples/<project-directory>.policy.edn`, otherwise the first
`examples/*.policy.edn` sorted by filename, otherwise root `policy.edn`.
Relative `:out` paths resolve from the project root; absent `:out` uses the IR
default `diagram.edn` in the project root. Missing or malformed policy, invalid output,
and unreadable diagrams fail explicitly. A saved or explicit path that fails
to load does not trigger fallback. A launcher record from a removed runtime (`love` or
`jvm`) is refused. Do not SIGKILL.
Closing the window kills only that project's companion, not other Claude
sessions.

On a fresh start the canvas stays blank until the companion sends `:display`,
with **Waiting for agent to create diagram.** `R` reloads the current EDN
immediately and does not wait. A missing or unreadable file prints
`UML viewer: file not found: …` in the inspector instead of throwing.

`lua-fixture/` is a Lua project with `.metrics/` snapshots written by
crapper and mutator. The Rust cargo tests scan it and check the overlay joins
those snapshots; Lua remains a supported scanner language even though the
viewer no longer runs on Lua.

```sh
cargo build --release --locked
./uml ir                                               # examples policy → IR
./uml crap                                             # .metrics/crap.edn
./uml mutate crates/edn/src/value.rs                   # differential mutation
./uml dry                                              # .metrics/dry.edn
```

This repository's `uml-quality.json` declares
`standardTools.sourceRoots: ["crates"]`; the dispatcher runs the
project-local crapper and mutator. The full export pipeline is
`crapper` (CRAP + coverage) and `mutator` (differential mutation), both
installed by `get-uml-viewer` into `.uml-viewer/<tool>/.venv`. Commit
`.metrics/` so a clone has numbers without re-running those tools.

Rename or move of a function is a new form: overlay does not match old names.

The `fixtures/jvm-goldens/` files are the frozen parity oracle captured from
the removed JVM viewer. The Rust cargo golden tests read them unchanged; only
the JVM regeneration tooling was deleted.

### Linux / WSL2

Linux is the primary delivery path, exercised through WSL2 + Ubuntu 24.04 +
WSLg; no separate distribution is promised. `./uml`, `./uml ir`, the three
quality tools and the Herdr companion all run natively, with no PowerShell or
Windows executable involved. WSLg presents the window; Chinese rendering,
reload and the companion lifecycle were verified there (see
`acceptance-evidence/106-wsl/`). Chinese IME composition needs a Linux input
method in the WSL session; it is not provided by the Windows IME.

Existing legacy resources and queues are retained; follow the migration
instructions before starting fresh.

## Navigation

**Layer** and **component** mean the same thing: a namespace grouping
(the first segment after the prefix, or a named proposal group).

- First view: **namespace components** (layers). Dependencies between them
  collapse to one arrow. Each component lists nested namespaces.
- Double-click a component to open the next level. Esc or the ← label goes
  up a level, including after drilling a proposal group. Esc does not quit.
  The window close box or a `:quit-for-restart` mail message exits the app.
  Reload, mail and exit handling keep running while the window is hidden;
  SIGTERM/SIGINT follows the normal close path and releases the companion.
- Hover an arrow for a popup of every `from -> to` it bundles, in any
  declutter mode. Violating pairs are red.
- Right-click a class or component for **刷新 CRAP**, **刷新 mutation**,
  **全量 mutation**, or **隐藏** (Refresh CRAP / Refresh Mutation / Refresh
  All Mutation / Omit). Each writes
  `to-agent.edn` and wakes the companion. CRAP runs on that class or the
  files under that component. Mutation is differential `./uml mutate` on
  those src files; All Mutation passes `--mutate-all`. Omit adds the id to
  the current proposal's `:omit`, or to policy `:omit` on the real diagram.
- Component titles paint **on top of** crossing arrows.
- The viewer UI is Chinese (one string table, `crates/viewer/src/strings.rs`);
  metric and tool names stay English. The inspector (检查器) lists the
  **real diagram** (源码图, the namespace tree) above
  **Proposals**. Click the real row to return to the tree; click a proposal
  to show it (marked as not in the code). Either click writes `:context`
  so the companion treats that diagram as the discussion. **New Proposal**
  (新建提案) adds a timestamp-named proposal. Right-click to rename or delete.
  **Declutter** cycles Declutter arrows / Remove arrows / Declutter
  elements / Declutter classes / Declutter none. **Remove arrows** hides
  the lines and puts a triangle on the top (incoming) and bottom
  (outgoing) of each box, **including nested classes** as well as their
  packages. A triangle is red if any bundled pair is violating. Hover it
  for the `from -> to` list.
- Double-click a leaf module for its **class card**.
- The class card names the **module** (`:ns`). Click it to open that source
  file at the top. Hover a member to highlight it; click it to open the same
  file at the declaration. See [Source extractors](#source-extractors).
- Methods on the card are `+` public and `-` private. Private functions are not
  drawn on the class box.
- Abstract classes show a white **α** in the upper-left, after the level number
  when there is one; interfaces a white **I** in the same place. Names of
  rectangles that are not classes (components/layers, interfaces, enumerations,
  package banners) are italic. Foreign libraries listed in policy are ovals
  outside the components.
- The main window is resizable.
- Scroll to pan vertically; Shift-scroll (or left/right arrows) for
  horizontal. Pan can follow arrows that bow past the origin.
- **Ctrl+** (or **Ctrl+=**) zooms in 10%; **Ctrl-** zooms out 10%;
  **Ctrl+0** restores 100%. Zoom keeps the view center still.
- **Regen** in the inspector asks the companion to rewrite policy and IR
  (see [Companion mailbox](#companion-mailbox)).
- `R` reloads the current EDN. The watcher also reloads on save, and when
  `.metrics/` snapshots change. An open class card updates with the new
  numbers.
- `Esc` on the class card closes it (it does not quit the viewer). Closing
  the main window exits the app.

## Policy

This project's diagram is **generated**. Do not edit `examples/uml-viewer.edn`.
Edit `examples/uml-viewer.policy.edn`, then run `./uml ir` (or press Regen).

The **scanner** reads source and emits facts. One class is emitted per module.
A project-local import/require is a dependency. External requires/imports
become **foreign** classes. The overlay fills members from `.metrics/`.
TypeScript, Rust, Python, and Lua each have a scanner (see
[Language graphs](#language-graphs)). This repository's policy uses
`:lang :rust` with one `:sources` root per crate and namespace prefix
`uml.<crate>`. Class namespaces are `uml.ir.graph` with ids `ir.graph`;
crapper names the same module `ir::graph`, and the overlay reads `::` as `.`,
so the id joins. The overlay joins each snapshot by `:ns`, then by the class id.

### Do not invent layers (components)

The tree **is** the namespaces. After `:prefix`, every `.` is a nesting
level. `uml.scene.layout` is a child of `uml.scene`.
`uml.ir.scan.lua` is a child of `uml.ir`.
The policy does **not** assign nses to invented packages. If you want Domain /
Engine / Adapters boxes **in the source tree**, those segments must exist as
namespaces. To **view** a grouping that is not in the code, use `:proposals`
(see [Proposed components](#proposed-components)) — do not rewrite namespaces.

To write a policy for a project:

1. Set `:prefix` to the id prefix (`uml` for `uml.ir.graph`). A single
   language also sets `:src` (default `src`) and `:lang`. More than one
   language or root sets `:sources` instead of `:src`.
2. Set `:hierarchical true` (or omit `:packages` and `:diagrams`).
3. List top-level **segments** in `:order` — the first dotted part after
   the prefix, in the order you want the boxes. Do not invent names.
4. List real libraries in `:foreign` if they should appear as ovals. Leave
   out the language's standard library (`std`, `core`, `alloc`, `node`):
   nearly every module uses it, so it says nothing about the architecture.
5. Optionally override a require with `:edge-kinds {[:from :to] :association}`
   using the **leaf** ids (`lua_language.source_lua`, not
   `lua_language`).
6. Set `:levels` so the generator can mark dependency-rule violations
   (see [Dependency rule](#dependency-rule)).
7. Optionally set `:proposals` to name design components that are not namespaces
   (see [Proposed components](#proposed-components)).
8. Run `./uml ir` (or Regen).

If `foo.bar` and `foo.bar.baz` both exist, the `bar` box lists `bar` (the
module) and `baz` (the child). Module titles are the last namespace
segment (`layout`, not `engine.layout`). A hyphenated segment capitalizes
each part (`source-lua` → `SourceLua`). A segment that already
contains a capital keeps that spelling and capitalizes the first letter
(`tauriFs` → `TauriFs`). Double-click the component to open that level;
double-click the `bar` module line for its class card.

Wrong (invented partitions):

```edn
:packages [{:id :domain :nses [ir geom source]}
           {:id :engine :nses [layout route]}]
```

Right (the ns tree):

```edn
{:title "UML viewer"
 :prefix "uml"
 :lang :rust
 :out "examples/uml-viewer.edn"
 :hierarchical true
 :sources [{:lang :rust :root "crates/edn/src" :prefix "uml.edn"}
           {:lang :rust :root "crates/ir/src" :prefix "uml.ir"}
           {:lang :rust :root "crates/scene/src" :prefix "uml.scene"}
           {:lang :rust :root "crates/companion/src" :prefix "uml.companion"}
           {:lang :rust :root "crates/viewer/src" :prefix "uml.viewer"}
           {:lang :rust :root "crates/uml-viewer/src" :prefix "uml.main"}]
 :proposals [{:id :p-1791033119418, :name "test", :layers []}]
 :foreign [libc egui eframe image winit log]
 :order [main viewer companion scene ir edn]
 :levels [[edn] [ir scene companion] [viewer] [main]]}
```

A project with more than one language lists each tree under `:sources`.
The policy `:prefix` is stripped from ids. A source `:prefix` is that
tree's namespace root. `src/model.ts` becomes `bookwriter.model`
(`:model`). `src-tauri/src/lib.rs` becomes `bookwriter.rust` (`:rust`).
`main.rs` becomes `bookwriter.rust.main` (`:rust.main`).

```edn
{:title "Bookwriter"
 :prefix "bookwriter"
 :sources [{:lang :typescript :root "src"}
           {:lang :rust :root "src-tauri/src" :prefix "bookwriter.rust"}]
 :foreign [at.tauri-apps markdown-it highlight.js tauri serde]
 :hierarchical true}
```

| Key | Role |
|-----|------|
| `:prefix` | Strip this from each ns; remaining dots are the tree |
| `:hierarchical` | Namespace tree (default when `:packages` is omitted) |
| `:order` | Order of **existing** top-level ns segments, not new component names |
| `:levels` | Groups of those segments, **inner (higher-level) first**. Same group = same rank |
| `:proposals` | Named groupings of real segments; **not** instantiated in source. Inspector **源码图** (real diagram) row returns to the ns tree |
| `:omit` | On a proposal or the policy: nses (and their children) left off the diagram |
| `:edge-kinds` | Override parser kind for `[from to]` (usually `:association`) |
| `:omit-edges` | Drop `[from to]` |
| `:lang` | Scanner when `:sources` is absent (`:lua`, `:typescript`, `:rust`, or `:python`) |
| `:src` | Source root when `:sources` is absent |
| `:sources` | One scan per `{:lang :root :prefix?}`. The entry `:prefix` is that tree's namespace root |
| `:foreign` | External libs as ovals. A listed prefix collapses `at.tauri-apps.api` to `at.tauri-apps`. Unlisted externals are dropped |

**Viewer Claude loop** (passed with `--append-system-prompt` to the
companion session only)

On launch: from the examined directory, write or update the hierarchical
policy and regenerate the IR, then wait.

After **every** later source or policy change, refresh metrics and
regenerate the IR.

For this repository the maintainer runs that refresh by hand during
development (see `AGENTS.md`): `./uml crap`, then `./uml mutate` on the changed
source files, then `./uml ir`. Uncovered mutants that remain are coverage
gaps. Keep the snapshot. Do not re-run the file or force a full mutation
because mutate exited non-zero.

For another language, run that language's CRAP and mutation tools when
they exist. The overlay joins each snapshot by `:ns`, then by the class
id. Missing CRAP or mutation data is red. Then regenerate the IR.

- Add/rename/delete a namespace: the tree updates on `./uml ir`. Put a new
  top-level **segment** in `:order` if you care about box order.
- Nested nses appear as contents of the parent component.
- “This require is really an association”: one `:edge-kinds` entry.
- Show a library like egui as an oval: add it to `:foreign`.
- Do not add `:packages` to fake Clean Architecture components. Use
  `:proposals` to view a grouping that is not in the code.
- Preserve `:proposals` when rewriting policy. Do not invent them on launch.
  If instructed, add a named proposal (default name is a timestamp).

Hand-written sample IRs (e.g. `examples/library.edn`) are still valid; they
are not generated.

### Dependency rule

A `:dependency` edge is **violating** when it runs from a **higher-level**
(inner) component to a **lower-level** (outer) one. That is the Clean
Architecture dependency rule: source-code dependencies point inward.

Evaluation is deterministic given `:levels`:

1. Take the first dotted segment of each end (`engine.layout` → `engine`).
2. Look up that segment in `:levels`. Rank is the group's index; **smaller
   is inner / higher-level**.
3. If both ends have a rank and `from-rank < to-rank`, the edge is
   `:violating true`. Same rank is allowed. `:implements` and
   `:association` are never violating. Foreign / unranked ends are not
   compared.
4. Collapsed component arrows keep the flag if any bundled leaf dependency
   was violating. Remapping a pair to `:association` clears it.

`:order` is visual box order, not rank. Nesting is not layering: you cannot
infer inner vs outer from the namespace tree alone, so `:levels` must group
segments that sit at the same architectural level (e.g. `domain`, `source`,
and `graph`). Omit `:levels` and nothing is marked. If `:levels` is omitted
and `:proposals` is set, rank follows the first proposal's component order.

### Proposed components

`:proposals` is a list of named groupings of **existing** top-level segments.
Those names are not namespaces. Each item is `{:id :name :layers [...] :omit [...]}`
(`:layers` here are named components; `:omit` keeps listed nses off
**Unassigned**). The as-is diagram stays the ns tree.
The inspector lists the real diagram (the namespace tree) just above
**Proposals**; click it to return to the tree. Click a proposal to show it
(canvas marked **PROPOSAL — not instantiated in code**). Either click
tells the companion that diagram is the context of discussion. **New
Proposal** adds an empty proposal named with a timestamp. Right-click a
name to rename or delete it. Double-click a ns box to drill; **←** at the
top returns to the proposal.

The **Declutter** button cycles **Declutter arrows** (one arrow per
component pair per direction) → **Remove arrows** (triangles on each box
and on nested classes instead of lines; hover lists deps; red if any pair
is violating) →
**Declutter elements** (also hide nested names, members, and ports) →
**Declutter classes** (also hide classes inside components) →
**Declutter none**.

When a proposal is shown, rank follows **that** proposal's component order
and violating arrows are re-evaluated. The real diagram uses `:levels`.
A component inherits the **max** level of its elements.
Class boxes show the current view's rank (innermost **0**)
at the upper left; the class card repeats **Level n**. The viewer lays the
source-tree views out **wide**: level 0 is drawn on the **right**, good
arrows (outer → inner) point right and violating arrows (inner → outer)
point left and stay red. Proposal views and the default layout (the
headless `uml-viewer scene` export without `--wide`) still run top to
bottom: level 0 at the **bottom**, good arrows down, violating arrows up.
Either way dependency ports and collapsed-arrow triangles stay above
(incoming) and below (outgoing) their box. When arrows are collapsed,
selecting a class highlights the component arrows it belongs to.
Collapsed components keep their color
and C/M dots; double-click still opens a component.

```edn
:proposals [{:id :ccp
             :name "2026-09-18 10:30:00"
             :layers [{:id :playfield :label "Playfield"
                       :nses [entities world missiles cities batteries flyers]}
                      {:id :hosts :label "Hosts" :nses [jvm browser]}
                      {:id :jvm :label "JVM"
                       :nses [jvm.cli jvm.main
                              {:id :quil-swing :label "Quil/Swing"
                               :nses [jvm.sketch jvm.window]}]}]
             :omit [cli]}]
```

The companion must not invent `:proposals` on launch and must keep them when
updating `:order`. If instructed, add a named proposal and regenerate the IR.

The viewer draws a violating arrow **red**, and **bold red** when a selected
element highlights it. Hand-written IR may set `:violating true` directly.

## Companion mailbox

The viewer and the companion Claude talk through `.uml-viewer/` in the examined
project (gitignored). The file is the mail; Herdr delivers the wake-up.

| File | Direction |
|------|-----------|
| `.uml-viewer/to-viewer.edn` | Claude → viewer |
| `.uml-viewer/to-agent.edn` | viewer → Claude |

Each mailbox file is a small queue `{:next-id n :queue [cmd …]}` (tmp-then-rename).
Commands have a rising `:id`. Append; do not overwrite. Handling a command
**pops** it from `:queue` and rewrites the file (oldest first). The viewer
does this itself on `to-viewer.edn`. The companion must pop each
`to-agent.edn` command as it handles it.

`.uml-viewer/session.edn` is the last view (diagram path, discussion context,
depth, pan, zoom, proposal, and mail cursor) written on `:quit-for-restart`
and restored by `--restart`. `.uml-viewer/viewer-process.json` records the
launcher identity, and `.uml-viewer/viewer-process-identity.json` records the
exact viewer PID/start; the public wrapper verifies and waits for those
identities before starting a replacement. `.uml-viewer/viewer-jvm.json` is no
longer recognized. `.uml-viewer/companion.edn`
records the Herdr session, socket, workspace, pane, server/shell process
identities and recovery-controller reference. Each operation verifies this
project's resources before acting; other projects and the shared Herdr service
remain untouched. Terminal attachment failure retains the companion and prints
an attach command to the project log.

| `:op` | Meaning |
|-------|---------|
| `:display` | Viewer loads `:path` (relative to the project root). Canonical paths identifying the current diagram reload IR/metrics and retain its valid view; a different diagram starts at its own root. |
| `:regen` | Claude rewrites hierarchical policy, regenerates IR, then `:display` |
| `:quit-for-restart` | Viewer saves the diagram path, discussion context, view, and mail cursor, then exits the viewer process without killing Claude. The associated agent waits for exit and runs `./uml --restart`. |
| `:refresh-crap` | Public `./uml crap` with declared scope (standard tools: whole business tree; operation override: targets or whole tree); valid result → successful IR → display |
| `:refresh-mutate` | Public `./uml mutate` on target files, differential; valid results → successful IR → display |
| `:refresh-mutate-all` | Public `./uml mutate --mutate-all` on target files only; valid results → successful IR → display |
| `:omit` | Add `:target` `:id` to proposal or policy `:omit`, then IR |
| `:context` | Inspector selection is the discussion context: `{:context :real}` or `{:context :proposal :proposal-id id :name "…"}` |

Quality refreshes share CLI dispatch and its configured scope/exclusions. Install
an absent public entry with `get-uml-viewer --install-only`. Process mail oldest
first, popping and rewriting the queue; keep real/proposal discussion context
until later context mail, and preserve proposal definitions and omit semantics.
Pass every selected source file to one mutation call: mutator shares one
coverage run and baseline and still publishes each file's snapshot. Scoped
mutation passes selected files without adding source-root flags.

Before each serialized quality invocation remove the previous
`.uml-viewer/quality-result.json`, await completion and capture the actual exit.
The newly published record must have version 1, the requested operation, this
canonical project root, the actual exit code and `validResult: true`. Require
all component results to pass before IR. Missing, stale, malformed or mismatched
records and invalid results stop that request's IR/display and report the cause.
IR must itself exit 0 before display. Continue later FIFO requests after a
failure. Recovery follows the same gate and never displays a failed refresh.
Survivors and uncovered mutants retain their snapshots; they do not trigger
retries or implicit full mutation.

The optional version-1 `uml-quality.json` declares `standardTools` or
complete `operations` overrides. The Clojure alias route and
`aliasResultExitCodes` were removed with the JVM implementation. Standard
CRAP accepts exit 0 (analysis completed) and 2 (threshold finding after a
valid snapshot); standard mutation accepts 0 (including uncovered coverage
gaps) and 3 (survivors with published snapshots). Exit 3 is never universally
accepted: it requires the tool contract or an explicit project operation
declaration. Direct tool commands publish no wrapper result record.
Noninteractive wrapper and application tests establish these boundaries; the
real-tool right-click/FIFO/display path is recorded in
[acceptance-evidence/106-wsl](acceptance-evidence/106-wsl/README.md).

Same-diagram display keeps focus, pan, zoom, proposal/open layer, declutter,
and valid selection/detail cards. A missing focus falls back to the nearest
remaining namespace ancestor; a missing layer returns to proposal root; a
missing proposal returns to the namespace tree. Missing selection/card targets
are cleared. Display preserves discussion context until another `:context`
arrives. Automated application specs cover this refresh contract.

Clicking **Real diagram** or a proposal (including **New Proposal**) writes
`:context` and wakes Claude. Stay on that architecture until the next
`:context`.

Right-click ops include `:target {:id :ns :kind :class|:component :proposal-id?}`.
`:kind` is `:component` for a layer box (and its nested nses) and `:class`
for a module.

**Regen** in the inspector queues `:regen` before asking Herdr to wake the
verified companion. The wake-up tells Claude to consume the FIFO mailbox; if
Claude is busy, it finishes first. A failed wake keeps the queued request and
the inspector reports that the session is not attached.

## Language graphs

Generating the IR asks the Rust scanner registry (`crates/ir/src/graph.rs`)
to scan a source tree. `:lang` selects the scanner; `:sources` scans each root
and merges them. Each scanner implements the `Scanner` trait:

| method | role |
|--------|------|
| `scan` | From a root and `{:prefix :ns-prefix?}`, return `{:classes :edges}` |

`:commands` and `:invokes` are optional. `merge_scans` turns a matching
command name into a `:dependency` and does not copy those keys onto a
class.

Classes are `{:id :name :ns :stereotype :lang :file}`. Edges are
`{:from :to :kind}` (`:dependency` or `:implements`). The policy layer is
language-neutral. Do not special-case a language in `policy`. Main
requires each scanner so it registers; `ir-generator` looks the scanner
up.

**TypeScript** (`crates/ir/src/scan/typescript.rs`) emits
one class per module file (`.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`,
`.mjs`, `.cjs`). It skips `*.test.*`, `*.spec.*`, declaration files
(`.d.ts`, `.d.mts`, `.d.cts`), and paths under `node_modules`, `dist`, and
`target`. Imports, type imports, re-exports, literal `import("./a")`, and
`require("./a")` count as dependencies. An ESM specifier such as `./a.js`
can resolve to `a.ts`; `.mjs` and `.cjs` can resolve to `.mts` and `.cts`.
The source root's `tsconfig.json` or `jsconfig.json`, then its parent's
inside the project, supplies `compilerOptions.paths` and `baseUrl`;
inherited configs are not followed. An asset import (CSS, images, JSON,
and the like) is ignored. An unresolved bare specifier is foreign:
`@tauri-apps/api/core` becomes `:at.tauri-apps.api.core`. A listed
`:foreign` prefix such as `at.tauri-apps` collapses that id.
`export function f(): T`, `export const x: T`, and a typed arrow function
are `:implements` when `T` is an imported project name. `Promise<Book>`
and `TreeNode[]` are not. Exported class/interface `extends` and class
`implements` clauses resolve imported names to their modules. A module
that exports only interfaces or type aliases is `:stereotype :interface`.
Exported functions, constants, classes, and enums are `:ops`, including
modified declarations such as `declare`, `abstract`, and `const enum`.
Strings, regexes, and template text are masked; executable `${...}`
expressions are scanned. Dynamic specifiers draw no guessed edge.
`invoke("name")` is recorded for the cross-language link.

**Rust** (`crates/ir/src/scan/rust.rs`) walks the modules
reachable from `lib.rs` and `main.rs` through `mod`. The crate name is
the `[lib] name` in `Cargo.toml`, else the `[package] name` when the root
has a `lib.rs` (`-` becomes `_`), so `bookwriter_lib::run()` in `main.rs`
depends on the lib class. Every Rust class is named by its source
file. The directory box keeps the namespace segment. `pub fn` functions
are `:ops`. `pub(crate)` functions are not,
unless they are `#[tauri::command]` functions, which are `:ops` even when
private. A `use` tree is expanded, so `use crate::{a, b::C};` depends on
`a` and `b`. A path written in code needs no `use`: `crate::a::f()`,
`super::g()`, and `dep::item` for a crate under `[dependencies]` count.
A visible module or `use` binding shadows that crate; an import inside an
inline module cannot change an outer `impl`. Repeated `super` segments
climb one module each. A path names the deepest module on it, falling back
to its crate root for a root item. Method turbofish syntax such as
`value.log::<T>()` is not a path into a dependency called `log`.
Another crate's path stays qualified (`egui::Context` is `egui.Context`):
under `:sources` it joins the deepest module only when the scanned
Cargo crate identity also matches. Thus `scene::document::ViewState`
depends on `scene.document`, while an unrelated internal `scene` module
cannot capture it. Within one crate, a path headed by a listed dependency
whose id equals a module id is dropped rather than joined to that module.
Across `:sources` roots, an exact id still joins that class: telling them
apart would need distinct stub ids. Otherwise a listed `:foreign` prefix
collapses it. `impl Trait for Type` is `:implements` when the trait
resolves to a project module or a path into another crate
(`impl eframe::App` implements `eframe`). Strings, including raw and
byte-raw strings, and nested block comments never supply dependencies.

**Python** (`crates/ir/src/scan/python.rs`) emits one class per
`.py` module. `__init__.py` is the package. Point `:src` at the package
directory or at its parent. A path that already begins with the
prefix stays as written, so `src/tools/parse.py` and
`src/tools/__init__.py` become `tools.parse` (`:parse`) and `tools`
(`:tools`). It skips `test_*.py`, `*_test.py`, `*_tests.py`,
`conftest.py`, and paths under `__pycache__`, `tests`, virtualenvs,
`dist`, and `build`. `import` and `from ... import` are dependencies,
including imports inside functions, after a compound header
(`try: import x`), or separated by semicolons. LF/CRLF continuations and a UTF-8 BOM
are accepted. `import a.b.c` binds its package names; an explicit `as`
keeps the imported module. A relative import (`from . import`,
`from ..mod import`) stays inside the package, falling back to the package
module when the imported name is a package attribute. Strings, including
Python 3.14 template strings, never supply imports.
A name that is not a project module is foreign
(`os.path` stays `os.path`, and a listed `:foreign` prefix such as `os`
collapses it). `class Dog(Animal)` is `:inheritance` when `Animal` is a
project module; a subscripted base such as `Animal[T]` keeps that module.
Class keywords such as `metaclass` do not hide the other bases. A base of
`Protocol` or `ABC` is `:implements`.
`def load() -> Animal` is `:implements` when `Animal` is a bare imported
project name. `list[Animal]` is not. Public module-level functions and
classes are `:ops`. A module whose public classes are only `Protocol` or
`ABC` bases, with no public functions, is `:stereotype :interface`.

**Lua** (`crates/ir/src/scan/lua.rs`) emits one class per `.lua`
module. The module name is the `require` path: strip the longest of
`src/lua/`, `src/`, `lua/`, drop `.lua`, collapse a trailing `/init`,
then `/` becomes `.`, so `src/calc/init.lua` and `src/calc/util.lua`
become `calc` (`:calc`) and `calc.util` (`:calc.util`). It skips
`*_spec.lua` and paths under `spec`, `lua_modules`, `dist`, `build`,
`target`, and `.metrics`. `require "a.b"`, `require("a.b")`, and
`require 'a.b'` are dependencies, as is `pcall(require, "a.b")`. The
argument may start on the next line. A slash in the name is read as a dot,
so `require "a/b"` names `a.b`; a hyphen stays part of the filename.
LÖVE paths such as `src.entities.player` resolve from the project root,
and an explicit `.init` names that package. A name that is not a project
module is foreign (`lfs` stays `:lfs`). Comments, strings, and long brackets
are masked, so a `require` inside them is ignored. Top-level functions that
are not `local` are `:ops`, named as written in the source (`M.clamp`,
`Account:deposit`). A function declared inside another, or inside a table
constructor (`M.t = { f = function() end }`), is not an op; one in a
top-level `if` or `do` block is, as crapper counts entries.
`require(name)`, `require("prefix." .. name)`, and a name built from `...`
cannot be resolved statically: they draw no edge, and `uml ir` prints
`Unresolved dynamic require: <file>:<line>` to stderr.
A top-level `setmetatable(t, {__index = Base})` or
`setmetatable(t, Base)`, where `local Base = require("app.base")` names a
project module, is `:inheritance` on that module. The first table may have
fields, and `__index` may sit among other metatable fields across lines.
`Base:extend()`, `Base:subclass("Name")`, and middleclass's
`class("Name", Base)` also name that base; the last form requires `class`
to be bound to `require "middleclass"` (or a dotted path ending in it).
This is a pattern match, not an evaluation: a `setmetatable` inside a
function makes an instance and
draws nothing, and a base that is not a local bound by `require` is not
followed. There are no `:implements` edges and no interface stereotype.

These scanners match static source patterns. They do not evaluate Lua
`package.path`, dynamic import names, or arbitrary class factories.
TypeScript JSX text is not fully tokenized, and Python nested f-string
format specifications can still hide later imports. The fixed JVM
fixtures remain compatibility checks; regression tests cover additional
source forms.

`merge-scans` links a TypeScript `invoke("read_text")` to the Rust class
that owns `#[tauri::command] fn read_text`, as a `:dependency`. Two
project classes with the same id are an error. When a dependency and an
`:implements` edge join the same pair, the IR keeps `:implements`.

CRAP and mutation for TypeScript, Rust, Python, and Lua come from separate
tools. The overlay joins a snapshot to the class whose `:ns` equals that
namespace. Otherwise the class id owns that name (`bookwriter.model`
owns `model`), a dotted child rolls up (`pdf` owns `pdf.Layout`), and
the policy prefix belongs to the single undotted Rust class. `::` is
read as `.`. A source file drawn inside its directory keeps its own
CRAP and mutation scores. The directory box pools the functions under
it, one weight per function. A file with no snapshot stays red, and in
that pool it counts as one red function per op, or one function when it
lists none. A class with no CRAP or mutation data is red.

## IR

A hierarchical policy writes one EDN document of all classes and edges
(`:hierarchical true`). The viewer builds each screen from the namespace tree
at the current drill level. A hand-written IR with `:packages` (or
`:diagrams`) is still a static diagram, e.g. `examples/library.edn`.

Metrics on the class card do not have to be authored. If `.metrics/` is
present, the overlay fills CC, coverage, CRAP, killed/survived/uncovered,
and any functions found in the snapshots (including privates). Authored
`:crap` / `:coverage` / `:ops` are the fallback when no snapshot exists.

The overlay joins a snapshot to the class whose `:ns` equals that
namespace. The generator writes `:ns` from the scanned ns. Hand-written
IR must set `:ns` the same way. When no class has that `:ns`, the class
id, a dotted child of that id, and the policy prefix on the single Rust
crate root are the fallbacks.

```edn
{:title "Lending library"
 :direction :tb
 :packages
 [{:id :domain
   :label "Domain"
   :classes
   [{:id :book
     :name "Book"
     :stereotype :class          ;; optional: :interface :enumeration :abstract
     :fields [{:name "isbn" :type "String"}]
     :ops [{:name "find" :args ["isbn"] :returns "Book"}]}]}
  {:id :app
   :label "Application"
   :classes
   [{:id :repo
     :name "CatalogRepo"
     :stereotype :interface
     :ops [{:name "get" :args ["isbn"] :returns "Book"}]}]}]
 :edges
 [{:from :sql-repo :to :repo :kind :implements}
  {:from :loan :to :book :kind :association :label "borrows"}]}
```

Optional authored metrics, used when snapshots are missing:

- `:crap` — a number (`μ`) or `{:mu :max :sigma}`
- `:coverage` — ratio 0–1 on a class or op
- `:cc`, `:killed`, `:survived`, `:uncovered`, `:private` on ops
- `:hide-members true` — compact box

Package and class **color** maps CRAP (`μ + σ`) and mutation score each onto
1–10 using `crates/scene/src/config.rs` cutoffs, averages them, and paints a
0–10 red–green fill. Missing CRAP or mutation data counts as red (grade 1),
not unknown. A parent pools CRAP by function count: μ and σ of the
functions underneath, then the same grade. It pools mutation by summing
killed and survived. A child with no data tints that pool once per
function. It does not replace the parent. A **C** and **M** dot in
the upper-right show the two scores. The boxes no longer print μ / max / σ.

On the class card, a `Crap μ … max … σ …` line sits above the table (max is
the worst function in the namespace, not a sum). Column groups are labeled
`--crap--` (Crap, CC, Cov) and `--mutation--` (killed, survived, uncovered).
The class row shows average CRAP with a `μ` suffix and omits CC. Killed is
white. Survived and uncovered are green at 0 and red when nonzero. A row
with no mutation operators (`:sites` 0) shows `---no mutation sites---`.
A form with operators still shows killed/survived/uncovered, including zeros.

Edge `:kind` values:

| kind | line | head |
|------|------|------|
| `:inheritance` | solid grey | empty triangle |
| `:implements` | solid grey | empty triangle |
| `:association` | solid grey | open arrow |
| `:dependency` | solid grey | open arrow |
| `:aggregation` | solid grey | empty diamond |
| `:composition` | solid grey | filled diamond |

Layout follows Mermaid's three stages:

1. **Size** each class from its text (padding 12).
2. **Place** packages in document order; classes Sugiyama-ranked inside a
   package; ~40px spacing. An arrow spanning more than one rank gets a
   zero-width lane in every rank between its ends, ordered with the boxes,
   so it passes each rank in a reserved gap instead of across a box.
   With `:levels`, a level is a constraint: inside it, a class sits one row
   above its highest same-level dependency, and classes on a cycle share a
   row. A level splits into at most three rows; deeper chains share the top
   one. Foreign ovals of a single `:levels` package take a row below the
   lowest level, so arrows to them pass the ranks between in lanes too.
   Classes without any arrow leave their level for a grid of four per row
   at the bottom, and levels sit 220px apart (sub-rows of one level 140px).
   The wide layout (the viewer's source-tree views, `scene --wide`) turns
   the ranks into columns from left to right, keeps the levels, the sink and
   the grid, and folds a rank too tall for the canvas into more columns: of
   all column heights it picks the one whose diagram fits a target aspect
   (default 1220:884, the viewer canvas; `--aspect`) at the largest zoom.
3. **Route** like Mermaid/ELK: ports on facing sides, orthogonal tracks in
   the rank gap, short same-rank connections through the stack gap (local U
   only when a sibling sits in the way), then stroke with D3 `curveBasis`
   cubics. Ordinary arrows are solid grey; violating dependencies are red
   (bold red when selected). Component titles draw after the arrows so
   names stay visible. Paths that pass the target and reverse are rejected.

## Source extractors

Clicking a member asks `crates/viewer/src/source.rs` for the **whole file** and a
**start line**. The IR (and the class card) only supply an **identity
map**; a language-specific extractor turns that into
`{:title :file :body :line}`. The source window opens on that file and
scrolls to the member (highlighted). Clicking the module name opens the same
file at the top (`:line` omitted).

```rust
let ident = source::Ident {
    lang: Some("lua".into()),
    file: Some("lua-fixture/src/calc/util.lua".into()),
    ns: None,
    name: Some("helper".into()),
};
source::member_source(&ident, &files);
```

`:lang` selects the extractor; there is no Clojure extractor after the cutover.
The extractors live in `crates/viewer/src/source.rs` (`FileSource`, entered
through `member_source`):

| method | role |
|--------|------|
| `locate` | path to the file that should contain the member |
| `extract` | slice that member out of the file text |
| `title` | window title |

**TypeScript**, **Rust**, **Python**, and **Lua** (`crates/viewer/src/source.rs`)
open `:file` and find the
exported declaration, the `fn`, the `def`, or the `function`. The class card passes
`:lang` and `:file` from the class. The protocol is the seam; do
not special-case languages in the class card.

egui stays in the front end (`crates/viewer/src/paint.rs`,
`crates/viewer/src/commands.rs`, `crates/viewer/src/inspector.rs`). The scene
core (`crates/scene`) does not depend on egui.
