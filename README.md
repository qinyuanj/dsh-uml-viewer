# UML viewer

The DeepSeek Harness plugin opens a UML panel beside the session conversation.
It provides `uml_ir`, `uml_crap`, `uml_mutate`, `uml_dry` and `uml_display`.
See [plugin development](docs/plugin-development.md) and the
[isolated browser harness](docs/e2e-harness.md) for their contracts and checks.
The panel's prompt rules come from [docs/companion-rules.md](docs/companion-rules.md).
The old `get-uml-viewer` / `./uml` flow with its Herdr companion was deleted
(#18); it stays frozen in qinyuanj/uml-viewer.

## Plugin installation and maintenance

The main package includes the Host and Web client, Sarasa font and one Cordis
row. Its four exact-version optional dependencies target Windows x64, Linux
x64, macOS arm64 and macOS x64. Unsupported combinations refuse activation with
a reason. End users need DSH (`>=0.2.2-rc.1 <0.3.0`) and git; native packages
require no Rust, Python or Herdr installation. Registry publication is a later
release operation; local package assembly is available now.

Install local archives through DSH plugin management (use absolute paths):

```sh
dsh plugin --profile web add /absolute/dsh-uml-viewer-1.0.0.tgz /absolute/dsh-uml-viewer-win32-x64-1.0.0.tgz
dsh plugin --profile web remove dsh-uml-viewer @dsh-uml-viewer/win32-x64
```

Use the matching platform archive. The plugin pins its packaged binary under
`<DSH_HOME>/dsh-uml-viewer/<version>/` (atomic, hash-verified copy) and records
the current version in `current.json`; an open panel keeps its version across a
plugin update. Collapsing the sidebar, switching fullscreen, moving or hot-reloading the
panel is not a close: the resident waits a short bounded window for the same
page to re-attach. Closing the UML tab is the close, and opening the tab again
then runs the new version. A new page runs the new version when no other open page still uses
that session; otherwise it shares the running one (see
`docs/plugin-development.md`; browser behavior is not yet run-verified). Only an
unused version directory is removed. Uninstall
removes packages and the Loader contribution, preserving project policy,
proposals and `.metrics/`. It does not migrate old `.uml-viewer/` data. Use a
scratch `DSH_HOME` for all development installation and testing; never test
against a working profile.

```sh
pnpm install --frozen-lockfile --ignore-scripts --store-dir /absolute/scratch/pnpm-store
pnpm typecheck
pnpm test
pnpm validate:manifest
pnpm version:check
cargo fmt --all -- --check
cargo build --release --locked -p uml-viewer
pnpm package:local --binary /absolute/target/release/uml-viewer --out /absolute/scratch/packages
```

On Windows use Windows-absolute paths and `uml-viewer.exe`. Check disk space
before building; the script consumes an explicit binary and never installs
a toolchain or builds other platforms. It writes deterministic npm tarballs
and a SHA-256/inventory report, with no install scripts or registry calls.
The source checkout stays private; the assembler removes development-only
fields from the installable main package.
Keep generated archives, binaries, IR and metrics out of commits. The four
platform builds and the npm provenance release are configured in
[release packaging](docs/releasing.md): a pull request assembles the archives and
runs `npm publish --dry-run`, and a `v<version>` tag on the canonical repository
is the only publishing path. `scripts/release.mjs verify` opens every staged
archive and checks its real contents and package metadata before anything is
published. [Release CI7 at `7fbace6`](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42600985433)
passed all four native builds, archive-content inventory and `npm publish --dry-run`.
Actual tag publishing and provenance have not run (#34). Browser acceptance is
[partial, with a separate passing packaged lifecycle case](docs/e2e-harness.md#ci);
installed-profile update acceptance remains unverified (#32).

`Cargo.toml` `[workspace.package].version` is the release version authority;
every crate inherits it. Change it, run `pnpm version:sync`, update the Cargo
lockfile, then run `pnpm version:check`; CI checks npm and Cargo consistency.
The assembler writes the same version into main and platform manifests and
all four optional dependency constraints. To verify an assembled archive:

```sh
UML_VIEWER_PACKAGE_REPORT=/absolute/scratch/packages/package-report.json pnpm test:package
UML_VIEWER_PACKAGE_MAIN=/absolute/scratch/packages/dsh-uml-viewer-1.0.0.tgz UML_VIEWER_PACKAGE_PLATFORM=/absolute/scratch/packages/dsh-uml-viewer-win32-x64-1.0.0.tgz UML_VIEWER_E2E_SCRATCH=/absolute/scratch/e2e pnpm test:e2e package.spec.mjs
```

Set explicit `npm_config_cache`, `npm_config_store_dir`, `PNPM_CACHE_DIR`,
`COREPACK_HOME`, `XDG_CACHE_HOME` and `PLAYWRIGHT_BROWSERS_PATH` under scratch
before package-manager/browser commands. `pnpm e2e:install` uses the configured
E2E scratch browser prefix. Remove generated packages, test homes and Cargo
targets when finished. The local acceptance opens a real panel, removes the
packages through management and compares project bytes and nanosecond mtimes.

## Abstract

This project displays a project as UML. The UML is *dynamic*. You can click
elements to drill down — all the way to source code. You can also play
what-if games by proposing architecture changes.

The panel is coupled to the session's agent. The two communicate, so you can
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

A Rust binary that lays out UML from an EDN IR, and a DeepSeek Harness panel
that draws it. A **policy** plus a language-specific scanner writes the
topology; this tool displays it, routes the arrows, colors CRAP, and lets you
click. One Rust binary runs the panel's resident session, IR generation, the
four scanners and the three quality tools; the standalone egui window was
deleted in dsh-uml-viewer#30, so the panel is the only viewer. The JVM/Clojure implementation was removed in the JVM cutover
and the Lua/LOVE implementation in the #144 cutover (ADR 0008). The supported platforms are **WSL2 + Ubuntu 24.04 + WSLg**,
**macOS** and **native Windows 11 / Windows 10 22H2 (build 19045) or
later** on x64 (ADR 0010).

The IR is **topology**: the namespace tree, classes, and edges. **Metrics**
(CC, coverage, CRAP, killed/survived/uncovered) come from `.metrics/`
snapshots produced by [crapper](https://github.com/qinyuanj/crapper) and
[mutator](https://github.com/qinyuanj/mutator). The viewer overlays
those files at load, keyed by namespace and function name. Agents edit the
policy, not the IR. See [Policy](#policy).

[dryer](https://github.com/qinyuanj/dryer) is the third quality tool. It
reports structurally duplicated forms in `.metrics/dry.edn`; the viewer does
not overlay that snapshot yet, so `uml-viewer quality dry` is a report you
read from the terminal.

## Run

Needs the Rust toolchain pinned by `rust-toolchain.toml` (1.99.1 with
`rustfmt` and `clippy`), `git`, and for the plugin Node 24 and pnpm.
Development checks and the pre-commit hook require Python 3.11+; the binary
itself has no Python, Herdr, Java, Clojure, Lua, LOVE or PowerShell dependency.

```sh
cargo build --release --locked
target/release/uml-viewer ir examples/uml-viewer.policy.edn   # policy → IR
cargo test --workspace --locked          # pure and integration suites
python -I scripts/check.py               # source rules + the two above
git config core.hooksPath .githooks      # once per clone: enable the pre-commit hook
```

The diagram is viewed in the DeepSeek Harness panel, which hot-reloads the
diagram and its `.metrics/` snapshots (see
[docs/plugin-development.md](docs/plugin-development.md); its browser checks
are [docs/e2e-harness.md](docs/e2e-harness.md)). The versioned
`.githooks/pre-commit` runs `cargo fmt --all -- --check`, the static source
rules and `cargo clippy --workspace --all-targets --locked -- -D warnings`; a
failure blocks the commit.

Quality data comes from the same binary. `uml-viewer quality crap|mutate|dry`
applies the declared scope in `uml-quality.json` (ADR 0001;
[docs/quality-dispatch.md](docs/quality-dispatch.md)) and prints the
version-1 result record; `crap`, `mutate` and `dry` are also subcommands of
their own with the original tools' options. Mutation isolation is per
platform: [docs/mutation-linux.md](docs/mutation-linux.md) and
[docs/mutation-windows.md](docs/mutation-windows.md); macOS `mutate` refuses
with exit 1.

```sh
./target/release/uml-viewer quality crap                          # .metrics/crap.edn
./target/release/uml-viewer quality mutate crates/edn/src/lib.rs    # differential mutation
./target/release/uml-viewer quality dry                           # .metrics/dry.edn
./target/release/uml-viewer ir examples/uml-viewer.policy.edn       # then regenerate the IR
```

Run these calls from the repository root after the release build. On native
Windows, use `.\target\release\uml-viewer.exe` for each call. Follow the
current user resource policy before building or refreshing. Only run IR after
a successful quality result with `validResult: true`.

## Navigation

**Layer** and **component** mean the same thing: a namespace grouping
(the first segment after the prefix, or a named proposal group).

- First view: **namespace components** (layers). Dependencies between them
  collapse to one arrow. Each component lists nested namespaces.
- Double-click a component to open the next level. Esc or the ← label goes
  up a level, including after drilling a proposal group.
- Hover an arrow for a popup of every `from -> to` it bundles, in any
  declutter mode. Violating pairs are red.
- The element operations **刷新 CRAP**, **刷新 mutation**, **全量 mutation**
  and **隐藏** (Refresh CRAP / Refresh Mutation / Refresh All Mutation / Omit)
  are panel requests to the session's agent
  ([docs/companion-rules.md](docs/companion-rules.md)).
  CRAP runs on that class or the files under that component. Mutation is
  differential on those src files; All Mutation passes `--mutate-all`. Omit
  adds the id to the current proposal's `:omit`, or to policy `:omit` on the
  real diagram.
- Component titles paint **on top of** crossing arrows.
- Panel DOM text is Chinese or English from `locale/*.json`; resident-laid
  rows come from `crates/frame/src/strings.rs`. Metric and tool names stay
  English. The top bar's discussion-context switcher lists the **real
  diagram** (源码图, the namespace tree) and then each **Proposal**; pick a
  row to show it and make it the discussion context (the panel tells the
  agent in the conversation). Its last row, **新建提案**, is a request to the
  agent, named with a timestamp by default; the agent edits the policy in
  place. **P** returns to the source diagram's tree root. The inspector that
  used to list all this was deleted (#47). **Declutter** cycles Declutter arrows / Remove arrows / Declutter
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
- The panel uses the right sidebar's own fullscreen for a full-width view.
  Closing the UML tab stops its resident; the tab's refresh command and **R**
  reload the diagram.
- Scroll to pan vertically; Shift-scroll (or left/right arrows) for
  horizontal; Ctrl+scroll zooms at the cursor. Pan can follow arrows that
  bow past the origin.
- **Ctrl+** (or **Ctrl+=**) zooms in 10%; **Ctrl-** zooms out 10%;
  **Ctrl+0** fits the whole view. **Ctrl+F** searches classes.
- `R` reloads the current EDN. The watcher also reloads on save, and when
  `.metrics/` snapshots change. An open class card updates with the new
  numbers.
- The class card's close button closes it.

## Policy

This project's diagram is **generated**. Do not edit `examples/uml-viewer.edn`.
Edit `examples/uml-viewer.policy.edn`, then run
`uml-viewer ir examples/uml-viewer.policy.edn` (in the panel: `uml_ir`).

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
8. Run `uml-viewer ir` with the policy (in the panel: `uml_ir`).

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
           {:lang :rust :root "crates/frame/src" :prefix "uml.frame"}
           {:lang :rust :root "crates/uml-viewer/src" :prefix "uml.main"}]
 :proposals [{:id :p-1791033119418, :name "test", :layers []}]
 :foreign [libc log]
 :order [main frame scene ir edn]
 :levels [[edn] [ir scene] [frame] [main]]}
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
| `:proposals` | Named groupings of real segments; **not** instantiated in source. The switcher's **源码图** (real diagram) row returns to the ns tree; **P** does the same |
| `:omit` | On a proposal or the policy: nses (and their children) left off the diagram |
| `:edge-kinds` | Override parser kind for `[from to]` (usually `:association`) |
| `:omit-edges` | Drop `[from to]` |
| `:lang` | Scanner when `:sources` is absent (`:lua`, `:typescript`, `:rust`, or `:python`) |
| `:src` | Source root when `:sources` is absent |
| `:sources` | One scan per `{:lang :root :prefix?}`. The entry `:prefix` is that tree's namespace root |
| `:foreign` | External libs as ovals. A listed prefix collapses `at.tauri-apps.api` to `at.tauri-apps`. Unlisted externals are dropped |

**Agent loop** (the panel's prompt section adds the request rules,
[docs/companion-rules.md](docs/companion-rules.md))

When asked to show a project: from its directory, write or update the
hierarchical policy and regenerate the IR, then wait.

After **every** later source or policy change, refresh metrics and
regenerate the IR.

For this repository the maintainer runs that refresh by hand during
development (see `AGENTS.md`): `uml-viewer quality crap`, then
`uml-viewer quality mutate` on the changed source files, then `uml-viewer ir`. Uncovered mutants that remain are coverage
gaps. Keep the snapshot. Do not re-run the file or force a full mutation
because mutate exited non-zero.

For another language, run that language's CRAP and mutation tools when
they exist. The overlay joins each snapshot by `:ns`, then by the class
id. Missing CRAP or mutation data is red. Then regenerate the IR.

- Add/rename/delete a namespace: the tree updates on `uml-viewer ir`. Put a new
  top-level **segment** in `:order` if you care about box order.
- Nested nses appear as contents of the parent component.
- “This require is really an association”: one `:edge-kinds` entry.
- Show a library like `libc` as an oval: add it to `:foreign`.
- Do not add `:packages` to fake Clean Architecture components. Use
  `:proposals` to view a grouping that is not in the code.
- Preserve `:proposals` when rewriting policy. Do not invent them.
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
The top bar's discussion-context switcher lists the real diagram (the
namespace tree) and then **Proposals**; pick the source diagram to return to
the tree (**P** does the same), or a proposal to show it (canvas marked
**PROPOSAL — not instantiated in code**). Either pick makes that diagram the
context of discussion (in the panel the agent is told). **新建提案** (New
Proposal), the switcher's last row, asks the agent to add an empty proposal
named with a timestamp; renaming or deleting a proposal is also a request
to the agent, which edits the policy in place. Double-click a ns box to drill; **←** at the
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

The agent must not invent `:proposals` and must keep them when updating
`:order`. If instructed, add a named proposal and regenerate the IR.

The viewer draws a violating arrow **red**, and **bold red** when a selected
element highlights it. Hand-written IR may set `:violating true` directly.

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
cannot be resolved statically: they draw no edge, and `uml-viewer ir` prints
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

Clicking a member asks `crates/frame/src/source.rs` for the **whole file** and a
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
The extractors live in `crates/frame/src/source.rs` (`FileSource`, entered
through `member_source`):

| method | role |
|--------|------|
| `locate` | path to the file that should contain the member |
| `extract` | slice that member out of the file text |
| `title` | window title |

**TypeScript**, **Rust**, **Python**, and **Lua** (`crates/frame/src/source.rs`)
open `:file` and find the
exported declaration, the `fn`, the `def`, or the `function`. The class card passes
`:lang` and `:file` from the class. The protocol is the seam; do
not special-case languages in the class card.

Drawing happens only in the panel (`packages/client`). Neither the scene
core (`crates/scene`) nor the window-free viewer model (`crates/frame`: draw
commands, theme roles, LOD, class card, source window, discussion, strings,
view snapshots, watch) links a window or GUI stack. The scene frame
the viewer panel draws is `uml-viewer scene --frame` (`docs/scene-frame.md`).
