# Release packaging and publishing

Status: **configured in source, NOT RUN** (2026-11-09). No workflow in this file
has executed on a remote runner. No native binary or npm archive has been built
on this host, because the current resource policy forbids local Cargo builds and
native executable generation. The four platform artifacts are **NOT VERIFIED**
and acceptance criterion 1 of dsh-uml-viewer#33 ("four platform packages produced
by CI") remains open until an authorized remote run.

The files involved are `.github/workflows/package-release.yml`,
`scripts/package.mjs` (assembler), `scripts/release.mjs` (release guards),
`scripts/test-package.mjs` and `scripts/version.mjs` (version authority).

## What the pipeline does

`pull_request` and `push` of a `v*` tag trigger it. The `assemble` job runs once
per delivered platform on a native runner, because `scripts/package.mjs` executes
the supplied binary as `--version` and refuses a mismatch. A foreign target
binary is never executed.

| Platform package | Runner label | Real architecture |
| --- | --- | --- |
| `@dsh-uml-viewer/win32-x64` | `windows-latest` | Windows x64 |
| `@dsh-uml-viewer/linux-x64` | `ubuntu-latest` | Linux x64 |
| `@dsh-uml-viewer/darwin-arm64` | `macos-latest` | macOS arm64 |
| `@dsh-uml-viewer/darwin-x64` | `macos-15-intel` | macOS Intel x64 |

The labels follow GitHub's hosted-runner reference: `macos-latest` is arm64, so
Intel macOS needs an explicit `-intel` label.

Each `assemble` runner builds the plugin and `target/release/uml-viewer`, packs
the main and current-platform archives with the assembler, verifies the report
hash against the staged bytes, and uploads its platform archive. Font, license
and the rest of the main bundle are uploaded once, by `linux-x64` only.

The `inventory` job is the single place that decides whether a release is
publishable. `scripts/release.mjs verify` requires all of:

- exactly four assemble reports, one per delivered platform, no duplicates;
- every report at the Cargo workspace version, with `optionalDependencies`
  pinning exactly the four platform packages at that version;
- a current-platform archive whose name is
  `dsh-uml-viewer-<platform>-<version>.tgz` and whose bytes match its recorded
  SHA-256;
- exactly one staged main archive, staged by `linux-x64`;
- the same main archive SHA-256 on all four runners, so the main bundle is
  reproducible across hosts;
- exactly five `.tgz` files in total, all with expected names, so a stale or
  stray archive fails the run.

It writes `release-inventory.json` (artifact `release-inventory`): version,
per-artifact name, platform, file, path and SHA-256, plus the publish order.

Pull requests stop after packaging:

- `pnpm test:package` runs the package contract, the stale-payload identity gate
  and the release guards;
- `npm publish --dry-run` prints what each publish would contain, from the staged
  archives only, and never in the repository root, which stays `private`;
- no `id-token` is requested and no credential is read on this path.

On a `v*` tag push the `publish` job runs in the `release` environment with
`contents: read` and `id-token: write` and nothing else, checks that the tag is
`v<workspace version>`, re-verifies the inventory, then publishes the four
platform archives before the main archive with `--provenance`. The order matters:
the main package's optional dependencies must already exist when it becomes
installable.

## Version authority

`Cargo.toml` `[workspace.package].version` is the only release version.
`scripts/version.mjs` checks the npm mirrors and every crate's
`version.workspace = true`. The release tag must be `v<version>`; any other tag
stops at `check-tag` before a single publish. To bump: edit the workspace
version, run `pnpm version:sync`, update `Cargo.lock`, run `pnpm version:check`.

## One-time maintainer setup (owner only, not automated)

None of this is in the repository and none of it ran here.

1. Repository environment: create the `release` environment, add the required
   reviewers, and limit its deployment branches and tags to `v*`. The `publish`
   job waits for that approval. Add a tag protection rule for `v*` so only
   maintainers can create release tags.
2. Package trust, after the package exists on the registry: for each of the five
   packages (`dsh-uml-viewer` and the four `@dsh-uml-viewer/*`), open npmjs.com
   package settings and add a GitHub Actions trusted publisher with
   - organization or user `qinyuanj`,
   - repository `dsh-uml-viewer`,
   - workflow filename `package-release.yml` (filename only, case sensitive;
     renaming the workflow breaks publishing),
   - environment `release`,
   - allowed actions: `npm publish`. Choose stage-only (`npm stage publish`)
     instead if a maintainer should approve every package with 2FA before it
     becomes public.

   `package.json` `repository.url` must keep matching the GitHub repository
   exactly, otherwise npm refuses the OIDC exchange.
3. First publish of each package is the bootstrap: a trusted publisher cannot be
   configured before the package exists, so an owner publishes the first
   version manually from the assembled archives with 2FA,
   `npm publish <tgz> --access public`, and only then configures trusted
   publishing. A new trusted publisher configuration must complete its first
   successful publish within two days, otherwise it expires and must be deleted
   and recreated.
4. After trusted publishing works, set package access to "Require two-factor
   authentication and disallow tokens" so no long-lived publish token can be
   used. No token, `.npmrc`, `NODE_AUTH_TOKEN` or registry secret is authored in
   this repository, and `actions/setup-node` is deliberately called without
   `registry-url` so no auth file is written on the runner. OIDC is the only
   credential on the publish path.

## Tag operations (prepared, not run)

These are the commands a maintainer runs when a release is authorized. This
ticket ran none of them.

```sh
git switch main
git pull --ff-only
node scripts/version.mjs --check    # prints the single release version
git tag -s v1.0.0 -m "dsh-uml-viewer 1.0.0"   # substitute the printed version
git push origin v1.0.0
```

Pushing the tag starts the only publishing path. Never create the tag on a fork,
and never disable the `release` environment approval to get a publish through.

## What a maintainer can check locally

Node only, no network, no archive mocking beyond the controlled fixtures:

```sh
pnpm test:package                              # contract, stale payload, release guards
node scripts/release.mjs check-tag v1.0.0      # tag/version guard
```

`scripts/release.test.mjs` drives the real `stage`, `verify` and `plan` commands
over controlled fixture archives. Those fixtures prove the guards, not the
native packages: they contain no built binary, font or plugin bundle.

## Failure modes

- **Tag does not match the version.** `check-tag` fails before any upload. Fix the
  tag; do not edit the version to match a stray tag.
- **Fewer or more artifacts, a missing platform, a duplicated main archive, or an
  archive named for another version.** `verify` fails the `inventory` job and no
  publish runs. Re-run the failed `assemble` leg, not the whole matrix with a
  mutated artifact directory.
- **Main archive hashes differ between runners.** The main bundle is not
  reproducible; the release is blocked. Compare the assembled `lib/*.js`,
  `fonts/` and `locale/` inventories before publishing anything. Do not relax the
  check to get a release out.
- **Partial publish.** Versions are immutable, so a re-run fails with a conflict
  on the packages that already published. Publish the remaining archives manually
  in plan order (`npm publish <tgz> --provenance --access public`) or release a
  new patch version. The workflow never un-publishes or re-tags.

## Not verified here

- the remote matrix never ran, so no real Windows, Linux or macOS artifact exists;
- no current-source native binary was built under the resource policy;
- `npm publish`, provenance attestation and the trusted publisher handshake were
  never executed;
- the cross-runner main-bundle reproducibility assertion is untested against real
  hosts, by nature of not having run it;
- acceptance criterion 1 (four platform packages produced by CI) is **NOT
  VERIFIED**, and criteria 2 and 3 are configured but unexecuted. No workflow
  run is green on this evidence.
