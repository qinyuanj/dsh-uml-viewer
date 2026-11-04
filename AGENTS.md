# UML viewer

The diagram is already on screen.

- Do **not** edit `examples/uml-viewer.edn`. It is generated from
  `examples/uml-viewer.policy.edn` by the Rust IR pipeline via `./uml ir`.
- Policy follows **namespace nesting**, not invented layers/components.
  Dots after the prefix are the tree. Do not add Domain/Engine-style
  packages. **Layer** and **component** mean the same thing.
- TypeScript, Rust, Python, and Lua are the supported scanners. This
  repository's policy is `:lang :rust` with one `:sources` root per crate
  (`crates/<crate>/src`, ns prefix `uml.<crate>`). A class with no CRAP or
  mutation data is red; never hide it. The overlay joins a snapshot by the
  class `:ns`, then by the class id. crapper and mutator name a Rust module
  `crate::module` (and a method `crate::module::Type`); the overlay treats
  `::` and `.` alike, and the id drops the policy `:prefix`, so
  `uml.ir.graph` has id `ir.graph` and joins `ir::graph`. Do not strip
  source roots on either side. A non-Rust project sets `:lang` or
  `:sources`.
- `:proposals` are named groupings that are **not** in the source. Preserve
  them when rewriting policy. Do not invent them on launch. The inspector
  lists them; **P** returns to the namespace tree. If instructed, add a
  named proposal (default name is a timestamp) and regenerate the IR.
  Edit policy in place: change only the keys you mean to and keep its
  comments, ordering and line layout; never re-serialize the whole file.
- Developing this repository: the maintainer runs the quality refresh by
  hand (`./uml crap`, a differential `./uml mutate` on the changed source
  files, then `./uml ir`). Your hand-off ends with the list of changed
  source files for that refresh. Commit source and docs only; `.metrics/`
  snapshots and `examples/uml-viewer.edn` go in the maintainer's refresh
  commit. Requests (launch, `:omit`, proposals, quality mail) still run IR
  under their own rules.
- In a git worktree of this repository, run `./uml` with
  `UML_VIEWER_TOOL_ROOT` set to that worktree so it uses the tree under
  change, not the clone in `.uml-viewer/uml-viewer/`. The viewer on
  screen may be another project's; its working directory names it:
  `for p in $(pgrep -f '[u]ml-viewer viewer'); do readlink /proc/$p/cwd; done`
  (macOS: `lsof -a -d cwd -Fn -p $p`).
- Mail files are queues `{:next-id n :queue [cmd …]}`. Pop the head of
  `:queue` as you handle it (rewrite the file). Oldest first.
- Right-click quality mail uses public `./uml crap` / `./uml mutate` dispatch
  (install the entry with `get-uml-viewer --install-only` if absent).
  `:refresh-crap` follows declared scope (standard tools measure the whole
  business tree; an operation override may declare target scope);
  `:refresh-mutate` is differential on target files; `:refresh-mutate-all`
  passes `--mutate-all` only on those files. Pass every target file to one
  `./uml mutate` call: mutator shares one coverage run and baseline across
  them and still publishes each file's snapshot. Scoped mutation keeps
  explicit targets, without source-root flags.
- Serialize quality calls. Remove the old `.uml-viewer/quality-result.json`
  before each call, await its actual exit, and require a freshly published
  version-1 record matching operation, canonical project root and actual exit,
  with `validResult: true`. Only then generate IR; only IR exit 0 permits
  `:display`. Missing/stale/mismatched records or failures stop that request's
  IR/display and report the cause; continue later FIFO mail. Keep findings
  snapshots without retries. Exit 3 requires a tool or project operation
  declaration.
- `:omit` → add `:id` to the current proposal's `:omit`, or to policy `:omit`
  on the real diagram, then IR; display only after IR succeeds.
- `:context` mail: `{:context :real}` or `{:context :proposal :proposal-id
  id :name "..."}` is the diagram under discussion. Stay on that context
  until a later `:context` arrives.
- `./uml` is the user's or maintainer's fresh start (spawn the companion and
  the Rust viewer, wait for `:display`); a companion never runs it and uses only
  its own project's `./uml --restart` (new Rust process, keep this session,
  restore last view). If `./uml` is missing, tell the user to run
  `get-uml-viewer` in this directory (it clones uml-viewer into gitignored
  `.uml-viewer/uml-viewer/`).
- Do not start the viewer on launch; EDN reloads when the file mtime
  changes. To restart it: write `:quit-for-restart` to
  `.uml-viewer/to-viewer.edn`, wait for the viewer to exit, then
  `./uml --restart`. The new process restores the saved diagram path,
  discussion context, view, and mailbox cursor; omitting the path uses the
  saved diagram, while an explicit path overrides it. The public wrapper
  verifies and waits for the old launcher/viewer identity and refuses missing
  or changed companion ownership instead of replacing it. Do not pass
  `--restart` except through that wrapper. Do not SIGKILL. Companion
  ownership in `.uml-viewer/companion.edn` must explicitly record Herdr;
  closing the viewer cancels recovery first and releases only this
  project's verified resources. Verified abnormal exits retry at most three
  times (1, 2, 4 seconds) through the durable controller; normal or unknown
  exits stop recovery. Other Claude sessions stay up.
- Herdr is the sole companion backend on every supported platform.
  The delivery targets are **WSL2 + Ubuntu 24.04 + WSLg** and **macOS**
  (ADR 0007).
  Legacy tmux/psmux or unmarked ownership is retained and refused; before
  migration read `docs/herdr-backend-contract.md` §9.
- Installer prerequisites are the Rust toolchain pinned by
  `rust-toolchain.toml`, git, Herdr >= 0.9.3, and Python 3.11+. The single
  Rust binary runs the viewer, IR generation, the four scanners and the
  companion roles. `get-uml-viewer` builds the pinned binary with
  `cargo build --release --locked`, provisions the Sarasa face into
  `.uml-viewer/`, fetches all three quality tools — crapper, mutator and
  dryer — and provisions their project-local `.uml-viewer/<tool>/.venv`
  environments. dryer reports duplicate structure in `.metrics/dry.edn`;
  nothing overlays that snapshot, so `./uml dry` is read from the terminal.

Do not commit or push unless asked. Esc interrupts a turn in this terminal;
do not kill the process on interrupt.

## Agent skills

### Issue tracker

Issues 和 specs 使用 GitHub Issues；操作前读取 `docs/agents/issue-tracker.md`。

### Triage labels

使用五个默认 triage 标签；分类和应用标签前读取 `docs/agents/triage-labels.md`。

### Domain docs

领域文档采用 single-context；探索代码前读取 `docs/agents/domain.md`。

## Cursor Cloud specific instructions

Cloud Agent machines run native Ubuntu 24.04 with an X display on `DISPLAY=:1`. The public `./uml` entry accepts a WSL2 kernel (ADR 0007). On this host, generate IR and open the diagram with the release binary:

```sh
target/release/uml-viewer ir examples/uml-viewer.policy.edn /tmp/diagram.edn
GALLIUM_DRIVER=llvmpipe LIBGL_ALWAYS_SOFTWARE=1 WGPU_BACKEND=gl \
  UML_CJK_FONT="$PWD/.uml-viewer/fonts/SarasaMonoSC-Regular.ttf" \
  target/release/uml-viewer viewer /tmp/diagram.edn
```

The binary selects the WSLg `d3d12` Gallium driver when `GALLIUM_DRIVER` is unset, so set `llvmpipe` before a launch on this display. Sarasa is provisioned at `.uml-viewer/fonts/SarasaMonoSC-Regular.ttf`. Herdr 0.9.5 (protocol 22) is on `PATH`; companion attach still belongs on WSL with Windows Terminal. `cargo fmt --all -- --check` and `cargo test --workspace --locked` are the checks that run here. Leave the viewer stopped until a turn is demonstrating the GUI.
