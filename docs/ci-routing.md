# CI routing

The structure this page describes is [ADR 0019](adr/0019-ci-tiered-routing.md); this
page is the operational reference.

`.github/workflows/ci.yml` and `.github/workflows/package-release.yml` both
start with a `changes` job on `ubuntu-latest` that runs
`scripts/ci-changes.py`. That command is the only place a repository path
becomes a lane: it writes one `key=true|false` line per lane into
`GITHUB_OUTPUT`, and every heavy job carries `needs: changes` plus an `if:`
on its own key. A path rule is therefore never written twice.

## Tiers

| Tier | Where | Runs |
| --- | --- | --- |
| 0 | `changes` | the classifier alone, so a documentation-only change takes no Windows runner |
| 1 | `windows-fast`, `typescript` | `cargo fmt`, `cargo clippy`, the fast guard suites, and the TypeScript type check, unit tests, build and manifest validation |
| 2 | `panel-e2e`, `windows-crap`, `windows-mutate`, `windows-quality`, `windows-check`, and the `assemble` job of `package-release` | the native Windows suites of the domains the change touches |
| 3 | the whole matrix | `workflow_dispatch`, `schedule`, a `v*` tag push, a workflow or classifier edit, an unmapped path, an empty change set, or a diff that cannot be read |

## Lanes

| Output key | Job | What it runs |
| --- | --- | --- |
| `fast` | `windows-fast` | formatting, lint and `scripts/check.py --fast` |
| `typescript` | `typescript` | version check, package contract, type check, unit and seam tests, build, manifest |
| `panel_e2e` | `panel-e2e` | the browser harness and the packaged install, upgrade and removal acceptance |
| `crap` | `windows-crap` | the CRAP unit and integration suites |
| `mutate` | `windows-mutate` | the maintainer CRAP, differential mutation and gated IR refresh over the declared scope, which covers both the Rust crates and the plugin source roots |
| `quality` | `windows-quality` | the Java, Clojure, Go, Python, TypeScript and Lua fixture toolchains and language suites |
| `check` | `windows-check` | `scripts/check.py`: source rules, release build and the workspace suite |
| `release` | `package-release` `assemble` | the Windows archive assembly and the offline dry run |
| `docs_only` | none | every changed path is a pure document |
| `full` | every lane | the Tier 3 fallback fired |

`release-drift` keeps its own `if:`: it compares npm, the newest `v*` tag and
the repository version on `main` only, and it is cheap on an ubuntu runner.

`crapper`, `mutator` and `dryer` link into the viewer binary but the panel
harness never drives them, so a change confined to those crates runs every Rust
lane except `panel_e2e`. Anything else under `crates/` is the viewer runtime and
does trigger it.

## Coupled documents

Three documents are build inputs, not prose, so they route lanes instead of
Tier 0:

| Document | Why | Lanes |
| --- | --- | --- |
| `docs/companion-rules.md` | `packages/host/src/prompt.ts` imports it as text into `lib/host.js`, and `.gitattributes` keeps its bytes LF for the prompt | `typescript`, `panel_e2e`, `check`, `release` |
| `docs/plugin-development.md` | `scripts/package.mjs` and `scripts/release.mjs` put it in the main archive | `typescript`, `panel_e2e`, `release` |
| `README.md` | npm packs it unconditionally, and `scripts/package-version.test.mjs` copies it | `typescript`, `panel_e2e`, `release` |

Everything else under `docs/`, a top-level `*.md` and `acceptance-evidence/`
is a pure document.

## Adding a path

Add it to the domain its siblings already live in, in `scripts/ci-changes.py`,
then run `python -I scripts/test_ci_changes.py`. A path the tables do not
recognise forces the whole matrix, so a forgotten entry costs a full run rather
than silently skipping a lane.

## Concurrency

`ci.yml` groups by `github.ref` and the event name, and cancels in progress only
for `pull_request`: a new commit on a pull request drops the stale run, while
`main`, a release tag and the nightly run never cancel each other. Keeping the
event name in the group is what stops the nightly full matrix from holding the
slot a `main` push needs. `package-release.yml` keeps
`cancel-in-progress: false`, because a cancelled assembly strands a publish.

## Contract

`scripts/test_ci_changes.py` drives the classifier as a command over explicit
change sets and real temporary git ranges. It pins the lane sets, the Tier 3
fallbacks, and that every `needs.changes.outputs.<key>` a workflow reads is a
key the classifier writes. It runs in the fast guard suite, so
`python -I scripts/check.py` executes it with the other guards.
