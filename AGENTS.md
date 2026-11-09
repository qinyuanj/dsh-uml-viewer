# UML viewer

- Do **not** edit `examples/uml-viewer.edn`. It is generated from
  `examples/uml-viewer.policy.edn` by the Rust IR pipeline via
  `./target/release/uml-viewer ir examples/uml-viewer.policy.edn`.
- Policy follows **namespace nesting**, not invented layers/components.
  Dots after the prefix are the tree. Do not add Domain/Engine-style
  packages. **Layer** and **component** mean the same thing.
- TypeScript, Rust, Python, and Lua are the supported scanners. This
  repository's policy is `:lang :rust` with one `:sources` root per crate
  (`crates/<crate>/src`, ns prefix `uml.<crate>`); each plugin TypeScript
  package joins as one `:lang :typescript` source (`packages/<pkg>/src`,
  prefix `uml.<pkg>`), added by the ticket that creates it (ADR 0013).
  A class with no CRAP or mutation data is red; never hide it. The
  overlay joins a snapshot by the class `:ns`, then by the class id.
  crapper and mutator name a Rust module `crate::module` (and a method
  `crate::module::Type`); the overlay treats `::` and `.` alike, and the
  id drops the policy `:prefix`, so `uml.ir.graph` has id `ir.graph` and
  joins `ir::graph`. Do not strip source roots on either side. A non-Rust
  project sets `:lang` or `:sources`.
- The rules for panel requests (serialized quality calls, IR and display
  gating, `:omit`, proposals, in-place policy edits, the discussion
  context) are `docs/companion-rules.md`, the plugin's prompt section; they
  apply here too. Never re-serialize the whole policy file.
- Developing this repository (ADR 0013): dsh-uml-viewer is the only
  repository for the `dsh-uml-viewer` plugin; qinyuanj/uml-viewer is frozen
  on the old `get-uml-viewer` / `./uml` flow, which this repository deleted
  (#18). After `cargo build --release --locked`, the maintainer runs the
  quality refresh from the repository root with the built binary:
  `./target/release/uml-viewer quality crap`, a differential
  `./target/release/uml-viewer quality mutate` on the changed source files
  (all of them in one call), optionally
  `./target/release/uml-viewer quality dry`, then
  `./target/release/uml-viewer ir examples/uml-viewer.policy.edn`.
  On native Windows use `.\target\release\uml-viewer.exe` for each call.
  Respect the current user resource policy before building or refreshing.
  `quality` applies the declared scope in `uml-quality.json` and prints the
  version-1 result
  record; run IR only after a record with `validResult: true`. Your
  hand-off ends with the list of changed source files for that refresh.
  Commit source and docs only; `.metrics/` snapshots and
  `examples/uml-viewer.edn` go in the maintainer's refresh commit.
- Plugin development and isolated DSH testing: read
  `docs/plugin-development.md` for workspace checks and `UML_VIEWER_BIN`.
  Use a scratch DSH home; keep the user's live profile untouched.
- Prerequisites are the Rust toolchain pinned by `rust-toolchain.toml`,
  git, Node 24 and pnpm (the plugin packages), and Python 3.11+ for
  `scripts/check.py` and the pre-commit hook. The single Rust binary runs
  the panel's `resident` session (the only viewer; the standalone egui
  window was deleted in #30), IR generation, the four
  scanners and the three quality tools (`crap`, `mutate`, `dry`, or
  `quality` for the declared scope). dryer reports duplicate structure in
  `.metrics/dry.edn`; nothing overlays that snapshot, so read
  `./target/release/uml-viewer quality dry` from the terminal (use the
  `.exe` path above on Windows). The Sarasa face is
  packaged in `fonts/`.

Do not commit or push unless asked.

## Agent skills

### Issue tracker

Issues 和 specs 使用 qinyuanj/dsh-uml-viewer 的 GitHub Issues；操作前读取 `docs/agents/issue-tracker.md`。历史 commit、注释与 ADR 0001–0012 中的裸 `#N` 指 `qinyuanj/uml-viewer#N`；跨仓库引用写全名。

### Triage labels

使用五个默认 triage 标签；分类和应用标签前读取 `docs/agents/triage-labels.md`。

### Domain docs

领域文档采用 single-context；探索代码前读取 `docs/agents/domain.md`。

## Cursor Cloud specific instructions

Cloud Agent machines run native Ubuntu 24.04. On this host, build and generate IR with the release binary:

```sh
target/release/uml-viewer ir examples/uml-viewer.policy.edn /tmp/diagram.edn
```

There is no standalone window: the diagram is viewed in the DSH panel (`docs/plugin-development.md`, with a scratch DSH home and `UML_VIEWER_BIN`), and its browser checks are the Playwright suite in `packages/e2e` (`docs/e2e-harness.md`). `cargo fmt --all -- --check` and `cargo test --workspace --locked` are the Rust checks that run here; respect the current user resource policy before building.
