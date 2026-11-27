# CI routing

The structure this page describes is [ADR 0020](adr/0020-test-tiering-and-unified-ci-graph.md)
(superseding [ADR 0019](adr/0019-ci-tiered-routing.md)); this page is the
operational reference.

`.github/workflows/ci.yml` and `.github/workflows/package-release.yml` both
start with a `changes` job on `ubuntu-latest` that runs
`scripts/ci-changes.py`. That command is the only place a repository path
becomes an affected crate/package set or a CI lane: it parses live
`crates/*/Cargo.toml` and `packages/*/package.json` manifests via Python
stdlib (`tomllib` + `json`), computes the role-aware reverse-dependency
closure, and writes one `key=true|false` line per lane into `GITHUB_OUTPUT`.
Local `python -I scripts/check.py --affected` imports the exact same solver.

## Tiers

| Tier | Where | Runs |
| --- | --- | --- |
| 0 | `changes` | the unified dependency-graph classifier alone, so a pure documentation change takes no Windows runner |
| 1 | `windows-fast`, `typescript` | `cargo fmt`, `cargo clippy --workspace --all-targets --all-features`, `scripts/check.py --fast`, and the TypeScript type check, unit tests, build and manifest validation |
| 2 | `windows-hermetic`, `windows-mutate`, `panel-e2e`, and `package-release` `assemble` | consolidated native Windows hermetic suite, self-hosting quality refresh (`crap`, differential `mutate`, gated `ir`), and browser/package acceptance |
| 3 | `toolchain-crapper`, `toolchain-mutator`, `upgrade_e2e`, and full-matrix fallback | external 7-language contract tests (`--features toolchain-contract --test toolchain_contract`, throttled by `.config/nextest.toml` `test-groups`) and live next-patch upgrade acceptance; triggered on direct touch in Presubmit and always on non-docs `main` push, `schedule`, `workflow_dispatch`, `v*` tag push, or fail-closed fallback |

## Lanes

| Output key | Job | What it runs |
| --- | --- | --- |
| `fast` | `windows-fast` | formatting, all-features clippy, and `scripts/check.py --fast` |
| `typescript` | `typescript` | version check, package contract, type check, unit and seam tests, build, manifest |
| `rust_hermetic` | `windows-hermetic` | `scripts/check.py`: fast guards, release build, and full hermetic workspace suite (`nextest` + doctests) with no external Go/Java/Clojure/Lua setup |
| `panel_e2e` | `panel-e2e` | the real panel browser harness and packaged install/removal acceptance |
| `upgrade_e2e` | `panel-e2e` (step) | disposable next-patch upgrade package build and live `upgrade.spec.mjs` acceptance (direct touch in Presubmit; always on non-docs `main` push / nightly) |
| `mutate_refresh` | `windows-mutate` | self-hosting CRAP, differential mutation, and gated IR refresh over the hermetic suite |
| `toolchain_crapper` | `toolchain-crapper` | 7-language coverage & Lua mutation contract tests (`-p uml-viewer --features toolchain-contract --test toolchain_contract`) |
| `toolchain_mutator` | `toolchain-mutator` | 7-language mutation contract tests (`-p mutator --features toolchain-contract --test toolchain_contract`) + strict `coverage_reuse` |
| `release` | `package-release` `assemble` | the Windows archive assembly and offline dry run |
| `docs_only` | none | every changed path is a pure document |
| `full` | every lane | fail-closed full matrix fired |

`ci-gate` (`if: always()`) aggregates `changes` and all downstream CI lanes into
one deterministic check for GitHub branch protection. `post-submit-alert`
opens or updates a tracked regression issue (`bug`, `ci`) whenever `ci-gate`
fails on `main` push or `schedule`.

`release-drift` keeps its own `if:`: it compares npm, the newest `v*` tag and
the repository version on `main` only.

Role-aware `rdeps` prunes `crapper`, `mutator`, and `dryer` from `panel_e2e`
(they link into `uml-viewer` for `standardTools`, which the panel harness never
invokes), while `.metrics/**` and `examples/uml-viewer.policy.edn` do route
`panel_e2e` because `packages/e2e/real-snapshot.mjs` reads them.

## Coupled documents

| Document | Why | Lanes |
| --- | --- | --- |
| `docs/companion-rules.md` | `packages/host/src/prompt.ts` imports it as text into `lib/host.js`, and `.gitattributes` keeps its bytes LF | `fast`, `typescript`, `panel_e2e`, `mutate_refresh`, `release` |
| `docs/plugin-development.md` | `scripts/package.mjs` and `scripts/release.mjs` put it in the main archive | `fast`, `typescript`, `panel_e2e`, `upgrade_e2e`, `release` |
| `README.md` | npm packs it unconditionally, and `scripts/package-version.test.mjs` copies it | `fast`, `typescript`, `panel_e2e`, `upgrade_e2e`, `release` |

## Adding a path

Register new top-level files or directories in `classify_node()` inside
`scripts/ci-changes.py`, then run `python -I scripts/test_ci_changes.py`. That
suite asserts zero `unknown` paths across `git ls-files`, `rdeps` closure
monotonicity, and the workflow output contract.

## Local inner loop

- `python -I scripts/check.py`: full-workspace hermetic suite + release build (default).
- `python -I scripts/check.py --affected [--base <ref>]`: prunes Rust tests to the affected `rdeps` closure.
- `python -I scripts/check.py --toolchain-contract`: opt-in external toolchain contract suite.
- `python -I scripts/check.py --fast`: fast Python guard suites only.
