# Original mutation oracle

This fixture is the real Cargo workspace from pinned mutator
`tests/test_rust_slots_cargo.py:22–62`. Copy the entire `rust/` directory to a
native Linux temporary directory, preserving sibling `project/` and `extdep/`.
Use `project/` as the CLI root. Do not build or mutate the checked-in fixture.
The app manifest uses `extdep = { path = "../../extdep" }`; the original helper
substitutes an absolute path to the same sibling. This is the only fixture-byte
adaptation. Every Rust source/test and other manifest byte matches the helper.
There are no crates.io dependencies, generated lockfiles or build artifacts.

Targets, in invocation order:

```text
util/src/lib.rs
app/src/lib.rs
app/src/more.rs
```

Run the CLI with `--root <copy>/project --max-workers 1 --timeout-factor 20` and
all three targets. The first run also supplies `--mutate-all`; the second leaves
that flag out and changes no files. Default coverage and real `cargo test` are
required. The first run exits 0 with three killed sites, zero survivors and zero
uncovered sites. The unchanged differential run exits 0 with `executed=0 reused=3`
and preserves the three snapshot files byte for byte. Original manifests,
source and tests remain unchanged after both runs. The fresh original coverage
run warns that reports are not stamped as fresh because Cargo created files;
mutation still succeeds. The unchanged rerun returns before coverage.

The EDN under `goldens/` is the original Python implementation's raw output,
including the final LF. Each has `:isolation :namespace-view` and
`:tested-at "2026-11-07T12:00:00+00:00"`. The clock comes from the original
`test_snapshot_bytes.FixedTime`, assigned to `mutator.metrics.datetime` before
calling `mutator.cli.run`. An integration test comparing bytes must inject the
same timestamp in Rust. Setting `TZ=UTC` alone does not freeze the clock.
On Windows the original isolation field is `:copy`; these Linux goldens retain
the observed `:namespace-view` bytes.

| Golden path (relative to `goldens/`) | Bytes | SHA-256 |
| --- | ---: | --- |
| `app.edn` | 406 | `919be95f1a4856e91619a060d4bfc062554cc5aee45447444248ea13d47cc095` |
| `app/more.edn` | 419 | `713568c32383e59f2f0b7569903cd570aa353984562b1cd4f4403f0fe5e7575e` |
| `util.edn` | 412 | `31190035242fa75d52f3ee27f0c6f431189198236bc4a5174f2bcd6bd66fe8c8` |
| `app-no-sites.edn` | 601 | `49a18a35b755671f972712523d78187064c2924b3121273f920c0df458ac5433` |

`app-no-sites.edn` is the differential guard oracle. After the covered run and
the unchanged rerun, the regeneration script copies the oracle tree, appends
`\npub fn no_sites() {}\n` to `app/src/lib.rs` and reruns the CLI on that file
without `--mutate-all`, using `--no-coverage --test-command
"grep -q 'x + x' app/src/lib.rs"`. The added function owns no mutation site, but
the function digests differ, so the original does not reuse the snapshot: it
exits 0, runs no test, republishes `app.edn` with both forms and the carried
`:killed` outcome, and leaves the other two snapshots byte-identical. The Rust
integration test performs the same edit and compares the republished bytes.

`help.txt` contains the original CLI `HELP` string as UTF-8 bytes, starting
with `Usage:` and retaining its final LF. It is suitable for Rust `include_str!`
without changing the help contract. The regeneration script publishes it from
the same pinned module.

The portable sibling layout was additionally copied to a fresh native `/tmp`
directory and passed `cargo test --offline --workspace`.

## Lua project

`lua/project` is a two-module Lua project: a root module (`.busted`, `lpath`
`src/?.lua;src/?/init.lua`) and a nested one (`nested/.busted`, `lib/?.lua`).
Two root spec files cover it, one of them with a space in its name
(`spec/double unit_spec.lua`), and `src/odd name.lua` is an untested source with
a space in its own name. `src/calc.lua` also holds a private `local function`.

The pinned original selects 16 sites over `src/calc.lua`, `src/odd name.lua`
and `nested/lib/mathx.lua`: 3 killed, one survivor (`//` to `/` is the same
function on even input) and 12 uncovered sites, so both the covered run and the
differential rerun exit 3; the rerun reuses the killed sites and republishes
identical bytes. `--select-specs` publishes the same bytes again.

`lua/goldens/` holds the three snapshots byte for byte, `lua/sites.json` the
original's `sites_in_file` output (offsets, originals, mutants, categories,
owners, mutation identities) and form digests, and `lua/evidence.json` the
recorded console run. Regenerate inside WSL with Lua 5.4, Busted, LuaCov,
cluacov 1.0.0 and dkjson installed:

```bash
python3 scripts/mutate_goldens.py --references /mnt/d/dev/github/spec203-notes/ref --lua
```

The fixture is copied to native `/tmp` first and its bytes are checked
afterwards; the Rust tests run the same arguments with the same fixed clock and
compare the published bytes. The Lua case keeps the caller's HOME, because
LuaRocks resolves Busted and cluacov from `~/.luarocks`.

## Provenance and regeneration

Original references (read only):

- mutator `f772a8329fba3f2f81bfca0a81abfa3210af9f4a`
- crapper `4068857bc44e6383cf4a5e4d0b5c1111b49ce0b4`

The development-only script `scripts/mutate_goldens.py` follows the complete
`spec203-notes/EXPLORE-linux-mutation.md` recipe. It verifies reference pins and
cleanliness, preflights installed Cargo coverage tools, installs only pinned
Python test dependencies into disposable native `/tmp`, and runs both original
test files (four tests) before generating the real covered Cargo oracle. It
asserts the unchanged differential behavior, all original fixture bytes, and
the exact sizes/hashes above before publishing anything. `goldens/evidence.json`
records stable assertions from that run. No Python is used by the Rust CLI.

Example from WSL (adjust the home/tool/reference paths for another host):

```bash
export PATH=/home/lzx/.cargo/bin:/home/lzx/.local/share/kami/venv/bin:/usr/local/bin:/usr/bin:/bin
export CARGO_HOME=/home/lzx/.cargo RUSTUP_HOME=/home/lzx/.rustup
export PYTHONDONTWRITEBYTECODE=1
python3 /mnt/d/dev/github/dsh-uml-viewer-wt/t8/scripts/mutate_goldens.py \
  --references /mnt/d/dev/github/spec203-notes/ref
```

Requires native Linux user namespaces plus overlayfs, installed Cargo,
`cargo-llvm-cov`, the Rust `llvm-tools` component and Python with pip. There is no
global installation or tool-install fallback. The temporary Python pins are
`tree-sitter==0.26.0`, `tree-sitter-language-pack==1.21.0`, `pytest==9.2.0`.
Grammar first use may download into the disposable cache. Reference cleanliness
checks use invocation-only Git settings for Windows/DrvFS line endings and modes;
reference files/configuration are never rewritten. HOME/cache and bytecode/test
cache settings prevent run artifacts in the reference clones. Scratch is removed
only after checking its exact resolved native `/tmp` path.
