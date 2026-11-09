# Release packaging and publishing

Status (2026-11-09): **remote packaging and publish dry-run passed**. Each run
below passed all four native platform builds, real archive-content inventory
verification and `npm publish --dry-run`, establishing the four-platform CI
packaging evidence for dsh-uml-viewer#33. These runs do not establish actual
publishing, provenance or installed behavior on all four platforms.

| Release run | Source head | Result |
| --- | --- | --- |
| [42585882805](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42585882805) | `0d115b2` | Success |
| [42587783277](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42587783277) | `ab9064c` | Success |
| [42591228972](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42591228972) | `f4c12e3` | Success |
| [42595826538](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42595826538) | `6147980` | Success |
| [42600985433](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42600985433) | `7fbace6` | Success |

Actual tag creation, registry publication and provenance remain **NOT RUN**,
intentionally pending authorization for #34.

The files involved are `.github/workflows/package-release.yml`,
`scripts/package.mjs` (assembler), `scripts/archive.mjs` (bounded gzip/ustar
reader and writer), `scripts/release.mjs` (release guards),
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
- the same main archive file inventory on all four runners, so every host packs
  the same files; each runner's main archive SHA-256 is recorded in the inventory
  as evidence, and the published archive is the one whose bytes match its report;
- exactly five `.tgz` files in total, all with expected names, so a stale or
  stray archive fails the run.

Every archive that would be published is then opened and read with a bounded
gzip/ustar reader (`scripts/archive.mjs`, Node builtins only, no dependency and
no external `tar`). `verify` refuses a file named `.tgz` that is not real gzip
data, a corrupt gzip stream or checksum, a tar header checksum mismatch, a
duplicate entry, an absolute or traversing path, a symbolic link, a hard link or
any other non-regular entry, and anything over 128 MiB compressed, 256 MiB
decompressed, 128 MiB for a single entry or 4096 entries, so a decompression
bomb cannot expand inside the verifier.

It then checks the real contents against the assembler report and the source
manifest:

- exactly one `package/package.json` per archive, and no member outside
  `package/`;
- the main package's name, version, `os`, `cpu` and `optionalDependencies` from
  the workspace and the platform table, with `private`, `scripts`,
  `devDependencies` and `packageManager` stripped;
- each platform package's name, version, `os` and `cpu`, plus a regular,
  non-empty `bin/uml-viewer` (`bin/uml-viewer.exe`) entry;
- the main archive holding `lib/host.js`, `lib/client.js`, the packaged Sarasa
  face and its license, `dsh-plugin.json`, `cordis.patch.yml`, `icon.svg`,
  `README.md` and `docs/plugin-development.md`;
- every main member covered by the source `package.json` `files` list, so no
  source, test or metrics file rides along;
- the real member list equal to the assembler report's inventory, including a
  per-file SHA-256 when a report supplies one.

The archives are read as data: no packaged binary is executed, so this proves
the archive and its metadata, not compiled binary behaviour.

Byte identity of the main archive is not required across runners. `.gitattributes`
pins `eol=lf` for EDN, Rust, fixtures, the prompt document and the git hooks, not
for the packaged text files, so a Windows checkout can pack CRLF where a Linux
checkout packs LF. The archive is built and published by one runner, so this
affects the evidence, not what users download; tightening it would mean pinning
`eol=lf` for the packaged files first.

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
over controlled fixture archives. The fixtures are real small gzip/ustar archives
written by the same writer a release uses, carrying fixture text and the exact
inventory and SHA-256 the report claims. The negative cases prove the guards by
execution: a plaintext `.tgz`, a corrupt gzip header, stream or checksum, a bad
tar header checksum, duplicate, missing, traversing, absolute and linked
entries, a lying package name, version, `os`, `cpu`, binary or
`optionalDependencies`, an invented or unreported inventory, and the size and
expansion limits. They are not native package acceptance: they contain no built
binary, real font or plugin bundle.

## Failure modes

- **Tag does not match the version.** `check-tag` fails before any upload. Fix the
  tag; do not edit the version to match a stray tag.
- **Fewer or more artifacts, a missing platform, a duplicated main archive, or an
  archive named for another version.** `verify` fails the `inventory` job and no
  publish runs. Re-run the failed `assemble` leg, not the whole matrix with a
  mutated artifact directory.
- **Main archive inventories differ between runners.** One host packed a
  different file set; the release is blocked. Compare the assembler report
  inventories and the `lib/`, `fonts/` and `locale/` contents before publishing
  anything. Do not relax the check to get a release out.
- **An archive does not open, or does not hold what the report claims.** `verify`
  names the archive and the reason (not gzip data, corrupt stream, header
  checksum, duplicate entry, unsafe path, link, entry over a limit, missing or
  unreported file, wrong manifest field) and no publish runs. Re-assemble that
  runner's package from a clean build; never patch the report or the archive by
  hand.
- **Partial publish.** Versions are immutable, so a re-run fails with a conflict
  on the packages that already published. Publish the remaining archives manually
  in plan order (`npm publish <tgz> --provenance --access public`) or release a
  new patch version. The workflow never un-publishes or re-tags.

## Remaining acceptance gaps

- Actual `npm publish`, provenance attestation and the trusted publisher
  handshake have not run (#34); the successful dry-runs do not establish them.
- Native builds and archive verification do not establish installed runtime
  behavior on every platform. The separate Windows real-tarball
  [install/display/uninstall case](e2e-harness.md#ci) passed in CI6 (#31).
- Real installed-profile update acceptance remains **UNVERIFIED** (#32).
  Acceptance uses scratch profiles; no live profile was exercised.

## Hardening items (open, not done)

- **Actions are pinned by tag, not by commit SHA.** `package-release.yml` grants
  `id-token: write` for npm trusted publishing, and a tag can be moved, so a
  compromised or replaced release moves with it. Pinning every `uses:` in this
  workflow to a full commit SHA is the hardening step; it is deliberately not
  done in the current change, and the pinned SHAs must be updated by hand.
- **Build parallelism.** `.cargo/config.toml` caps jobs at 2 for the
  maintainer's local resource policy (D17). Release builds must not inherit
  that: this workflow exports `CARGO_BUILD_JOBS: 4` for its own runners, and a
  workstation that wants full parallelism exports the same variable.
