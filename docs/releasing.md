# Release packaging and publishing

Status (2026-11-25): `v1.2.2` is the latest verified **published** release.
[Run 45421102962](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/45421102962)
assembled the native Windows x64 platform archive and the main archive, verified
the inventory, dry-ran both archives and published both with SLSA provenance
after the `release` environment approval. `v1.2.0` was the first tag-driven
release after the one-host decision (ADR 0002, #5, #12).

`v1.2.1` was tagged but never published:
[run 45031367826](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/45031367826)
for that tag was `cancelled` on 2026-11-22 while its `publish` job waited at that
same approval, so no `1.2.1` publication is claimed. `1.2.2` supersedes it and
carries the fixes merged after that tag. The npm registry listed 1.2.2 as the
latest published main and platform package version when checked on 2026-11-25.

The previous `v1.1.0` release (2026-11-16) assembled all four platforms and
published all five archives; it predates that decision and is history. Its first
attempt failed with `ENEEDAUTH` because no package had a trusted publisher yet;
after the five were configured, the failed `publish` job was rerun and succeeded
without a rebuild.

Historical CI7 evidence (2026-11-09): **remote packaging and publish dry-run
passed**. Each run below passed all four native platform builds, real
archive-content inventory verification and `npm publish --dry-run`, establishing
the four-platform CI packaging evidence for dsh-uml-viewer#33. These runs did
not establish actual publishing, provenance or installed behavior on all four
platforms.

| Release run | Source head | Result |
| --- | --- | --- |
| [42585882805](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42585882805) | `0d115b2` | Success |
| [42587783277](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42587783277) | `ab9064c` | Success |
| [42591228972](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42591228972) | `f4c12e3` | Success |
| [42595826538](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42595826538) | `6147980` | Success |
| [42600985433](https://github.com/qinyuanj/dsh-uml-viewer/actions/runs/42600985433) | `7fbace6` | Success |

All five `1.0.0` packages were published manually. `v1.1.0` is the first
**tag-driven** release: the tag pushed the four native builds and the publish,
which waited for the `release` environment approval and then signed provenance
for every archive (#34).

The files involved are `.github/workflows/package-release.yml`,
`scripts/package.mjs` (assembler), `scripts/archive.mjs` (bounded gzip/ustar
reader and writer), `scripts/release.mjs` (release guards),
`scripts/test-package.mjs` and `scripts/version.mjs` (version authority).

## What the pipeline does

`pull_request` and `push` of a `v*` tag trigger it. The `assemble` job runs once
per delivered platform on a native runner, because `scripts/package.mjs` executes
the supplied binary as `--version` and refuses a mismatch. A foreign target
binary is never executed.

| Delivered package | Runner label | Real architecture |
| --- | --- | --- |
| `@dsh-uml-viewer/win32-x64` | `windows-latest` | Windows x64 |

Native Windows x64 is the only delivered host (ADR 0002, #5, #12), so the matrix
has exactly one entry and the label follows GitHub's hosted-runner reference.

The `assemble` runner builds the plugin and `target/release/uml-viewer`, packs
the main and current-platform archives with the assembler, verifies the report
hash against the staged bytes, and uploads its platform archive. Font, license
and the rest of the main bundle are uploaded once, by `win32-x64` only.

The `inventory` job is the single place that validates the staged release
inventory. It and `publish` read and publish the archives the one
`windows-latest` `assemble` job produced, and they run on `ubuntu-latest`
deliberately: they are host-neutral by construction — they open tarballs,
compare report fields and call `npm publish` — and moving a tag-driven publish
onto a runner nobody has exercised is the riskier change. They carry no
delivery claim, and the `windows-latest` label above is the claim.

`scripts/release.mjs verify` requires all of:

- exactly one assemble report, for the one delivered platform, no duplicates;
- every report at the Cargo workspace version, with `optionalDependencies`
  pinning exactly the one platform package at that version;
- a current-platform archive whose name is
  `dsh-uml-viewer-<platform>-<version>.tgz` and whose bytes match its recorded
  SHA-256;
- exactly one staged main archive, staged by `win32-x64`;
- the main archive file inventory that runner reported; its SHA-256 is recorded
  in the inventory as evidence, and the published archive is the one whose bytes
  match its report;
- exactly two `.tgz` files in total, all with expected names, so a stale or
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
pins `eol=lf` for EDN, Rust, fixtures and the prompt document, not for the
packaged text files, so a Windows checkout can pack CRLF where a Linux
checkout packs LF. The archive is built and published by one runner, so this
affects the evidence, not what users download; tightening it would mean pinning
`eol=lf` for the packaged files first.

It writes `release-inventory.json` (artifact `release-inventory`): version,
per-artifact name, platform, file, path and SHA-256, plus the publish order.

Pull requests stop after offline packaging acceptance:

- `pnpm test:package` runs the package contract, the stale-payload identity gate
  and the release guards;
- `npm pack <tgz> --dry-run --ignore-scripts` reads each of the two staged
  archives in plan order and checks its package contents without contacting the
  registry. The repository root stays `private` and is never a pack target;
- a pack or inventory failure is a real package/CI failure and stops the job. An
  already-published version is not a PR failure because this path deliberately
  does not ask the registry about versions, credentials, access, trusted
  publishing or provenance. It does not establish that the archives can be
  published;
- no `id-token` is requested and no credential is read on this path.

On a `v*` tag push the `publish` job runs in the `release` environment with
`contents: read` and `id-token: write` and nothing else, checks that the tag is
`v<workspace version>`, and re-verifies the inventory. Before any real publish,
it runs `npm publish <tgz> --dry-run --access public --ignore-scripts
--provenance=false` on the same two staged archives in plan order. Only after
both dry-runs succeed does it publish the platform archive before the main
archive with `--provenance`. The order matters: the main package's optional
dependencies must already exist when it becomes installable. Any dry-run or real
publish error stops the job; errors are not ignored, retried or forced. The tag
dry-run is a registry preflight, not proof that trusted publishing, provenance
or the subsequent real publish will succeed.

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
2. Package trust, after the package exists on the registry: for each package
   (`dsh-uml-viewer` and the `@dsh-uml-viewer/*` ones the release shipped; five
   at 1.1.0, two now), open npmjs.com
   package settings and add a GitHub Actions trusted publisher. **Done for all
   five on 2026-11-16**, with `npm trust github <pkg> --file package-release.yml
   --repo qinyuanj/dsh-uml-viewer --env release --allow-publish`. The settings are
   - organization or user `qinyuanj`,
   - repository `dsh-uml-viewer`,
   - workflow filename `package-release.yml` (filename only, case sensitive;
     renaming the workflow breaks publishing),
   - environment `release`,
   - allowed actions: `npm publish`. Choose stage-only (`npm stage publish`)
     instead if a maintainer should approve every package with 2FA before it
     becomes public.

   `package.json` `repository.url` must keep matching the GitHub repository
   exactly, otherwise npm refuses the OIDC exchange. `--allow-publish` is
   required: the default configuration only permits `npm stage publish`.
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
git tag -a v1.2.0 -m "dsh-uml-viewer 1.2.0"   # substitute the printed version
git push origin v1.2.0
```

Pushing the tag starts the only publishing path. Never create the tag on a fork,
and never disable the `release` environment approval to get a publish through.

## What a maintainer can check locally

Node only, no network, no archive mocking beyond the controlled fixtures:

```sh
pnpm test:package                              # contract, stale payload, release guards
node scripts/release.mjs check-tag v1.2.0      # tag/version guard
```

`scripts/release.test.mjs` drives the real `stage`, `verify` and `plan` commands
over controlled fixture archives. It also executes the extracted pull-request and
tag workflow loops against a bounded recording `npm` stub, including pack,
dry-run and real-publish failures; those tests never contact a registry. The
fixtures are real small gzip/ustar archives
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
  publish runs. Re-run the failed `assemble` job, never a mutated artifact
  directory.
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
- **Tag dry-run conflict.** No real publish has started. Do not force or bypass
  the failure: an already-published version needs a new release version, while an
  authentication, access or other registry error needs its own correction.
- **Partial publish.** Versions are immutable, so a re-run's full dry-run stops
  with a conflict on the packages that already published, before any remaining
  real publish. Publish the remaining archives manually in plan order
  (`npm publish <tgz> --provenance --access public`) or release a new patch
  version. The workflow never un-publishes or re-tags.

## Remaining acceptance gaps

- The five `1.0.0` packages were published manually. `v1.1.0` ran the full
  tag-driven path — `npm publish` through OIDC trusted publishing, with a SLSA
  provenance attestation on each archive (#34). A tag run's registry state is
  still only what the registry reports; the run itself proves packaging, not
  installed behavior on every platform.
- Native builds and archive verification do not establish installed runtime
  behavior on every platform. The separate Windows real-tarball
  [install/display/uninstall case](e2e-harness.md#ci) passed in CI6 (#31).
- Real installed-profile update acceptance remains **UNVERIFIED** (#32).
  Acceptance uses scratch profiles; no live profile was exercised.

## Hardening items (open, not done)

- **Actions are pinned by commit SHA.** `package-release.yml` grants
  `id-token: write` for npm trusted publishing, and a tag can be moved, so every
  `uses:` in that workflow names a full commit SHA with its tag in a comment;
  `scripts/release.test.mjs` fails the suite if any entry drifts back to a tag.
  The SHAs must be updated by hand when an action is upgraded.
- **Build parallelism.** `.cargo/config.toml` caps jobs at 2 for the
  maintainer's local resource policy (D17). Release builds must not inherit
  that: this workflow exports `CARGO_BUILD_JOBS: 4` for its own runners, and a
  workstation that wants full parallelism exports the same variable.
